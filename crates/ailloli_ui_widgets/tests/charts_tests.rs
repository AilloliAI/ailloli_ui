use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Modifiers};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{ChartRange, Point, Theme};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State};
use ailloli_ui_runtime::input::InputRouter;
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{
    BarChart, ChartSize, ChartStyle, ChartTone, LineChart, RadialGauge,
};

#[test]
fn chart_style_from_theme_uses_default_tokens() {
    let theme = Theme::default();
    let palette = theme.palette();
    let style = ChartStyle::from_theme(theme, ChartSize::Default);

    assert_eq!(style.background, palette.surface);
    assert_eq!(style.colors[0], palette.accent);
    assert_eq!(style.colors[1], palette.success);
    assert_eq!(style.colors[2], palette.info);
    assert_eq!(style.border.colors.top, palette.border.with_alpha(0.72));
    assert_eq!(style.text.color, palette.text);
    assert_eq!(style.muted_text.color, palette.text_muted);
    assert_eq!(style.width, 240.0);
    assert_eq!(style.height, 164.0);
}

#[test]
fn chart_layout_sizes_are_stable() {
    let (app, root) = layout_view(
        BarChart::new()
            .series("Revenue", [12.0, 18.0, 14.0])
            .into_view(),
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 240.0);
    assert_eq!(layout.size.h, 164.0);

    let (app, root) = layout_view(
        LineChart::new()
            .series("Sessions", [(0.0, 1.0), (1.0, 2.0)])
            .chart_size(ChartSize::Compact)
            .into_view(),
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 180.0);
    assert_eq!(layout.size.h, 124.0);

    let (app, root) = layout_view(RadialGauge::new().value(0.72).width(180.0).into_view());
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 180.0);
}

#[test]
fn bar_chart_paint_emits_frame_bars_labels_and_border() {
    let palette = Theme::default().palette();
    let (app, _) = layout_view(
        BarChart::new()
            .series("Revenue", [12.0, 18.0, 14.0, 24.0])
            .labels(["Mon", "Tue", "Wed", "Thu"])
            .range(0.0, 30.0)
            .into_view(),
    );
    let scene = paint_scene(&app);

    assert!(scene.iter().any(|cmd| matches!(cmd, DrawCmd::Border(_))));
    assert!(scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.color == palette.accent)));
    assert!(scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "Revenue")));
    assert!(scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "Mon")));
}

#[test]
fn line_chart_paint_emits_polyline_and_points() {
    let palette = Theme::default().palette();
    let (app, _) = layout_view(
        LineChart::new()
            .series("Sessions", [(0.0, 2.0), (1.0, 6.0), (2.0, 4.0)])
            .show_points(true)
            .tone(ChartTone::Success)
            .into_view(),
    );
    let scene = paint_scene(&app);

    let green_polylines = scene
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DrawCmd::Polyline(polyline)
                    if polyline.stroke.color == palette.success && polyline.points.len() == 3
            )
        })
        .count();
    assert_eq!(green_polylines, 1);
    assert!(scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.color == palette.success)));
}

#[test]
fn line_chart_empty_series_paints_empty_state() {
    let (app, _) = layout_view(LineChart::new().empty_text("Empty").into_view());
    let scene = paint_scene(&app);

    assert!(scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "Empty")));
}

#[test]
fn radial_gauge_emits_ring_progress_and_labels() {
    let palette = Theme::default().palette();
    let (app, _) = layout_view(
        RadialGauge::new()
            .value(72.0)
            .range(0.0, 100.0)
            .label("CPU Usage".to_string())
            .show_value(true)
            .into_view(),
    );
    let scene = paint_scene(&app);

    assert!(scene.iter().any(|cmd| {
        matches!(
            cmd,
            DrawCmd::RingProgress(r)
                if r.fill_color == palette.accent && (r.fraction - 0.72).abs() <= 0.001
        )
    }));
    assert!(scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "72%")));
    assert!(scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "CPU Usage")));
}

#[test]
fn charts_are_non_focusable_and_do_not_consume_input() {
    let value = State::new(0.25);
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(RadialGauge::new().value(value.clone()).into_view());
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(20.0, 20.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );

    assert_eq!(router.focused(), None);
    assert_eq!(value.read(), 0.25);
}

#[test]
fn chart_range_core_mapping_is_available_to_widgets() {
    let range = ChartRange::new(-10.0, 30.0);
    assert!((range.fraction_for_value(10.0) - 0.5).abs() <= 0.001);
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
        Constraints::loose(420.0, 260.0),
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
