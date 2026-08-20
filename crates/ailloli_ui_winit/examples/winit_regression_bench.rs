//! Auto-terminating, surface-backed winit regression harness.
//!
//! The benchmark runner normally provides all environment variables:
//!
//! ```sh
//! AILLOLI_UI_BENCH=1 \
//! AILLOLI_UI_BENCH_PATH=artifacts/bench/phase125/manual/winit.jsonl \
//! AILLOLI_UI_BENCH_BACKEND=wayland \
//! AILLOLI_UI_BENCH_SCENARIO=wake_single \
//! AILLOLI_UI_BENCH_DURATION_MS=10000 \
//!   cargo run -p ailloli_ui_winit --example winit_regression_bench
//! ```
//!
//! `startup`, `resize_zero`, `surface_recovery`, `input_ime`, and
//! `popup_portal` require the non-default `test-support` feature for faithful
//! event-loop-thread observation/injection. `resize_zero` drives the
//! provider-neutral native presentation path through a synthetic
//! `0×0 → non-zero` round trip; it does not require a compositor to accept a
//! physically zero-sized window.

use std::error::Error;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(feature = "test-support")]
use ailloli_ui_core::{
    event::{Event, ImeEvent, Modifiers, PointerButton, PointerEvent},
    LogicalWindowId, Point,
};
use ailloli_ui_core::{Color, Size};
use ailloli_ui_runtime::app::{RuntimeHandle, RuntimeInbox, RuntimeSender, UiWake, UiWakeError};
use ailloli_ui_runtime::component::{IntoView, State};
#[cfg(feature = "test-support")]
use ailloli_ui_runtime::{
    app::PresentationGeneration,
    input::{EventEnvelope, EventId, EventMeta, EventTimestamp},
    popup::{PopupId, PopupMountPolicy, PopupRole},
};
use ailloli_ui_widgets::controls::{Button, Select, TextInput};
use ailloli_ui_widgets::layout::{Column, Container};
use ailloli_ui_widgets::text::Text;
use ailloli_ui_winit::{
    run_app_on_event_loop, try_init_ailloli_ui_bench_from_env, HostDriver, HostOutcome, UiApp,
    WindowOptions, WinitHost,
};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopProxy};

const WINIT_VERSION: &str = "0.30.13";
const MAILBOX_CAPACITY: usize = 1024;
const DEFAULT_WARMUPS: u32 = 3;
const DEFAULT_MEASURED_SAMPLES: u32 = 30;
const WAKE_BURST_SIZE: u32 = 16;
const MAIN_WINDOW_ID: &str = "main";
const INPUT_TARGET_KEY: &str = "winit-regression-input";
const POPUP_TRIGGER_KEY: &str = "winit-regression-popup-trigger";
const POPUP_BACKGROUND_BUTTON_KEY: &str = "winit-regression-popup-background-button";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Startup,
    Idle,
    WakeSingle,
    WakeBurst,
    ResizeZero,
    SurfaceRecovery,
    MultiWindow,
    InputIme,
    PopupPortal,
}

impl Scenario {
    fn parse(value: &str) -> Result<Self, HarnessError> {
        match value {
            "startup" | "cold_start" => Ok(Self::Startup),
            "idle" => Ok(Self::Idle),
            "wake_single" | "wake-single" => Ok(Self::WakeSingle),
            "wake_burst" | "wake-burst" => Ok(Self::WakeBurst),
            "resize_zero" | "resize-zero" => Ok(Self::ResizeZero),
            "surface_recovery" | "surface-recovery" => Ok(Self::SurfaceRecovery),
            "multi_window" | "multi-window" => Ok(Self::MultiWindow),
            "input_ime" | "input-ime" => Ok(Self::InputIme),
            "popup_portal" | "popup-portal" => Ok(Self::PopupPortal),
            other => Err(HarnessError(format!(
                "unsupported winit benchmark scenario {other:?}; expected startup, idle, wake_single, wake_burst, resize_zero, surface_recovery, multi_window, input_ime, or popup_portal"
            ))),
        }
    }

    const fn logical_window_count(self) -> usize {
        if matches!(self, Self::MultiWindow) {
            2
        } else {
            1
        }
    }

    const fn needs_periodic_redraw(self) -> bool {
        matches!(
            self,
            Self::SurfaceRecovery | Self::MultiWindow | Self::InputIme | Self::PopupPortal
        )
    }

    const fn uses_external_wakes(self) -> bool {
        matches!(self, Self::WakeSingle | Self::WakeBurst)
    }

    const fn periodic_metric_name(self) -> &'static str {
        match self {
            Self::Idle => "idle.wait_jitter_us",
            Self::ResizeZero => "resize_zero.scheduled_tick_jitter_us",
            Self::SurfaceRecovery => "surface_recovery.scheduled_tick_jitter_us",
            Self::MultiWindow => "multi_window.scheduled_tick_jitter_us",
            Self::InputIme => "input_ime.scheduled_tick_jitter_us",
            Self::PopupPortal => "popup_portal.scheduled_tick_jitter_us",
            Self::Startup | Self::WakeSingle | Self::WakeBurst => "host.scheduled_tick_jitter_us",
        }
    }

    const fn periodic_metric_role(self) -> ailloli_ui_bench::MetricRole {
        match self {
            Self::Idle | Self::SurfaceRecovery | Self::MultiWindow if self.gate_ready() => {
                ailloli_ui_bench::MetricRole::GatingSteady
            }
            Self::Startup
            | Self::WakeSingle
            | Self::WakeBurst
            | Self::ResizeZero
            | Self::InputIme
            | Self::PopupPortal
            | Self::SurfaceRecovery
            | Self::MultiWindow
            | Self::Idle => ailloli_ui_bench::MetricRole::Diagnostic,
        }
    }

    const fn gate_ready(self) -> bool {
        match self {
            Self::Startup
            | Self::ResizeZero
            | Self::SurfaceRecovery
            | Self::InputIme
            | Self::PopupPortal => {
                cfg!(feature = "test-support")
            }
            Self::Idle | Self::WakeSingle | Self::WakeBurst | Self::MultiWindow => true,
        }
    }

    const fn fidelity(self) -> &'static str {
        match self {
            Self::Startup if cfg!(feature = "test-support") => {
                "full:first-successful-swapchain-present-observed-on-event-loop-thread"
            }
            Self::Startup => "blocked:first-present-observation-requires-test-support",
            Self::Idle => "full:waituntil-probes-without-redraw",
            Self::WakeSingle => "full:bounded-mailbox-and-event-loop-proxy",
            Self::WakeBurst => "full:bounded-mailbox-burst-and-event-loop-proxy",
            Self::MultiWindow => "full:two-independent-native-presentations",
            Self::ResizeZero if cfg!(feature = "test-support") => {
                "full:event-loop-thread-zero-extent-lifecycle-and-surface-restore"
            }
            Self::ResizeZero => "blocked:zero-extent-injection-requires-test-support",
            Self::SurfaceRecovery if cfg!(feature = "test-support") => {
                "full:lost-and-outdated-detach-reattach-faults"
            }
            Self::SurfaceRecovery => {
                "partial:repeated-surface-redraw;enable-test-support-for-fault-injection"
            }
            Self::InputIme if cfg!(feature = "test-support") => {
                "full:provider-neutral-event-envelope-ime-lifecycle"
            }
            Self::InputIme => "blocked:event-envelope-injection-requires-test-support",
            Self::PopupPortal if cfg!(feature = "test-support") => {
                "full:retained-select-request-and-dismiss-to-successful-present"
            }
            Self::PopupPortal => "blocked:retained-popup-event-injection-requires-test-support",
        }
    }
}

#[derive(Debug)]
struct HarnessError(String);

impl std::fmt::Display for HarnessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for HarnessError {}

