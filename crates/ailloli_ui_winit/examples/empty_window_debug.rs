use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ailloli_ui_core::Color;
use ailloli_ui_render_wgpu::{Renderer, ResizeOutcome};
use ailloli_ui_winit::{create_window, new_event_loop, run_app_on_event_loop, WindowOptions};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

struct EmptyWindowDebug {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pending_resize: Option<PhysicalSize<u32>>,
    resize_retry_at: Option<Instant>,
    rendered_frames: u64,
    log_level: LogLevel,
}

const RESIZE_RETRY_DELAY: Duration = Duration::from_millis(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LogLevel {
    None,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "none" | "off" => Some(Self::None),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }

    fn allows(self, level: Self) -> bool {
        self != Self::None && level <= self
    }
}

impl EmptyWindowDebug {
    fn new(log_level: LogLevel) -> Self {
        Self {
            window: None,
            renderer: None,
            pending_resize: None,
            resize_retry_at: None,
            rendered_frames: 0,
            log_level,
        }
    }

    fn log(&self, level: LogLevel, message: fmt::Arguments<'_>) {
        log_message(self.log_level, level, message);
    }

    fn request_redraw(&self) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

impl ApplicationHandler for EmptyWindowDebug {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.log(LogLevel::Info, format_args!("resumed"));
        log_environment(self.log_level);

