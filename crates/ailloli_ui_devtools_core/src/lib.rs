//! Backend-agnostic DevTools snapshots, warnings, picking, and wire messages.
//!
//! Snapshot geometry uses logical pixels and stable numeric element IDs. The
//! collector walks retained children in tree order; picking walks the flattened
//! snapshot in reverse so later, deeper paint candidates win. Warning geometry
//! comparisons tolerate half a logical pixel.

use ailloli_ui_core::geometry::{ClipShape, Offset, Point, Rect};
use ailloli_ui_core::ids::ElementId;
use ailloli_ui_core::style::{AlignItems, FlexItemStyle, LayoutSizeHint, Length};
use ailloli_ui_core::{Color, Constraints, Size};
use ailloli_ui_runtime::element::{ElementKind, ElementTree, Key};
#[cfg(feature = "terminal")]
use ailloli_ui_terminal_core::{TerminalSnapshot, TerminalSnapshotConfig, TerminalState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Logical-pixel tolerance used by containment and assigned-size diagnostics.
const EPSILON: f32 = 0.5;

/// Serializable logical-pixel rectangle.
///
/// Edges are inclusive for point picking. Width and height are stored verbatim;
/// snapshot collection normally supplies non-negative layout values.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::DebugRect;
/// let rect = DebugRect { x: 10.0, y: 20.0, w: 30.0, h: 40.0 };
/// assert_eq!((rect.right(), rect.bottom()), (40.0, 60.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DebugRect {
    /// Left edge in logical pixels.
    pub x: f32,
    /// Top edge in logical pixels.
    pub y: f32,
    /// Width in logical pixels.
    pub w: f32,
    /// Height in logical pixels.
    pub h: f32,
}

/// Edge and containment helpers for debug rectangles.
impl DebugRect {
    /// Returns `x + w` without clamping or overflow handling.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_devtools_core::DebugRect;
    /// assert_eq!(DebugRect { x: 2.0, y: 0.0, w: 3.0, h: 1.0 }.right(), 5.0);
    /// ```
    pub fn right(self) -> f32 {
        self.x + self.w
    }

    /// Returns `y + h` without clamping or overflow handling.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_devtools_core::DebugRect;
    /// assert_eq!(DebugRect { x: 0.0, y: 4.0, w: 1.0, h: 6.0 }.bottom(), 10.0);
    /// ```
    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    /// Returns whether `point` lies on or within all four edges.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_devtools_core::{DebugPoint, DebugRect};
    /// let rect = DebugRect { x: 0.0, y: 0.0, w: 10.0, h: 5.0 };
    /// assert!(rect.contains(DebugPoint { x: 10.0, y: 5.0 }));
    /// assert!(!rect.contains(DebugPoint { x: 10.1, y: 5.0 }));
    /// ```
    pub fn contains(self, point: DebugPoint) -> bool {
        point.x >= self.x
            && point.y >= self.y
            && point.x <= self.right()
            && point.y <= self.bottom()
    }

    /// Returns whether `other` stays within each edge with [`EPSILON`] tolerance.
    fn contains_rect(self, other: DebugRect) -> bool {
        other.x + EPSILON >= self.x
            && other.y + EPSILON >= self.y
            && other.right() <= self.right() + EPSILON
            && other.bottom() <= self.bottom() + EPSILON
    }
}

/// Converts core logical geometry without normalization.
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

/// Converts back to core logical geometry without normalization.
impl From<DebugRect> for Rect {
    fn from(value: DebugRect) -> Self {
        Rect::new(value.x, value.y, value.w, value.h)
    }
}

/// Serializable logical width and height.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::DebugSize;
/// let size = DebugSize { w: 640.0, h: 480.0 };
/// assert_eq!(size.h, 480.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DebugSize {
    /// Width in logical pixels.
    pub w: f32,
    /// Height in logical pixels.
    pub h: f32,
}

/// Copies core size components verbatim.
impl From<Size> for DebugSize {
    fn from(value: Size) -> Self {
        Self {
            w: value.w,
            h: value.h,
        }
    }
}

/// Serializable logical translation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::DebugOffset;
/// let offset = DebugOffset { x: -4.0, y: 8.0 };
/// assert_eq!((offset.x, offset.y), (-4.0, 8.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DebugOffset {
    /// Horizontal delta in logical pixels.
    pub x: f32,
    /// Vertical delta in logical pixels.
    pub y: f32,
}

