//! Headless retained-layout scenarios for visible and hidden DevTools views.
//!
//! Tests reconcile the generated view into a runtime and verify that overlay
//! composition honors the host constraints without requiring a renderer.

use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_devtools_core::{
    DebugFlexItem, DebugLayoutInfo, DebugLayoutSizeHint, DebugLength, DebugNode, DebugRect,
    DebugSize, DebugSnapshot, DevToolsMode,
};
use ailloli_ui_devtools_ui::{build_devtools_overlay, DevToolsAction, DevToolsState};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_text::TextSystem;

/// Builds one fully laid-out selected container for overlay tests.
fn snapshot() -> DebugSnapshot {
    DebugSnapshot {
        root: 1,
        viewport: DebugRect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 300.0,
        },
        selected: Some(1),
        hovered: None,
        frame_index: 1,
        #[cfg(feature = "terminal")]
        terminal_inspections: Vec::new(),
        warnings: Vec::new(),
        nodes: vec![DebugNode {
            id: 1,
            parent: None,
            depth: 0,
            children: Vec::new(),
            widget_name: "Container".into(),
            key: Some("root".into()),
            layout_size: DebugSize { w: 400.0, h: 300.0 },
            assigned_slot: None,
            absolute_bounds: DebugRect {
                x: 0.0,
                y: 0.0,
                w: 400.0,
                h: 300.0,
            },
            paint_bounds: None,
            clip_bounds: None,
            size_hint: DebugLayoutSizeHint {
                width: DebugLength::Fill,
                height: DebugLength::Fill,
            },
            flex_item: DebugFlexItem {
                flex_grow: 0.0,
                flex_shrink: 0.0,
                flex_basis: DebugLength::Auto,
                align_self: None,
            },
            layout_debug: Some(DebugLayoutInfo {
                constraints_in: None,
                constraints_final: None,
                layout_size: DebugSize { w: 400.0, h: 300.0 },
            }),
            children_slots: Vec::new(),
            warnings: Vec::new(),
            has_layout: true,
        }],
    }
}

#[test]
fn overlay_builds_and_lays_out_without_touching_app_layout() {
    let state = DevToolsState {
        enabled: true,
        mode: DevToolsMode::Overlay,
        picker_active: false,
        selected: Some(1),
        hovered: None,
        filter: String::new(),
    };
    let view = build_devtools_overlay(&snapshot(), &state);
    let mut runtime = Runtime::<DevToolsAction>::new(RuntimeHandle::new());
    runtime.reconcile(view);
    let mut text = TextSystem::new();
    runtime.layout(Constraints::tight(400.0, 300.0), Scale::new(1.0), &mut text);

    let root = runtime.tree.root().expect("devtools root");
    let layout = runtime.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 400.0);
    assert_eq!(layout.size.h, 300.0);
}

#[test]
fn hidden_mode_returns_empty_view() {
    let state = DevToolsState {
        enabled: true,
        mode: DevToolsMode::Hidden,
        picker_active: false,
        selected: None,
        hovered: None,
        filter: String::new(),
    };
    let view = build_devtools_overlay(&snapshot(), &state);
    let mut runtime = Runtime::<DevToolsAction>::new(RuntimeHandle::new());
    runtime.reconcile(view);
    let mut text = TextSystem::new();
    runtime.layout(Constraints::tight(400.0, 300.0), Scale::new(1.0), &mut text);

    let root = runtime.tree.root().expect("devtools root");
    assert!(runtime.tree.children_of(root).is_empty());
}
