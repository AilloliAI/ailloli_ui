#[cfg(feature = "openxr")]
fn main() {
    if let Err(error) = run() {
        eprintln!("[ailloli_ui-xr-smoke] error={error}");
        std::process::exit(1);
    }
}

#[cfg(not(feature = "openxr"))]
fn main() {
    eprintln!(
        "xr_ui_smoke requires: cargo run -p ailloli_ui_openxr --example xr_ui_smoke --features openxr"
    );
    std::process::exit(1);
}

#[cfg(feature = "openxr")]
#[derive(Debug, Clone, Copy)]
enum SmokeAction {
    Primary,
    Secondary,
    Affordance(ailloli_ui_widgets::chrome::WindowAffordanceKind),
    Exit,
}

#[cfg(feature = "openxr")]
#[derive(Debug, Clone)]
struct SmokeArgs {
    prefer_left: bool,
    hands: bool,
    distance_m: f32,
    scale: f32,
    timeout_sec: Option<f32>,
    log_path: Option<std::path::PathBuf>,
    affordance_demo: bool,
    panel_facing: ailloli_ui_openxr::OpenXrPanelFacingOptions,
}

#[cfg(feature = "openxr")]
impl Default for SmokeArgs {
    fn default() -> Self {
        Self {
            prefer_left: false,
            hands: true,
            distance_m: 2.0,
            scale: 1.0,
            timeout_sec: None,
            log_path: None,
            affordance_demo: false,
            panel_facing: ailloli_ui_openxr::OpenXrPanelFacingOptions::default(),
        }
    }
}

#[cfg(feature = "openxr")]
fn run() -> Result<(), String> {
    use std::time::{Duration, Instant};

    use ailloli_ui_core::math::Scale;
    use ailloli_ui_openxr::{
        OpenXrPointerSelectionPolicy, OpenXrRuntime, OpenXrRuntimeOptions, OpenXrUiFrameLoopOptions,
    };
    use ailloli_ui_runtime::app::RuntimeHandle;

    let args = parse_args()?;
    let mut logger = SmokeLogger::new(args.log_path.as_deref())?;
    print_startup(&mut logger, &args);

    let mut runtime = match OpenXrRuntime::new(OpenXrRuntimeOptions::default()) {
        Ok(runtime) => runtime,
        Err(error) => {
            let error = error.to_string();
            logger.log(format!("[ailloli_ui-xr-smoke] error={error}"));
            return Err(error);
        }
    };
    logger.log(format!(
        "[ailloli_ui-xr-smoke] runtime=initialized hand_tracking_supported={} hand_aim_supported={}",
        runtime.xr.hand_tracking_supported, runtime.xr.hand_aim_supported
    ));

    let mut options = OpenXrUiFrameLoopOptions {
        scale: Scale::new(args.scale),
        ..OpenXrUiFrameLoopOptions::default()
    };
    options.layer.pose.position.z = -args.distance_m.abs();
    options.input.hands = args.hands;
    if args.prefer_left {
        options.input.pointer_selection = OpenXrPointerSelectionPolicy::PreferLeftController;
    }

    let runtime_handle = RuntimeHandle::<SmokeAction>::new();
    let action_handle = runtime_handle.clone();
    let timeout = args.timeout_sec.map(Duration::from_secs_f32);
    let started_at = Instant::now();
    let mut primary_count = 0_u32;
    let mut secondary_count = 0_u32;
    let mut exit_requested = false;

    runtime
        .run_ailloli_ui_frame_loop(options, runtime_handle, smoke_view(args), move || {
            for action in action_handle.take_actions() {
                match action {
                    SmokeAction::Primary => {
                        primary_count = primary_count.saturating_add(1);
                        logger.log(format!(
                            "[ailloli_ui-xr-smoke] action=primary count={primary_count}"
                        ));
                    }
                    SmokeAction::Secondary => {
                        secondary_count = secondary_count.saturating_add(1);
                        logger.log(format!(
                            "[ailloli_ui-xr-smoke] action=secondary count={secondary_count}"
                        ));
                    }
                    SmokeAction::Affordance(kind) => {
                        logger.log(format!(
                            "[ailloli_ui-xr-smoke] action=affordance kind={kind:?}"
                        ));
                    }
                    SmokeAction::Exit => {
                        logger.log("[ailloli_ui-xr-smoke] action=exit");
                        exit_requested = true;
                    }
                }
            }

            if let Some(timeout) = timeout {
                if started_at.elapsed() >= timeout {
                    logger.log(format!(
                        "[ailloli_ui-xr-smoke] action=timeout elapsed_sec={:.1}",
                        started_at.elapsed().as_secs_f32()
                    ));
                    return true;
                }
            }

            exit_requested
        })
        .map_err(|error| error.to_string())
}