/// Copies core offset components verbatim.
impl From<Offset> for DebugOffset {
    fn from(value: Offset) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

/// Serializable point in logical viewport coordinates.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::DebugPoint;
/// let point = DebugPoint { x: 12.0, y: 24.0 };
/// assert_eq!(point.x, 12.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DebugPoint {
    /// Horizontal coordinate in logical pixels.
    pub x: f32,
    /// Vertical coordinate in logical pixels.
    pub y: f32,
}

/// Copies core point components verbatim.
impl From<Point> for DebugPoint {
    fn from(value: Point) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

/// Serializable floating-point RGBA color.
///
/// Channels are copied verbatim from [`Color`], normally in `[0, 1]`, and are
/// not clamped by this diagnostic representation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::DebugColor;
/// let color = DebugColor { r: 1.0, g: 0.5, b: 0.0, a: 1.0 };
/// assert_eq!(color.a, 1.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DebugColor {
    /// Red channel.
    pub r: f32,
    /// Green channel.
    pub g: f32,
    /// Blue channel.
    pub b: f32,
    /// Alpha channel where zero is transparent and one is opaque.
    pub a: f32,
}

/// Copies core color channels verbatim.
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

/// Serializable min/max layout constraints in logical pixels.
///
/// Maximum values may be positive infinity to mean unbounded; no normalization
/// is performed during conversion.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::DebugConstraints;
/// let constraints = DebugConstraints { min_w: 0.0, max_w: 800.0, min_h: 20.0, max_h: 600.0 };
/// assert!(constraints.min_w <= constraints.max_w);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DebugConstraints {
    /// Minimum width in logical pixels.
    pub min_w: f32,
    /// Maximum width in logical pixels.
    pub max_w: f32,
    /// Minimum height in logical pixels.
    pub min_h: f32,
    /// Maximum height in logical pixels.
    pub max_h: f32,
}

/// Copies core constraint components verbatim.
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

/// Serializable declarative length used by size hints and flex basis.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::DebugLength;
/// let length = DebugLength::Percent(0.5);
/// assert_eq!(serde_json::to_string(&length).unwrap(), r#"{"kind":"Percent","value":0.5}"#);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum DebugLength {
    /// Intrinsic sizing.
    Auto,
    /// Fixed logical pixels, stored verbatim.
    Px(f32),
    /// Fill/remaining-space sizing.
    Fill,
    /// Fraction of parent available space, where `0.5` means 50%.
    Percent(f32),
}

/// Preserves the exact core length variant and payload.
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

/// Serializable width/height hints exposed by a retained element.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::{DebugLayoutSizeHint, DebugLength};
/// let hint = DebugLayoutSizeHint { width: DebugLength::Fill, height: DebugLength::Px(24.0) };
/// assert_eq!(hint.height, DebugLength::Px(24.0));
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugLayoutSizeHint {
    /// Declarative horizontal length.
    pub width: DebugLength,
    /// Declarative vertical length.
    pub height: DebugLength,
}

/// Converts both core size-hint axes.
impl From<LayoutSizeHint> for DebugLayoutSizeHint {
    fn from(value: LayoutSizeHint) -> Self {
        Self {
            width: value.width.into(),
            height: value.height.into(),
        }
    }
}

/// Serializable flex-item inputs attached to a retained child.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::{DebugFlexItem, DebugLength};
/// let item = DebugFlexItem { flex_grow: 1.0, flex_shrink: 0.0,
///     flex_basis: DebugLength::Auto, align_self: None };
/// assert_eq!(item.align_self, None);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugFlexItem {
    /// Non-negative grow factor copied from computed style.
    pub flex_grow: f32,
    /// Non-negative shrink factor copied from computed style.
    pub flex_shrink: f32,
    /// Preferred main-axis basis.
    pub flex_basis: DebugLength,
    /// Lowercase alignment name, or `None` to inherit the container.
    pub align_self: Option<String>,
}

/// Converts numeric flex values, basis, and optional alignment name.
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

/// Optional layout-pass inputs and measured output for one element.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::{DebugLayoutInfo, DebugSize};
/// let info = DebugLayoutInfo { constraints_in: None, constraints_final: None,
///     layout_size: DebugSize { w: 10.0, h: 20.0 } };
/// assert_eq!(info.layout_size.w, 10.0);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugLayoutInfo {
    /// Constraints entering the layout pass, when runtime diagnostics retained them.
    pub constraints_in: Option<DebugConstraints>,
    /// Constraints after widget adjustment, when recorded.
    pub constraints_final: Option<DebugConstraints>,
    /// Size returned by the recorded layout pass, in logical pixels.
    pub layout_size: DebugSize,
}

