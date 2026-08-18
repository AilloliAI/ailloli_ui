use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::style::{AlignItems, Background, Border, BoxStyle, JustifyContent, Radius};
use ailloli_ui_core::{BoxShadow, Color, Size};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::IntoView;
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::button::ButtonStyle;
use ailloli_ui_widgets::controls::Button;
use ailloli_ui_widgets::primitives::Icon;
use lucide_icons::Icon as LucideIcon;

#[test]
fn button_with_icon_child_centers_child() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(
        Button::new()
            .child(Icon::lucide(LucideIcon::Plus).size(16.0))
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(200.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 40.0);
    assert_eq!(layout.size.h, 36.0);
    assert_eq!(layout.children.len(), 1);
    let child = &layout.children[0];
    assert_eq!(child.size, Size::new(16.0, 16.0));
    assert_eq!(child.offset.x, 12.0);
    assert_eq!(child.offset.y, 10.0);
}

#[test]
fn button_explicit_size_centers_content_by_default() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(
        Button::new()
            .width(120.0)
            .height(60.0)
            .child(Icon::lucide(LucideIcon::Plus).size(16.0))
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(200.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size, Size::new(120.0, 60.0));
    assert_eq!(layout.children.len(), 1);
    assert_eq!(layout.children[0].offset.x, 52.0);
    assert_eq!(layout.children[0].offset.y, 22.0);
}

#[test]
fn button_justify_and_align_builders_move_content() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(
        Button::new()
            .width(120.0)
            .height(60.0)
            .border(2.0, Color::WHITE)
            .justify_content(JustifyContent::End)
            .align_items(AlignItems::End)
            .child(Icon::lucide(LucideIcon::Plus).size(16.0))
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(200.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size, Size::new(120.0, 60.0));
    assert_eq!(layout.children.len(), 1);
    assert_eq!(layout.children[0].offset.x, 90.0);
    assert_eq!(layout.children[0].offset.y, 34.0);
}

#[test]
fn button_with_label_matches_text_child_layout() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(Button::with_label("OK").into_view());

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(200.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert!(layout.size.w > 24.0);
    assert_eq!(layout.size.h, 36.0);
    assert_eq!(layout.children.len(), 1);
}

#[test]
fn button_box_style_border_participates_in_child_layout() {
    let mut style = ButtonStyle::primary();
    style.container.normal = BoxStyle::new()
        .background(Background::color(Color::BLACK))
        .border(Border::new(2.0, Color::WHITE))
        .radius(Radius::zero());
    style.container.hovered = None;
    style.container.pressed = None;
    style.container.focused = None;
    style.container.disabled = None;
    style.baseline_shift = 0.0;

    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(
        Button::new()
            .button_style(style)
            .child(Icon::lucide(LucideIcon::Plus).size(16.0))
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(200.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 44.0);
    assert_eq!(layout.size.h, 36.0);
    assert_eq!(layout.children.len(), 1);
    let child = &layout.children[0];
    assert_eq!(child.size, Size::new(16.0, 16.0));
    assert_eq!(child.offset.x, 14.0);
    assert_eq!(child.offset.y, 10.0);
}

#[test]
fn button_border_builder_participates_in_child_layout() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(
        Button::new()
            .border(2.0, Color::WHITE)
            .child(Icon::lucide(LucideIcon::Plus).size(16.0))
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(200.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 44.0);
    assert_eq!(layout.children.len(), 1);
    assert_eq!(layout.children[0].size, Size::new(16.0, 16.0));
    assert_eq!(layout.children[0].offset.x, 14.0);

    let scene = app.paint(&mut text_system);
    assert!(scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .any(|cmd| matches!(cmd, DrawCmd::Border(_))));
}

#[test]
fn button_shadow_builder_does_not_change_layout_and_emits_shadow() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(
        Button::new()
            .shadow(BoxShadow::new(0.0, 4.0, 8.0, 0.0, Color::BLACK))
            .child(Icon::lucide(LucideIcon::Plus).size(16.0))
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(200.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 40.0);
    assert_eq!(layout.size.h, 36.0);
    assert_eq!(layout.paint_bounds.w, 40.0);
    assert!(layout.visual_bounds.h > layout.paint_bounds.h);

    let scene = app.paint(&mut text_system);
    assert!(scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .any(|cmd| matches!(cmd, DrawCmd::BoxShadow(_))));
}