#[cfg(feature = "openxr")]
fn smoke_view(args: SmokeArgs) -> ailloli_ui_runtime::component::View<SmokeAction> {
    use ailloli_ui_core::{Color, FontId, TextStyle};
    use ailloli_ui_runtime::component::{IntoView, IntoViewKeyExt};
    use ailloli_ui_widgets::chrome::{WindowAffordanceFrame, WindowAffordanceKind};
    use ailloli_ui_widgets::controls::Button;
    use ailloli_ui_widgets::layout::{Column, Container, Row, ScrollView};
    use ailloli_ui_widgets::text::Text;

    if args.affordance_demo {
        let title = TextStyle::new(FontId::Ui, 24, Color::rgb(245, 248, 255));
        let body = TextStyle::new(FontId::Ui, 15, Color::rgb(218, 226, 238));
        let dim = TextStyle::new(FontId::Ui, 13, Color::rgb(157, 173, 196));
        let accent = TextStyle::new(FontId::Ui, 14, Color::rgb(126, 231, 215));

        return Container::<SmokeAction>::new()
            .fill()
            .background(Color::rgb(8, 13, 24))
            .padding(36.0)
            .child(
                WindowAffordanceFrame::<SmokeAction>::new("XR Window Affordances")
                    .logical_window_id("ui-xr17-xr-smoke")
                    .width(860.0)
                    .height(430.0)
                    .on_affordance(|event| match event.kind {
                        WindowAffordanceKind::Close => SmokeAction::Exit,
                        kind => SmokeAction::Affordance(kind),
                    })
                    .content(
                        Container::<SmokeAction>::new()
                            .fill()
                            .background(Color::rgb(15, 23, 42))
                            .padding(18.0)
                            .child(
                                Column::<SmokeAction>::new()
                                    .fill()
                                    .gap(12.0)
                                    .child(Text::new("Framework slate contract").style(title))
                                    .child(
                                        Text::new(format!(
                                            "scale={:.2} distance={:.2}m pointer={} hands={} facing={:?}",
                                            args.scale,
                                            args.distance_m,
                                            if args.prefer_left { "left" } else { "right" },
                                            if args.hands { "enabled" } else { "disabled" },
                                            args.panel_facing.mode
                                        ))
                                        .style(dim),
                                    )
                                    .child(Text::new("Move from the titlebar, resize from edges/corners, and click inner controls normally.").style(body))
                                    .child(
                                        Row::<SmokeAction>::new()
                                            .gap(12.0)
                                            .child(
                                                Button::<SmokeAction>::with_label("Primary")
                                                    .on_click(SmokeAction::Primary)
                                                    .width(160.0),
                                            )
                                            .child(
                                                Button::<SmokeAction>::with_label("Secondary")
                                                    .on_click(SmokeAction::Secondary)
                                                    .width(180.0),
                                            )
                                            .child(
                                                Button::<SmokeAction>::with_label("Exit")
                                                    .on_click(SmokeAction::Exit)
                                                    .width(120.0),
                                            ),
                                    )
                                    .child(Text::new("Validation points").style(accent))
                                    .child(
                                        Container::<SmokeAction>::new()
                                            .fill_width()
                                            .height(155.0)
                                            .background(Color::rgb(11, 18, 32))
                                            .padding(12.0)
                                            .child(
                                                ScrollView::<SmokeAction>::vertical().child(
                                                    Column::<SmokeAction>::new()
                                                        .gap(8.0)
                                                        .child(Text::new("1. Rounded surface, border and shadow are visible").style(body))
                                                        .child(Text::new("2. Titlebar and resize handles produce affordance logs").style(body))
                                                        .child(Text::new("3. Inner buttons remain interactive").style(body))
                                                        .child(Text::new("4. Close chrome exits the smoke loop").style(body)),
                                                ),
                                            ),
                                    ),
                            ),
                    ),
            )
            .key("ui-xr17-window-affordances-smoke")
            .into_view();
    }

    let title = TextStyle::new(FontId::Ui, 34, Color::rgb(245, 248, 255));
    let body = TextStyle::new(FontId::Ui, 17, Color::rgb(218, 226, 238));
    let dim = TextStyle::new(FontId::Ui, 14, Color::rgb(157, 173, 196));
    let accent = TextStyle::new(FontId::Ui, 16, Color::rgb(126, 231, 215));

    Container::<SmokeAction>::new()
        .fill()
        .background(Color::rgb(10, 16, 28))
        .padding(28.0)
        .child(
            Column::<SmokeAction>::new()
                .fill()
                .gap(14.0)
                .child(Text::new("Ailloli UI XR Smoke").style(title))
                .child(
                    Text::new(format!(
                        "scale={:.2} distance={:.2}m pointer={} hands={} facing={:?}",
                        args.scale,
                        args.distance_m,
                        if args.prefer_left { "left" } else { "right" },
                        if args.hands { "enabled" } else { "disabled" },
                        args.panel_facing.mode
                    ))
                    .style(dim),
                )
                .child(Text::new("Visual: quad centered, text readable, buttons filled.").style(body))
                .child(Text::new("Interaction: hover a button, trigger click, thumbstick scroll.").style(body))
                .child(
                    Row::<SmokeAction>::new()
                        .gap(12.0)
                        .child(
                            Button::<SmokeAction>::with_label("Primary")
                                .on_click(SmokeAction::Primary)
                                .width(180.0),
                        )
                        .child(
                            Button::<SmokeAction>::with_label("Secondary")
                                .on_click(SmokeAction::Secondary)
                                .width(180.0),
                        )
                        .child(
                            Button::<SmokeAction>::with_label("Exit")
                                .on_click(SmokeAction::Exit)
                                .width(140.0),
                        ),
                )
                .child(Text::new("Scrollable checklist").style(accent))
                .child(
                    Container::<SmokeAction>::new()
                        .fill_width()
                        .height(210.0)
                        .background(Color::rgb(17, 25, 40))
                        .padding(14.0)
                        .child(
                            ScrollView::<SmokeAction>::vertical().child(
                                Column::<SmokeAction>::new()
                                    .gap(10.0)
                                    .child(Text::new("1. Quad visible in front of the user").style(body))
                                    .child(Text::new("2. Text is readable, not mirrored, not upside down").style(body))
                                    .child(Text::new("3. Controller ray changes button hover/press state").style(body))
                                    .child(Text::new("4. Trigger click logs action=primary or action=secondary").style(body))
                                    .child(Text::new("5. Thumbstick scroll moves this checklist").style(body))
                                    .child(Text::new("6. Hand pinch works when OpenXR hand extensions are available").style(body))
                                    .child(Text::new("7. Exit button closes the frame loop cleanly").style(body))
                                    .child(Text::new("8. Report distance/scale/pointer offset if anything feels wrong").style(body)),
                            ),
                        ),
                )
                .child(
                    Text::new("Feedback format: quad/text/hover/click/scroll/hand + observed issue.")
                        .style(dim),
                ),
        )
        .into_view()
}