/// Parent-assigned child slot in local and absolute logical coordinates.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::{DebugOffset, DebugRect, DebugSize, DebugSlot};
/// let slot = DebugSlot { child: 7, offset: DebugOffset { x: 2.0, y: 3.0 },
///     size: DebugSize { w: 10.0, h: 20.0 },
///     absolute: DebugRect { x: 12.0, y: 13.0, w: 10.0, h: 20.0 } };
/// assert_eq!(slot.child, 7);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugSlot {
    /// Numeric child element ID.
    pub child: u64,
    /// Offset from the parent's absolute origin, in logical pixels.
    pub offset: DebugOffset,
    /// Assigned width and height in logical pixels.
    pub size: DebugSize,
    /// Slot translated into viewport coordinates.
    pub absolute: DebugRect,
}

/// Flattened retained-tree node with layout, clip, style, and warning diagnostics.
///
/// Nodes are emitted in pre-order. Child IDs preserve retained child order, and
/// `depth` is zero for the requested root. Bounds are logical viewport pixels.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_devtools_core::DebugNode;
/// fn describe(node: &DebugNode) -> (&str, usize, bool) {
///     (&node.widget_name, node.depth, node.has_layout)
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugNode {
    /// Numeric [`ElementId`] value.
    pub id: u64,
    /// Parent ID, or `None` for the collected root.
    pub parent: Option<u64>,
    /// Edge distance from the collected root.
    pub depth: usize,
    /// Child IDs in retained order, including children without layout.
    pub children: Vec<u64>,
    /// Widget debug name, or `Empty`/`Component` for structural nodes.
    pub widget_name: String,
    /// String representation of the retained key, if any.
    pub key: Option<String>,
    /// Layout result size in logical pixels, or zero when layout is absent.
    pub layout_size: DebugSize,
    /// Parent-assigned absolute slot; `None` for roots or unlaid-out parents.
    pub assigned_slot: Option<DebugRect>,
    /// Best available absolute bounds, falling back through layout and ancestor bounds.
    pub absolute_bounds: DebugRect,
    /// Paint bounds translated by the node origin, when layout exists.
    pub paint_bounds: Option<DebugRect>,
    /// Bounding rectangle of the translated clip shape, when present.
    pub clip_bounds: Option<DebugRect>,
    /// Declarative width and height hints.
    pub size_hint: DebugLayoutSizeHint,
    /// Flex-item inputs attached by the parent-facing element.
    pub flex_item: DebugFlexItem,
    /// Recorded layout-pass diagnostics, when runtime instrumentation retained them.
    pub layout_debug: Option<DebugLayoutInfo>,
    /// Parent-local and absolute slots paired with existing child IDs.
    pub children_slots: Vec<DebugSlot>,
    /// Warnings attributed to this node, refreshed by [`compute_warnings`].
    pub warnings: Vec<DebugWarning>,
    /// Whether this retained node currently has a layout result.
    pub has_layout: bool,
}

/// Stable warning categories emitted by [`compute_warnings`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::DebugWarningKind;
/// assert_eq!(serde_json::to_string(&DebugWarningKind::DuplicateKey).unwrap(), "\"duplicate_key\"");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugWarningKind {
    /// Node bounds exceed the snapshot viewport by more than 0.5 logical pixels.
    OutsideViewport,
    /// Parent-assigned slot exceeds parent bounds by more than the tolerance.
    SlotOutsideParent,
    /// Measured size differs from assigned slot by more than the tolerance.
    LayoutExceedsAssignedSlot,
    /// Child slot exceeds its parent's clip bounding rectangle.
    ChildOutsideParentClip,
    /// A fill/growing item measured larger than its final slot.
    FlexMeasuredLargerThanSlot,
    /// A nonempty retained key occurs more than once in the snapshot.
    DuplicateKey,
    /// Retained node has no current layout result.
    MissingLayout,
}

/// Human-readable diagnostic optionally attributed to one node.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::{DebugWarning, DebugWarningKind};
/// let warning = DebugWarning { node: Some(4), kind: DebugWarningKind::MissingLayout,
///     message: "node is in the tree but has no layout".into() };
/// assert_eq!(warning.node, Some(4));
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugWarning {
    /// Affected element ID, or `None` for future snapshot-wide warnings.
    pub node: Option<u64>,
    /// Stable machine-readable category.
    pub kind: DebugWarningKind,
    /// Human-readable English explanation; not a stable protocol key.
    pub message: String,
}

