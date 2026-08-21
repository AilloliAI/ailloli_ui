use ailloli_ui_core::{Constraints, Scale};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::IntoView;
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{
    TreeItem, TreeModel, TreeModelHandle, TreeMutation, TreeView, TreeViewDiagnostics,
};
use ailloli_ui_widgets::layout::ScrollView;

fn flat_model(count: usize) -> TreeModelHandle<u64> {
    let mut model = TreeModel::new();
    model
        .apply_batch((0..count).map(|index| TreeMutation::Insert {
            parent: None,
            index,
            item: TreeItem::leaf(index as u64, format!("row-{index:06}")),
        }))
        .unwrap();
    TreeModelHandle::new(model)
}

fn text_command_count(scene: &ailloli_ui_runtime::Scene) -> usize {
    scene
        .layers
        .iter()
        .flat_map(|layer| &layer.cmds)
        .filter(|command| matches!(command, DrawCmd::Text(_)))
        .count()
}

#[test]
fn one_hundred_thousand_rows_layout_only_the_viewport_and_overscan() {
    let model = flat_model(100_000);
    let initial_rebuilds = model.read(|model| model.flat_index().rebuilds());
    let runtime = RuntimeHandle::<()>::new();
    let mut app = Runtime::new(runtime);
    let diagnostics = TreeViewDiagnostics::new();
    app.reconcile(
        ScrollView::vertical()
            .initial_scroll_y(50_000.0 * 28.0)
            .child(
                TreeView::new()
                    .model(model.clone())
                    .virtualized(true)
                    .diagnostics(diagnostics.clone()),
            )
            .into_view(),
    );

    let mut text = TextSystem::new();
    app.layout(Constraints::tight(720.0, 520.0), Scale::new(1.0), &mut text);
    let scene = app.paint(&mut text);

    // The Phase 127 contract allows at most the visible rows plus eight rows
    // of overscan on either side. The 53-row ceiling also covers partial rows
    // introduced by padding and fractional scroll offsets.
    assert!(
        text.cached_layout_count() <= 53,
        "only visible text is measured"
    );
    assert!(
        text_command_count(&scene) <= 53,
        "only visible rows are painted"
    );
    assert_eq!(
        model.read(|model| model.flat_index().rebuilds()),
        initial_rebuilds,
        "layout and paint must not rebuild the flattened index",
    );
    let first_diagnostics = diagnostics.snapshot();
    assert_eq!(first_diagnostics.loaded_rows, 100_000);
    assert!(first_diagnostics.visible_rows <= 53);
    assert!(first_diagnostics.layout_rows_visited <= 53);
    assert!(first_diagnostics.paint_rows_visited <= 53);
    assert_eq!(first_diagnostics.virtualization_fallbacks, 0);

    let cached_layouts = text.cached_layout_count();
    for _ in 0..10 {
        app.layout(Constraints::tight(720.0, 520.0), Scale::new(1.0), &mut text);
        let _ = app.paint(&mut text);
    }
    assert_eq!(text.cached_layout_count(), cached_layouts);
    assert_eq!(
        model.read(|model| model.flat_index().rebuilds()),
        initial_rebuilds,
    );
    let after_redraws = diagnostics.snapshot();
    assert_eq!(after_redraws.layout_calls, first_diagnostics.layout_calls);
    assert_eq!(
        after_redraws.layout_rows_visited,
        first_diagnostics.layout_rows_visited,
    );
    assert_eq!(
        after_redraws.flatten_rebuilds,
        first_diagnostics.flatten_rebuilds
    );
    assert_eq!(
        after_redraws.paint_calls,
        first_diagnostics.paint_calls + 10
    );
}

#[test]
fn a_model_delta_invalidates_layout_without_rebuilding_the_component() {
    let model = flat_model(3);
    let runtime = RuntimeHandle::<()>::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(TreeView::new().model(model.clone()).virtualized(true));
    let mut text = TextSystem::new();
    app.layout(Constraints::tight(300.0, 100.0), Scale::new(1.0), &mut text);
    assert!(app.runtime.frame_work_plan().is_empty());

    model
        .apply(TreeMutation::Insert {
            parent: None,
            index: 3,
            item: TreeItem::leaf(9, "added"),
        })
        .unwrap();
    let plan = app.runtime.frame_work_plan();
    assert!(!plan.needs_build());
    assert!(plan.needs_layout());
    assert!(plan.needs_paint());

    app.layout(Constraints::tight(300.0, 100.0), Scale::new(1.0), &mut text);
    assert!(app.runtime.frame_work_plan().is_empty());
}
