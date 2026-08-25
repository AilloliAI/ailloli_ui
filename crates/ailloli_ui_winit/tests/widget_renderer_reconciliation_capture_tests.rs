//! Widget and renderer reconciliation visual proof across public rendering paths.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit \
//!   --test widget_renderer_reconciliation_capture_tests \
//!   widget_renderer_reconciliation_capture \
//!   -- --ignored --nocapture --test-threads=1
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::core::style::BoxShadow;
use ailloli_ui::core::TextStyle;
use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

/// Resolves the repository-local directory used for diagnostic captures.
fn repo_captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

/// Counts RGBA8 pixels accepted by `pred`; trailing incomplete bytes are ignored.
fn count_pixels(rgba: &[u8], pred: impl Fn([u8; 4]) -> bool) -> u64 {
    rgba.chunks_exact(4)
        .filter(|px| pred([px[0], px[1], px[2], px[3]]))
        .count() as u64
}

/// Verifies merged widget families, palette diversity, and encoded PNG output.
fn assert_widget_renderer_reconciliation_frame(frame: &CapturedFrame) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "capture must contain encoded PNG data");
    assert!(frame.width > 900, "capture width={}", frame.width);
    assert!(frame.height > 560, "capture height={}", frame.height);

    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(24)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 36, "distinct sampled colors={distinct}");

    let slate_pixels = count_pixels(&frame.rgba, |px| {
        px[0] > 5
            && px[0] < 65
            && px[1] > 10
            && px[1] < 80
            && px[2] > 24
            && px[2] < 110
            && px[3] > 210
    });
    let teal_pixels = count_pixels(&frame.rgba, |px| {
        px[0] < 100 && px[1] > 130 && px[2] > 115 && px[2] < 235 && px[3] > 190
    });
    let amber_pixels = count_pixels(&frame.rgba, |px| {
        px[0] > 180 && px[1] > 105 && px[1] < 230 && px[2] < 110 && px[3] > 190
    });
    let bright_pixels = count_pixels(&frame.rgba, |px| {
        px[0] > 160 && px[1] > 160 && px[2] > 160 && px[3] > 210
    });
    let border_pixels = count_pixels(&frame.rgba, |px| {
        px[0] > 55
            && px[0] < 180
            && px[1] > 70
            && px[1] < 195
            && px[2] > 90
            && px[2] < 225
            && px[3] > 180
    });

    assert!(slate_pixels > 28_000, "slate/surface pixels={slate_pixels}");
    assert!(teal_pixels > 100, "switch/affordance pixels={teal_pixels}");
    assert!(amber_pixels > 45, "rotated icon pixels={amber_pixels}");
    assert!(bright_pixels > 500, "text/thumb pixels={bright_pixels}");
    assert!(border_pixels > 500, "border/popup pixels={border_pixels}");
}

/// Writes the frame's required PNG payload under its semantic filename.
fn write_capture(frame: &CapturedFrame) {
    let out_dir = repo_captures_dir();
    std::fs::create_dir_all(&out_dir).expect("mkdir captures");
    std::fs::write(
        out_dir.join("widget_renderer_reconciliation.png"),
        frame.png_data.as_ref().expect("png data"),
    )
    .expect("write capture");
}

#[test]
#[ignore]
fn widget_renderer_reconciliation_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let capture_id = cap.request_element(
        "widget-renderer-reconciliation",
        "widget-renderer-reconciliation-window",
    );

    App::new()
        .window(
            Window::new("widget-renderer-reconciliation")
                .title_text("widget_renderer_reconciliation")
                .no_chrome()
                .size(1180.0, 760.0)
                .content(widget_renderer_reconciliation_showcase),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(capture_id)
        .expect("widget-renderer-reconciliation capture slot")
        .expect("widget-renderer-reconciliation capture ok")
        .frame;

    assert_widget_renderer_reconciliation_frame(&frame);
    write_capture(&frame);
}

/// Builds the complete cross-backend validation board.
fn widget_renderer_reconciliation_showcase() -> impl IntoView<()> {
    Container::<()>::new()
        .fill()
        .background(Color::rgb(4, 9, 18))
        .padding(34.0)
        .child(
            WindowAffordanceFrame::<()>::new("Widget and renderer reconciliation • Cross-backend validation")
                .logical_window_id("widget-renderer-reconciliation-window")
                .width(1070.0)
                .height(650.0)
                .window_affordance_style(validation_affordance_style())
                .on_affordance(|_| ())
                .content(
                    Container::<()>::new()
                        .fill()
                        .background(Color::rgb(15, 23, 42))
                        .padding(22.0)
                        .child(
                            Column::<()>::new()
                                .fill()
                                .gap(14.0)
                                .child(Text::new("OpenXR • Vulkan • Android").style(
                                    TextStyle::new(
                                        FontId::Ui,
                                        25,
                                        Color::rgb(245, 248, 255),
                                    ),
                                ))
                                .child(
                                    Text::new("One scene exercises reconciled chrome, orientation, icon rotation, popup overlays and rounded Vulkan primitives.")
                                        .style(TextStyle::new(
                                            FontId::Ui,
                                            15,
                                            Color::rgb(190, 202, 220),
                                        )),
                                )
                                .child(
                                    Row::<()>::new()
                                        .fill_width()
                                        .gap(18.0)
                                        .child(switch_card())
                                        .child(icon_card())
                                        .child(select_card()),
                                ),
                        ),
                ),
        )
        .key("widget-renderer-reconciliation-window")
}

