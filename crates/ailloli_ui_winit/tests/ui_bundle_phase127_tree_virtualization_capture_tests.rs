//! Native visual proof for the Phase 127 retained and viewport-bounded TreeView.

#![cfg(feature = "test_support")]

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ailloli_ui::prelude::*;
use ailloli_ui::runtime::component::{component, Context};
use ailloli_ui_core::LogicalWindowId;
use ailloli_ui_render_wgpu::CapturedFrame;
use ailloli_ui_winit::{run_winit_host, NoopHostDriver, UiApp, WindowOptions, WinitHost};

/// Stable logical identity of the native fixture window.
const WINDOW_ID: &str = "phase127-tree-virtualization";
/// Stable retained key used for focus and diagnostics lookup.
const TREE_KEY: &str = "phase127-virtual-tree";
/// Output filename beneath the repository capture directory.
const CAPTURE_NAME: &str = "ui_bundle_phase127_tree_virtualization.png";
/// Total retained nodes loaded into the synthetic tree.
const ROW_COUNT: u64 = 100_000;
/// Deep row selected and scrolled into the initial viewport.
const SELECTED_ROW: u64 = 75_005;
/// Fixed logical row height used to compute initial scroll position.
const ROW_HEIGHT: f32 = 28.0;
/// Maximum rows that layout or paint may visit for one viewport frame.
const ROW_BUDGET: u64 = 53;

#[derive(Clone)]
/// Shared component inputs for the retained virtual-tree scene.
struct SceneProps {
    /// Mutable retained tree model shared with post-render assertions.
    model: TreeModelHandle<u64>,
    /// Counters used to prove viewport-bounded layout and paint work.
    diagnostics: TreeViewDiagnostics,
}

/// Builds the focused, initially deep-scrolled virtual tree capture scene.
fn scene(ctx: &mut Context<()>, props: SceneProps) -> View<()> {
    ctx.runtime().request_focus_key(TREE_KEY);
    let palette = Theme::default().palette();
    let tree = TreeView::<u64, ()>::new()
        .model(props.model)
        .selected(SELECTED_ROW)
        .virtualized(true)
        .diagnostics(props.diagnostics)
        .fill_width()
        .into_view()
        .key(TREE_KEY);
    Container::new()
        .fill()
        .background(palette.background)
        .padding(22.0)
        .clip_children(true)
        .window_root_clip(true)
        .child(
            Column::new()
                .fill()
                .gap(10.0)
                .child(Text::new("Phase 127 — retained virtual TreeView").size(24.0))
                .child(
                    Row::new()
                        .gap(10.0)
                        .child(Badge::new("100,000 retained nodes"))
                        .child(Badge::new("viewport + 8-row overscan"))
                        .child(Badge::new("no flatten on redraw")),
                )
                .child(
                    ScrollView::vertical()
                        .initial_scroll_y((SELECTED_ROW as f32 - 9.0) * ROW_HEIGHT)
                        .child(tree)
                        .flex_grow(),
                ),
        )
        .into_view()
}

/// Builds a 100,000-node expanded tree with one deliberately wide nearby row.
fn synthetic_model() -> TreeModelHandle<u64> {
    let mut model = TreeModel::new();
    let mut mutations = Vec::with_capacity(ROW_COUNT as usize);
    mutations.push(TreeMutation::Insert {
        parent: None,
        index: 0,
        item: TreeItem::branch(0, "Synthetic workspace root"),
    });
    for id in 1..ROW_COUNT {
        let label = if id == SELECTED_ROW + 3 {
            format!("row-{id:06} — late-discovered-width-abcdefghijklmnopqrstuvwxyz-0123456789")
        } else {
            format!("row-{id:06}")
        };
        mutations.push(TreeMutation::Insert {
            parent: Some(0),
            index: (id - 1) as usize,
            item: TreeItem::leaf(id, label),
        });
    }
    mutations.push(TreeMutation::SetExpanded {
        id: 0,
        expanded: true,
    });
    model
        .apply_batch(mutations)
        .expect("build the 100,000-node retained tree");
    assert_eq!(model.len(), ROW_COUNT as usize);
    assert_eq!(model.visible_len(), ROW_COUNT as usize);
    TreeModelHandle::new(model)
}

/// Resolves the repository-local directory used for the final PNG artifact.
fn captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