/// Requested DevTools presentation mode.
///
/// `Overlay` is the default. This core model does not implement docking; hosts
/// interpret the serialized selection.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::DevToolsMode;
/// assert_eq!(DevToolsMode::default(), DevToolsMode::Overlay);
/// assert_eq!(serde_json::to_string(&DevToolsMode::DockRight).unwrap(), "\"dock_right\"");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevToolsMode {
    /// Do not render DevTools UI.
    Hidden,
    /// Draw on top of application content.
    #[default]
    Overlay,
    /// Reserve a dock on the logical right side.
    DockRight,
    /// Reserve a dock on the logical bottom side.
    DockBottom,
}

/// Serializable snapshot of one retained subtree and its derived diagnostics.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_devtools_core::DebugSnapshot;
/// fn selected(snapshot: &DebugSnapshot) -> Option<u64> { snapshot.selected }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugSnapshot {
    /// Requested root element ID, even if the ID was absent from the tree.
    pub root: u64,
    /// Logical viewport used by outside-viewport warnings.
    pub viewport: DebugRect,
    /// Pre-order flattened nodes reachable from `root`.
    pub nodes: Vec<DebugNode>,
    /// All derived warnings in node traversal/category order.
    pub warnings: Vec<DebugWarning>,
    #[cfg(feature = "terminal")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Optional terminal inspections; omitted from JSON when the feature is enabled but empty.
    pub terminal_inspections: Vec<TerminalDebugSnapshot>,
    /// Selected element ID copied from caller state, whether or not it is present.
    pub selected: Option<u64>,
    /// Hovered element ID copied from caller state, whether or not it is present.
    pub hovered: Option<u64>,
    /// Opaque monotonically increasing frame identity supplied by the host.
    pub frame_index: u64,
}

/// Linear ID lookup helpers over snapshot nodes.
impl DebugSnapshot {
    /// Returns the first node with `id`, or `None` when absent.
    ///
    /// Complexity is O(number of nodes).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_devtools_core::DebugSnapshot;
    /// fn root_name(snapshot: &DebugSnapshot) -> Option<&str> {
    ///     snapshot.node(snapshot.root).map(|node| node.widget_name.as_str())
    /// }
    /// ```
    pub fn node(&self, id: u64) -> Option<&DebugNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Returns the first mutable node with `id`, or `None` when absent.
    ///
    /// Complexity is O(number of nodes). Mutating geometry does not recompute
    /// warnings automatically; call [`compute_warnings`] afterward when needed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_devtools_core::DebugSnapshot;
    /// fn clear_node_warnings(snapshot: &mut DebugSnapshot, id: u64) {
    ///     if let Some(node) = snapshot.node_mut(id) { node.warnings.clear(); }
    /// }
    /// ```
    pub fn node_mut(&mut self, id: u64) -> Option<&mut DebugNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }
}

#[cfg(feature = "terminal")]
/// Terminal snapshot plus deterministic cell identities for DevTools inspection.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_devtools_core::TerminalDebugSnapshot;
/// fn dimensions(terminal: &TerminalDebugSnapshot) -> (usize, usize) {
///     (terminal.snapshot.lines.len(), terminal.stable_ids.len())
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalDebugSnapshot {
    /// Host-supplied terminal identity embedded in every stable cell ID.
    pub id: String,
    /// Human-readable terminal title.
    pub title: String,
    /// Bounded terminal viewport/history snapshot.
    pub snapshot: TerminalSnapshot,
    /// Cell IDs in line order, then column order, parallel to all snapshot cells.
    pub stable_ids: Vec<String>,
}

