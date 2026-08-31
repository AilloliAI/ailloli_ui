//! Retained text layout reuse, wrapping, binding, interaction, and composition scenarios.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Modifiers};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::Point;
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State, View};
use ailloli_ui_runtime::input::{absolute_paint_bounds, InputRouter};
use ailloli_ui_runtime::layout::LayoutArtifact;
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::Slider;
use ailloli_ui_widgets::layout::{Column, Row};
use ailloli_ui_widgets::text::Text;

fn layout_root(root_view: View<()>, constraints: Constraints) -> Runtime<()> {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(root_view);
    let mut text_system = TextSystem::new();
    app.layout(constraints, Scale::new(1.0), &mut text_system);
    app
}

fn layout_app<A: 'static>(app: &mut Runtime<A>, constraints: Constraints) {
    let mut text_system = TextSystem::new();
    app.layout(constraints, Scale::new(1.0), &mut text_system);
}

fn first_text(app: &Runtime<()>) -> String {
    let mut text_system = TextSystem::new();
    app.paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .find_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some(text.layout.text().to_string()),
            _ => None,
        })
        .expect("text paint command")
}

fn pointer_button(x: f32, y: f32, pressed: bool) -> Event {
    Event::Pointer(PointerEvent::Button {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        pressed,
        modifiers: Modifiers::default(),
    })
}

#[test]
fn text_layout_artifact_is_reused_by_paint() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root_id = app.reconcile(Text::new("retained text").into_view());
    let mut text_system = TextSystem::new();

    app.layout(
        Constraints::loose(400.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    let layout_handle = match layout.artifact.as_ref().unwrap() {
        LayoutArtifact::Text(layout_handle) => layout_handle.clone(),
    };

    let scene = app.paint(&mut text_system);
    let text_cmd = scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .find_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some(text),
            _ => None,
        })
        .expect("text paint command");

    assert!(Arc::ptr_eq(&layout_handle, &text_cmd.layout));
}