        let window = match create_window(
            event_loop,
            WindowOptions {
                title: "Ailloli UI empty window debug".into(),
                inner_size: Some(LogicalSize::new(720.0, 420.0)),
                ..Default::default()
            },
        ) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("empty_window_debug: create window failed: {err}");
                event_loop.exit();
                return;
            }
        };

        log_window(self.log_level, "created", window.as_ref());
        let renderer = match ailloli_ui_winit::renderer_from_window(window.clone()) {
            Ok(renderer) => renderer,
            Err(err) => {
                eprintln!("empty_window_debug: renderer failed: {err}");
                event_loop.exit();
                return;
            }
        };
        log_renderer(self.log_level, "created", &renderer);

        self.renderer = Some(renderer);
        self.window = Some(window);
        self.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.as_ref().cloned() else {
            return;
        };
        if window.id() != id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                self.log(LogLevel::Info, format_args!("close requested"));
                event_loop.exit();
            }
            WindowEvent::Focused(focused) => {
                self.log(LogLevel::Debug, format_args!("focused={focused}"));
            }
            WindowEvent::Moved(position) => {
                self.log(
                    LogLevel::Debug,
                    format_args!("moved x={} y={}", position.x, position.y),
                );
            }
            WindowEvent::Resized(size) => {
                self.log(
                    LogLevel::Debug,
                    format_args!("resized event physical={}x{}", size.width, size.height),
                );
                ailloli_ui_bench::record(ailloli_ui_bench::Event::ResizePending {
                    ts_ms: now_ms(),
                    w: size.width,
                    h: size.height,
                });
                self.pending_resize = Some(size);
                self.resize_retry_at = None;
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let size = window.inner_size();
                self.log(
                    LogLevel::Debug,
                    format_args!(
                        "scale_factor_changed scale={} size={}x{}",
                        scale_factor, size.width, size.height
                    ),
                );
                self.pending_resize = Some(size);
                self.resize_retry_at = None;
                window.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match &event.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        self.log(LogLevel::Info, format_args!("escape -> exit"));
                        event_loop.exit();
                    }
                    Key::Named(NamedKey::F11) => {
                        toggle_fullscreen(self.log_level, window.as_ref());
                        window.request_redraw();
                    }
                    Key::Character(ch) if ch.as_str().eq_ignore_ascii_case("m") => {
                        let next = !window.is_maximized();
                        self.log(LogLevel::Info, format_args!("set_maximized({next})"));
                        window.set_maximized(next);
                        window.request_redraw();
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                let mut skip_render = false;
                let resize_is_ready = self
                    .resize_retry_at
                    .is_none_or(|ready_at| ready_at <= Instant::now());
                if self.pending_resize.is_some() && !resize_is_ready {
                    skip_render = true;
                } else if let Some(size) = self.pending_resize.take() {
                    let current_size = window.inner_size();
                    let size = if current_size.width == 0 || current_size.height == 0 {
                        size
                    } else {
                        current_size
                    };
                    let start = Instant::now();
                    let Some(renderer) = self.renderer.as_mut() else {
                        return;
                    };
                    match renderer.try_resize(ailloli_ui_render_wgpu::PhysicalExtent::new(
                        size.width,
                        size.height,
                    )) {
                        Ok(ResizeOutcome::Deferred(reason)) => {
                            log_message(
                                self.log_level,
                                LogLevel::Debug,
                                format_args!("resize deferred: {}", reason.as_str()),
                            );
                            self.pending_resize = Some(size);
                            self.resize_retry_at = Some(Instant::now() + RESIZE_RETRY_DELAY);
                            skip_render = true;
                        }
                        Ok(ResizeOutcome::SkippedZero) => {
                            log_message(
                                self.log_level,
                                LogLevel::Debug,
                                format_args!("resize skipped: zero-sized surface"),
                            );
                            self.resize_retry_at = None;
                            skip_render = true;
                        }
                        Ok(outcome) => {
                            log_message(
                                self.log_level,
                                LogLevel::Debug,
                                format_args!("resize outcome={outcome:?}"),
                            );
                            ailloli_ui_bench::record(ailloli_ui_bench::Event::ResizeApply {
                                ts_ms: now_ms(),
                                w: size.width,
                                h: size.height,
                                dur_us: start.elapsed().as_micros(),
                            });
                            self.resize_retry_at = None;
                            log_renderer(self.log_level, "after_resize", renderer);
                        }
                        Err(err) => {
                            eprintln!("empty_window_debug: resize failed: {err}");
                            event_loop.exit();
                            return;
                        }
                    }
                }

                if skip_render {
                    self.log(
                        LogLevel::Trace,
                        format_args!("render skipped while resize is deferred"),
                    );
                    return;
                }

                let surface_deferred_reason = self
                    .renderer
                    .as_ref()
                    .and_then(|renderer| renderer.surface_config_deferred_reason());
                if let Some(reason) = surface_deferred_reason {
                    self.log(
                        LogLevel::Debug,
                        format_args!(
                            "render skipped while surface is not ready: {}",
                            reason.as_str()
                        ),
                    );
                    self.pending_resize = Some(window.inner_size());
                    self.resize_retry_at = Some(Instant::now() + RESIZE_RETRY_DELAY);
                    return;
                }

                let render_start = Instant::now();
                let Some(renderer) = self.renderer.as_mut() else {
                    return;
                };
                match renderer.render(Color::new(0.025, 0.030, 0.035, 1.0), &[]) {
                    Ok(()) => {
                        self.rendered_frames += 1;
                        log_message(
                            self.log_level,
                            LogLevel::Trace,
                            format_args!(
                                "frame={} render_us={}",
                                self.rendered_frames,
                                render_start.elapsed().as_micros()
                            ),
                        );
                    }
                    Err(err) => {
                        eprintln!("empty_window_debug: render failed: {err}");
                        event_loop.exit();
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if let Some(ready_at) = self.resize_retry_at {
            ailloli_ui_bench::record(ailloli_ui_bench::Event::AboutToWaitRedraw {
                ts_ms: now_ms(),
                awaiting_resize: self.pending_resize.is_some(),
            });
            if ready_at <= now {
                self.resize_retry_at = None;
                self.request_redraw();
                event_loop.set_control_flow(ControlFlow::Wait);
            } else {
                event_loop.set_control_flow(ControlFlow::WaitUntil(ready_at));
            }
        } else if self.pending_resize.is_some() {
            ailloli_ui_bench::record(ailloli_ui_bench::Event::AboutToWaitRedraw {
                ts_ms: now_ms(),
                awaiting_resize: true,
            });
            self.request_redraw();
            event_loop.set_control_flow(ControlFlow::Wait);
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

fn toggle_fullscreen(log_level: LogLevel, window: &Window) {
    if window.fullscreen().is_some() {
        log_message(log_level, LogLevel::Info, format_args!("fullscreen off"));
        window.set_fullscreen(None);
    } else {
        log_message(
            log_level,
            LogLevel::Info,
            format_args!("fullscreen borderless"),
        );
        window.set_fullscreen(Some(Fullscreen::Borderless(window.current_monitor())));
    }
}

fn log_environment(log_level: LogLevel) {
    log_message(
        log_level,
        LogLevel::Info,
        format_args!(
            "env WINIT_UNIX_BACKEND={:?} WAYLAND_DISPLAY={:?} DISPLAY={:?} WGPU_BACKEND={:?}",
            std::env::var("WINIT_UNIX_BACKEND").ok(),
            std::env::var("WAYLAND_DISPLAY").ok(),
            std::env::var("DISPLAY").ok(),
            std::env::var("WGPU_BACKEND").ok()
        ),
    );
}

fn log_window(log_level: LogLevel, prefix: &str, window: &Window) {
    let size = window.inner_size();
    let monitor = window.current_monitor();
    log_message(
        log_level,
        LogLevel::Info,
        format_args!(
            "{prefix} window_id={:?} size={}x{} scale={} maximized={} fullscreen={}",
            window.id(),
            size.width,
            size.height,
            window.scale_factor(),
            window.is_maximized(),
            window.fullscreen().is_some()
        ),
    );
    if let Some(monitor) = monitor {
        let size = monitor.size();
        log_message(
            log_level,
            LogLevel::Info,
            format_args!(
                "{prefix} monitor name={:?} size={}x{} scale={}",
                monitor.name(),
                size.width,
                size.height,
                monitor.scale_factor()
            ),
        );
    }
}

fn log_renderer(log_level: LogLevel, prefix: &str, renderer: &Renderer) {
    let adapter = renderer.adapter_info();
    let caps = renderer.surface_capabilities();
    let config = renderer.surface_config();
    log_message(
        log_level,
        LogLevel::Debug,
        format_args!(
            "{prefix} adapter name={:?} backend={:?} device_type={:?}",
            adapter.name, adapter.backend, adapter.device_type
        ),
    );
    log_message(
        log_level,
        LogLevel::Debug,
        format_args!(
            "{prefix} caps formats={:?} present_modes={:?} alpha_modes={:?} usages={:?}",
            caps.formats, caps.present_modes, caps.alpha_modes, caps.usages
        ),
    );
    log_message(
        log_level,
        LogLevel::Debug,
        format_args!(
            "{prefix} config size={}x{} format={:?} present_mode={:?} alpha_mode={:?} usage={:?} frame_latency={}",
            config.width,
            config.height,
            config.format,
            config.present_mode,
            config.alpha_mode,
            config.usage,
            config.desired_maximum_frame_latency
        ),
    );
}

fn log_message(active: LogLevel, level: LogLevel, message: fmt::Arguments<'_>) {
    if active.allows(level) {
        println!("empty_window_debug: {message}");
    }
}

fn parse_log_level_arg() -> LogLevel {
    let mut args = std::env::args().skip(1);
    let mut log_level = LogLevel::Debug;

    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            print_usage_and_exit(0);
        }

        let value = if let Some(value) = arg.strip_prefix("--log-level=") {
            value.to_owned()
        } else if arg == "--log-level" {
            match args.next() {
                Some(value) => value,
                None => {
                    eprintln!("empty_window_debug: --log-level requires a value");
                    print_usage_and_exit(2);
                }
            }
        } else {
            eprintln!("empty_window_debug: unknown argument `{arg}`");
            print_usage_and_exit(2);
        };

        match LogLevel::parse(&value) {
            Some(level) => log_level = level,
            None => {
                eprintln!(
                    "empty_window_debug: invalid log level `{value}`; expected none, info, debug, or trace"
                );
                print_usage_and_exit(2);
            }
        }
    }

    log_level
}

fn print_usage_and_exit(code: i32) -> ! {
    eprintln!(
        "usage: cargo run -p ailloli_ui_winit --example empty_window_debug -- [--log-level none|info|debug|trace]"
    );
    std::process::exit(code);
}

fn now_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn main() -> Result<(), Box<dyn Error>> {
    let log_level = parse_log_level_arg();

    let bench = ailloli_ui_bench::try_init_from_env("artifacts/empty_window_debug.jsonl")?;
    if let Some(path) = bench.path() {
        log_message(
            log_level,
            LogLevel::Info,
            format_args!("ailloli_ui_bench_path={}", path.display()),
        );
    }

    let event_loop = new_event_loop()?;
    let mut app = EmptyWindowDebug::new(log_level);
    let run_result = run_app_on_event_loop(event_loop, &mut app, ControlFlow::Wait);
    let finish_result = bench.finish();
    run_result?;
    finish_result?;
    Ok(())
}
