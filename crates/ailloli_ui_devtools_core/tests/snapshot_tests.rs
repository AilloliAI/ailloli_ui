use ailloli_ui_core::geometry::{ClipShape, Rect, Size};
use ailloli_ui_core::Offset;
use ailloli_ui_devtools_core::{
    collect_debug_snapshot, pick_element_at, DebugWarningKind, DevToolsClientMessage, DevToolsMode,
};
use ailloli_ui_runtime::element::{ElementKind, ElementTree, Key};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutResult};

fn layout(size: Size, children: Vec<ChildLayout>, clip: Option<ClipShape>) -> LayoutResult {
    LayoutResult {
        size,
        children,
        paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
        visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
        overlay_hit_bounds: Vec::new(),
        clip,
        is_window_root_clip: false,
        artifact: None,
    }
}

#[test]
fn protocol_decodes_select_and_rejects_unknown_message() {
    let select = r#"{"type":"select","id":42}"#;
    let decoded: DevToolsClientMessage = serde_json::from_str(select).unwrap();
    assert_eq!(decoded, DevToolsClientMessage::Select { id: Some(42) });

    let mode = r#"{"type":"set_mode","mode":"dock_right"}"#;
    let decoded: DevToolsClientMessage = serde_json::from_str(mode).unwrap();
    assert_eq!(
        decoded,
        DevToolsClientMessage::SetMode {
            mode: DevToolsMode::DockRight
        }
    );

    assert!(serde_json::from_str::<DevToolsClientMessage>(r#"{"type":"exec"}"#).is_err());
}

#[test]
fn snapshot_warns_when_layout_exceeds_assigned_slot() {
    let mut tree = ElementTree::<()>::new();
    let root = tree.create_element(ElementKind::Empty, None, None);
    let child = tree.create_element(ElementKind::Empty, None, Some(root));
    tree.set_children(root, vec![child]);
    tree.set_layout(
        root,
        layout(
            Size::new(100.0, 100.0),
            vec![ChildLayout {
                offset: Offset::new(0.0, 0.0),
                size: Size::new(100.0, 50.0),
                paint_bounds: Rect::new(0.0, 0.0, 100.0, 50.0),
                visual_bounds: Rect::new(0.0, 0.0, 100.0, 50.0),
            }],
            None,
        ),
    );
    tree.set_layout(child, layout(Size::new(100.0, 120.0), Vec::new(), None));

    let snapshot = collect_debug_snapshot(&tree, root, Rect::new(0.0, 0.0, 100.0, 100.0));

    assert!(snapshot
        .warnings
        .iter()
        .any(|w| w.node == Some(child.0) && w.kind == DebugWarningKind::LayoutExceedsAssignedSlot));
}

#[test]
fn snapshot_warns_on_duplicate_keys() {
    let mut tree = ElementTree::<()>::new();
    let root = tree.create_element(ElementKind::Empty, None, None);
    let a = tree.create_element(
        ElementKind::Empty,
        Some(Key::String("dup".into())),
        Some(root),
    );
    let b = tree.create_element(
        ElementKind::Empty,
        Some(Key::String("dup".into())),
        Some(root),
    );
    tree.set_children(root, vec![a, b]);
    tree.set_layout(
        root,
        layout(
            Size::new(100.0, 100.0),
            vec![
                ChildLayout {
                    offset: Offset::new(0.0, 0.0),
                    size: Size::new(50.0, 50.0),
                    paint_bounds: Rect::new(0.0, 0.0, 50.0, 50.0),
                    visual_bounds: Rect::new(0.0, 0.0, 50.0, 50.0),
                },
                ChildLayout {
                    offset: Offset::new(50.0, 0.0),
                    size: Size::new(50.0, 50.0),
                    paint_bounds: Rect::new(0.0, 0.0, 50.0, 50.0),
                    visual_bounds: Rect::new(0.0, 0.0, 50.0, 50.0),
                },
            ],
            None,
        ),
    );
    tree.set_layout(a, layout(Size::new(50.0, 50.0), Vec::new(), None));
    tree.set_layout(b, layout(Size::new(50.0, 50.0), Vec::new(), None));

    let snapshot = collect_debug_snapshot(&tree, root, Rect::new(0.0, 0.0, 100.0, 100.0));

    assert_eq!(
        snapshot
            .warnings
            .iter()
            .filter(|w| w.kind == DebugWarningKind::DuplicateKey)
            .count(),
        2
    );
}

#[test]
fn picker_returns_topmost_deepest_node_and_respects_clip() {
    let mut tree = ElementTree::<()>::new();
    let root = tree.create_element(ElementKind::Empty, None, None);
    let a = tree.create_element(ElementKind::Empty, None, Some(root));
    let b = tree.create_element(ElementKind::Empty, None, Some(root));
    tree.set_children(root, vec![a, b]);
    tree.set_layout(
        root,
        layout(
            Size::new(100.0, 100.0),
            vec![
                ChildLayout {
                    offset: Offset::new(0.0, 0.0),
                    size: Size::new(100.0, 100.0),
                    paint_bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
                    visual_bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
                },
                ChildLayout {
                    offset: Offset::new(0.0, 0.0),
                    size: Size::new(100.0, 100.0),
                    paint_bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
                    visual_bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
                },
            ],
            Some(ClipShape::Rect(Rect::new(0.0, 0.0, 50.0, 50.0))),
        ),
    );
    tree.set_layout(a, layout(Size::new(100.0, 100.0), Vec::new(), None));
    tree.set_layout(b, layout(Size::new(100.0, 100.0), Vec::new(), None));

    let snapshot = collect_debug_snapshot(&tree, root, Rect::new(0.0, 0.0, 100.0, 100.0));

    assert_eq!(
        pick_element_at(&snapshot, ailloli_ui_core::Point::new(10.0, 10.0)),
        Some(b.0)
    );
    assert_eq!(
        pick_element_at(&snapshot, ailloli_ui_core::Point::new(75.0, 75.0)),
        None
    );
}