#[cfg(feature = "terminal")]
/// Construction from an already captured terminal snapshot.
impl TerminalDebugSnapshot {
    /// Builds stable IDs as `terminal:ID:line:LINE:cell:COLUMN`.
    ///
    /// Logical history lines use their global index; visual-only lines use the
    /// `visual-N` sentinel. No terminal text is embedded in the IDs.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_devtools_core::TerminalDebugSnapshot;
    /// use ailloli_ui_terminal_core::TerminalSnapshot;
    /// fn wrap(snapshot: TerminalSnapshot) -> TerminalDebugSnapshot {
    ///     TerminalDebugSnapshot::from_terminal_snapshot("shell-1", "Shell", snapshot)
    /// }
    /// ```
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
/// Captures terminal state with `config` and adds deterministic cell identities.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_devtools_core::terminal_debug_snapshot;
/// use ailloli_ui_terminal_core::{TerminalSnapshotConfig, TerminalState};
/// fn capture(state: &TerminalState, config: TerminalSnapshotConfig) {
///     let snapshot = terminal_debug_snapshot("shell-1", "Shell", state, config);
///     assert_eq!(snapshot.id, "shell-1");
/// }
/// ```
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

/// Backend-neutral overlay primitive derived from hover, selection, or warnings.
///
/// All positions and thicknesses are logical pixels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::{DebugColor, DebugDrawCmd, DebugRect};
/// let cmd = DebugDrawCmd::RectOutline { rect: DebugRect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
///     color: DebugColor { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }, thickness: 2.0 };
/// assert!(matches!(cmd, DebugDrawCmd::RectOutline { thickness: 2.0, .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DebugDrawCmd {
    /// Stroked rectangle overlay.
    RectOutline {
        /// Bounds in logical viewport coordinates.
        rect: DebugRect,
        /// Stroke color.
        color: DebugColor,
        /// Stroke thickness in logical pixels.
        thickness: f32,
    },
    /// Filled rectangle overlay.
    RectFill {
        /// Bounds in logical viewport coordinates.
        rect: DebugRect,
        /// Fill color.
        color: DebugColor,
    },
    /// Text label anchored at a logical viewport point.
    TextLabel {
        /// Label origin in logical pixels.
        pos: DebugPoint,
        /// UTF-8 label contents.
        text: String,
        /// Text color.
        color: DebugColor,
    },
}

/// Tagged client-to-host DevTools protocol message.
///
/// Unknown `type` values are rejected by Serde. `Select`/`Hover` with `None`
/// clear the corresponding host state.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::DevToolsClientMessage;
/// let message: DevToolsClientMessage = serde_json::from_str(r#"{"type":"select","id":42}"#).unwrap();
/// assert_eq!(message, DevToolsClientMessage::Select { id: Some(42) });
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DevToolsClientMessage {
    /// Selects or clears an element.
    Select {
        /// Numeric element ID, or `None` to clear selection.
        id: Option<u64>,
    },
    /// Sets or clears the hovered element.
    Hover {
        /// Numeric element ID, or `None` to clear hover.
        id: Option<u64>,
    },
    /// Requests a host presentation mode.
    SetMode {
        /// Requested mode.
        mode: DevToolsMode,
    },
    /// Liveness request answered by [`DevToolsServerMessage::Pong`].
    Ping,
}

/// Tagged host-to-client DevTools protocol message.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::DevToolsServerMessage;
/// let hello = DevToolsServerMessage::Hello { protocol: 1 };
/// assert_eq!(serde_json::to_string(&hello).unwrap(), r#"{"type":"hello","protocol":1}"#);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DevToolsServerMessage {
    /// Announces the host-selected protocol version.
    Hello {
        /// Protocol version interpreted by the host/client pair.
        protocol: u32,
    },
    /// Delivers a complete replacement snapshot.
    Snapshot {
        /// Current retained-tree diagnostic state.
        snapshot: DebugSnapshot,
    },
    /// Liveness response with no payload.
    Pong,
    /// Human-readable protocol or host failure.
    Error {
        /// Error text; not a stable machine-readable code.
        message: String,
    },
}

/// Collects a subtree with no selected/hovered state at frame index zero.
///
/// If `root` is absent, the returned snapshot has no nodes but still preserves
/// the requested root ID and viewport.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_devtools_core::collect_debug_snapshot;
/// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
/// let mut tree = ElementTree::<()>::new();
/// let root = tree.create_element(ElementKind::Empty, None, None);
/// let snapshot = collect_debug_snapshot(&tree, root, Rect::new(0.0, 0.0, 100.0, 50.0));
/// assert_eq!(snapshot.root, root.0);
/// ```
pub fn collect_debug_snapshot<A: 'static>(
    tree: &ElementTree<A>,
    root: ElementId,
    viewport: Rect,
) -> DebugSnapshot {
    collect_debug_snapshot_with_state(tree, root, viewport, None, None, 0)
}