#[cfg(feature = "openxr")]
fn parse_args() -> Result<SmokeArgs, String> {
    let mut args = SmokeArgs::from_env()?;
    let mut iter = std::env::args().skip(1);

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            "--prefer-left" => args.prefer_left = true,
            "--no-hands" => args.hands = false,
            "--affordance-demo" => args.affordance_demo = true,
            "--distance-m" => {
                let value = next_value(&mut iter, "--distance-m")?;
                args.distance_m = parse_positive_f32("--distance-m", &value)?;
            }
            "--scale" => {
                let value = next_value(&mut iter, "--scale")?;
                args.scale = parse_positive_f32("--scale", &value)?;
            }
            "--timeout-sec" => {
                let value = next_value(&mut iter, "--timeout-sec")?;
                args.timeout_sec = Some(parse_positive_f32("--timeout-sec", &value)?);
            }
            "--panel-facing" => {
                let value = next_value(&mut iter, "--panel-facing")?;
                args.panel_facing.mode = parse_panel_facing("--panel-facing", &value)?;
            }
            "--panel-pitch-min-deg" => {
                let value = next_value(&mut iter, "--panel-pitch-min-deg")?;
                args.panel_facing.pitch_min_rad =
                    parse_finite_f32("--panel-pitch-min-deg", &value)?.to_radians();
                args.panel_facing.normalize_pitch_bounds();
            }
            "--panel-pitch-max-deg" => {
                let value = next_value(&mut iter, "--panel-pitch-max-deg")?;
                args.panel_facing.pitch_max_rad =
                    parse_finite_f32("--panel-pitch-max-deg", &value)?.to_radians();
                args.panel_facing.normalize_pitch_bounds();
            }
            "--log" => {
                let value = next_value(&mut iter, "--log")?;
                args.log_path = Some(value.into());
            }
            _ => return Err(format!("unknown argument: {arg}\n\n{}", usage())),
        }
    }

    Ok(args)
}

