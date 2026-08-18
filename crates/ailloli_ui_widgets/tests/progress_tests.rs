use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Modifiers};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Point, Theme};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State};
use ailloli_ui_runtime::input::InputRouter;
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{
    CircularProgress, ProgressBar, ProgressSize, ProgressStyle, ProgressVariant,
};

#[test]
fn progress_style_from_theme_uses_default_tokens() {
    let theme = Theme::default();
    let palette = theme.palette();
    let style = ProgressStyle::from_theme(theme, ProgressSize::Default);

    assert_eq!(style.track, palette.surface_elevated);
    assert_eq!(style.fill, palette.accent);
    assert_eq!(style.border.colors.top, palette.border.with_alpha(0.72));
    assert_eq!(style.text.color, palette.text);
    assert_eq!(style.muted_text.color, palette.text_muted);
    assert_eq!(style.focus_neutral, palette.focus);
    assert_eq!(style.bar_width, 220.0);
    assert_eq!(style.bar_height, 8.0);
    assert_eq!(style.circular_size, 58.0);
    assert_eq!(style.circular_thickness, 6.0);
}

#[test]
fn progress_layout_sizes_are_stable() {
    let (app, root) = layout_view(ProgressBar::new().value(0.65).into_view());
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 220.0);
    assert_eq!(layout.size.h, 8.0);

    let (app, root) = layout_view(
        ProgressBar::new()
            .value(0.65)
            .progress_size(ProgressSize::Compact)
            .into_view(),
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 180.0);
    assert_eq!(layout.size.h, 6.0);

    let (app, root) = layout_view(
        CircularProgress::new()
            .value(0.66)
            .progress_size(ProgressSize::Large)
            .into_view(),
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 72.0);
    assert_eq!(layout.size.h, 72.0);

    let (app, root) = layout_view(ProgressBar::new().value(0.65).width(320.0).into_view());
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 320.0);
}

#[test]
fn progress_bar_paint_emits_track_fill_and_stripes() {
    let palette = Theme::default().palette();
    let (app, _) = layout_view(
        ProgressBar::new()
            .value(45.0)
            .range(0.0, 100.0)
            .variant(ProgressVariant::Striped)
            .into_view(),
    );
    let scene = paint_scene(&app);

    assert!(scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.color == palette.surface_elevated)));
    assert!(scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.color == palette.accent)));
    assert!(scene.iter().any(|cmd| matches!(cmd, DrawCmd::Rect(_))));
    assert!(scene.iter().any(|cmd| matches!(cmd, DrawCmd::Border(_))));
}

#[test]
fn circular_progress_paint_emits_ring_and_optional_label() {
    let palette = Theme::default().palette();
    let (app, _) = layout_view(
        CircularProgress::new()
            .value(0.66)
            .show_label(true)
            .into_view(),
    );
    let scene = paint_scene(&app);

    assert!(scene.iter().any(|cmd| {
        matches!(
            cmd,
            DrawCmd::RingProgress(r)
                if r.fill_color == palette.accent && (r.fraction - 0.66).abs() <= 0.001
        )
    }));
    assert!(scene.iter().any(|cmd| matches!(cmd, DrawCmd::Text(_))));
}

#[test]
fn progress_values_clamp_and_disabled_diminishes_alpha() {
    let style = ProgressStyle::default();
    let (app, _) = layout_view(
        ProgressBar::new()
            .value(200.0)
            .range(0.0, 100.0)
            .disabled(true)
            .into_view(),
    );
    let scene = paint_scene(&app);

    let fill = scene
        .iter()
        .find_map(|cmd| match cmd {
            DrawCmd::RRect(r) if r.color == style.disabled_fill.with_alpha(0.38 * 0.48) => Some(r),
            _ => None,
        })
        .expect("disabled fill");
    assert_eq!(fill.rect.w, style.bar_width);
}

#[test]
fn progress_is_non_focusable_and_does_not_mutate_signal() {
    let value = State::new(0.25);
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(ProgressBar::new().value(value.clone()).into_view());
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(20.0, 4.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );

    assert_eq!(router.focused(), None);
    assert_eq!(value.read(), 0.25);
}

fn layout_view(
    view: ailloli_ui_runtime::component::View<()>,
) -> (Runtime<()>, ailloli_ui_core::ElementId) {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(view);
    layout_app(&mut app);
    (app, root)
}

fn layout_app<A: 'static>(app: &mut Runtime<A>) -> ailloli_ui_core::Size {
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(360.0, 180.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let root = app.tree.root().expect("root element");
    app.tree.get(root).unwrap().layout.as_ref().unwrap().size
}

fn paint_scene(app: &Runtime<()>) -> Vec<DrawCmd> {
    let mut text_system = TextSystem::new();
    app.paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter().cloned())
        .collect()
}