/// Collects a pre-order subtree and derives warnings with explicit host state.
///
/// `selected` and `hovered` are copied even when they are outside the collected
/// subtree. `frame_index` is opaque and is not incremented internally. Traversal
/// is O(nodes + child slots); warning computation is expected O(nodes).
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_core::{ids::ElementId, Rect};
/// use ailloli_ui_devtools_core::collect_debug_snapshot_with_state;
/// use ailloli_ui_runtime::element::ElementTree;
/// fn capture(tree: &ElementTree<()>, root: ElementId) {
///     let snapshot = collect_debug_snapshot_with_state(tree, root,
///         Rect::new(0.0, 0.0, 800.0, 600.0), Some(root), None, 42);
///     assert_eq!(snapshot.frame_index, 42);
/// }
/// ```
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

/// Replaces snapshot-wide and per-node derived warning lists.
///
/// Bounds/size comparisons tolerate 0.5 logical pixels. Missing-layout nodes
/// receive only `MissingLayout` because the remaining geometric checks require
/// layout. Duplicate keys are counted globally by their exact string. Repeated
/// calls are idempotent for unchanged snapshot inputs.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_devtools_core::{compute_warnings, DebugSnapshot};
/// fn refresh(snapshot: &mut DebugSnapshot) {
///     compute_warnings(snapshot);
///     assert_eq!(snapshot.warnings.iter().filter(|w| w.node.is_none()).count(), 0);
/// }
/// ```
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

/// Returns the topmost laid-out node containing a logical viewport point.
///
/// Nodes are inspected in reverse snapshot order, which prefers later siblings
/// and descendants from the pre-order collection. A candidate must pass its own
/// bounds/clip and every available ancestor clip. Missing ancestor records stop
/// clip traversal permissively. Complexity is O(nodes + ancestor depth).
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_devtools_core::{pick_element_at, DebugPoint, DebugSnapshot};
/// fn pick(snapshot: &DebugSnapshot) -> Option<u64> {
///     pick_element_at(snapshot, DebugPoint { x: 40.0, y: 20.0 })
/// }
/// ```
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

/// Builds hover, selection, then warning outlines in deterministic order.
///
/// Hover is blue at 2 logical pixels, selection red at 2, and each node warning
/// yellow at 1. Unknown IDs and snapshot-wide warnings are skipped. Duplicate
/// warnings for one node intentionally produce duplicate outlines.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_devtools_core::{debug_draw_cmds, DebugDrawCmd, DebugSnapshot};
/// fn overlays(snapshot: &DebugSnapshot) -> Vec<DebugDrawCmd> {
///     debug_draw_cmds(snapshot)
/// }
/// ```
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

/// Recursively appends one retained node and descendants in pre-order.
///
/// Missing IDs end that branch. Without layout, a node inherits the fallback
/// bounds and passes no assigned slots to children. Child layout slots are paired
/// by index; extra slots or children simply have no pair.
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

/// Converts retained layout instrumentation, or returns `None` when unavailable.
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

/// Clones one node-attributed warning into global and per-node collections.
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

/// Compares width/height independently using the 0.5-logical-pixel tolerance.
fn size_exceeds_slot(layout_size: DebugSize, slot: DebugRect) -> bool {
    (layout_size.w - slot.w).abs() > EPSILON || (layout_size.h - slot.h).abs() > EPSILON
}

/// Requires `point` inside every recorded ancestor clip.
///
/// An absent parent record ends traversal permissively to keep partial snapshots pickable.
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

/// Translates clip geometry while preserving rounded radius.
fn translate_clip(clip: ClipShape, origin: Offset) -> ClipShape {
    match clip {
        ClipShape::Rect(r) => ClipShape::Rect(r.translate(origin)),
        ClipShape::RoundRect { rect, radius } => ClipShape::RoundRect {
            rect: rect.translate(origin),
            radius,
        },
    }
}

/// Returns structural names or the widget's static debug name.
fn widget_name<A: 'static>(kind: &ElementKind<A>) -> &'static str {
    match kind {
        ElementKind::Empty => "Empty",
        ElementKind::Widget(widget) => widget.debug_name(),
        ElementKind::Component(_) => "Component",
    }
}

/// Converts each retained key representation to its exact visible string.
fn key_string(key: &Option<Key>) -> Option<String> {
    match key {
        None => None,
        Some(Key::Static(value)) => Some((*value).to_string()),
        Some(Key::String(value)) => Some(value.clone()),
        Some(Key::U64(value)) => Some(value.to_string()),
    }
}

/// Maps cross-axis alignment to stable lowercase protocol text.
fn align_items_name(value: AlignItems) -> &'static str {
    match value {
        AlignItems::Start => "start",
        AlignItems::Center => "center",
        AlignItems::End => "end",
        AlignItems::Stretch => "stretch",
    }
}