#[test]
fn text_wraps_by_default_under_width_constraint() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root_id = app.reconcile(
        Text::new("Project settings were updated successfully and should wrap cleanly.")
            .width(140.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();

    app.layout(
        Constraints::loose(400.0, 220.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    let artifact = match layout.artifact.as_ref().unwrap() {
        LayoutArtifact::Text(layout_handle) => layout_handle.clone(),
    };

    assert_eq!(layout.size.w, 140.0);
    assert!(
        artifact.lines.len() > 1,
        "expected wrapped text, got {} line(s)",
        artifact.lines.len()
    );
}

#[test]
fn text_max_width_shapes_before_flex_assigns_its_height() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root_id = app.reconcile(
        Text::new("OpenXR integration")
            .size(24.0)
            .max_width(210.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();

    app.layout(
        Constraints::loose(682.0, 220.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    let LayoutArtifact::Text(artifact) = layout.artifact.as_ref().unwrap();
    assert_eq!(artifact.lines.len(), 2);
    assert!(
        layout.size.h >= artifact.metrics.height,
        "text artifact exceeds its committed slot: size={:?} metrics={:?}",
        layout.size,
        artifact.metrics
    );
}

#[test]
fn text_nowrap_builder_keeps_single_line() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root_id = app.reconcile(
        Text::new("Project settings were updated successfully and should not wrap.")
            .width(140.0)
            .nowrap()
            .into_view(),
    );
    let mut text_system = TextSystem::new();

    app.layout(
        Constraints::loose(400.0, 220.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    let artifact = match layout.artifact.as_ref().unwrap() {
        LayoutArtifact::Text(layout_handle) => layout_handle.clone(),
    };

    assert_eq!(layout.size.w, 140.0);
    assert_eq!(artifact.lines.len(), 1);
}

#[test]
fn text_accepts_string_references_and_bindings() {
    let owned = String::from("owned");
    let state = State::new("state".to_string());
    let memo = state.to_text_with(|value| format!("{value}!"));

    let mut app: Runtime<()> = Runtime::new(RuntimeHandle::new());
    app.reconcile(
        Column::new()
            .child(Text::new(String::from("static")))
            .child(Text::new(&owned))
            .child(Text::new(state.clone()))
            .child(Text::new(memo))
            .into_view(),
    );
    layout_app(&mut app, Constraints::loose(400.0, 220.0));

    let mut text_system = TextSystem::new();
    let texts = app
        .paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .filter_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some(text.layout.text().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(texts, vec!["static", "owned", "state", "state!"]);
}

#[test]
fn bound_text_updates_after_state_change_and_relayout() {
    let state = State::new("A".to_string());
    let mut app: Runtime<()> = Runtime::new(RuntimeHandle::new());
    app.reconcile(Text::new(state.clone()).into_view());
    layout_app(&mut app, Constraints::loose(400.0, 120.0));

    assert_eq!(first_text(&app), "A");

    state.set("B".to_string());
    layout_app(&mut app, Constraints::loose(400.0, 120.0));

    assert_eq!(first_text(&app), "B");
}

#[test]
fn bound_text_paints_only_the_committed_artifact_before_relayout() {
    let state = State::new("short".to_string());
    let mut app: Runtime<()> = Runtime::new(RuntimeHandle::new());
    let root_id = app.reconcile(Text::new(state.clone()).width(140.0).into_view());
    layout_app(&mut app, Constraints::loose(400.0, 220.0));

    let committed = match app
        .tree
        .get(root_id)
        .and_then(|element| element.layout.as_ref())
        .and_then(|layout| layout.artifact.as_ref())
        .expect("committed text artifact")
    {
        LayoutArtifact::Text(layout) => layout.clone(),
    };

    state.set("Project settings were updated successfully and should wrap cleanly.".to_string());

    let mut text_system = TextSystem::new();
    let painted = app
        .paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .find_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some(text.layout.clone()),
            _ => None,
        })
        .expect("text paint command");

    assert!(Arc::ptr_eq(&committed, &painted));
    assert_eq!(painted.text(), "short");

    layout_app(&mut app, Constraints::loose(400.0, 220.0));
    assert_eq!(
        first_text(&app),
        "Project settings were updated successfully and should wrap cleanly."
    );
}

#[test]
fn stale_text_replay_uses_the_committed_artifact_without_reading_its_binding() {
    let state = State::new("short".to_string());
    let binding_reads = Rc::new(Cell::new(0_u32));
    let reads = binding_reads.clone();
    let content = state.map(move |value| {
        binding_reads.set(binding_reads.get() + 1);
        value
    });
    let mut app: Runtime<()> = Runtime::new(RuntimeHandle::new());
    let root_id = app.reconcile(Text::new(content).width(140.0).into_view());
    layout_app(&mut app, Constraints::loose(400.0, 220.0));
    assert_eq!(reads.get(), 1);

    let committed = match app
        .tree
        .get(root_id)
        .and_then(|element| element.layout.as_ref())
        .and_then(|layout| layout.artifact.as_ref())
        .expect("committed text artifact")
    {
        LayoutArtifact::Text(layout) => layout.clone(),
    };
    state.set("fresh content that requires another authoritative layout".to_string());

    let mut text_system = TextSystem::new();
    let painted = app
        .paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .find_map(|command| match command {
            DrawCmd::Text(text) => Some(text.layout.clone()),
            _ => None,
        })
        .expect("stale-safe committed text replay");

    assert!(Arc::ptr_eq(&committed, &painted));
    assert_eq!(painted.text(), "short");
    assert_eq!(
        reads.get(),
        1,
        "stale replay must not evaluate the current text binding"
    );
}

#[test]
fn bound_text_keeps_the_long_committed_artifact_until_short_content_is_relaid_out() {
    let long = "Project settings were updated successfully and should wrap cleanly.";
    let state = State::new(long.to_string());
    let mut app: Runtime<()> = Runtime::new(RuntimeHandle::new());
    app.reconcile(Text::new(state.clone()).width(140.0).into_view());
    layout_app(&mut app, Constraints::loose(400.0, 220.0));

    state.set("short".to_string());
    assert_eq!(first_text(&app), long);

    layout_app(&mut app, Constraints::loose(400.0, 220.0));
    assert_eq!(first_text(&app), "short");
}

#[test]
fn state_signal_and_memo_to_text_helpers_format_display_values() {
    let count = State::new(7_u8);
    let ratio = State::new(2.5_f32);
    let enabled = State::new(true);
    let ratio_signal = ratio.clone().into_signal();
    let doubled = count.map(|value| value * 2);

    assert_eq!(count.to_text().read(), "7");
    assert_eq!(ratio_signal.to_text().read(), "2.5");
    assert_eq!(enabled.to_text().read(), "true");
    assert_eq!(doubled.to_text().read(), "14");
    assert_eq!(
        ratio.to_text_with(|value| format!("{value:.1}%")).read(),
        "2.5%"
    );
}

#[test]
fn bound_text_rewraps_when_state_content_changes() {
    let state = State::new("short".to_string());
    let mut app: Runtime<()> = Runtime::new(RuntimeHandle::new());
    let root_id = app.reconcile(Text::new(state.clone()).width(140.0).into_view());
    layout_app(&mut app, Constraints::loose(400.0, 220.0));

    let layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    let artifact = match layout.artifact.as_ref().unwrap() {
        LayoutArtifact::Text(layout_handle) => layout_handle.clone(),
    };
    assert_eq!(artifact.lines.len(), 1);

    state.set("Project settings were updated successfully and should wrap cleanly.".to_string());
    layout_app(&mut app, Constraints::loose(400.0, 220.0));

    let layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    let artifact = match layout.artifact.as_ref().unwrap() {
        LayoutArtifact::Text(layout_handle) => layout_handle.clone(),
    };
    assert!(artifact.lines.len() > 1);
}

#[test]
fn derived_text_memo_preserves_the_source_layout_revision() {
    let state = State::new("short".to_string());
    let mut app: Runtime<()> = Runtime::new(RuntimeHandle::new());
    let root_id = app.reconcile(Text::new(state.to_text()).width(140.0).into_view());
    layout_app(&mut app, Constraints::loose(400.0, 220.0));

    state.set("A derived memo must also invalidate wrapping geometry.".to_string());
    layout_app(&mut app, Constraints::loose(400.0, 220.0));

    let layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    let artifact = match layout.artifact.as_ref().unwrap() {
        LayoutArtifact::Text(layout_handle) => layout_handle.clone(),
    };
    assert!(artifact.lines.len() > 1);
}

#[test]
fn slider_bound_text_paints_updated_value_after_interaction() {
    let value = State::new(5.0_f32);
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root_id = app.reconcile(
        Column::new()
            .child(Text::new(value.to_text_with(|value| format!("{value:.0}"))))
            .child(Slider::<()>::new().range(0.0, 100.0).bind(value.clone()))
            .into_view(),
    );
    layout_app(&mut app, Constraints::loose(520.0, 240.0));

    assert_eq!(first_text(&app), "5");

    let slider_id = app.tree.children_of(root_id)[1];
    let slider_bounds = absolute_paint_bounds(&app.tree, slider_id).expect("slider bounds");
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &pointer_button(
            slider_bounds.x + slider_bounds.w * 0.5,
            slider_bounds.y + 14.0,
            true,
        ),
    );
    layout_app(&mut app, Constraints::loose(520.0, 240.0));

    assert_eq!(first_text(&app), "50");
}

#[test]
fn sample_app_like_text_layout_does_not_overlap_rows_or_panes() {
    let root_view: View<()> = Column::new()
        .gap(4.0)
        .child(Text::new("titlebar pane"))
        .child(
            Row::new()
                .gap(8.0)
                .child(Text::new("left pane"))
                .child(Text::new("center pane"))
                .child(Text::new("right pane")),
        )
        .into_view();

    let app = layout_root(root_view, Constraints::tight(640.0, 480.0));
    let root_id = app.root.expect("root id");
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();

    let title = root_layout.children[0].clone();
    let row = root_layout.children[1].clone();
    assert!(
        row.offset.y >= title.offset.y + title.size.h,
        "row starts before the titlebar ends"
    );

    let row_id = app.tree.children_of(root_id)[1];
    let row_layout = app.tree.get(row_id).unwrap().layout.as_ref().unwrap();
    let left = &row_layout.children[0];
    let center = &row_layout.children[1];
    let right = &row_layout.children[2];

    assert!(center.offset.x >= left.offset.x + left.size.w);
    assert!(right.offset.x >= center.offset.x + center.size.w);
}