/// Verifies exact extent, encoded data, color diversity, and non-empty viewport content.
fn assert_visual_frame(frame: &CapturedFrame) {
    assert_eq!((frame.width, frame.height), (1_000, 640));
    assert!(!frame.png_data.as_ref().expect("PNG bytes").is_empty());
    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(24)
        .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 16, "distinct sampled colors={distinct}");
    let mut region_colors = HashSet::new();
    let mut bright_pixels = 0_usize;
    for y in (95..610).step_by(3) {
        for x in (20..970).step_by(3) {
            let offset = ((y * frame.width + x) * 4) as usize;
            let pixel = &frame.rgba[offset..offset + 4];
            region_colors.insert([pixel[0], pixel[1], pixel[2], pixel[3]]);
            bright_pixels +=
                usize::from(pixel[3] > 180 && (pixel[0] > 90 || pixel[1] > 90 || pixel[2] > 90));
        }
    }
    assert!(
        region_colors.len() >= 8 && bright_pixels >= 200,
        "tree viewport is visually empty: colors={}, bright={bright_pixels}",
        region_colors.len()
    );
}

#[test]
#[ignore = "requires one native event loop, compositor and WGPU adapter"]
fn ui_bundle_phase127_tree_virtualization_capture() {
    let model = synthetic_model();
    let flatten_before = model.read(|model| model.flat_index().rebuilds());
    let diagnostics = TreeViewDiagnostics::new();
    let capture = CaptureHandle::new();
    capture.set_exit_after_all_captures(true);
    let capture_id = Arc::new(Mutex::new(None));
    let ui = UiApp::new().capture_handle(capture.clone()).window(
        WindowOptions {
            logical_window_id: WINDOW_ID.to_string(),
            title: "Phase 127 virtual TreeView".to_string(),
            decorations: false,
            ..Default::default()
        }
        .with_logical_inner_size(Size::new(1_000.0, 640.0)),
        component(
            SceneProps {
                model: model.clone(),
                diagnostics: diagnostics.clone(),
            },
            scene,
        ),
    );
    let logical_id = LogicalWindowId::new(WINDOW_ID);
    let capture_for_service = capture.clone();
    let capture_id_for_service = capture_id.clone();
    let diagnostics_for_service = diagnostics.clone();
    let model_for_service = model.clone();
    let mut baseline = None;
    let mut baseline_frame = 0_u64;
    let mut requested = false;
    let mut host = WinitHost::new(ui, NoopHostDriver).test_service(move |ui| {
        if requested {
            return;
        }
        let Some(presentation) = ui.presentation_test_state(&logical_id) else {
            return;
        };
        if presentation.rendered_frame_count < 1 {
            return;
        }
        if baseline.is_none() {
            let snapshot = diagnostics_for_service.snapshot();
            assert_eq!(snapshot.loaded_rows, ROW_COUNT as usize);
            assert!(snapshot.visible_rows > 0 && snapshot.visible_rows <= ROW_BUDGET as usize);
            assert_eq!(snapshot.virtualization_fallbacks, 0);
            assert_eq!(
                ui.presentation_test_focus_within_key(&logical_id, TREE_KEY),
                Some(true),
                "the selected deep row must be inside a focused TreeView"
            );
            baseline = Some(snapshot);
            baseline_frame = presentation.rendered_frame_count;
            ui.request_redraw_all();
            return;
        }
        if presentation.rendered_frame_count <= baseline_frame {
            return;
        }
        let before = baseline.expect("diagnostic baseline");
        let after = diagnostics_for_service.snapshot();
        assert_eq!(
            model_for_service.read(|model| model.flat_index().rebuilds()),
            flatten_before,
            "a redraw must not rebuild the persistent flat index"
        );
        assert_eq!(
            after
                .layout_rows_visited
                .saturating_sub(before.layout_rows_visited),
            0,
            "paint-only redraw must not relayout TreeView rows"
        );
        assert!(
            after
                .paint_rows_visited
                .saturating_sub(before.paint_rows_visited)
                <= ROW_BUDGET,
            "paint work must remain viewport-bounded"
        );
        *capture_id_for_service.lock().expect("capture id lock") =
            Some(capture_for_service.request_window(WINDOW_ID));
        ui.request_redraw_all();
        requested = true;
    });
    run_winit_host(&mut host).expect("run Phase 127 TreeView capture host");
    if let Some(error) = host.take_error() {
        panic!("Phase 127 TreeView capture failed: {error}");
    }
    let id = capture_id
        .lock()
        .expect("capture id lock")
        .take()
        .expect("capture request issued");
    let frame = capture
        .take_result(id)
        .expect("capture slot")
        .expect("capture result")
        .frame;
    assert_visual_frame(&frame);
    std::fs::create_dir_all(captures_dir()).expect("create capture directory");
    std::fs::write(
        captures_dir().join(CAPTURE_NAME),
        frame.png_data.as_ref().expect("PNG bytes"),
    )
    .expect("write Phase 127 TreeView capture");
    eprintln!("wrote {}", captures_dir().join(CAPTURE_NAME).display());
}
