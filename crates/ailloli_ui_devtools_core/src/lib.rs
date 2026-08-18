//! Backend-agnostic DevTools data model for Ailloli UI.

use ailloli_ui_core::geometry::{ClipShape, Offset, Point, Rect};
use ailloli_ui_core::ids::ElementId;
use ailloli_ui_core::style::{AlignItems, FlexItemStyle, LayoutSizeHint, Length};
use ailloli_ui_core::{Color, Constraints, Size};
use ailloli_ui_runtime::element::{ElementKind, ElementTree, Key};
#[cfg(feature = "terminal")]
use ailloli_ui_terminal_core::{TerminalSnapshot, TerminalSnapshotConfig, TerminalState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const EPSILON: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DebugRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl DebugRect {
    pub fn right(self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    pub fn contains(self, point: DebugPoint) -> bool {
        point.x >= self.x
            && point.y >= self.y
            && point.x <= self.right()
            && point.y <= self.bottom()
    }

    fn contains_rect(self, other: DebugRect) -> bool {
        other.x + EPSILON >= self.x
            && other.y + EPSILON >= self.y
            && other.right() <= self.right() + EPSILON
            && other.bottom() <= self.bottom() + EPSILON
    }
}

impl From<Rect> for DebugRect {
    fn from(value: Rect) -> Self {
        Self {
            x: value.x,
            y: value.y,
            w: value.w,
            h: value.h,
        }
    }
}

impl From<DebugRect> for Rect {
    fn from(value: DebugRect) -> Self {
        Rect::new(value.x, value.y, value.w, value.h)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DebugSize {
    pub w: f32,
    pub h: f32,
}

impl From<Size> for DebugSize {
    fn from(value: Size) -> Self {
        Self {
            w: value.w,
            h: value.h,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DebugOffset {
    pub x: f32,
    pub y: f32,
}

impl From<Offset> for DebugOffset {
    fn from(value: Offset) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DebugPoint {
    pub x: f32,
    pub y: f32,
}

impl From<Point> for DebugPoint {
    fn from(value: Point) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DebugColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl From<Color> for DebugColor {
    fn from(value: Color) -> Self {
        Self {
            r: value.r,
            g: value.g,
            b: value.b,
            a: value.a,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DebugConstraints {
    pub min_w: f32,
    pub max_w: f32,
    pub min_h: f32,
    pub max_h: f32,
}

impl From<Constraints> for DebugConstraints {
    fn from(value: Constraints) -> Self {
        Self {
            min_w: value.min_w,
            max_w: value.max_w,
            min_h: value.min_h,
            max_h: value.max_h,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum DebugLength {
    Auto,
    Px(f32),
    Fill,
    Percent(f32),
}

impl From<Length> for DebugLength {
    fn from(value: Length) -> Self {
        match value {
            Length::Auto => Self::Auto,
            Length::Px(v) => Self::Px(v),
            Length::Fill => Self::Fill,
            Length::Percent(v) => Self::Percent(v),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugLayoutSizeHint {
    pub width: DebugLength,
    pub height: DebugLength,
}

impl From<LayoutSizeHint> for DebugLayoutSizeHint {
    fn from(value: LayoutSizeHint) -> Self {
        Self {
            width: value.width.into(),
            height: value.height.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugFlexItem {
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: DebugLength,
    pub align_self: Option<String>,
}

impl From<FlexItemStyle> for DebugFlexItem {
    fn from(value: FlexItemStyle) -> Self {
        Self {
            flex_grow: value.flex_grow,
            flex_shrink: value.flex_shrink,
            flex_basis: value.flex_basis.into(),
            align_self: value.align_self.map(align_items_name).map(str::to_string),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugLayoutInfo {
    pub constraints_in: Option<DebugConstraints>,
    pub constraints_final: Option<DebugConstraints>,
    pub layout_size: DebugSize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugSlot {
    pub child: u64,
    pub offset: DebugOffset,
    pub size: DebugSize,
    pub absolute: DebugRect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugNode {
    pub id: u64,
    pub parent: Option<u64>,
    pub depth: usize,
    pub children: Vec<u64>,
    pub widget_name: String,
    pub key: Option<String>,
    pub layout_size: DebugSize,
    pub assigned_slot: Option<DebugRect>,
    pub absolute_bounds: DebugRect,
    pub paint_bounds: Option<DebugRect>,
    pub clip_bounds: Option<DebugRect>,
    pub size_hint: DebugLayoutSizeHint,
    pub flex_item: DebugFlexItem,
    pub layout_debug: Option<DebugLayoutInfo>,
    pub children_slots: Vec<DebugSlot>,
    pub warnings: Vec<DebugWarning>,
    pub has_layout: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugWarningKind {
    OutsideViewport,
    SlotOutsideParent,
    LayoutExceedsAssignedSlot,
    ChildOutsideParentClip,
    FlexMeasuredLargerThanSlot,
    DuplicateKey,
    MissingLayout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugWarning {
    pub node: Option<u64>,
    pub kind: DebugWarningKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevToolsMode {
    Hidden,
    #[default]
    Overlay,
    DockRight,
    DockBottom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugSnapshot {
    pub root: u64,
    pub viewport: DebugRect,
    pub nodes: Vec<DebugNode>,
    pub warnings: Vec<DebugWarning>,
    #[cfg(feature = "terminal")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal_inspections: Vec<TerminalDebugSnapshot>,
    pub selected: Option<u64>,
    pub hovered: Option<u64>,
    pub frame_index: u64,
}

impl DebugSnapshot {
    pub fn node(&self, id: u64) -> Option<&DebugNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: u64) -> Option<&mut DebugNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }
}

#[cfg(feature = "terminal")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalDebugSnapshot {
    pub id: String,
    pub title: String,
    pub snapshot: TerminalSnapshot,
    pub stable_ids: Vec<String>,
}

#[cfg(feature = "terminal")]
impl TerminalDebugSnapshot {
    pub fn from_terminal_snapshot(
        id: impl Into<String>,
        title: impl Into<String>,
        snapshot: TerminalSnapshot,
    ) -> Self {
        let id = id.into();
        let stable_ids = snapshot
            .lines
            .iter()
            .flat_map(|line| {
                let line_id = line
                    .global_index
                    .map(|idx| idx.to_string())
                    .unwrap_or_else(|| format!("visual-{}", line.visual_index));
                let terminal_id = id.clone();
                line.cells.iter().map(move |cell| {
                    format!("terminal:{terminal_id}:line:{line_id}:cell:{}", cell.col)
                })
            })
            .collect();
        Self {
            id,
            title: title.into(),
            snapshot,
            stable_ids,
        }
    }
}

#[cfg(feature = "terminal")]
pub fn terminal_debug_snapshot(
    id: impl Into<String>,
    title: impl Into<String>,
    state: &TerminalState,
    config: TerminalSnapshotConfig,
) -> TerminalDebugSnapshot {
    TerminalDebugSnapshot::from_terminal_snapshot(
        id,
        title,
        TerminalSnapshot::from_state(state, config),
    )
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DebugDrawCmd {
    RectOutline {
        rect: DebugRect,
        color: DebugColor,
        thickness: f32,
    },
    RectFill {
        rect: DebugRect,
        color: DebugColor,
    },
    TextLabel {
        pos: DebugPoint,
        text: String,
        color: DebugColor,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DevToolsClientMessage {
    Select { id: Option<u64> },
    Hover { id: Option<u64> },
    SetMode { mode: DevToolsMode },
    Ping,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DevToolsServerMessage {
    Hello { protocol: u32 },
    Snapshot { snapshot: DebugSnapshot },
    Pong,
    Error { message: String },
}

pub fn collect_debug_snapshot<A: 'static>(
    tree: &ElementTree<A>,
    root: ElementId,
    viewport: Rect,
) -> DebugSnapshot {
    collect_debug_snapshot_with_state(tree, root, viewport, None, None, 0)
}

pub fn collect_debug_snapshot_with_state<A: 'static>(
    tree: &ElementTree<A>,
    root: ElementId,
    viewport: Rect,
    selected: Option<ElementId>,
    hovered: Option<ElementId>,
    frame_index: u64,
) -> DebugSnapshot {
    let mut snapshot = DebugSnapshot {
        root: root.0,
        viewport: viewport.into(),
        nodes: Vec::new(),
        warnings: Vec::new(),
        #[cfg(feature = "terminal")]
        terminal_inspections: Vec::new(),
        selected: selected.map(|id| id.0),
        hovered: hovered.map(|id| id.0),
        frame_index,
    };
    collect_node(
        tree,
        root,
        None,
        0,
        None,
        DebugRect::from(viewport),
        &mut snapshot,
    );
    compute_warnings(&mut snapshot);
    snapshot
}

pub fn compute_warnings(snapshot: &mut DebugSnapshot) {
    let mut warnings = Vec::new();
    let by_id = snapshot
        .nodes
        .iter()
        .enumerate()
        .map(|(idx, node)| (node.id, idx))
        .collect::<HashMap<_, _>>();

    let mut key_counts = HashMap::<String, usize>::new();
    for node in &snapshot.nodes {
        if let Some(key) = &node.key {
            *key_counts.entry(key.clone()).or_default() += 1;
        }
    }

    let mut per_node = HashMap::<u64, Vec<DebugWarning>>::new();
    for node in &snapshot.nodes {
        if !node.has_layout {
            push_warning(
                &mut warnings,
                &mut per_node,
                node.id,
                DebugWarningKind::MissingLayout,
                "node is in the tree but has no layout",
            );
            continue;
        }

        if !snapshot.viewport.contains_rect(node.absolute_bounds) {
            push_warning(
                &mut warnings,
                &mut per_node,
                node.id,
                DebugWarningKind::OutsideViewport,
                "node bounds exceed the viewport",
            );
        }

        if let Some(parent_id) = node.parent {
            if let Some(parent_idx) = by_id.get(&parent_id) {
                let parent = &snapshot.nodes[*parent_idx];
                if let Some(slot) = node.assigned_slot {
                    if !parent.absolute_bounds.contains_rect(slot) {
                        push_warning(
                            &mut warnings,
                            &mut per_node,
                            node.id,
                            DebugWarningKind::SlotOutsideParent,
                            "assigned slot exceeds parent bounds",
                        );
                    }
                    if let Some(clip) = parent.clip_bounds {
                        if !clip.contains_rect(slot) {
                            push_warning(
                                &mut warnings,
                                &mut per_node,
                                node.id,
                                DebugWarningKind::ChildOutsideParentClip,
                                "assigned slot exceeds parent clip",
                            );
                        }
                    }
                    if size_exceeds_slot(node.layout_size, slot) {
                        push_warning(
                            &mut warnings,
                            &mut per_node,
                            node.id,
                            DebugWarningKind::LayoutExceedsAssignedSlot,
                            "layout size differs from assigned slot size",
                        );
                        if node.flex_item.flex_grow > 0.0
                            || matches!(node.size_hint.width, DebugLength::Fill)
                            || matches!(node.size_hint.height, DebugLength::Fill)
                        {
                            push_warning(
                                &mut warnings,
                                &mut per_node,
                                node.id,
                                DebugWarningKind::FlexMeasuredLargerThanSlot,
                                "flex/fill item layout is larger than its final slot",
                            );
                        }
                    }
                }
            }
        }

        if let Some(key) = &node.key {
            if key_counts.get(key).copied().unwrap_or_default() > 1 {
                push_warning(
                    &mut warnings,
                    &mut per_node,
                    node.id,
                    DebugWarningKind::DuplicateKey,
                    "duplicate view key in this tree",
                );
            }
        }
    }

    for node in &mut snapshot.nodes {
        node.warnings = per_node.remove(&node.id).unwrap_or_default();
    }
    snapshot.warnings = warnings;
}

pub fn pick_element_at(snapshot: &DebugSnapshot, point: impl Into<DebugPoint>) -> Option<u64> {
    let point = point.into();
    let by_id = snapshot
        .nodes
        .iter()
        .map(|node| (node.id, node))
        .collect::<HashMap<_, _>>();
    for node in snapshot.nodes.iter().rev() {
        if !node.has_layout || !node.absolute_bounds.contains(point) {
            continue;
        }
        if let Some(clip) = node.clip_bounds {
            if !clip.contains(point) {
                continue;
            }
        }
        if !ancestor_clips_contain(&by_id, node, point) {
            continue;
        }
        return Some(node.id);
    }
    None
}

pub fn debug_draw_cmds(snapshot: &DebugSnapshot) -> Vec<DebugDrawCmd> {
    let mut out = Vec::new();
    if let Some(id) = snapshot.hovered {
        if let Some(node) = snapshot.node(id) {
            out.push(DebugDrawCmd::RectOutline {
                rect: node.absolute_bounds,
                color: Color::rgba(70, 150, 255, 0.95).into(),
                thickness: 2.0,
            });
        }
    }
    if let Some(id) = snapshot.selected {
        if let Some(node) = snapshot.node(id) {
            out.push(DebugDrawCmd::RectOutline {
                rect: node.absolute_bounds,
                color: Color::rgba(255, 64, 64, 0.95).into(),
                thickness: 2.0,
            });
        }
    }
    for warning in &snapshot.warnings {
        let Some(id) = warning.node else {
            continue;
        };
        let Some(node) = snapshot.node(id) else {
            continue;
        };
        out.push(DebugDrawCmd::RectOutline {
            rect: node.absolute_bounds,
            color: Color::rgba(255, 210, 0, 0.65).into(),
            thickness: 1.0,
        });
    }
    out
}

fn collect_node<A: 'static>(
    tree: &ElementTree<A>,
    id: ElementId,
    parent: Option<ElementId>,
    depth: usize,
    assigned_slot: Option<DebugRect>,
    fallback_bounds: DebugRect,
    snapshot: &mut DebugSnapshot,
) {
    let Some(el) = tree.get(id) else {
        return;
    };

    let has_layout = el.layout.is_some();
    let absolute_bounds = assigned_slot
        .or_else(|| el.layout.as_ref().map(|layout| layout.paint_bounds.into()))
        .unwrap_or(fallback_bounds);
    let paint_bounds = el.layout.as_ref().map(|layout| {
        layout
            .paint_bounds
            .translate(Offset::new(absolute_bounds.x, absolute_bounds.y))
            .into()
    });
    let clip_bounds = el.layout.as_ref().and_then(|layout| {
        layout.clip.map(|clip| {
            translate_clip(clip, Offset::new(absolute_bounds.x, absolute_bounds.y))
                .bounding_rect()
                .into()
        })
    });
    let layout_size = el
        .layout
        .as_ref()
        .map(|layout| layout.size)
        .unwrap_or_default();
    let children = el.children.iter().map(|child| child.0).collect::<Vec<_>>();
    let children_slots = el
        .layout
        .as_ref()
        .map(|layout| {
            layout
                .children
                .iter()
                .enumerate()
                .filter_map(|(idx, slot)| {
                    let child = el.children.get(idx)?;
                    let absolute = DebugRect {
                        x: absolute_bounds.x + slot.offset.x,
                        y: absolute_bounds.y + slot.offset.y,
                        w: slot.size.w,
                        h: slot.size.h,
                    };
                    Some(DebugSlot {
                        child: child.0,
                        offset: slot.offset.into(),
                        size: slot.size.into(),
                        absolute,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    snapshot.nodes.push(DebugNode {
        id: id.0,
        parent: parent.map(|id| id.0),
        depth,
        children,
        widget_name: widget_name(&el.kind).to_string(),
        key: key_string(&el.key),
        layout_size: layout_size.into(),
        assigned_slot,
        absolute_bounds,
        paint_bounds,
        clip_bounds,
        size_hint: el.size_hint.into(),
        flex_item: el.flex_item.into(),
        layout_debug: layout_debug(tree, id, layout_size),
        children_slots,
        warnings: Vec::new(),
        has_layout,
    });

    if let Some(layout) = el.layout.as_ref() {
        for (idx, child_id) in el.children.iter().copied().enumerate() {
            let child_slot = layout.children.get(idx).map(|slot| DebugRect {
                x: absolute_bounds.x + slot.offset.x,
                y: absolute_bounds.y + slot.offset.y,
                w: slot.size.w,
                h: slot.size.h,
            });
            collect_node(
                tree,
                child_id,
                Some(id),
                depth + 1,
                child_slot,
                absolute_bounds,
                snapshot,
            );
        }
    } else {
        for child_id in el.children.iter().copied() {
            collect_node(
                tree,
                child_id,
                Some(id),
                depth + 1,
                None,
                absolute_bounds,
                snapshot,
            );
        }
    }
}

fn layout_debug<A>(
    tree: &ElementTree<A>,
    id: ElementId,
    _layout_size: Size,
) -> Option<DebugLayoutInfo> {
    let debug = tree.get(id)?.layout_debug.as_ref()?;
    Some(DebugLayoutInfo {
        constraints_in: Some(debug.constraints_in.into()),
        constraints_final: debug.constraints_final.map(Into::into),
        layout_size: debug.layout_size.into(),
    })
}

fn push_warning(
    warnings: &mut Vec<DebugWarning>,
    per_node: &mut HashMap<u64, Vec<DebugWarning>>,
    node: u64,
    kind: DebugWarningKind,
    message: &'static str,
) {
    let warning = DebugWarning {
        node: Some(node),
        kind,
        message: message.to_string(),
    };
    warnings.push(warning.clone());
    per_node.entry(node).or_default().push(warning);
}

fn size_exceeds_slot(layout_size: DebugSize, slot: DebugRect) -> bool {
    (layout_size.w - slot.w).abs() > EPSILON || (layout_size.h - slot.h).abs() > EPSILON
}

fn ancestor_clips_contain(
    by_id: &HashMap<u64, &DebugNode>,
    node: &DebugNode,
    point: DebugPoint,
) -> bool {
    let mut current = node.parent;
    while let Some(id) = current {
        let Some(parent) = by_id.get(&id) else {
            return true;
        };
        if let Some(clip) = parent.clip_bounds {
            if !clip.contains(point) {
                return false;
            }
        }
        current = parent.parent;
    }
    true
}

fn translate_clip(clip: ClipShape, origin: Offset) -> ClipShape {
    match clip {
        ClipShape::Rect(r) => ClipShape::Rect(r.translate(origin)),
        ClipShape::RoundRect { rect, radius } => ClipShape::RoundRect {
            rect: rect.translate(origin),
            radius,
        },
    }
}

fn widget_name<A: 'static>(kind: &ElementKind<A>) -> &'static str {
    match kind {
        ElementKind::Empty => "Empty",
        ElementKind::Widget(widget) => widget.debug_name(),
        ElementKind::Component(_) => "Component",
    }
}

fn key_string(key: &Option<Key>) -> Option<String> {
    match key {
        None => None,
        Some(Key::Static(value)) => Some((*value).to_string()),
        Some(Key::String(value)) => Some(value.clone()),
        Some(Key::U64(value)) => Some(value.to_string()),
    }
}

fn align_items_name(value: AlignItems) -> &'static str {
    match value {
        AlignItems::Start => "start",
        AlignItems::Center => "center",
        AlignItems::End => "end",
        AlignItems::Stretch => "stretch",
    }
}