/// Builds the switch-state comparison card.
fn switch_card() -> impl IntoView<()> {
    Container::<()>::new()
        .width(280.0)
        .height(370.0)
        .background(Color::rgb(11, 18, 32))
        .radius(18.0)
        .border(2.0, Color::rgba(71, 85, 105, 0.92))
        .shadow(BoxShadow::new(
            0.0,
            10.0,
            24.0,
            0.0,
            Color::rgba(0, 0, 0, 0.48),
        ))
        .padding(22.0)
        .child(
            Column::<()>::new()
                .fill()
                .gap(18.0)
                .child(Text::new("Vertical switches").style(TextStyle::new(
                    FontId::Ui,
                    19,
                    Color::rgb(235, 241, 250),
                )))
                .child(
                    Row::<()>::new()
                        .gap(54.0)
                        .child(
                            Column::<()>::new()
                                .gap(10.0)
                                .child(Switch::<()>::new().checked(false).vertical())
                                .child(Text::new("OFF").style(TextStyle::new(
                                    FontId::Ui,
                                    13,
                                    Color::rgb(148, 163, 184),
                                ))),
                        )
                        .child(
                            Column::<()>::new()
                                .gap(10.0)
                                .child(Switch::<()>::new().checked(true).vertical())
                                .child(Text::new("ON").style(TextStyle::new(
                                    FontId::Ui,
                                    13,
                                    Color::rgb(45, 212, 191),
                                ))),
                        ),
                )
                .child(
                    Text::new("The thumb moves on the vertical axis while retaining the shared switch state model.")
                        .style(TextStyle::new(
                            FontId::Ui,
                            14,
                            Color::rgb(180, 194, 214),
                        )),
                ),
        )
}

/// Builds the icon rendering and sizing comparison card.
fn icon_card() -> impl IntoView<()> {
    Container::<()>::new()
        .width(280.0)
        .height(370.0)
        .background(Color::rgb(11, 18, 32))
        .radius(18.0)
        .border(2.0, Color::rgba(71, 85, 105, 0.92))
        .shadow(BoxShadow::new(
            0.0,
            10.0,
            24.0,
            0.0,
            Color::rgba(0, 0, 0, 0.48),
        ))
        .padding(22.0)
        .child(
            Column::<()>::new()
                .fill()
                .gap(18.0)
                .child(Text::new("Lucide rotation").style(TextStyle::new(
                    FontId::Ui,
                    19,
                    Color::rgb(235, 241, 250),
                )))
                .child(
                    Container::<()>::new()
                        .width(150.0)
                        .height(150.0)
                        .background(Color::rgb(30, 41, 59))
                        .radius(28.0)
                        .border(2.0, Color::rgba(251, 191, 36, 0.72))
                        .padding(31.0)
                        .child(
                            Icon::new(IconId::Check)
                                .size(88.0)
                                .tint(Color::rgb(251, 191, 36))
                                .rotation_rad(std::f32::consts::FRAC_PI_4),
                        ),
                )
                .child(
                    Text::new("45° reaches DrawImage and the Vulkan image quad.")
                        .style(TextStyle::new(FontId::Ui, 14, Color::rgb(180, 194, 214))),
                ),
        )
}

/// Builds the select control state comparison card.
fn select_card() -> impl IntoView<()> {
    Container::<()>::new()
        .width(360.0)
        .height(370.0)
        .background(Color::rgb(11, 18, 32))
        .radius(18.0)
        .border(2.0, Color::rgba(71, 85, 105, 0.92))
        .shadow(BoxShadow::new(
            0.0,
            10.0,
            24.0,
            0.0,
            Color::rgba(0, 0, 0, 0.48),
        ))
        .padding(22.0)
        .child(
            Column::<()>::new()
                .fill()
                .gap(16.0)
                .child(Text::new("Select + popup").style(TextStyle::new(
                    FontId::Ui,
                    19,
                    Color::rgb(235, 241, 250),
                )))
                .child(
                    Text::new("The upstream popup extraction remains the single overlay path.")
                        .style(TextStyle::new(FontId::Ui, 14, Color::rgb(180, 194, 214))),
                )
                .child(
                    Select::<String>::new()
                        .width(290.0)
                        .selected("Cylinder layer".to_owned())
                        .default_open(true)
                        .option("Flat layer".to_owned(), "Flat layer")
                        .option("Cylinder layer".to_owned(), "Cylinder layer")
                        .option("Panel surface".to_owned(), "Panel surface"),
                )
                .child(
                    Text::new("Rounded popup • shadow • border • overlay").style(TextStyle::new(
                        FontId::Ui,
                        13,
                        Color::rgb(45, 212, 191),
                    )),
                ),
        )
}

/// Returns the deterministic native-window affordance validation palette.
fn validation_affordance_style() -> WindowAffordanceStyle {
    WindowAffordanceStyle {
        titlebar_background: Color::rgba(20, 28, 44, 1.0),
        background: Color::rgb(17, 24, 39),
        border: Color::rgba(148, 163, 184, 0.94),
        shadow: BoxShadow::new(0.0, 14.0, 32.0, 0.0, Color::rgba(0, 0, 0, 0.55)),
        control_idle: Color::rgba(148, 163, 184, 0.78),
        control_hover: Color::rgba(148, 163, 184, 0.96),
        control_active: Color::rgb(45, 212, 191),
        handle_idle: Color::rgba(45, 212, 191, 0.9),
        handle_hover: Color::rgba(45, 212, 191, 1.0),
        handle_active: Color::rgb(45, 212, 191),
        ..WindowAffordanceStyle::default()
    }
}
