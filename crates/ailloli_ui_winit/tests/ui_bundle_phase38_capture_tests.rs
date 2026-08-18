//! Local-only Phase 38 visual test for the default theme tokens.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase38_capture_tests -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use ailloli_ui::prelude::*;
use ailloli_ui::{App, CaptureHandle, Widget, Window};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::{Rect, Size};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_widgets::controls::{draw_checkbox, CheckboxStyle};

fn repo_captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

#[derive(Clone)]
struct CheckboxDemo {
    checked: bool,
    disabled: bool,
    label: &'static str,
}

impl Widget<()> for CheckboxDemo {
    fn debug_name(&self) -> &'static str {
        "CheckboxDemo"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        let size = Size::new(150.0, 24.0);
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let Some(text_system) = ctx.text_system.as_deref_mut() else {
            return;
        };
        for cmd in draw_checkbox(
            bounds,
            self.checked,
            Some(self.label),
            self.disabled,
            CheckboxStyle::default(),
            text_system,
        ) {
            ctx.push(cmd);
        }
    }
}

impl IntoView<()> for CheckboxDemo {
    fn into_view(self) -> View<()> {
        View::leaf(self)
    }
}

fn swatch(name: &'static str, color: Color) -> impl IntoView<()> {
    let theme = Theme::default();
    Row::new()
        .gap(8.0)
        .child(
            Container::new()
                .width(44.0)
                .height(28.0)
                .background(color)
                .radius(6.0),
        )
        .child(Text::new(name).style(theme.typography().ui_sm))
}

fn panel(title: &'static str, child: impl IntoView<()>) -> impl IntoView<()> {
    let theme = Theme::default();
    Container::panel(theme).width(900.0).padding(16.0).child(
        Column::new()
            .gap(12.0)
            .child(Text::new(title).style(theme.typography().ui_sm))
            .child(child),
    )
}

fn phase38_board(primary: State<String>, filled: State<String>) -> impl IntoView<()> {
    let theme = Theme::default();
    let palette = theme.palette();

    Container::new()
        .fill()
        .background(palette.background)
        .child(
            ScrollView::vertical().child(
                Column::new()
                    .gap(18.0)
                    .padding(24.0)
                    .child(
                        Text::new("Ailloli UI Phase 38 Theme Tokens")
                            .style(theme.typography().ui_lg),
                    )
                    .child(panel(
                        "Palette",
                        Row::new()
                            .gap(14.0)
                            .child(swatch("Background", palette.background))
                            .child(swatch("Surface", palette.surface))
                            .child(swatch("Elevated", palette.surface_elevated))
                            .child(swatch("Border", palette.border))
                            .child(swatch("Accent", palette.accent))
                            .child(swatch("Danger", palette.danger))
                            .child(swatch("Success", palette.success)),
                    ))
                    .child(panel(
                        "Buttons",
                        Column::new()
                            .gap(10.0)
                            .child(
                                Row::new()
                                    .gap(8.0)
                                    .child(Button::<()>::with_label_variant(
                                        "Primary",
                                        ButtonVariant::Primary,
                                    ))
                                    .child(Button::<()>::with_label_variant(
                                        "Secondary",
                                        ButtonVariant::Secondary,
                                    ))
                                    .child(Button::<()>::with_label_variant(
                                        "Outline",
                                        ButtonVariant::Outline,
                                    ))
                                    .child(Button::<()>::with_label_variant(
                                        "Ghost",
                                        ButtonVariant::Ghost,
                                    )),
                            )
                            .child(
                                Row::new()
                                    .gap(8.0)
                                    .child(Button::<()>::with_label_variant(
                                        "Destructive",
                                        ButtonVariant::Destructive,
                                    ))
                                    .child(Button::<()>::with_label_variant(
                                        "Success",
                                        ButtonVariant::Success,
                                    ))
                                    .child(Button::<()>::with_label_variant(
                                        "Warning",
                                        ButtonVariant::Warning,
                                    ))
                                    .child(Button::<()>::with_label_variant(
                                        "Info",
                                        ButtonVariant::Info,
                                    )),
                            ),
                    ))
                    .child(panel(
                        "Inputs And Checkbox Helper",
                        Row::new()
                            .gap(18.0)
                            .child(
                                Column::new()
                                    .gap(10.0)
                                    .child(
                                        TextInput::new()
                                            .bind(primary)
                                            .placeholder("Input")
                                            .width(260.0),
                                    )
                                    .child(
                                        TextInput::new()
                                            .bind(filled)
                                            .placeholder("Filled input")
                                            .width(260.0),
                                    ),
                            )
                            .child(
                                Column::new()
                                    .gap(10.0)
                                    .child(CheckboxDemo {
                                        checked: true,
                                        disabled: false,
                                        label: "Checked",
                                    })
                                    .child(CheckboxDemo {
                                        checked: false,
                                        disabled: false,
                                        label: "Unchecked",
                                    })
                                    .child(CheckboxDemo {
                                        checked: true,
                                        disabled: true,
                                        label: "Disabled",
                                    }),
                            ),
                    ))
                    .child(panel(
                        "Border Radius Shadow",
                        Row::new()
                            .gap(18.0)
                            .child(
                                Container::surface(theme)
                                    .width(180.0)
                                    .height(84.0)
                                    .padding(12.0)
                                    .child(Text::new("Surface").style(theme.typography().ui_md)),
                            )
                            .child(
                                Container::panel(theme)
                                    .width(180.0)
                                    .height(84.0)
                                    .padding(12.0)
                                    .child(
                                        Text::new("Panel shadow").style(theme.typography().ui_md),
                                    ),
                            ),
                    ))
                    .key("phase38-theme-board"),
            ),
        )
}

