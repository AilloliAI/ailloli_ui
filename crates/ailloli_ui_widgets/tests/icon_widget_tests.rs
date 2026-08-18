use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Color, IconId, Size};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::IntoView;
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::primitives::Icon;

#[test]
fn icon_intrinsic_size_matches_size_builder() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(Icon::new(IconId::Plus).size(24.0).into_view());

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(200.0, 200.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size, Size::new(24.0, 24.0));
}

#[test]
fn icon_fill_width_keeps_height() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(Icon::new(IconId::Check).size(16.0).fill_width().into_view());

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(100.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 100.0);
    assert_eq!(layout.size.h, 16.0);
}

#[test]
fn icon_paint_emits_draw_image_with_tint() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        Icon::new(IconId::Plus)
            .size(16.0)
            .tint(Color::rgb(255, 0, 0))
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(32.0, 32.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let scene = app.paint(&mut text_system);

    let img_cmd = scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .find_map(|cmd| match cmd {
            DrawCmd::Image(img) => Some(img),
            _ => None,
        })
        .expect("icon paint command");

    assert_eq!(img_cmd.icon, IconId::Plus);
    assert_eq!(img_cmd.tint, Color::rgb(255, 0, 0));
    assert_eq!(img_cmd.rotation_rad, 0.0);
}

#[test]
fn icon_rotation_builder_reaches_draw_image() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        Icon::new(IconId::Check)
            .size(16.0)
            .rotation_rad(std::f32::consts::FRAC_PI_2)
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(32.0, 32.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let scene = app.paint(&mut text_system);

    let img_cmd = scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .find_map(|cmd| match cmd {
            DrawCmd::Image(img) => Some(img),
            _ => None,
        })
        .expect("icon paint command");

    assert_eq!(img_cmd.rotation_rad, std::f32::consts::FRAC_PI_2);
}
