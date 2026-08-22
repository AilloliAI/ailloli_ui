//! Badge, tag, and interactive chip layout, paint, focus, and action scenarios.

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::style::Background;
use ailloli_ui_core::{Color, Point};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::IntoView;
use ailloli_ui_runtime::input::{dispatch_event_to_target, InputRouter};
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{Badge, BadgeStyle, BadgeTone, BadgeVariant, Chip, Tag};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Close,
}

#[test]
fn badge_style_from_theme_uses_tone_and_variant_colors() {
    let theme = ailloli_ui_core::Theme::default();
    let palette = theme.palette();

    let accent = BadgeStyle::from_theme(theme, BadgeTone::Accent, BadgeVariant::Filled);
    assert_eq!(accent.background, Background::color(palette.accent));
    assert_eq!(accent.text.color, Color::hex_rgb(0xF4F7F8));
    assert!(!accent.border.is_visible());

    let warning = BadgeStyle::from_theme(theme, BadgeTone::Warning, BadgeVariant::Soft);
    assert_eq!(warning.dot_color, palette.warning);
    assert!(warning.border.is_visible());

    let muted = BadgeStyle::from_theme(theme, BadgeTone::Muted, BadgeVariant::Ghost);
    assert_eq!(muted.text.color, palette.text_muted);
}

#[test]
fn badge_count_layout_includes_count_and_paints_text_and_background() {
    let (app, root) = layout_view(
        Badge::new("Badge")
            .tone(BadgeTone::Accent)
            .variant(BadgeVariant::Filled)
            .count(7)
            .into_view(),
    );

    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert!(layout.size.w > 58.0, "layout width={}", layout.size.w);
    assert_eq!(layout.size.h, 26.0);

    let mut text_system = TextSystem::new();
    let scene = app.paint(&mut text_system);
    let cmds: Vec<_> = scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .collect();
    assert!(cmds.iter().any(|cmd| matches!(cmd, DrawCmd::RRect(_))));
    assert!(
        cmds.iter()
            .filter(|cmd| matches!(cmd, DrawCmd::Text(_)))
            .count()
            >= 2
    );
}

#[test]
fn badge_dot_paints_status_dot_with_tone_color() {
    let (app, _root) = layout_view(
        Badge::dot("Online")
            .tone(BadgeTone::Success)
            .variant(BadgeVariant::Soft)
            .into_view(),
    );

    let palette = ailloli_ui_core::Theme::default().palette();
    let mut text_system = TextSystem::new();
    let scene = app.paint(&mut text_system);
    assert!(scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .any(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.color == palette.success)));
}

#[test]
fn tag_is_non_interactive_and_paints_outline_border() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(
        Tag::new("Filter")
            .tone(BadgeTone::Neutral)
            .variant(BadgeVariant::Outline)
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(240.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        RuntimeHandle::new(),
        &pointer_button(5.0, 5.0, true),
    );
    assert_eq!(router.focused(), None);

    let scene = app.paint(&mut text_system);
    assert!(scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .any(|cmd| matches!(cmd, DrawCmd::Border(_))));
    assert!(app.tree.get(root).unwrap().layout.as_ref().unwrap().size.w > 30.0);
}

#[test]
fn chip_close_dispatches_only_from_close_zone() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Chip::new("Close")
            .tone(BadgeTone::Accent)
            .on_close(Action::Close)
            .into_view(),
    );
    let size = layout_app(&mut app);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(6.0, size.h * 0.5, false),
    );
    assert!(runtime.take_actions().is_empty());

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(size.w - 12.0, size.h * 0.5, false),
    );
    assert_eq!(runtime.take_actions(), vec![Action::Close]);
}

#[test]
fn chip_disabled_blocks_close_and_paints_dimmed() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Chip::new("Disabled")
            .tone(BadgeTone::Danger)
            .on_close(Action::Close)
            .disabled(true)
            .into_view(),
    );
    let size = layout_app(&mut app);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(size.w - 12.0, size.h * 0.5, false),
    );
    assert!(runtime.take_actions().is_empty());

    let mut text_system = TextSystem::new();
    let scene = app.paint(&mut text_system);
    assert!(scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .any(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.color.a < 0.6)));
}

#[test]
fn focused_chip_close_activates_with_keyboard() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(Chip::new("Keyboard").on_close(Action::Close).into_view());
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(&app.tree, runtime.clone(), &pointer_button(5.0, 5.0, true));
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Named(NamedKey::Enter),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: Some(Point::new(5.0, 5.0)),
            text: None,
        }),
    );

    assert_eq!(runtime.take_actions(), vec![Action::Close]);
}

fn layout_view(
    view: ailloli_ui_runtime::component::View<()>,
) -> (Runtime<()>, ailloli_ui_core::ElementId) {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(view);
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(320.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );
    (app, root)
}

fn layout_app(app: &mut Runtime<Action>) -> ailloli_ui_core::Size {
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(320.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let root = app.tree.root().expect("root element");
    app.tree.get(root).unwrap().layout.as_ref().unwrap().size
}

fn pointer_button(x: f32, y: f32, pressed: bool) -> Event {
    Event::Pointer(PointerEvent::Button {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        pressed,
        modifiers: Modifiers::default(),
    })
}