#[cfg(feature = "openxr")]
impl SmokeArgs {
    fn from_env() -> Result<Self, String> {
        let mut args = SmokeArgs::default();
        if env_bool("AILLOLI_UI_XR_PREFER_LEFT", "OCTAVUI_XR_PREFER_LEFT")? {
            args.prefer_left = true;
        }
        if env_bool("AILLOLI_UI_XR_NO_HANDS", "OCTAVUI_XR_NO_HANDS")? {
            args.hands = false;
        }
        if let Some(value) = optional_env("AILLOLI_UI_XR_DISTANCE_M", "OCTAVUI_XR_DISTANCE_M") {
            args.distance_m = parse_positive_f32("AILLOLI_UI_XR_DISTANCE_M", &value)?;
        }
        if let Some(value) = optional_env("AILLOLI_UI_XR_SCALE", "OCTAVUI_XR_SCALE") {
            args.scale = parse_positive_f32("AILLOLI_UI_XR_SCALE", &value)?;
        }
        if let Some(value) = optional_env("AILLOLI_UI_XR_TIMEOUT_SEC", "OCTAVUI_XR_TIMEOUT_SEC") {
            args.timeout_sec = Some(parse_positive_f32("AILLOLI_UI_XR_TIMEOUT_SEC", &value)?);
        }
        if let Some(value) = optional_env("AILLOLI_UI_XR_PANEL_FACING", "OCTAVUI_XR_PANEL_FACING") {
            args.panel_facing.mode = parse_panel_facing("AILLOLI_UI_XR_PANEL_FACING", &value)?;
        }
        if let Some(value) = optional_env(
            "AILLOLI_UI_XR_PANEL_PITCH_MIN_DEG",
            "OCTAVUI_XR_PANEL_PITCH_MIN_DEG",
        ) {
            args.panel_facing.pitch_min_rad =
                parse_finite_f32("AILLOLI_UI_XR_PANEL_PITCH_MIN_DEG", &value)?.to_radians();
            args.panel_facing.normalize_pitch_bounds();
        }
        if let Some(value) = optional_env(
            "AILLOLI_UI_XR_PANEL_PITCH_MAX_DEG",
            "OCTAVUI_XR_PANEL_PITCH_MAX_DEG",
        ) {
            args.panel_facing.pitch_max_rad =
                parse_finite_f32("AILLOLI_UI_XR_PANEL_PITCH_MAX_DEG", &value)?.to_radians();
            args.panel_facing.normalize_pitch_bounds();
        }
        if let Some(value) = optional_env("AILLOLI_UI_XR_LOG", "OCTAVUI_XR_LOG") {
            args.log_path = Some(value.into());
        }
        if env_bool(
            "AILLOLI_UI_XR_AFFORDANCE_DEMO",
            "OCTAVUI_XR_AFFORDANCE_DEMO",
        )? {
            args.affordance_demo = true;
        }
        Ok(args)
    }
}

#[cfg(feature = "openxr")]
fn optional_env(primary: &str, legacy: &str) -> Option<String> {
    match std::env::var(primary) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        Ok(_) | Err(std::env::VarError::NotUnicode(_)) => None,
        Err(std::env::VarError::NotPresent) => match std::env::var(legacy) {
            Ok(value) if !value.trim().is_empty() => Some(value),
            _ => None,
        },
    }
}

#[cfg(feature = "openxr")]
fn env_bool(primary: &str, legacy: &str) -> Result<bool, String> {
    let Some(value) = optional_env(primary, legacy) else {
        return Ok(false);
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(format!(
            "{primary} expects one of 1,true,yes,on,0,false,no,off; got: {value}"
        )),
    }
}

#[cfg(feature = "openxr")]
fn next_value(iter: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    iter.next()
        .ok_or_else(|| format!("{flag} expects a value\n\n{}", usage()))
}

#[cfg(feature = "openxr")]
fn parse_positive_f32(flag: &str, value: &str) -> Result<f32, String> {
    let parsed = parse_finite_f32(flag, value)?;
    if parsed > 0.0 {
        Ok(parsed)
    } else {
        Err(format!("{flag} expects a finite value > 0, got: {value}"))
    }
}