#[derive(Debug, Clone, Copy)]
struct WakeSample {
    sent_at: Instant,
    phase: ailloli_ui_bench::SamplePhase,
    kind: HarnessActionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HarnessActionKind {
    Wake,
    PopupBackground,
}

#[derive(Debug, Clone, Copy)]
struct FirstPresentObservation {
    observed_at: Instant,
    generation: u64,
}

#[derive(Debug, Default)]
struct HarnessAccounting {
    successful_sends: AtomicU64,
    failed_sends: AtomicU64,
    observed_actions: AtomicU64,
    input_sequences: AtomicU64,
    input_rejected_events: AtomicU64,
    input_focus_failures: AtomicU64,
    resize_zero_round_trips: AtomicU64,
    resize_zero_injection_failures: AtomicU64,
    popup_round_trips: AtomicU64,
    popup_request_present_samples: AtomicU64,
    popup_dismiss_present_samples: AtomicU64,
    popup_rejected_events: AtomicU64,
    popup_lost: AtomicU64,
    popup_duplicate: AtomicU64,
    popup_dismiss_failures: AtomicU64,
    popup_background_activations: AtomicU64,
    popup_focus_restore_failures: AtomicU64,
}

#[derive(Debug, Clone, Copy)]
struct SamplingPlan {
    started_at: Instant,
    duration: Duration,
    settle: Duration,
    warmup_samples: u32,
    measured_samples: u32,
}

impl SamplingPlan {
    fn total_samples(self) -> u32 {
        self.warmup_samples
            .saturating_add(self.measured_samples)
            .max(1)
    }

    fn interval(self) -> Duration {
        self.duration
            .saturating_sub(self.settle)
            .checked_div(self.total_samples().saturating_add(1))
            .unwrap_or(Duration::from_millis(1))
            .max(Duration::from_millis(1))
    }
}

#[derive(Debug)]
struct EventLoopWake(EventLoopProxy<()>);

impl UiWake for EventLoopWake {
    fn wake(&self) -> Result<(), UiWakeError> {
        self.0.send_event(()).map_err(|_| UiWakeError::TargetClosed)
    }
}

struct RegressionDriver {
    scenario: Scenario,
    started_at: Instant,
    duration: Duration,
    settle: Duration,
    deadline: Instant,
    next_sample_at: Option<Instant>,
    sample_interval: Duration,
    warmup_samples: u32,
    total_samples: u32,
    emitted_samples: u32,
    first_service_recorded: bool,
    first_present_recorded: bool,
    ready_at: Arc<OnceLock<Instant>>,
    first_present: Arc<OnceLock<FirstPresentObservation>>,
    accounting: Arc<HarnessAccounting>,
    sender: RuntimeSender<WakeSample>,
}

impl RegressionDriver {
    fn new(
        scenario: Scenario,
        sampling: SamplingPlan,
        sender: RuntimeSender<WakeSample>,
        accounting: Arc<HarnessAccounting>,
        ready_at: Arc<OnceLock<Instant>>,
        first_present: Arc<OnceLock<FirstPresentObservation>>,
    ) -> Self {
        let total_samples = sampling.total_samples();
        let sample_interval = sampling.interval();

        Self {
            scenario,
            started_at: sampling.started_at,
            duration: sampling.duration,
            settle: sampling.settle,
            deadline: sampling.started_at + sampling.duration,
            next_sample_at: None,
            sample_interval,
            warmup_samples: sampling.warmup_samples,
            total_samples,
            emitted_samples: 0,
            first_service_recorded: false,
            first_present_recorded: false,
            ready_at,
            first_present,
            accounting,
            sender,
        }
    }

    fn sample_phase(&self, index: u32) -> ailloli_ui_bench::SamplePhase {
        if index < self.warmup_samples {
            ailloli_ui_bench::SamplePhase::Warmup
        } else {
            ailloli_ui_bench::SamplePhase::Measured
        }
    }

    fn record_sample(
        name: &'static str,
        value: f64,
        phase: ailloli_ui_bench::SamplePhase,
        role: ailloli_ui_bench::MetricRole,
    ) {
        let _ = ailloli_ui_bench::try_record(
            ailloli_ui_bench::Event::Metric {
                ts_ms: now_ms(),
                name: name.to_string(),
                value,
                role,
            },
            ailloli_ui_bench::EventContext::default().with_sample_phase(phase),
        );
    }

    fn record_first_service(&mut self, now: Instant) {
        if self.first_service_recorded {
            return;
        }
        self.first_service_recorded = true;
        let ready_at = *self.ready_at.get_or_init(|| now);
        self.deadline = ready_at + self.duration;
        if !self.scenario.uses_external_wakes() && self.scenario != Scenario::Startup {
            self.next_sample_at = Some(ready_at + self.settle + self.sample_interval);
        }
        if self.scenario == Scenario::Startup {
            Self::record_sample(
                "startup.host_ready_us",
                now.saturating_duration_since(self.started_at).as_micros() as f64,
                ailloli_ui_bench::SamplePhase::Measured,
                ailloli_ui_bench::MetricRole::Diagnostic,
            );
        }
    }

    fn record_first_present(&mut self) -> bool {
        if self.scenario != Scenario::Startup || self.first_present_recorded {
            return false;
        }
        let Some(observation) = self.first_present.get().copied() else {
            return false;
        };
        self.first_present_recorded = true;
        let _ = ailloli_ui_bench::try_record(
            ailloli_ui_bench::Event::Metric {
                ts_ms: now_ms(),
                name: "startup.first_present_us".to_string(),
                value: observation
                    .observed_at
                    .saturating_duration_since(self.started_at)
                    .as_micros() as f64,
                role: ailloli_ui_bench::MetricRole::GatingColdStart,
            },
            ailloli_ui_bench::EventContext::default()
                .with_window(ailloli_ui_bench::BenchWindowId::new(MAIN_WINDOW_ID))
                .with_surface(
                    ailloli_ui_bench::BenchSurfaceId::new(MAIN_WINDOW_ID),
                    observation.generation,
                )
                .with_sample_phase(ailloli_ui_bench::SamplePhase::Measured),
        );
        true
    }

