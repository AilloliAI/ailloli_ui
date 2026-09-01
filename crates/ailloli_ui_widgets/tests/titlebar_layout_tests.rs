//! Regression: Ailloli UI title bar with usable height and square icons.

use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::AppIcon;
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::IntoView;
use ailloli_ui_runtime::element::ElementKind;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::chrome::{
    ailloli_ui_default_titlebar, ailloli_ui_default_titlebar_with_icon,
};

fn collect_icon_layout_sizes<A: 'static>(app: &Runtime<A>) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    for (_id, el) in app.tree.iter_elements() {
        if let ElementKind::Widget(w) = &el.kind {
            if w.debug_name() == "Icon" {
                if let Some(layout) = el.layout.as_ref() {
                    out.push((layout.size.w, layout.size.h));
                }
            }
        }
    }
    out
}

#[test]
fn titlebar_places_a_20px_brand_icon_before_the_chrome_controls() {
    static SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="red"/></svg>"#;
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        ailloli_ui_default_titlebar_with_icon(
            "main",
            "Narrow title",
            Some(AppIcon::from_static_svg(SVG, "icon.svg")),
        )
        .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 36.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let sizes = collect_icon_layout_sizes(&app);
    assert_eq!(sizes.len(), 4);
    assert!(sizes
        .iter()
        .any(|(width, height)| (*width - 20.0).abs() < 0.01 && (*height - 20.0).abs() < 0.01));
}

#[test]
fn ailloli_ui_titlebar_icons_remain_square_in_36px_bar() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(ailloli_ui_default_titlebar("main", "Sample App").into_view());

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(640.0, 36.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let root_layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert!(
        (root_layout.size.h - 36.0).abs() < 0.01,
        "titlebar root height: {}",
        root_layout.size.h
    );

    let icon_sizes = collect_icon_layout_sizes(&app);
    assert_eq!(icon_sizes.len(), 3, "expected 3 chrome icons");
    for (w, h) in &icon_sizes {
        assert!(
            (w - h).abs() < 0.01,
            "icon layout must stay square (was {w}×{h})"
        );
        assert!(*h >= 13.0 && *h <= 15.0, "icon side ~14px logical, got {h}");
    }
}