fn count_pixels(rgba: &[u8], pred: impl Fn([u8; 4]) -> bool) -> u64 {
    rgba.chunks_exact(4)
        .filter(|px| pred([px[0], px[1], px[2], px[3]]))
        .count() as u64
}

#[test]
#[ignore]
fn ui_bundle_phase38_theme_tokens_capture() {
    let out_dir = repo_captures_dir();
    std::fs::create_dir_all(&out_dir).expect("mkdir captures");

    let primary = State::new("Themed input".to_string());
    let filled = State::new("Value".to_string());

    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let id_win = cap.request_window("main");

    App::new()
        .window(
            Window::new("main")
                .title_text("ui_bundle_phase38_theme_tokens")
                .no_chrome()
                .size(980.0, 680.0)
                .content(move || phase38_board(primary.clone(), filled.clone())),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let result = cap
        .take_result(id_win)
        .expect("capture slot")
        .expect("capture ok");
    let png = result.frame.png_data.as_ref().expect("png data");
    let out_file = out_dir.join("ui_bundle_phase38_theme_tokens.png");
    std::fs::write(out_file, png).expect("write png");

    assert!(!png.is_empty());
    assert!(result.frame.width >= 700, "width={}", result.frame.width);
    assert!(result.frame.height >= 480, "height={}", result.frame.height);

    let rgba = &result.frame.rgba;
    let accent = count_pixels(rgba, |px| {
        px[3] > 160 && px[0] > 180 && px[1] >= 40 && px[1] <= 220 && px[2] < 100
    });
    let dark_surface = count_pixels(rgba, |px| {
        px[3] > 180
            && px[0] >= 5
            && px[0] < 115
            && px[1] >= 5
            && px[1] < 120
            && px[2] >= 5
            && px[2] < 125
            && px[0].abs_diff(px[1]) < 18
            && px[1].abs_diff(px[2]) < 18
    });
    let light_text = count_pixels(rgba, |px| {
        px[3] > 120 && px[0] > 180 && px[1] > 180 && px[2] > 180
    });

    assert!(accent > 300, "accent orange pixels: {accent}");
    assert!(dark_surface > 10_000, "dark surface pixels: {dark_surface}");
    assert!(light_text > 80, "light text pixels: {light_text}");
}