    fn drain_wake_samples(&self, runtime: &RuntimeHandle<WakeSample>, now: Instant) -> bool {
        let mut received = false;
        for sample in runtime.take_actions() {
            match sample.kind {
                HarnessActionKind::Wake => {
                    received = true;
                    self.accounting
                        .observed_actions
                        .fetch_add(1, Ordering::Relaxed);
                    Self::record_sample(
                        "wake.round_trip_us",
                        now.saturating_duration_since(sample.sent_at).as_micros() as f64,
                        sample.phase,
                        ailloli_ui_bench::MetricRole::GatingSteady,
                    );
                }
                HarnessActionKind::PopupBackground => {
                    self.accounting
                        .popup_background_activations
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        received
    }

    fn service_periodic_samples(&mut self, now: Instant) -> bool {
        let mut redraw = false;
        while self.emitted_samples < self.total_samples
            && self
                .next_sample_at
                .is_some_and(|sample_at| sample_at <= now)
        {
            let sample_at = self.next_sample_at.expect("sample deadline checked");
            let phase = self.sample_phase(self.emitted_samples);
            Self::record_sample(
                self.scenario.periodic_metric_name(),
                now.saturating_duration_since(sample_at).as_micros() as f64,
                phase,
                self.scenario.periodic_metric_role(),
            );

            if self.scenario.needs_periodic_redraw() {
                redraw = true;
            }

            self.emitted_samples = self.emitted_samples.saturating_add(1);
            self.next_sample_at = (self.emitted_samples < self.total_samples)
                .then_some(sample_at + self.sample_interval);
        }
        redraw
    }

    fn next_wake(&self) -> Instant {
        self.next_sample_at
            .map_or(self.deadline, |sample| sample.min(self.deadline))
    }

    fn finish_metrics(&self) {
        let stats = self.sender.stats();
        let successful_sends = self.accounting.successful_sends.load(Ordering::Relaxed);
        let observed_actions = self.accounting.observed_actions.load(Ordering::Relaxed);
        let failed_sends = self.accounting.failed_sends.load(Ordering::Relaxed);

        if self.scenario.uses_external_wakes() {
            record_correctness(
                "correctness.lost_wake",
                successful_sends.saturating_sub(observed_actions) as f64,
            );
            record_correctness("correctness.send_failure", failed_sends as f64);
            record_correctness("correctness.mailbox_overflow", stats.overflow as f64);
            record_correctness("correctness.mailbox_disconnect", stats.disconnected as f64);
            record_correctness(
                "correctness.mailbox_wake_failure",
                stats.wake_failures as f64,
            );
        }

        match self.scenario {
            Scenario::Startup => record_correctness(
                "correctness.startup_first_present_missing",
                u8::from(self.first_present.get().is_none()) as f64,
            ),
            Scenario::InputIme => {
                let sequences = self.accounting.input_sequences.load(Ordering::Relaxed);
                record_correctness(
                    "correctness.input_ime_sequence_count_mismatch",
                    u64::from(self.total_samples).abs_diff(sequences) as f64,
                );
                record_correctness(
                    "correctness.input_ime_rejected_event",
                    self.accounting
                        .input_rejected_events
                        .load(Ordering::Relaxed) as f64,
                );
                record_correctness(
                    "correctness.input_ime_focus_failure",
                    self.accounting.input_focus_failures.load(Ordering::Relaxed) as f64,
                );
            }
            Scenario::ResizeZero => {
                let round_trips = self
                    .accounting
                    .resize_zero_round_trips
                    .load(Ordering::Relaxed);
                record_correctness(
                    "correctness.resize_zero_round_trip_count_mismatch",
                    u64::from(self.total_samples).abs_diff(round_trips) as f64,
                );
                record_correctness(
                    "correctness.resize_zero_injection_failure",
                    self.accounting
                        .resize_zero_injection_failures
                        .load(Ordering::Relaxed) as f64,
                );
            }
            Scenario::SurfaceRecovery if !cfg!(feature = "test-support") => {
                record_correctness("correctness.surface_recovery_not_exercised", 1.0);
            }
            Scenario::PopupPortal => {
                let round_trips = self.accounting.popup_round_trips.load(Ordering::Relaxed);
                let request_present_samples = self
                    .accounting
                    .popup_request_present_samples
                    .load(Ordering::Relaxed);
                let dismiss_present_samples = self
                    .accounting
                    .popup_dismiss_present_samples
                    .load(Ordering::Relaxed);
                record_correctness(
                    "correctness.popup_round_trip_count_mismatch",
                    u64::from(self.total_samples).abs_diff(round_trips) as f64,
                );
                record_correctness(
                    "correctness.popup_request_present_count_mismatch",
                    u64::from(self.total_samples).abs_diff(request_present_samples) as f64,
                );
                record_correctness(
                    "correctness.popup_dismiss_present_count_mismatch",
                    u64::from(self.total_samples).abs_diff(dismiss_present_samples) as f64,
                );
                record_correctness(
                    "correctness.popup_rejected_event",
                    self.accounting
                        .popup_rejected_events
                        .load(Ordering::Relaxed) as f64,
                );
                record_correctness(
                    "correctness.popup_lost",
                    self.accounting.popup_lost.load(Ordering::Relaxed) as f64,
                );
                record_correctness(
                    "correctness.popup_duplicate",
                    self.accounting.popup_duplicate.load(Ordering::Relaxed) as f64,
                );
                record_correctness(
                    "correctness.popup_dismiss_failure",
                    self.accounting
                        .popup_dismiss_failures
                        .load(Ordering::Relaxed) as f64,
                );
                record_correctness(
                    "correctness.popup_background_activation",
                    self.accounting
                        .popup_background_activations
                        .load(Ordering::Relaxed) as f64,
                );
                record_correctness(
                    "correctness.popup_focus_restore_failure",
                    self.accounting
                        .popup_focus_restore_failures
                        .load(Ordering::Relaxed) as f64,
                );
            }
            Scenario::Idle
            | Scenario::WakeSingle
            | Scenario::WakeBurst
            | Scenario::SurfaceRecovery
            | Scenario::MultiWindow => {}
        }
    }
}

impl HostDriver<WakeSample> for RegressionDriver {
    fn service(&mut self, runtime: &RuntimeHandle<WakeSample>, now: Instant) -> HostOutcome {
        self.record_first_service(now);
        let wake_redraw = self.drain_wake_samples(runtime, now);

        if self.record_first_present() {
            return HostOutcome::exit();
        }
        if self.scenario == Scenario::InputIme
            && self.accounting.input_sequences.load(Ordering::Relaxed)
                >= u64::from(self.total_samples)
        {
            return HostOutcome::exit();
        }
        if self.scenario == Scenario::ResizeZero
            && self
                .accounting
                .resize_zero_round_trips
                .load(Ordering::Relaxed)
                >= u64::from(self.total_samples)
        {
            return HostOutcome::exit();
        }
        if self.scenario == Scenario::PopupPortal
            && self.accounting.popup_round_trips.load(Ordering::Relaxed)
                >= u64::from(self.total_samples)
        {
            return HostOutcome::exit();
        }

        if now >= self.deadline {
            return HostOutcome::exit();
        }

        HostOutcome {
            exit: false,
            redraw_all: wake_redraw || self.service_periodic_samples(now),
            next_wake: Some(self.next_wake()),
        }
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn record_correctness(name: &'static str, value: f64) {
    ailloli_ui_bench::metric_with_role(name, value, ailloli_ui_bench::MetricRole::Correctness);
}

fn env_value(primary: &str, legacy: &str, historical: &str) -> Option<String> {
    std::env::var(primary)
        .ok()
        .or_else(|| std::env::var(legacy).ok())
        .or_else(|| std::env::var(historical).ok())
        .filter(|value| !value.trim().is_empty())
}

fn sample_count(suffix: &str, fallback: u32) -> u32 {
    env_value(
        &format!("AILLOLI_UI_BENCH_{suffix}"),
        &format!("OCTAVUI_BENCH_{suffix}"),
        &format!("BENCH_{suffix}"),
    )
    .and_then(|value| value.parse::<u32>().ok())
    .unwrap_or(fallback)
}

fn validate_sampling_contract(
    scenario: Scenario,
    warmup_samples: u32,
    measured_samples: u32,
) -> Result<(), HarnessError> {
    if scenario != Scenario::PopupPortal {
        return Ok(());
    }
    if warmup_samples != DEFAULT_WARMUPS {
        return Err(HarnessError(format!(
            "popup_portal requires exactly {DEFAULT_WARMUPS} excluded warmup samples; got {warmup_samples}"
        )));
    }
    if measured_samples < DEFAULT_MEASURED_SAMPLES {
        return Err(HarnessError(format!(
            "popup_portal requires at least {DEFAULT_MEASURED_SAMPLES} measured samples; got {measured_samples}"
        )));
    }
    Ok(())
}

fn validate_scenario_capability(scenario: Scenario) -> Result<(), HarnessError> {
    if scenario == Scenario::PopupPortal && !cfg!(feature = "test-support") {
        return Err(HarnessError(
            "popup_portal requires the ailloli_ui_winit test-support feature for real EventEnvelope injection and successful-present observation"
                .to_string(),
        ));
    }
    Ok(())
}

#[cfg(feature = "test-support")]
fn popup_native_focus_ready(native_focus: Option<bool>) -> bool {
    native_focus == Some(true)
}

fn requested_backend() -> String {
    env_value(
        "AILLOLI_UI_BENCH_BACKEND",
        "OCTAVUI_BENCH_BACKEND",
        "BENCH_BACKEND",
    )
    .unwrap_or_else(|| "auto".to_string())
    .to_ascii_lowercase()
}

fn create_event_loop(requested: &str) -> Result<(EventLoop<()>, String), Box<dyn Error>> {
    #[cfg(target_os = "linux")]
    {
        use winit::platform::wayland::{EventLoopBuilderExtWayland, EventLoopExtWayland};
        use winit::platform::x11::EventLoopBuilderExtX11;

        let mut builder = EventLoop::builder();
        match requested {
            "auto" => {}
            "wayland" => {
                builder.with_wayland();
            }
            "x11" => {
                builder.with_x11();
            }
            other => {
                return Err(Box::new(HarnessError(format!(
                    "unsupported Linux winit backend {other:?}; expected auto, wayland, or x11"
                ))));
            }
        }
        let event_loop = builder.build()?;
        let actual = if event_loop.is_wayland() {
            "wayland"
        } else {
            "x11"
        };
        Ok((event_loop, actual.to_string()))
    }

    #[cfg(not(target_os = "linux"))]
    {
        if !matches!(requested, "auto" | "native" | std::env::consts::OS) {
            return Err(Box::new(HarnessError(format!(
                "backend {requested:?} does not match native platform {}",
                std::env::consts::OS
            ))));
        }
        Ok((EventLoop::new()?, std::env::consts::OS.to_string()))
    }
}

fn root_view(scenario: Scenario, window_name: &str) -> impl IntoView<WakeSample> {
    let mut column = Column::<WakeSample>::new()
        .gap(12.0)
        .child(Text::new("Ailloli UI winit regression harness").size(20.0))
        .child(Text::new(format!("scenario: {scenario:?}")))
        .child(Text::new(format!("window: {window_name}")));
    if scenario == Scenario::InputIme {
        column = column.child(
            TextInput::<WakeSample>::new()
                .bind(State::new(String::new()))
                .placeholder("IME benchmark target")
                .width(360.0)
                .into_view()
                .key(INPUT_TARGET_KEY),
        );
    }
    if scenario == Scenario::PopupPortal {
        column = column
            .child(
                Select::<String, WakeSample>::new()
                    .bind(State::new("One".to_string()))
                    .option("One".to_string(), "One")
                    .option("Two".to_string(), "Two")
                    .option("Three".to_string(), "Three")
                    .width(280.0)
                    .into_view()
                    .key(POPUP_TRIGGER_KEY),
            )
            .child(Container::<WakeSample>::new().height(180.0))
            .child(
                Button::with_label("Background action must remain suppressed")
                    .on_click(WakeSample {
                        sent_at: Instant::now(),
                        phase: ailloli_ui_bench::SamplePhase::Measured,
                        kind: HarnessActionKind::PopupBackground,
                    })
                    .into_view()
                    .key(POPUP_BACKGROUND_BUTTON_KEY),
            );
    }

    Container::<WakeSample>::new()
        .fill()
        .padding(24.0)
        .background(Color::rgb(24, 26, 34))
        .child(column)
}

fn window_options(id: &str, title: &str, size: Size) -> WindowOptions {
    WindowOptions {
        logical_window_id: id.to_string(),
        title: title.to_string(),
        ..WindowOptions::default()
    }
    .with_logical_inner_size(size)
}

fn wait_until_or_stopped(deadline: Instant, stop: &AtomicBool) -> bool {
    while !stop.load(Ordering::Acquire) {
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        thread::sleep(
            deadline
                .saturating_duration_since(now)
                .min(Duration::from_millis(10)),
        );
    }
    false
}

fn spawn_wake_producer(
    scenario: Scenario,
    sampling: SamplingPlan,
    sender: RuntimeSender<WakeSample>,
    accounting: Arc<HarnessAccounting>,
    ready_at: Arc<OnceLock<Instant>>,
    stop: Arc<AtomicBool>,
) -> Option<JoinHandle<()>> {
    if !scenario.uses_external_wakes() {
        return None;
    }

    Some(thread::spawn(move || {
        let sampling_anchor = loop {
            if let Some(ready_at) = ready_at.get().copied() {
                break ready_at;
            }
            if stop.load(Ordering::Acquire) {
                return;
            }
            thread::sleep(Duration::from_millis(1));
        };
        let total_samples = sampling.total_samples();
        let interval = sampling.interval();
        let burst_size = if scenario == Scenario::WakeBurst {
            WAKE_BURST_SIZE
        } else {
            1
        };
        for sample_index in 0..total_samples {
            let deadline =
                sampling_anchor + sampling.settle + interval.saturating_mul(sample_index + 1);
            if !wait_until_or_stopped(deadline, &stop) {
                break;
            }
            let phase = if sample_index < sampling.warmup_samples {
                ailloli_ui_bench::SamplePhase::Warmup
            } else {
                ailloli_ui_bench::SamplePhase::Measured
            };
            for _ in 0..burst_size {
                let action = WakeSample {
                    sent_at: Instant::now(),
                    phase,
                    kind: HarnessActionKind::Wake,
                };
                match sender.dispatch(action) {
                    Ok(()) => {
                        accounting.successful_sends.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => {
                        accounting.failed_sends.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }))
}

fn default_bench_path(scenario: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("bench")
        .join("phase125")
        .join("manual")
        .join(format!("winit-{scenario}-{}.jsonl", std::process::id()))
}

fn update_harness_metadata(
    scenario: Scenario,
    requested_backend: &str,
    actual_backend: &str,
    duration: Duration,
    warmup_samples: u32,
    measured_samples: u32,
) -> Result<(), ailloli_ui_bench::BenchWriteError> {
    let mut metadata = ailloli_ui_bench::RunMetadata::default();
    metadata.winit_version = Some(WINIT_VERSION.to_string());
    metadata.window_backend = Some(actual_backend.to_string());
    metadata.harness = Some("winit_regression_bench".to_string());
    metadata.warmup_samples = Some(warmup_samples);
    metadata.measured_samples = Some(measured_samples);
    metadata.extensions.insert(
        "harness".to_string(),
        serde_json::Value::String("winit_regression_bench".to_string()),
    );
    metadata
        .extensions
        .insert("surface_backed".to_string(), serde_json::Value::Bool(true));
    metadata.extensions.insert(
        "single_event_loop".to_string(),
        serde_json::Value::Bool(true),
    );
    metadata.extensions.insert(
        "winit_backend_requested".to_string(),
        serde_json::Value::String(requested_backend.to_string()),
    );
    metadata.extensions.insert(
        "winit_backend_actual".to_string(),
        serde_json::Value::String(actual_backend.to_string()),
    );
    metadata.extensions.insert(
        "scenario_fidelity".to_string(),
        serde_json::Value::String(scenario.fidelity().to_string()),
    );
    metadata.extensions.insert(
        "scenario_gate_ready".to_string(),
        serde_json::Value::Bool(scenario.gate_ready()),
    );
    metadata.extensions.insert(
        "duration_ms".to_string(),
        serde_json::Value::from(duration.as_millis() as u64),
    );
    metadata.extensions.insert(
        "duration_origin".to_string(),
        serde_json::Value::String("app_run".to_string()),
    );
    if scenario == Scenario::Startup {
        metadata.extensions.insert(
            "startup_measurement_end".to_string(),
            serde_json::Value::String(
                "first_successful_swapchain_present_observed_by_test_service".to_string(),
            ),
        );
    }
    if scenario == Scenario::ResizeZero {
        metadata.extensions.insert(
            "resize_zero_injection".to_string(),
            serde_json::Value::String(
                if cfg!(feature = "test-support") {
                    "event-loop-thread:synthetic-zero-extent-then-native-surface-restore"
                } else {
                    "unavailable:zero-extent-injection-requires-test-support"
                }
                .to_string(),
            ),
        );
    }
    if scenario == Scenario::InputIme {
        metadata.extensions.insert(
            "input_source".to_string(),
            serde_json::Value::String(
                if cfg!(feature = "test-support") {
                    "provider-neutral-event-envelope"
                } else {
                    "unavailable:test-support-disabled"
                }
                .to_string(),
            ),
        );
        metadata.extensions.insert(
            "input_latency_end".to_string(),
            serde_json::Value::String(
                "first_successful_frame_after_event_envelope_sequence".to_string(),
            ),
        );
    }
    if scenario == Scenario::PopupPortal {
        metadata.extensions.insert(
            "popup_mount_policy".to_string(),
            serde_json::Value::String("retained_overlay".to_string()),
        );
        metadata.extensions.insert(
            "popup_latency_start".to_string(),
            serde_json::Value::String(
                "portal-open-or-dismiss-transition-observed-after-event-envelope".to_string(),
            ),
        );
        metadata.extensions.insert(
            "popup_latency_end".to_string(),
            serde_json::Value::String(
                "first-successful-present-observed-after-transition".to_string(),
            ),
        );
        metadata.extensions.insert(
            "popup_background_action".to_string(),
            serde_json::Value::String("must-remain-zero".to_string()),
        );
    }
    ailloli_ui_bench::try_update_metadata(metadata).map(|_| ())
}

#[cfg(feature = "test-support")]
struct NativeHarnessProbe {
    scenario: Scenario,
    sampling: SamplingPlan,
    first_present: Arc<OnceLock<FirstPresentObservation>>,
    accounting: Arc<HarnessAccounting>,
    next_event_id: u64,
    input_focus_ready: bool,
    next_input_sample: u32,
    pending_input_sample: Option<PendingInputSample>,
    next_resize_zero_sample: u32,
    pending_resize_zero_sample: Option<PendingResizeZeroSample>,
    next_popup_sample: u32,
    pending_popup_sample: Option<PendingPopupSample>,
}

#[cfg(feature = "test-support")]
struct PendingInputSample {
    phase: ailloli_ui_bench::SamplePhase,
    injected_at: Instant,
    rendered_frame_count_before: u64,
    cause_event_id: Option<ailloli_ui_bench::EventId>,
}

#[cfg(feature = "test-support")]
struct PendingResizeZeroSample {
    phase: ailloli_ui_bench::SamplePhase,
    injected_at: Instant,
    zero_extent_count_before: u64,
    rendered_frame_count_before: u64,
    cause_event_id: Option<ailloli_ui_bench::EventId>,
}

#[cfg(feature = "test-support")]
struct PendingPopupSample {
    phase: ailloli_ui_bench::SamplePhase,
    stage: PendingPopupStage,
    transition_observed_at: Instant,
    rendered_frame_count_before: u64,
    cause_event_id: Option<ailloli_ui_bench::EventId>,
}

#[cfg(feature = "test-support")]
enum PendingPopupStage {
    Opening(PopupId),
    Dismissing(PopupId),
}

#[cfg(feature = "test-support")]
impl NativeHarnessProbe {
    fn new(
        scenario: Scenario,
        sampling: SamplingPlan,
        first_present: Arc<OnceLock<FirstPresentObservation>>,
        accounting: Arc<HarnessAccounting>,
    ) -> Self {
        Self {
            scenario,
            sampling,
            first_present,
            accounting,
            next_event_id: 1,
            input_focus_ready: false,
            next_input_sample: 0,
            pending_input_sample: None,
            next_resize_zero_sample: 0,
            pending_resize_zero_sample: None,
            next_popup_sample: 0,
            pending_popup_sample: None,
        }
    }

    fn service(&mut self, ui: &mut UiApp<WakeSample>) {
        let logical_window_id = LogicalWindowId::new(MAIN_WINDOW_ID);
        let Some(state) = ui.presentation_test_state(&logical_window_id) else {
            return;
        };
        if !state.attached || state.rendered_frame_count == 0 {
            return;
        }

        self.first_present.get_or_init(|| FirstPresentObservation {
            observed_at: Instant::now(),
            generation: state.generation.get(),
        });

        if self.scenario == Scenario::ResizeZero {
            self.service_resize_zero(ui, &state);
            return;
        }
        if self.scenario == Scenario::PopupPortal {
            self.service_popup_portal(ui, &state);
            return;
        }
        if self.scenario != Scenario::InputIme {
            return;
        }

        if self
            .pending_input_sample
            .as_ref()
            .is_some_and(|pending| state.rendered_frame_count > pending.rendered_frame_count_before)
        {
            let pending = self
                .pending_input_sample
                .take()
                .expect("pending sample was checked above");
            let mut context = Self::bench_context(state.generation, pending.phase);
            if let Some(cause_event_id) = pending.cause_event_id {
                context = context.caused_by(cause_event_id);
            }
            let _ = ailloli_ui_bench::try_record(
                ailloli_ui_bench::Event::Metric {
                    ts_ms: now_ms(),
                    name: "input_ime.event_to_present_us".to_string(),
                    value: pending.injected_at.elapsed().as_micros() as f64,
                    role: ailloli_ui_bench::MetricRole::GatingSteady,
                },
                context,
            );
            self.accounting
                .input_sequences
                .fetch_add(1, Ordering::Relaxed);
        }
        if self.pending_input_sample.is_some()
            || self.next_input_sample >= self.sampling.total_samples()
        {
            return;
        }

        let Some(bounds) =
            ui.presentation_test_element_bounds(&logical_window_id, INPUT_TARGET_KEY)
        else {
            return;
        };
        if !self.input_focus_ready {
            let focus_point = Point::new(bounds.x + bounds.w * 0.5, bounds.y + bounds.h * 0.5);
            for pressed in [true, false] {
                self.inject(
                    ui,
                    state.generation,
                    Event::Pointer(PointerEvent::button(
                        focus_point,
                        PointerButton::Left,
                        pressed,
                        Modifiers::default(),
                    )),
                );
            }
            if ui.presentation_test_focus_within_key(&logical_window_id, INPUT_TARGET_KEY)
                != Some(true)
            {
                self.accounting
                    .input_focus_failures
                    .fetch_add(1, Ordering::Relaxed);
                self.next_input_sample = self.sampling.total_samples();
                return;
            }
            self.input_focus_ready = true;
        }

        let phase = if self.next_input_sample < self.sampling.warmup_samples {
            ailloli_ui_bench::SamplePhase::Warmup
        } else {
            ailloli_ui_bench::SamplePhase::Measured
        };
        let cause_event_id = ailloli_ui_bench::try_record(
            ailloli_ui_bench::Event::Marker {
                ts_ms: now_ms(),
                name: "input_ime.sequence_injected".to_string(),
            },
            Self::bench_context(state.generation, phase),
        )
        .ok()
        .flatten();
        let sequence_started_at = Instant::now();
        let preedit = ImeEvent::try_preedit("a", Some((1, 1)), None)
            .expect("static IME preedit selection is valid");
        for event in [
            Event::Ime(ImeEvent::enabled()),
            Event::Ime(preedit),
            Event::Ime(ImeEvent::commit("x")),
            Event::Ime(ImeEvent::disabled()),
        ] {
            self.inject(ui, state.generation, event);
        }
        let mut route_context = Self::bench_context(state.generation, phase);
        if let Some(cause_event_id) = cause_event_id {
            route_context = route_context.caused_by(cause_event_id);
        }
        let _ = ailloli_ui_bench::try_record(
            ailloli_ui_bench::Event::Metric {
                ts_ms: now_ms(),
                name: "input_ime.event_envelope_sequence_us".to_string(),
                value: sequence_started_at.elapsed().as_micros() as f64,
                role: ailloli_ui_bench::MetricRole::Diagnostic,
            },
            route_context,
        );
        self.pending_input_sample = Some(PendingInputSample {
            phase,
            injected_at: sequence_started_at,
            rendered_frame_count_before: state.rendered_frame_count,
            cause_event_id,
        });
        self.next_input_sample = self.next_input_sample.saturating_add(1);
        ui.request_redraw_all();
    }

    fn service_resize_zero(
        &mut self,
        ui: &mut UiApp<WakeSample>,
        state: &ailloli_ui_winit::PresentationTestState,
    ) {
        if self
            .pending_resize_zero_sample
            .as_ref()
            .is_some_and(|pending| {
                state.zero_extent_count > pending.zero_extent_count_before
                    && state.rendered_frame_count > pending.rendered_frame_count_before
            })
        {
            let pending = self
                .pending_resize_zero_sample
                .take()
                .expect("pending zero-extent sample was checked above");
            let mut context = Self::bench_context(state.generation, pending.phase);
            if let Some(cause_event_id) = pending.cause_event_id {
                context = context.caused_by(cause_event_id);
            }
            let _ = ailloli_ui_bench::try_record(
                ailloli_ui_bench::Event::Metric {
                    ts_ms: now_ms(),
                    name: "resize_zero.event_to_present_us".to_string(),
                    value: pending.injected_at.elapsed().as_micros() as f64,
                    role: ailloli_ui_bench::MetricRole::GatingSteady,
                },
                context,
            );
            self.accounting
                .resize_zero_round_trips
                .fetch_add(1, Ordering::Relaxed);
        }
        if self.pending_resize_zero_sample.is_some()
            || self.next_resize_zero_sample >= self.sampling.total_samples()
        {
            return;
        }

        let sample_at = self.sampling.started_at
            + self.sampling.settle
            + self
                .sampling
                .interval()
                .saturating_mul(self.next_resize_zero_sample + 1);
        if Instant::now() < sample_at {
            return;
        }

        let phase = if self.next_resize_zero_sample < self.sampling.warmup_samples {
            ailloli_ui_bench::SamplePhase::Warmup
        } else {
            ailloli_ui_bench::SamplePhase::Measured
        };
        let cause_event_id = ailloli_ui_bench::try_record(
            ailloli_ui_bench::Event::Marker {
                ts_ms: now_ms(),
                name: "resize_zero.round_trip_injected".to_string(),
            },
            Self::bench_context(state.generation, phase),
        )
        .ok()
        .flatten();
        let injected_at = Instant::now();
        let accepted = ui.inject_presentation_fault(
            &state.logical_window_id,
            ailloli_ui_winit::PresentationTestFault::ZeroExtentRoundTrip,
        );
        if !accepted {
            self.accounting
                .resize_zero_injection_failures
                .fetch_add(1, Ordering::Relaxed);
            self.next_resize_zero_sample = self.sampling.total_samples();
            return;
        }
        self.pending_resize_zero_sample = Some(PendingResizeZeroSample {
            phase,
            injected_at,
            zero_extent_count_before: state.zero_extent_count,
            rendered_frame_count_before: state.rendered_frame_count,
            cause_event_id,
        });
        self.next_resize_zero_sample = self.next_resize_zero_sample.saturating_add(1);
    }

    fn service_popup_portal(
        &mut self,
        ui: &mut UiApp<WakeSample>,
        state: &ailloli_ui_winit::PresentationTestState,
    ) {
        if self
            .pending_popup_sample
            .as_ref()
            .is_some_and(|pending| state.rendered_frame_count > pending.rendered_frame_count_before)
        {
            let pending = self
                .pending_popup_sample
                .take()
                .expect("pending popup sample was checked above");
            match pending.stage {
                PendingPopupStage::Opening(popup_id) => {
                    if !ui.runtime().popup_is_open(popup_id) {
                        self.accounting.popup_lost.fetch_add(1, Ordering::Relaxed);
                        self.accounting
                            .popup_round_trips
                            .fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    let mut context = Self::bench_context(state.generation, pending.phase);
                    if let Some(cause_event_id) = pending.cause_event_id {
                        context = context.caused_by(cause_event_id);
                    }
                    let _ = ailloli_ui_bench::try_record(
                        ailloli_ui_bench::Event::Metric {
                            ts_ms: now_ms(),
                            name: "popup_portal.request_to_present_us".to_string(),
                            value: pending.transition_observed_at.elapsed().as_micros() as f64,
                            role: ailloli_ui_bench::MetricRole::GatingSteady,
                        },
                        context,
                    );
                    self.accounting
                        .popup_request_present_samples
                        .fetch_add(1, Ordering::Relaxed);

                    let Some(bounds) = ui.presentation_test_element_bounds(
                        &state.logical_window_id,
                        POPUP_BACKGROUND_BUTTON_KEY,
                    ) else {
                        self.accounting.popup_lost.fetch_add(1, Ordering::Relaxed);
                        self.accounting
                            .popup_round_trips
                            .fetch_add(1, Ordering::Relaxed);
                        return;
                    };
                    let point = Point::new(bounds.x + bounds.w * 0.5, bounds.y + bounds.h * 0.5);
                    self.inject_popup_click(ui, state.generation, point);
                    if ui.runtime().popup_is_open(popup_id)
                        || !Self::retained_select_popups(ui, state).is_empty()
                    {
                        self.accounting
                            .popup_dismiss_failures
                            .fetch_add(1, Ordering::Relaxed);
                        self.accounting
                            .popup_round_trips
                            .fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    let transition_observed_at = Instant::now();
                    let cause_event_id = ailloli_ui_bench::try_record(
                        ailloli_ui_bench::Event::Marker {
                            ts_ms: now_ms(),
                            name: "popup_portal.dismiss_observed".to_string(),
                        },
                        Self::bench_context(state.generation, pending.phase),
                    )
                    .ok()
                    .flatten();
                    self.pending_popup_sample = Some(PendingPopupSample {
                        phase: pending.phase,
                        stage: PendingPopupStage::Dismissing(popup_id),
                        transition_observed_at,
                        rendered_frame_count_before: state.rendered_frame_count,
                        cause_event_id,
                    });
                    ui.request_redraw_all();
                    return;
                }
                PendingPopupStage::Dismissing(popup_id) => {
                    if ui.runtime().popup_is_open(popup_id)
                        || !Self::retained_select_popups(ui, state).is_empty()
                    {
                        self.accounting
                            .popup_dismiss_failures
                            .fetch_add(1, Ordering::Relaxed);
                        self.accounting
                            .popup_round_trips
                            .fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    if ui.presentation_test_focus_within_key(
                        &state.logical_window_id,
                        POPUP_TRIGGER_KEY,
                    ) != Some(true)
                    {
                        self.accounting
                            .popup_focus_restore_failures
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    let mut context = Self::bench_context(state.generation, pending.phase);
                    if let Some(cause_event_id) = pending.cause_event_id {
                        context = context.caused_by(cause_event_id);
                    }
                    let _ = ailloli_ui_bench::try_record(
                        ailloli_ui_bench::Event::Metric {
                            ts_ms: now_ms(),
                            name: "popup_portal.dismiss_to_present_us".to_string(),
                            value: pending.transition_observed_at.elapsed().as_micros() as f64,
                            role: ailloli_ui_bench::MetricRole::GatingSteady,
                        },
                        context,
                    );
                    self.accounting
                        .popup_dismiss_present_samples
                        .fetch_add(1, Ordering::Relaxed);
                    self.accounting
                        .popup_round_trips
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        if self.pending_popup_sample.is_some()
            || self.next_popup_sample >= self.sampling.total_samples()
        {
            return;
        }
        if !popup_native_focus_ready(
            ui.presentation_test_window_has_native_focus(&state.logical_window_id),
        ) {
            return;
        }

        let sample_at = self.sampling.started_at
            + self.sampling.settle
            + self
                .sampling
                .interval()
                .saturating_mul(self.next_popup_sample + 1);
        if Instant::now() < sample_at {
            return;
        }
        let Some(bounds) =
            ui.presentation_test_element_bounds(&state.logical_window_id, POPUP_TRIGGER_KEY)
        else {
            return;
        };
        let phase = if self.next_popup_sample < self.sampling.warmup_samples {
            ailloli_ui_bench::SamplePhase::Warmup
        } else {
            ailloli_ui_bench::SamplePhase::Measured
        };
        let point = Point::new(bounds.x + bounds.w * 0.5, bounds.y + bounds.h * 0.5);
        self.inject_popup_click(ui, state.generation, point);
        let open_popups = Self::retained_select_popups(ui, state);
        if open_popups.is_empty() {
            self.accounting.popup_lost.fetch_add(1, Ordering::Relaxed);
            self.accounting
                .popup_round_trips
                .fetch_add(1, Ordering::Relaxed);
            self.next_popup_sample = self.next_popup_sample.saturating_add(1);
            return;
        }
        self.accounting.popup_duplicate.fetch_add(
            open_popups.len().saturating_sub(1) as u64,
            Ordering::Relaxed,
        );
        let popup_id = *open_popups
            .last()
            .expect("non-empty retained popup list was checked above");
        let transition_observed_at = Instant::now();
        let cause_event_id = ailloli_ui_bench::try_record(
            ailloli_ui_bench::Event::Marker {
                ts_ms: now_ms(),
                name: "popup_portal.request_observed".to_string(),
            },
            Self::bench_context(state.generation, phase),
        )
        .ok()
        .flatten();
        self.pending_popup_sample = Some(PendingPopupSample {
            phase,
            stage: PendingPopupStage::Opening(popup_id),
            transition_observed_at,
            rendered_frame_count_before: state.rendered_frame_count,
            cause_event_id,
        });
        self.next_popup_sample = self.next_popup_sample.saturating_add(1);
        ui.request_redraw_all();
    }

    fn retained_select_popups(
        ui: &UiApp<WakeSample>,
        state: &ailloli_ui_winit::PresentationTestState,
    ) -> Vec<PopupId> {
        let portal = ui.runtime().popup_portal();
        let portal = portal.borrow();
        portal
            .open_ids()
            .filter(|popup_id| {
                portal.request(*popup_id).is_some_and(|request| {
                    request
                        .owner()
                        .belongs_to(&state.logical_window_id, state.generation)
                        && request.parent().is_none()
                        && request.semantics().role() == PopupRole::Listbox
                        && request.mount_policy() == PopupMountPolicy::RetainedOverlay
                })
            })
            .collect()
    }

    fn bench_context(
        generation: PresentationGeneration,
        phase: ailloli_ui_bench::SamplePhase,
    ) -> ailloli_ui_bench::EventContext {
        ailloli_ui_bench::EventContext::default()
            .with_window(ailloli_ui_bench::BenchWindowId::new(MAIN_WINDOW_ID))
            .with_surface(
                ailloli_ui_bench::BenchSurfaceId::new(MAIN_WINDOW_ID),
                generation.get(),
            )
            .with_sample_phase(phase)
    }

    fn inject(
        &mut self,
        ui: &mut UiApp<WakeSample>,
        generation: PresentationGeneration,
        event: Event,
    ) {
        let event_id = self.next_event_id;
        self.next_event_id = self.next_event_id.saturating_add(1);
        let envelope = EventEnvelope::new(
            EventMeta::new(
                EventId::new(event_id),
                EventTimestamp::new(Duration::from_micros(event_id)),
                MAIN_WINDOW_ID,
                generation,
            ),
            event,
        );
        if !ui.inject_event_envelope(envelope) {
            self.accounting
                .input_rejected_events
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    fn inject_popup_click(
        &mut self,
        ui: &mut UiApp<WakeSample>,
        generation: PresentationGeneration,
        position: Point,
    ) {
        for pressed in [true, false] {
            let event_id = self.next_event_id;
            self.next_event_id = self.next_event_id.saturating_add(1);
            let envelope = EventEnvelope::new(
                EventMeta::new(
                    EventId::new(event_id),
                    EventTimestamp::new(Duration::from_micros(event_id)),
                    MAIN_WINDOW_ID,
                    generation,
                ),
                Event::Pointer(PointerEvent::button(
                    position,
                    PointerButton::Left,
                    pressed,
                    Modifiers::default(),
                )),
            );
            if !ui.inject_event_envelope(envelope) {
                self.accounting
                    .popup_rejected_events
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(feature = "test-support")]
fn queue_surface_recovery_faults(ui: &mut UiApp<WakeSample>) -> Result<(), HarnessError> {
    let logical_window_id = LogicalWindowId::new(MAIN_WINDOW_ID);
    for fault in [
        ailloli_ui_winit::PresentationTestFault::Lost,
        ailloli_ui_winit::PresentationTestFault::Outdated,
    ] {
        if !ui.inject_presentation_fault(&logical_window_id, fault) {
            return Err(HarnessError(
                "surface recovery fault target was not registered".to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(feature = "test-support")]
fn record_surface_recovery_correctness(ui: &UiApp<WakeSample>) -> Result<(), HarnessError> {
    let logical_window_id = LogicalWindowId::new(MAIN_WINDOW_ID);
    let state = ui
        .presentation_test_state(&logical_window_id)
        .ok_or_else(|| HarnessError("surface recovery state is unavailable".to_string()))?;
    record_correctness(
        "correctness.surface_lost_count_mismatch",
        state.lost_count.abs_diff(1) as f64,
    );
    record_correctness(
        "correctness.surface_outdated_count_mismatch",
        state.outdated_count.abs_diff(1) as f64,
    );
    record_correctness(
        "correctness.surface_recovery_missing",
        2_u64.saturating_sub(state.recovery_count) as f64,
    );
    record_correctness(
        "correctness.surface_detach_missing",
        2_u64.saturating_sub(state.detach_count) as f64,
    );
    record_correctness(
        "correctness.surface_pending_fault",
        state.pending_fault_count as f64,
    );
    record_correctness(
        "correctness.surface_not_attached",
        u8::from(!state.attached) as f64,
    );
    Ok(())
}

#[cfg(feature = "test-support")]
fn record_resize_zero_correctness(
    ui: &UiApp<WakeSample>,
    expected_round_trips: u32,
) -> Result<(), HarnessError> {
    let logical_window_id = LogicalWindowId::new(MAIN_WINDOW_ID);
    let state = ui
        .presentation_test_state(&logical_window_id)
        .ok_or_else(|| HarnessError("zero-extent presentation state is unavailable".to_string()))?;
    record_correctness(
        "correctness.resize_zero_fault_count_mismatch",
        u64::from(expected_round_trips).abs_diff(state.zero_extent_count) as f64,
    );
    record_correctness(
        "correctness.resize_zero_pending_fault",
        state.pending_fault_count as f64,
    );
    record_correctness(
        "correctness.resize_zero_not_ready",
        u8::from(state.state != ailloli_ui_runtime::app::PresentationState::Ready) as f64,
    );
    Ok(())
}

fn run_harness(
    scenario: Scenario,
    config: &ailloli_ui_bench::BenchConfig,
    started_at: Instant,
) -> Result<(), Box<dyn Error>> {
    validate_scenario_capability(scenario)?;
    let duration = Duration::from_millis(u64::from(config.duration_ms));
    let settle = if scenario == Scenario::SurfaceRecovery && cfg!(feature = "test-support") {
        (duration / 3)
            .min(Duration::from_secs(4))
            .max(Duration::from_millis(20))
    } else {
        (duration / 5)
            .min(Duration::from_secs(1))
            .max(Duration::from_millis(20))
    };
    let warmup_samples = sample_count("WARMUP_SAMPLES", DEFAULT_WARMUPS);
    let measured_samples = sample_count("MEASURED_SAMPLES", DEFAULT_MEASURED_SAMPLES).max(1);
    validate_sampling_contract(scenario, warmup_samples, measured_samples)?;
    let sampling = SamplingPlan {
        started_at,
        duration,
        settle,
        warmup_samples,
        measured_samples,
    };

    let requested_backend = requested_backend();
    let (event_loop, actual_backend) = create_event_loop(&requested_backend)?;
    update_harness_metadata(
        scenario,
        &requested_backend,
        &actual_backend,
        duration,
        warmup_samples,
        measured_samples,
    )?;

    let (sender, inbox) = RuntimeInbox::channel(
        NonZeroUsize::new(MAILBOX_CAPACITY).expect("mailbox capacity is non-zero"),
    );
    inbox.install_wake(Arc::new(EventLoopWake(event_loop.create_proxy())))?;

    let accounting = Arc::new(HarnessAccounting::default());
    let ready_at = Arc::new(OnceLock::new());
    let first_present = Arc::new(OnceLock::new());
    let stop = Arc::new(AtomicBool::new(false));
    let producer = spawn_wake_producer(
        scenario,
        sampling,
        sender.clone(),
        Arc::clone(&accounting),
        Arc::clone(&ready_at),
        Arc::clone(&stop),
    );

    let size = Size::new(config.window_w as f32, config.window_h as f32);
    let mut ui = UiApp::new().window_with_clear(
        window_options(MAIN_WINDOW_ID, "Ailloli UI winit regression", size),
        Color::rgb(24, 26, 34),
        root_view(scenario, "main"),
    );
    if scenario == Scenario::MultiWindow {
        ui = ui.window_with_clear(
            window_options("secondary", "Ailloli UI secondary", size),
            Color::rgb(18, 20, 28),
            root_view(scenario, "secondary"),
        );
    }
    #[cfg(feature = "test-support")]
    if scenario == Scenario::SurfaceRecovery {
        queue_surface_recovery_faults(&mut ui)?;
    }

    let driver = RegressionDriver::new(
        scenario,
        sampling,
        sender,
        Arc::clone(&accounting),
        ready_at,
        Arc::clone(&first_present),
    );
    let mut host = WinitHost::new(ui, driver).runtime_inbox(inbox);
    #[cfg(feature = "test-support")]
    {
        let mut probe =
            NativeHarnessProbe::new(scenario, sampling, first_present, Arc::clone(&accounting));
        host = host.test_service(move |ui| probe.service(ui));
    }
    let loop_result = run_app_on_event_loop(event_loop, &mut host, ControlFlow::Wait);
    stop.store(true, Ordering::Release);
    if let Some(producer) = producer {
        producer
            .join()
            .map_err(|_| HarnessError("wake producer thread panicked".to_string()))?;
    }

    loop_result?;
    if let Some(error) = host.take_error() {
        return Err(Box::new(error));
    }
    if let Some(error) = host.take_inbox_wake_error() {
        return Err(Box::new(error));
    }

    host.driver().finish_metrics();
    #[cfg(feature = "test-support")]
    if scenario == Scenario::SurfaceRecovery {
        record_surface_recovery_correctness(host.ui())?;
    }
    #[cfg(feature = "test-support")]
    if scenario == Scenario::ResizeZero {
        record_resize_zero_correctness(host.ui(), sampling.total_samples())?;
    }
    let expected_windows = scenario.logical_window_count();
    let actual_windows = host.ui().window_snapshots().len();
    record_correctness(
        "correctness.window_count_mismatch",
        expected_windows.abs_diff(actual_windows) as f64,
    );
    eprintln!(
        "winit regression bench: scenario={scenario:?} backend={actual_backend} duration_ms={} fidelity={}",
        duration.as_millis(),
        scenario.fidelity()
    );
    Ok(())
}

fn execute() -> Result<(), Box<dyn Error>> {
    let started_at = Instant::now();
    let config = ailloli_ui_bench::config_from_env();
    let scenario = Scenario::parse(&config.scenario)?;
    let default_path = default_bench_path(&config.scenario);
    let bench = try_init_ailloli_ui_bench_from_env(&default_path.to_string_lossy())?;
    let run_result = run_harness(scenario, &config, started_at);
    let finish_result = bench.finish();

    match run_result {
        Err(error) => {
            if let Err(finish_error) = finish_result {
                eprintln!("winit benchmark finalization also failed: {finish_error}");
            }
            Err(error)
        }
        Ok(()) => {
            if let Some(completed) = finish_result? {
                eprintln!(
                    "published benchmark run {} ({})",
                    completed.path.display(),
                    completed.sha256
                );
            }
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    match execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("winit regression bench failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_public_scenario_name_is_accepted() {
        for name in [
            "startup",
            "idle",
            "wake_single",
            "wake_burst",
            "resize_zero",
            "surface_recovery",
            "multi_window",
            "input_ime",
            "popup_portal",
        ] {
            assert!(Scenario::parse(name).is_ok(), "scenario {name}");
        }
        assert!(Scenario::parse("unknown").is_err());
    }

    #[test]
    fn fidelity_tracks_test_support_injection_capabilities() {
        if cfg!(feature = "test-support") {
            assert!(Scenario::Startup.fidelity().starts_with("full:"));
            assert!(Scenario::ResizeZero.fidelity().starts_with("full:"));
            assert!(Scenario::SurfaceRecovery.fidelity().starts_with("full:"));
            assert!(Scenario::InputIme.fidelity().starts_with("full:"));
            assert!(Scenario::PopupPortal.fidelity().starts_with("full:"));
            assert!(Scenario::Startup.gate_ready());
            assert!(Scenario::ResizeZero.gate_ready());
            assert!(Scenario::InputIme.gate_ready());
            assert!(Scenario::PopupPortal.gate_ready());
        } else {
            assert!(Scenario::Startup.fidelity().starts_with("blocked:"));
            assert!(Scenario::ResizeZero.fidelity().starts_with("blocked:"));
            assert!(Scenario::SurfaceRecovery.fidelity().starts_with("partial:"));
            assert!(Scenario::InputIme.fidelity().starts_with("blocked:"));
            assert!(Scenario::PopupPortal.fidelity().starts_with("blocked:"));
            assert!(!Scenario::Startup.gate_ready());
            assert!(!Scenario::ResizeZero.gate_ready());
            assert!(!Scenario::InputIme.gate_ready());
            assert!(!Scenario::PopupPortal.gate_ready());
        }
        assert!(Scenario::WakeSingle.fidelity().starts_with("full:"));
        assert!(Scenario::WakeSingle.gate_ready());
    }

    #[test]
    fn only_faithful_periodic_workloads_are_gating() {
        assert_eq!(
            Scenario::Idle.periodic_metric_role(),
            ailloli_ui_bench::MetricRole::GatingSteady
        );
        assert_eq!(
            Scenario::ResizeZero.periodic_metric_role(),
            ailloli_ui_bench::MetricRole::Diagnostic
        );
        assert_eq!(
            Scenario::InputIme.periodic_metric_role(),
            ailloli_ui_bench::MetricRole::Diagnostic
        );
        assert_eq!(
            Scenario::PopupPortal.periodic_metric_role(),
            ailloli_ui_bench::MetricRole::Diagnostic
        );
    }

    #[test]
    fn popup_sampling_contract_requires_three_warmups_and_thirty_measurements() {
        assert!(validate_sampling_contract(
            Scenario::PopupPortal,
            DEFAULT_WARMUPS,
            DEFAULT_MEASURED_SAMPLES,
        )
        .is_ok());
        assert!(validate_sampling_contract(
            Scenario::PopupPortal,
            DEFAULT_WARMUPS,
            DEFAULT_MEASURED_SAMPLES + 1,
        )
        .is_ok());
        assert!(validate_sampling_contract(
            Scenario::PopupPortal,
            DEFAULT_WARMUPS - 1,
            DEFAULT_MEASURED_SAMPLES,
        )
        .is_err());
        assert!(validate_sampling_contract(
            Scenario::PopupPortal,
            DEFAULT_WARMUPS,
            DEFAULT_MEASURED_SAMPLES - 1,
        )
        .is_err());
        assert!(validate_sampling_contract(Scenario::Idle, 0, 1).is_ok());
    }

    #[test]
    fn popup_scenario_requires_real_test_support_injection() {
        assert_eq!(
            validate_scenario_capability(Scenario::PopupPortal).is_ok(),
            cfg!(feature = "test-support")
        );
        assert!(validate_scenario_capability(Scenario::Idle).is_ok());
    }

    #[cfg(feature = "test-support")]
    #[test]
    fn popup_sampling_waits_for_observable_native_focus() {
        assert!(!popup_native_focus_ready(None));
        assert!(!popup_native_focus_ready(Some(false)));
        assert!(popup_native_focus_ready(Some(true)));
    }
}