#[cfg(feature = "openxr")]
fn parse_finite_f32(flag: &str, value: &str) -> Result<f32, String> {
    let parsed = value
        .parse::<f32>()
        .map_err(|_| format!("{flag} expects a number, got: {value}"))?;
    if parsed.is_finite() {
        Ok(parsed)
    } else {
        Err(format!("{flag} expects a finite number, got: {value}"))
    }
}

#[cfg(feature = "openxr")]
fn parse_panel_facing(
    flag: &str,
    value: &str,
) -> Result<ailloli_ui_openxr::OpenXrPanelFacingMode, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "fixed" => Ok(ailloli_ui_openxr::OpenXrPanelFacingMode::Fixed),
        "yaw-only" | "yaw_only" | "face-user-on-drag" => {
            Ok(ailloli_ui_openxr::OpenXrPanelFacingMode::FaceUserOnDrag)
        }
        "yaw-pitch" | "yaw_pitch" => {
            Ok(ailloli_ui_openxr::OpenXrPanelFacingMode::FaceUserYawPitchOnDrag)
        }
        other => Err(format!(
            "{flag} expects fixed, yaw-only, or yaw-pitch; got: {other}"
        )),
    }
}

#[cfg(feature = "openxr")]
struct SmokeLogger {
    file: Option<std::fs::File>,
}

#[cfg(feature = "openxr")]
impl SmokeLogger {
    fn new(path: Option<&std::path::Path>) -> Result<Self, String> {
        let Some(path) = path else {
            return Ok(Self { file: None });
        };
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    format!("create log directory {}: {error}", parent.display())
                })?;
            }
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| format!("open log file {}: {error}", path.display()))?;
        Ok(Self { file: Some(file) })
    }

    fn log(&mut self, line: impl AsRef<str>) {
        use std::io::Write;

        let line = line.as_ref();
        println!("{line}");
        if let Some(file) = &mut self.file {
            let _ = writeln!(file, "{line}");
        }
    }
}

#[cfg(feature = "openxr")]
fn print_startup(logger: &mut SmokeLogger, args: &SmokeArgs) {
    logger.log("[ailloli_ui-xr-smoke] starting");
    logger.log(format!(
        "[ailloli_ui-xr-smoke] options prefer_left={} hands={} distance_m={:.2} scale={:.2} timeout_sec={} affordance_demo={} panel_facing={:?} panel_pitch_min_deg={:.1} panel_pitch_max_deg={:.1}",
        args.prefer_left,
        args.hands,
        args.distance_m,
        args.scale,
        args.timeout_sec
            .map(|value| format!("{value:.1}"))
            .unwrap_or_else(|| "none".to_string()),
        args.affordance_demo,
        args.panel_facing.mode,
        args.panel_facing.pitch_min_rad.to_degrees(),
        args.panel_facing.pitch_max_rad.to_degrees()
    ));
    logger.log(format!(
        "[ailloli_ui-xr-smoke] log={}",
        args.log_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "stdout-only".to_string())
    ));
    logger.log("[ailloli_ui-xr-smoke] checklist visual=quad/text/buttons/scroll interaction=hover/click/scroll/exit");
}

#[cfg(feature = "openxr")]
fn print_usage() {
    println!("{}", usage());
}

#[cfg(feature = "openxr")]
fn usage() -> &'static str {
    "Usage: cargo run -p ailloli_ui_openxr --example xr_ui_smoke --features openxr -- [options]\n\
     Options:\n\
       --prefer-left          Prefer the left controller pointer\n\
       --no-hands             Disable OpenXR hand input collection\n\
       --affordance-demo      Show the UI-XR 17 window affordance slate\n\
       --distance-m <meters>  Place the UI quad at this forward distance, default 2.0\n\
       --scale <dpr>          Logical scale / DPR, default 1.0\n\
       --timeout-sec <sec>    Stop automatically after this duration\n\
       --panel-facing <mode>  fixed, yaw-only, or yaw-pitch; default yaw-only\n\
       --panel-pitch-min-deg <deg>  Minimum yaw-pitch clamp, default -45\n\
       --panel-pitch-max-deg <deg>  Maximum yaw-pitch clamp, default 45\n\
       --log <path>           Also append smoke output to this file\n\
       -h, --help             Print this help"
}
