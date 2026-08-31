//! Interactive snapshot or retained-model tree with selection, editing, drag/drop,
//! creation/deletion, context menus, shortcuts, and optional row virtualization.
//!
//! Snapshot trees can mutate only through a bound node signal. Retained models
//! own hierarchy/expansion and support intent-only editing workflows; callbacks
//! carry application actions or receive direct event-context access.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::hash::Hash;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized, LayoutExt};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{Color, FontId, IconId, Point, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy, InputRole};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{
    DrawBorder, DrawCmd, DrawImage, DrawRRect, DrawRect, DrawText, Invalidation,
};
use ailloli_ui_text::{
    PreparedTextLayout, TextBuffer, TextEditState, TextLayoutParams, TextSelection, WrapMode,
};
use lucide_icons::Icon as LucideIcon;

use super::text_field_core::{
    handle_single_line_text_event, ime_cursor_rect, paint_single_line_text, TextFieldEventOptions,
};
use super::text_input::TextInputStyle;
use super::tree_model::{TreeModelHandle, TreeModelSubscription, TreeMutation};
use crate::transactional_layout::TransactionalLayoutPending;

/// Maximum interval between row clicks that can count as activation.
const TREE_ACTIVATE_MAX_DELAY: Duration = Duration::from_millis(500);
/// Maximum pointer displacement, in logical pixels, between activation clicks.
const TREE_ACTIVATE_MAX_DISTANCE: f32 = 4.0;
/// Extra rows visited above and below a virtual viewport.
const TREE_VIRTUAL_OVERSCAN_ROWS: usize = 8;

/// Permanent structural counters for a [`TreeView`]. The handle is UI-local
/// and intentionally cheap to clone into a benchmark or diagnostics panel.
///
/// Clones share counters through `Rc<RefCell<_>>`; updates saturate counters.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TreeViewDiagnostics;
/// let diagnostics = TreeViewDiagnostics::new();
/// assert_eq!(diagnostics.snapshot().layout_calls, 0);
/// ```
#[derive(Clone, Default)]
pub struct TreeViewDiagnostics {
    /// Shared single-threaded counters.
    inner: Rc<RefCell<TreeViewDiagnosticsSnapshot>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Point-in-time copy of tree structural/virtualization counters.
///
/// Counters are cumulative except `loaded_rows` and `visible_rows`, which report
/// the latest measured totals/range.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TreeViewDiagnosticsSnapshot;
/// let snapshot = TreeViewDiagnosticsSnapshot::default();
/// assert_eq!((snapshot.loaded_rows, snapshot.flatten_rebuilds), (0, 0));
/// ```
pub struct TreeViewDiagnosticsSnapshot {
    /// Saturating number of widget layout calls.
    pub layout_calls: u64,
    /// Saturating number of widget paint calls.
    pub paint_calls: u64,
    /// Saturating number of row hit-test attempts.
    pub hit_tests: u64,
    /// Total source rows observed by the latest layout/paint.
    pub loaded_rows: usize,
    /// Rows visited by the latest layout/paint range.
    pub visible_rows: usize,
    /// Saturating cumulative rows visited for layout measurement.
    pub layout_rows_visited: u64,
    /// Saturating cumulative rows visited for paint.
    pub paint_rows_visited: u64,
    /// Saturating cumulative successful single-row hit-test visits.
    pub hit_test_rows_visited: u64,
    /// Greatest observed retained rebuild count or snapshot-cache rebuild count.
    pub flatten_rebuilds: u64,
    /// Saturating cumulative snapshot rows cloned while rebuilding flat caches.
    pub snapshot_rows_cloned: u64,
    /// Saturating cumulative layout rows submitted for text measurement.
    pub text_measurements: u64,
    /// Saturating count of virtual layouts falling back to the entire tree.
    pub virtualization_fallbacks: u64,
}

impl TreeViewDiagnostics {
    /// Creates zeroed shared diagnostics.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeViewDiagnostics;
    /// assert_eq!(TreeViewDiagnostics::new().snapshot(), Default::default());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Copies current counters.
    ///
    /// # Panics
    ///
    /// Panics if diagnostics are mutably borrowed reentrantly.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeViewDiagnostics;
    /// let diagnostics = TreeViewDiagnostics::new();
    /// assert_eq!(diagnostics.snapshot().paint_calls, 0);
    /// ```
    pub fn snapshot(&self) -> TreeViewDiagnosticsSnapshot {
        *self.inner.borrow()
    }

    /// Applies one internal counter update under a mutable borrow.
    fn update(&self, update: impl FnOnce(&mut TreeViewDiagnosticsSnapshot)) {
        update(&mut self.inner.borrow_mut());
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Density preset used to derive [`TreeViewStyle`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TreeViewSize;
/// assert_eq!(TreeViewSize::default(), TreeViewSize::Default);
/// assert_ne!(TreeViewSize::Compact, TreeViewSize::Default);
/// ```
pub enum TreeViewSize {
    /// 24-pixel rows, 8/4-pixel x/y padding, and 16-pixel indentation.
    Compact,
    #[default]
    /// 28-pixel rows, 10/6-pixel x/y padding, and 18-pixel indentation.
    Default,
}

#[derive(Clone, Debug, PartialEq)]
/// Tree surfaces, typography, focus, geometry, and disabled treatment.
///
/// Dimensions are logical pixels and values are consumed as supplied. The
/// default is the regular preset derived from the default theme.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TreeViewStyle;
/// let style = TreeViewStyle::default();
/// assert_eq!((style.row_height, style.indent, style.icon_size), (28.0, 18.0, 16.0));
/// ```
pub struct TreeViewStyle {
    /// Optional root surface; transparent by default.
    pub background: Color,
    /// Selected-row surface.
    pub selected_background: Color,
    /// Focused keyboard-active row surface.
    pub active_background: Color,
    /// Reserved hover-row surface token.
    pub hover_background: Color,
    /// Reserved pressed-row surface token.
    pub pressed_background: Color,
    /// Before/after drop line and inside-drop border.
    pub drop_indicator: Color,
    /// Inside-drop row surface.
    pub drop_inside_background: Color,
    /// Inline editor surface.
    pub editing_background: Color,
    /// Inline editor border/focus color.
    pub editing_border: Color,
    /// Normal row text style.
    pub text: TextStyle,
    /// Selected row text style.
    pub selected_text: TextStyle,
    /// Disabled tree/node text style.
    pub disabled_text: TextStyle,
    /// Normal leading-icon tint.
    pub icon_tint: Color,
    /// Selected leading/trailing-icon tint.
    pub selected_icon_tint: Color,
    /// Branch chevron tint.
    pub chevron_tint: Color,
    /// Keyboard-active row border.
    pub focus_ring: Border,
    /// Row/root/editor corner radii.
    pub radius: Radius,
    /// Row height in logical pixels.
    pub row_height: f32,
    /// Outer horizontal content padding in logical pixels.
    pub padding_x: f32,
    /// Outer vertical content padding in logical pixels.
    pub padding_y: f32,
    /// Per-depth horizontal indentation in logical pixels.
    pub indent: f32,
    /// Chevron/icon/text spacing in logical pixels.
    pub gap: f32,
    /// Leading/trailing icon side length in logical pixels.
    pub icon_size: f32,
    /// Chevron side length in logical pixels.
    pub chevron_size: f32,
    /// Whole-tree or per-node disabled alpha multiplier.
    pub disabled_opacity: f32,
}

impl Default for TreeViewStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), TreeViewSize::Default)
    }
}

impl TreeViewStyle {
    /// Derives appearance from `theme` and density geometry.
    ///
    /// Compact uses row/padding/indent/text size `24, 8/4, 16, 12`; default uses
    /// `28, 10/6, 18, 13`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{TreeViewSize, TreeViewStyle};
    /// let style = TreeViewStyle::from_theme(Theme::default(), TreeViewSize::Compact);
    /// assert_eq!((style.row_height, style.padding_x, style.padding_y), (24.0, 8.0, 4.0));
    /// ```
    pub fn from_theme(theme: Theme, size: TreeViewSize) -> Self {
        let palette = theme.palette();
        let (row_height, padding_x, padding_y, indent, text_size) = match size {
            TreeViewSize::Compact => (24.0, 8.0, 4.0, 16.0, 12),
            TreeViewSize::Default => (28.0, 10.0, 6.0, 18.0, 13),
        };
        Self {
            background: Color::TRANSPARENT,
            selected_background: palette.accent.with_alpha(0.18),
            active_background: palette.accent.with_alpha(0.10),
            hover_background: palette.surface_elevated,
            pressed_background: Color::hex_rgb(0x20252A),
            drop_indicator: palette.accent,
            drop_inside_background: palette.accent.with_alpha(0.12),
            editing_background: palette.surface,
            editing_border: palette.focus,
            text: TextStyle::new(FontId::Ui, text_size, palette.text),
            selected_text: TextStyle::new(FontId::Ui, text_size, palette.text),
            disabled_text: TextStyle::new(
                FontId::Ui,
                text_size,
                palette.text_muted.with_alpha(0.70),
            ),
            icon_tint: palette.text_muted,
            selected_icon_tint: palette.accent,
            chevron_tint: palette.text_muted,
            focus_ring: Border::new(1.0, palette.focus),
            radius: Radius::uniform(theme.radius().md),
            row_height,
            padding_x,
            padding_y,
            indent,
            gap: 6.0,
            icon_size: 16.0,
            chevron_size: 14.0,
            disabled_opacity: 0.42,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Drop relation between a dragged source and target row.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TreeDropPosition;
/// assert_eq!([TreeDropPosition::Before, TreeDropPosition::After, TreeDropPosition::Inside].len(), 3);
/// ```
pub enum TreeDropPosition {
    /// Insert as the target's previous sibling.
    Before,
    /// Insert as the target's next sibling.
    After,
    /// Append as the target's last child.
    Inside,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Drag/drop move intent emitted after a valid drop.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TreeDropPosition, TreeMove};
/// let event = TreeMove { source: 1, target: 2, position: TreeDropPosition::After };
/// assert_eq!(event.source, 1);
/// ```
pub struct TreeMove<T> {
    /// Moved subtree ID.
    pub source: T,
    /// Drop target row ID.
    pub target: T,
    /// Relationship requested around the target.
    pub position: TreeDropPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Whether editing operations mutate bound snapshot nodes or emit intent only.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TreeMutationMode;
/// assert_ne!(TreeMutationMode::ApplyLocal, TreeMutationMode::IntentOnly);
/// ```
pub enum TreeMutationMode {
    /// Mutate a bound snapshot node signal before emitting callbacks.
    ApplyLocal,
    /// Leave source state unchanged and emit requested operations to the owner.
    IntentOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Supported platform-neutral keyboard command intent.
///
/// Paste carries the selected/active target ID or `None` when no target exists.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TreeShortcut;
/// let shortcut = TreeShortcut::<u8>::Paste { id: None };
/// assert!(matches!(shortcut, TreeShortcut::Paste { id: None }));
/// ```
pub enum TreeShortcut<T> {
    /// Delete key with an enabled target.
    Delete {
        /// Selected or active item to delete.
        id: T,
    },
    /// Control-C with an enabled target.
    Copy {
        /// Selected or active item to copy.
        id: T,
    },
    /// Control-X with an enabled target.
    Cut {
        /// Selected or active item to cut.
        id: T,
    },
    /// Control-V with an optional enabled target.
    Paste {
        /// Destination item, or `None` when the tree has no active target.
        id: Option<T>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Committed rename intent with trimmed replacement label.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TreeRename;
/// let rename = TreeRename { id: 1, old_label: "old".into(), new_label: "new".into() };
/// assert_eq!(rename.new_label, "new");
/// ```
pub struct TreeRename<T> {
    /// Renamed node ID.
    pub id: T,
    /// Label before local mutation.
    pub old_label: String,
    /// Trimmed committed label.
    pub new_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Placement requested by tree creation commands.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TreeCreateKind;
/// assert_ne!(TreeCreateKind::SiblingAfter, TreeCreateKind::Child);
/// ```
pub enum TreeCreateKind {
    /// Insert after a sibling, or append a root when `after` is absent.
    SiblingAfter,
    /// Append below the requested parent.
    Child,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Input to a user-provided new-node factory.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TreeCreateKind, TreeCreateRequest};
/// let request = TreeCreateRequest { parent: Some(1), after: None, kind: TreeCreateKind::Child, default_label: "New item".into() };
/// assert_eq!(request.parent, Some(1));
/// ```
pub struct TreeCreateRequest<T> {
    /// Parent ID, or `None` for a root-level sibling.
    pub parent: Option<T>,
    /// Previous sibling ID for sibling insertion, if any.
    pub after: Option<T>,
    /// Sibling or child placement.
    pub kind: TreeCreateKind,
    /// Suggested initial editor label.
    pub default_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Successfully committed create intent.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TreeCreate, TreeCreateKind};
/// let event = TreeCreate { id: 2, parent: Some(1), after: None, kind: TreeCreateKind::Child, label: "file".into() };
/// assert_eq!(event.label, "file");
/// ```
pub struct TreeCreate<T> {
    /// Factory-provided new node ID.
    pub id: T,
    /// Requested parent ID.
    pub parent: Option<T>,
    /// Requested previous sibling ID.
    pub after: Option<T>,
    /// Requested placement kind.
    pub kind: TreeCreateKind,
    /// Trimmed committed label.
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Cancellation of an in-progress create editor.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TreeCreateCancel, TreeCreateKind, TreeCreateRequest};
/// let request = TreeCreateRequest { parent: None, after: None, kind: TreeCreateKind::SiblingAfter, default_label: "New item".into() };
/// let event = TreeCreateCancel { id: 9, request };
/// assert_eq!(event.id, 9);
/// ```
pub struct TreeCreateCancel<T> {
    /// Factory-provided draft ID.
    pub id: T,
    /// Original creation request.
    pub request: TreeCreateRequest<T>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Delete intent for one node/subtree.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TreeDelete;
/// let event = TreeDelete { id: 2, parent: Some(1) };
/// assert_eq!(event.parent, Some(1));
/// ```
pub struct TreeDelete<T> {
    /// Deleted subtree root ID.
    pub id: T,
    /// Parent ID before deletion, or `None` for a root.
    pub parent: Option<T>,
}

#[derive(Debug, Clone, PartialEq)]
/// Right-click context carrying pointer geometry for a row or blank area.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Point;
/// use ailloli_ui_widgets::controls::TreeContextMenu;
/// let event = TreeContextMenu::<u8>::Blank { pointer_position: Point::new(2.0, 3.0) };
/// assert!(matches!(event, TreeContextMenu::Blank { .. }));
/// ```
pub enum TreeContextMenu<T> {
    /// Enabled row context; the row is selected first when necessary.
    Row {
        /// Context row ID.
        row_id: T,
        /// Pointer location in window coordinates.
        pointer_position: Point,
        /// Hit row rectangle in window coordinates.
        row_rect: Rect,
    },
    /// Context request inside tree bounds but outside a row.
    Blank {
        /// Pointer location in window coordinates.
        pointer_position: Point,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One-shot externally bound editor command.
///
/// The widget consumes a present command by resetting its signal to `None`
/// during layout or the next routed event.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TreeViewCommand;
/// let command = TreeViewCommand::BeginRename(4_u8);
/// assert!(matches!(command, TreeViewCommand::BeginRename(4)));
/// ```
pub enum TreeViewCommand<T> {
    /// Begin editing the visible enabled node with this ID when allowed.
    BeginRename(T),
    /// Run the create factory and begin editing the resulting node when allowed.
    BeginCreate(TreeCreateRequest<T>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Icon and optional tooltip metadata for a selected row's trailing action.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::IconId;
/// use ailloli_ui_widgets::controls::TreeNodeTrailingAction;
/// let action = TreeNodeTrailingAction::new(IconId::Close).tooltip("Remove");
/// assert_eq!(action.tooltip.as_deref(), Some("Remove"));
/// ```
pub struct TreeNodeTrailingAction {
    /// Painted action icon.
    pub icon: IconId,
    /// Optional presentation tooltip; the tree itself does not display it.
    pub tooltip: Option<String>,
}

impl TreeNodeTrailingAction {
    /// Creates action metadata without a tooltip.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::TreeNodeTrailingAction;
    /// assert!(TreeNodeTrailingAction::new(IconId::Close).tooltip.is_none());
    /// ```
    pub fn new(icon: IconId) -> Self {
        Self {
            icon,
            tooltip: None,
        }
    }

    /// Sets tooltip metadata, replacing any previous value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::TreeNodeTrailingAction;
    /// let action = TreeNodeTrailingAction::new(IconId::Close).tooltip("Delete");
    /// assert_eq!(action.tooltip.as_deref(), Some("Delete"));
    /// ```
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }
}

#[derive(Clone)]
/// Recursive snapshot-tree node with static/reactive disabled state.
///
/// IDs should be unique for deterministic lookup, but snapshot construction does
/// not validate uniqueness. Adding any child promotes a leaf to a branch.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TreeNode;
/// let node = TreeNode::branch(1, "src").child(TreeNode::leaf(2, "lib.rs"));
/// assert_eq!(node.child_nodes().len(), 1);
/// ```
pub struct TreeNode<T> {
    /// Application-defined node ID.
    id: T,
    /// Display label stored unchanged.
    label: String,
    /// Explicit branch flag; children also imply branch behavior.
    branch: bool,
    /// Child nodes in display order.
    children: Vec<TreeNode<T>>,
    /// Static or reactive unavailable state.
    disabled: Binding<bool>,
    /// Optional leading icon.
    leading_icon: Option<IconId>,
    /// Optional leading-icon tint override.
    leading_icon_tint: Option<Color>,
    /// Optional selected-row trailing action metadata.
    trailing_action: Option<TreeNodeTrailingAction>,
    /// Whether this node represents provisional UI state.
    transient: bool,
}

impl<T> TreeNode<T> {
    /// Creates an enabled empty branch with no icons/action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeNode;
    /// let node = TreeNode::branch("root", "Root");
    /// assert!(node.child_nodes().is_empty());
    /// ```
    pub fn branch(id: T, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            branch: true,
            children: Vec::new(),
            disabled: Binding::Static(false),
            leading_icon: None,
            leading_icon_tint: None,
            trailing_action: None,
            transient: false,
        }
    }

    /// Creates an enabled childless leaf with no icons/action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeNode;
    /// let node = TreeNode::leaf(1, "README");
    /// assert_eq!(node.label(), "README");
    /// ```
    pub fn leaf(id: T, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            branch: false,
            children: Vec::new(),
            disabled: Binding::Static(false),
            leading_icon: None,
            leading_icon_tint: None,
            trailing_action: None,
            transient: false,
        }
    }

    /// Appends one child and promotes this node to a branch.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeNode;
    /// let node = TreeNode::leaf(1, "parent").child(TreeNode::leaf(2, "child"));
    /// assert_eq!(node.child_nodes()[0].id(), &2);
    /// ```
    pub fn child(mut self, child: TreeNode<T>) -> Self {
        self.branch = true;
        self.children.push(child);
        self
    }

    /// Extends children in iterator order without clearing existing children.
    ///
    /// A nonempty input promotes this node to a branch; empty input preserves its
    /// current branch flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeNode;
    /// let node = TreeNode::branch(1, "root").children([TreeNode::leaf(2, "a"), TreeNode::leaf(3, "b")]);
    /// assert_eq!(node.child_nodes().len(), 2);
    /// ```
    pub fn children(mut self, children: impl IntoIterator<Item = TreeNode<T>>) -> Self {
        let children = children.into_iter().collect::<Vec<_>>();
        if !children.is_empty() {
            self.branch = true;
        }
        self.children.extend(children);
        self
    }

    /// Sets static or reactive unavailable state.
    ///
    /// Disabled nodes are skipped by selection/navigation/editing/drop targets.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeNode;
    /// let node = TreeNode::leaf(1, "locked").disabled(true);
    /// let _ = node;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Sets the leading icon, replacing any previous icon.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::TreeNode;
    /// let node = TreeNode::leaf(1, "file").leading_icon(IconId::History);
    /// let _ = node;
    /// ```
    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    /// Sets a leading-icon tint override.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::controls::TreeNode;
    /// let node = TreeNode::leaf(1, "file").leading_icon_tint(Color::WHITE);
    /// let _ = node;
    /// ```
    pub fn leading_icon_tint(mut self, color: Color) -> Self {
        self.leading_icon_tint = Some(color);
        self
    }

    /// Marks or unmarks provisional presentation state.
    ///
    /// The tree retains this metadata but otherwise treats the node normally.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeNode;
    /// assert!(TreeNode::leaf(1, "draft").transient(true).is_transient());
    /// ```
    pub fn transient(mut self, transient: bool) -> Self {
        self.transient = transient;
        self
    }

    /// Sets a tooltip-free trailing action.
    ///
    /// It is painted and hit-testable only while this row is selected.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::TreeNode;
    /// let node = TreeNode::leaf(1, "file").trailing_action(IconId::Close);
    /// assert!(node.trailing_action_ref().is_some());
    /// ```
    pub fn trailing_action(mut self, icon: IconId) -> Self {
        self.trailing_action = Some(TreeNodeTrailingAction::new(icon));
        self
    }

    /// Sets a trailing action with retained tooltip metadata.
    ///
    /// The tree does not itself render the tooltip.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::TreeNode;
    /// let node = TreeNode::leaf(1, "file").trailing_action_with_tooltip(IconId::Close, "Remove");
    /// assert_eq!(node.trailing_action_ref().unwrap().tooltip.as_deref(), Some("Remove"));
    /// ```
    pub fn trailing_action_with_tooltip(
        mut self,
        icon: IconId,
        tooltip: impl Into<String>,
    ) -> Self {
        self.trailing_action = Some(TreeNodeTrailingAction::new(icon).tooltip(tooltip));
        self
    }

    /// Borrows the application-defined ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeNode;
    /// assert_eq!(TreeNode::leaf(7, "file").id(), &7);
    /// ```
    pub fn id(&self) -> &T {
        &self.id
    }

    /// Borrows the stored display label.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeNode;
    /// assert_eq!(TreeNode::leaf(1, "file").label(), "file");
    /// ```
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Borrows children in display order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeNode;
    /// assert!(TreeNode::leaf(1, "file").child_nodes().is_empty());
    /// ```
    pub fn child_nodes(&self) -> &[TreeNode<T>] {
        &self.children
    }

    /// Reports provisional presentation metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeNode;
    /// assert!(!TreeNode::leaf(1, "file").is_transient());
    /// ```
    pub fn is_transient(&self) -> bool {
        self.transient
    }

    /// Borrows optional trailing action metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeNode;
    /// assert!(TreeNode::leaf(1, "file").trailing_action_ref().is_none());
    /// ```
    pub fn trailing_action_ref(&self) -> Option<&TreeNodeTrailingAction> {
        self.trailing_action.as_ref()
    }
}

/// Shared context-aware changed-selection callback.
type TreeSelectHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T)>;
/// Shared context-aware explicit-activation callback.
type TreeActivateHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T)>;
/// Shared context-aware branch-toggle callback carrying next state.
type TreeToggleHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T, bool)>;
/// Shared context-aware selected-row trailing-action callback.
type TreeTrailingActionHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T)>;
/// Shared context-aware drag/drop intent callback.
type TreeMoveHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, TreeMove<T>)>;
/// Shared context-aware committed-rename callback.
type TreeRenameHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, TreeRename<T>)>;
/// Shared context-aware committed-create callback.
type TreeCreateHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, TreeCreate<T>)>;
/// Shared context-aware cancelled-create callback.
type TreeCreateCancelHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, TreeCreateCancel<T>)>;
/// Shared context-aware delete intent callback.
type TreeDeleteHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, TreeDelete<T>)>;
/// Shared context-aware right-click callback.
type TreeContextMenuHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, TreeContextMenu<T>)>;
/// Shared context-aware keyboard shortcut callback.
type TreeShortcutHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, TreeShortcut<T>)>;
/// Factory that may reject a create request by returning `None`.
type TreeCreateFactory<T> = Rc<dyn Fn(TreeCreateRequest<T>) -> Option<TreeNode<T>>>;

/// Type-erased visible-row/expansion adapter for retained models.
trait RetainedTreeSource<T> {
    /// Returns persistent visible row count.
    fn visible_len(&self) -> usize;
    /// Clones one visible presentation row by index.
    fn flat_node_at(&self, index: usize) -> Option<FlatNode<T>>;
    /// Looks up visible row index by stable ID.
    fn row_of(&self, id: &T) -> Option<usize>;
    /// Returns first visible enabled row.
    fn first_enabled_row(&self) -> Option<usize>;
    /// Reads branch expansion state.
    fn is_expanded(&self, id: &T) -> bool;
    /// Attempts to change retained expansion state.
    fn set_expanded(&self, id: T, expanded: bool) -> bool;
    /// Registers a weak model-revision listener.
    fn subscribe(&self, callback: &Rc<dyn Fn(u64)>) -> TreeModelSubscription;
    /// Returns persistent flat-index rebuild count.
    fn flatten_rebuilds(&self) -> u64;
}

impl<T> RetainedTreeSource<T> for TreeModelHandle<T>
where
    T: Clone + Eq + Hash + fmt::Debug + 'static,
{
    /// Reads persistent visible row count from the model.
    fn visible_len(&self) -> usize {
        self.read(|model| model.visible_len())
    }

    /// Projects one retained row/item into paint-ready metadata.
    fn flat_node_at(&self, index: usize) -> Option<FlatNode<T>> {
        self.read(|model| {
            let row = model.flat_index().rows().get(index)?;
            let item = model.item(row.node_id())?;
            Some(FlatNode {
                id: item.id().clone(),
                label: item.label().to_string(),
                depth: usize::from(row.depth()),
                branch: item.is_branch(),
                disabled: item.is_disabled(),
                leading_icon: item.leading_icon_ref().cloned(),
                leading_icon_tint: item.leading_icon_tint_ref(),
                trailing_action: item.trailing_action_ref().cloned(),
                parent: model.parent(item.id()).cloned(),
            })
        })
    }

    /// Reads constant-time visible ID lookup.
    fn row_of(&self, id: &T) -> Option<usize> {
        self.read(|model| model.flat_index().row_of(id))
    }

    /// Reads the cached first enabled visible row.
    fn first_enabled_row(&self) -> Option<usize> {
        self.read(|model| model.flat_index().first_enabled_row())
    }

    /// Reads retained branch expansion.
    fn is_expanded(&self, id: &T) -> bool {
        self.read(|model| model.is_expanded(id))
    }

    /// Applies retained expansion and collapses any model error to `false`.
    fn set_expanded(&self, id: T, expanded: bool) -> bool {
        self.apply(TreeMutation::SetExpanded { id, expanded })
            .is_ok()
    }

    /// Forwards weak revision subscription to the retained handle.
    fn subscribe(&self, callback: &Rc<dyn Fn(u64)>) -> TreeModelSubscription {
        TreeModelHandle::subscribe(self, callback)
    }

    /// Reads retained flat-index full rebuild count.
    fn flatten_rebuilds(&self) -> u64 {
        self.read(|model| model.flat_index().rebuilds())
    }
}

/// Interactive hierarchical view backed by snapshot nodes or a retained model.
///
/// A retained model takes precedence over bound/static snapshot nodes; a bound
/// snapshot takes precedence over static nodes. Structural snapshot mutations
/// require [`Self::bind_nodes`]. Retained structural edits use
/// [`TreeMutationMode::IntentOnly`], leaving ownership to callbacks.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TreeNode, TreeView};
/// let tree: TreeView<u64> = TreeView::new().node(TreeNode::leaf(1, "README"));
/// let _ = tree;
/// ```
pub struct TreeView<T, A = ()> {
    /// Root layout declarations.
    pub(crate) layout: LayoutStyle,
    /// Flex-parent participation.
    pub(crate) flex_item: FlexItemStyle,
    /// Static snapshot roots.
    nodes: Vec<TreeNode<T>>,
    /// Writable snapshot roots, taking precedence over static roots.
    bound_nodes: Option<Signal<Vec<TreeNode<T>>>>,
    /// Retained source, taking precedence over snapshot roots.
    model: Option<Rc<dyn RetainedTreeSource<T>>>,
    /// Optional readable selected ID.
    selected: Option<Binding<T>>,
    /// Optional writable selected ID.
    bound_selected: Option<Signal<T>>,
    /// Optional readable expanded IDs for snapshot trees.
    expanded: Option<Binding<Vec<T>>>,
    /// Optional writable expanded IDs for snapshot trees.
    bound_expanded: Option<Signal<Vec<T>>>,
    /// Optional one-shot editor command signal.
    command: Option<Signal<Option<TreeViewCommand<T>>>>,
    /// Deduplicated fallback expanded IDs when no expanded binding exists.
    default_expanded: Vec<T>,
    /// Whole-tree disabled state.
    disabled: Binding<bool>,
    /// Whether drag gestures may begin.
    draggable: Binding<bool>,
    /// Local-versus-intent mutation policy.
    mutation_mode: Binding<TreeMutationMode>,
    /// Whether rename editors may begin.
    editable: Binding<bool>,
    /// Whether Delete may request removal.
    deletable: Binding<bool>,
    /// Whether Insert/create commands may request nodes.
    creatable: Binding<bool>,
    /// Optional factory for provisional created nodes.
    create_node: Option<TreeCreateFactory<T>>,
    /// Changed-selection callback.
    on_select: Option<TreeSelectHandler<T, A>>,
    /// Explicit activation callback.
    on_activate: Option<TreeActivateHandler<T, A>>,
    /// Branch toggle callback.
    on_toggle: Option<TreeToggleHandler<T, A>>,
    /// Selected-row trailing-action callback.
    on_trailing_action: Option<TreeTrailingActionHandler<T, A>>,
    /// Drag/drop callback.
    on_move: Option<TreeMoveHandler<T, A>>,
    /// Rename callback and local-rename enablement.
    on_rename: Option<TreeRenameHandler<T, A>>,
    /// Create callback.
    on_create: Option<TreeCreateHandler<T, A>>,
    /// Cancelled-create callback.
    on_create_cancel: Option<TreeCreateCancelHandler<T, A>>,
    /// Delete callback.
    on_delete: Option<TreeDeleteHandler<T, A>>,
    /// Context-menu callback.
    on_context_menu: Option<TreeContextMenuHandler<T, A>>,
    /// Shortcut callback, which intercepts supported keys.
    on_shortcut: Option<TreeShortcutHandler<T, A>>,
    /// Appearance and geometry tokens.
    style: TreeViewStyle,
    /// Whether layout/paint visit viewport row ranges plus overscan.
    virtualized: bool,
    /// Optional shared structural counters.
    diagnostics: Option<TreeViewDiagnostics>,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Default for TreeView<T, A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> LayoutExt for TreeView<T, A> {
    /// Returns mutable access to root layout declarations.
    fn layout_mut(&mut self) -> &mut LayoutStyle {
        &mut self.layout
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> TreeView<T, A> {
    /// Creates an enabled empty snapshot tree with default style.
    ///
    /// Selection/expansion are uncontrolled, structural features and
    /// virtualization are off, and mutation mode defaults to `ApplyLocal`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new();
    /// let _ = tree;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            nodes: Vec::new(),
            bound_nodes: None,
            model: None,
            selected: None,
            bound_selected: None,
            expanded: None,
            bound_expanded: None,
            command: None,
            default_expanded: Vec::new(),
            disabled: Binding::Static(false),
            draggable: Binding::Static(false),
            mutation_mode: Binding::Static(TreeMutationMode::ApplyLocal),
            editable: Binding::Static(false),
            deletable: Binding::Static(false),
            creatable: Binding::Static(false),
            create_node: None,
            on_select: None,
            on_activate: None,
            on_toggle: None,
            on_trailing_action: None,
            on_move: None,
            on_rename: None,
            on_create: None,
            on_create_cancel: None,
            on_delete: None,
            on_context_menu: None,
            on_shortcut: None,
            style: TreeViewStyle::default(),
            virtualized: false,
            diagnostics: None,
        }
    }

    /// Appends one static snapshot root.
    ///
    /// Retained or bound node sources take precedence when configured.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeNode, TreeView};
    /// let tree: TreeView<u8> = TreeView::new().node(TreeNode::leaf(1, "file"));
    /// let _ = tree;
    /// ```
    pub fn node(mut self, node: TreeNode<T>) -> Self {
        self.nodes.push(node);
        self
    }

    /// Extends static snapshot roots in iterator order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeNode, TreeView};
    /// let tree: TreeView<u8> = TreeView::new().nodes([TreeNode::leaf(1, "a"), TreeNode::leaf(2, "b")]);
    /// let _ = tree;
    /// ```
    pub fn nodes(mut self, nodes: impl IntoIterator<Item = TreeNode<T>>) -> Self {
        self.nodes.extend(nodes);
        self
    }

    /// Installs writable snapshot roots, taking precedence over static roots.
    ///
    /// This enables local structural drag/edit/create/delete operations. A
    /// retained model still takes precedence for rendering.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::{TreeNode, TreeView};
    /// let roots = State::new(vec![TreeNode::leaf(1_u8, "file")]);
    /// let tree: TreeView<u8> = TreeView::new().bind_nodes(roots);
    /// let _ = tree;
    /// ```
    pub fn bind_nodes(mut self, nodes: impl Into<Signal<Vec<TreeNode<T>>>>) -> Self {
        self.bound_nodes = Some(nodes.into());
        self
    }

    /// Uses a retained, revisioned model.
    ///
    /// This is recommended for large/incremental trees and takes precedence over
    /// snapshot roots. Expansion comes from the model. Structural editing must
    /// use `IntentOnly`; the owner then applies model mutations.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle, TreeView};
    /// let model = TreeModelHandle::new(TreeModel::<u64>::new());
    /// let tree: TreeView<u64> = TreeView::new().model(model);
    /// let _ = tree;
    /// ```
    pub fn model(mut self, model: TreeModelHandle<T>) -> Self
    where
        T: Eq + Hash + fmt::Debug,
    {
        self.model = Some(Rc::new(model));
        self
    }

    /// Sets readable static or reactive selection.
    ///
    /// Selection is not writable unless [`Self::bind_selected`] was also called.
    /// If called after `bind_selected`, the prior writable signal is retained for
    /// interaction while this value controls painting/comparison.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().selected(1);
    /// let _ = tree;
    /// ```
    pub fn selected(mut self, selected: impl Into<Binding<T>>) -> Self {
        self.selected = Some(selected.into());
        self
    }

    /// Installs readable/writable controlled selection.
    ///
    /// Selection callbacks run only when activation targets a different ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().bind_selected(State::new(1));
    /// let _ = tree;
    /// ```
    pub fn bind_selected(mut self, selected: impl Into<Signal<T>>) -> Self {
        let signal = selected.into();
        self.selected = Some(Binding::Signal(signal.clone()));
        self.bound_selected = Some(signal);
        self
    }

    /// Sets readable expanded IDs for snapshot trees.
    ///
    /// Duplicates are ignored when read. It is not writable unless
    /// `bind_expanded` was also called; retained models ignore this value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().expanded(vec![1, 2]);
    /// let _ = tree;
    /// ```
    pub fn expanded(mut self, expanded: impl Into<Binding<Vec<T>>>) -> Self {
        self.expanded = Some(expanded.into());
        self
    }

    /// Installs readable/writable expanded IDs for snapshot trees.
    ///
    /// Retained models use their own expansion state instead.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().bind_expanded(State::new(vec![1]));
    /// let _ = tree;
    /// ```
    pub fn bind_expanded(mut self, expanded: impl Into<Signal<Vec<T>>>) -> Self {
        let signal = expanded.into();
        self.expanded = Some(Binding::Signal(signal.clone()));
        self.bound_expanded = Some(signal);
        self
    }

    /// Installs a writable one-shot editor-command signal.
    ///
    /// Present commands are consumed by setting the signal to `None` during
    /// layout or the next routed event, even when the target cannot begin editing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::{TreeView, TreeViewCommand};
    /// let command = State::new(Some(TreeViewCommand::BeginRename(1_u8)));
    /// let tree: TreeView<u8> = TreeView::new().bind_command(command);
    /// let _ = tree;
    /// ```
    pub fn bind_command(mut self, command: impl Into<Signal<Option<TreeViewCommand<T>>>>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// Adds one unique uncontrolled initial expansion ID.
    ///
    /// It is used only when no explicit expanded binding is configured.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().default_expanded(1).default_expanded(1);
    /// let _ = tree;
    /// ```
    pub fn default_expanded(mut self, id: T) -> Self {
        if !self.default_expanded.iter().any(|open| open == &id) {
            self.default_expanded.push(id);
        }
        self
    }

    /// Replaces uncontrolled initial expansion IDs with unique iterator values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().default_expanded_many([1, 2, 1]);
    /// let _ = tree;
    /// ```
    pub fn default_expanded_many(mut self, ids: impl IntoIterator<Item = T>) -> Self {
        self.default_expanded.clear();
        for id in ids {
            if !self.default_expanded.iter().any(|open| open == &id) {
                self.default_expanded.push(id);
            }
        }
        self
    }

    /// Sets static or reactive whole-tree disabled state.
    ///
    /// Disabled trees are not focusable, ignore input, and paint disabled rows.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().disabled(true);
    /// let _ = tree;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Enables drag gestures when structural mutation capability also exists.
    ///
    /// A drag activates after moving more than four logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().draggable(true);
    /// let _ = tree;
    /// ```
    pub fn draggable(mut self, draggable: impl Into<Binding<bool>>) -> Self {
        self.draggable = draggable.into();
        self
    }

    /// Sets reactive local-versus-intent structural mutation policy.
    ///
    /// `ApplyLocal` requires bound snapshot nodes. Retained structural operations
    /// require `IntentOnly`; retained branch toggles apply locally only in
    /// `ApplyLocal`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeMutationMode, TreeView};
    /// let tree: TreeView<u8> = TreeView::new().mutation_mode(TreeMutationMode::IntentOnly);
    /// let _ = tree;
    /// ```
    pub fn mutation_mode(mut self, mode: impl Into<Binding<TreeMutationMode>>) -> Self {
        self.mutation_mode = mode.into();
        self
    }

    /// Enables inline rename/create editors when mutation capability exists.
    ///
    /// Rename commits additionally require an `on_rename` callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().editable(true);
    /// let _ = tree;
    /// ```
    pub fn editable(mut self, editable: impl Into<Binding<bool>>) -> Self {
        self.editable = editable.into();
        self
    }

    /// Enables built-in Delete behavior when mutation capability exists.
    ///
    /// Installing `on_shortcut` intercepts Delete as a shortcut before built-in
    /// deletion.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().deletable(true);
    /// let _ = tree;
    /// ```
    pub fn deletable(mut self, deletable: impl Into<Binding<bool>>) -> Self {
        self.deletable = deletable.into();
        self
    }

    /// Enables Insert/create commands when mutation capability and a factory exist.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().creatable(true);
    /// let _ = tree;
    /// ```
    pub fn creatable(mut self, creatable: impl Into<Binding<bool>>) -> Self {
        self.creatable = creatable.into();
        self
    }

    /// Sets the factory used to construct provisional nodes for create requests.
    ///
    /// Returning `None` rejects the request without editing or callbacks.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeNode, TreeView};
    /// let tree: TreeView<u8> = TreeView::new().create_node_with(|request| Some(TreeNode::leaf(9, request.default_label)));
    /// let _ = tree;
    /// ```
    pub fn create_node_with(
        mut self,
        factory: impl Fn(TreeCreateRequest<T>) -> Option<TreeNode<T>> + 'static,
    ) -> Self {
        self.create_node = Some(Rc::new(factory));
        self
    }

    /// Replaces tree appearance without altering explicit layout declarations.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeView, TreeViewStyle};
    /// let tree: TreeView<u8> = TreeView::new().tree_style(TreeViewStyle::default());
    /// let _ = tree;
    /// ```
    pub fn tree_style(mut self, style: TreeViewStyle) -> Self {
        self.style = style;
        self
    }

    /// Re-derives every style field from the default theme and density.
    ///
    /// Explicit layout declarations remain unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeView, TreeViewSize};
    /// let tree: TreeView<u8> = TreeView::new().tree_size(TreeViewSize::Compact);
    /// let _ = tree;
    /// ```
    pub fn tree_size(mut self, size: TreeViewSize) -> Self {
        self.style = TreeViewStyle::from_theme(Theme::default(), size);
        self
    }

    /// Enables viewport row-range layout/paint with eight-row overscan.
    ///
    /// A virtual layout without viewport and without finite maximum height falls
    /// back to all rows and increments `virtualization_fallbacks`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().virtualized(true);
    /// let _ = tree;
    /// ```
    pub fn virtualized(mut self, virtualized: bool) -> Self {
        self.virtualized = virtualized;
        self
    }

    /// Attaches shared structural counters, replacing any previous handle.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeView, TreeViewDiagnostics};
    /// let diagnostics = TreeViewDiagnostics::new();
    /// let tree: TreeView<u8> = TreeView::new().diagnostics(diagnostics.clone());
    /// assert_eq!(diagnostics.snapshot().layout_calls, 0);
    /// let _ = tree;
    /// ```
    pub fn diagnostics(mut self, diagnostics: TreeViewDiagnostics) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    /// Replaces preferred width; numeric inputs are logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().width(320.0);
    /// let _ = tree;
    /// ```
    pub fn width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.width = value.into();
        self
    }

    /// Replaces preferred height; numeric inputs are logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().height(480.0);
    /// let _ = tree;
    /// ```
    pub fn height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.height = value.into();
        self
    }

    /// Replaces the minimum-width declaration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().min_width(160.0);
    /// let _ = tree;
    /// ```
    pub fn min_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.min_width = value.into();
        self
    }

    /// Replaces the maximum-width declaration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().max_width(640.0);
    /// let _ = tree;
    /// ```
    pub fn max_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.max_width = value.into();
        self
    }

    /// Replaces the minimum-height declaration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().min_height(120.0);
    /// let _ = tree;
    /// ```
    pub fn min_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.min_height = value.into();
        self
    }

    /// Replaces the maximum-height declaration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().max_height(720.0);
    /// let _ = tree;
    /// ```
    pub fn max_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.max_height = value.into();
        self
    }

    /// Requests parent-fill sizing on both axes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().fill();
    /// let _ = tree;
    /// ```
    pub fn fill(mut self) -> Self {
        self.layout.width = ailloli_ui_core::style::Length::Fill;
        self.layout.height = ailloli_ui_core::style::Length::Fill;
        self
    }

    /// Requests parent-fill width while preserving height.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().fill_width();
    /// let _ = tree;
    /// ```
    pub fn fill_width(mut self) -> Self {
        self.layout.width = ailloli_ui_core::style::Length::Fill;
        self
    }

    /// Requests parent-fill height while preserving width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().fill_height();
    /// let _ = tree;
    /// ```
    pub fn fill_height(mut self) -> Self {
        self.layout.height = ailloli_ui_core::style::Length::Fill;
        self
    }

    /// Sets this tree's flex-grow weight to one.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().flex_grow();
    /// let _ = tree;
    /// ```
    pub fn flex_grow(mut self) -> Self {
        self.flex_item = self.flex_item.flex_grow(1.0);
        self
    }

    /// Dispatches the action returned when enabled selection changes.
    ///
    /// Equal IDs do not emit. Without writable selection, a different row can
    /// emit even though controlled painting remains unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8, u8> = TreeView::new().on_select(|id| id);
    /// let _ = tree;
    /// ```
    pub fn on_select(mut self, f: impl Fn(T) -> A + 'static) -> Self {
        self.on_select = Some(Rc::new(move |ctx, id| ctx.dispatch(f(id))));
        self
    }

    /// Handles changed selection with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().on_select_ctx(|_ctx, _id| {});
    /// let _ = tree;
    /// ```
    pub fn on_select_ctx(mut self, f: impl Fn(&mut EventCtx<A>, T) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    /// Dispatches the action returned on explicit enabled-row activation.
    ///
    /// Activation comes from a qualifying second click or Enter. With this
    /// handler installed, Enter activates instead of selecting.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8, u8> = TreeView::new().on_activate(|id| id);
    /// let _ = tree;
    /// ```
    pub fn on_activate(mut self, f: impl Fn(T) -> A + 'static) -> Self {
        self.on_activate = Some(Rc::new(move |ctx, id| ctx.dispatch(f(id))));
        self
    }

    /// Handles explicit activation with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().on_activate_ctx(|_ctx, _id| {});
    /// let _ = tree;
    /// ```
    pub fn on_activate_ctx(mut self, f: impl Fn(&mut EventCtx<A>, T) + 'static) -> Self {
        self.on_activate = Some(Rc::new(f));
        self
    }

    /// Dispatches the action returned after a valid branch toggle intent.
    ///
    /// The boolean is the requested next expansion state. In retained
    /// `IntentOnly` mode the model remains unchanged until its owner applies it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8, bool> = TreeView::new().on_toggle(|_id, open| open);
    /// let _ = tree;
    /// ```
    pub fn on_toggle(mut self, f: impl Fn(T, bool) -> A + 'static) -> Self {
        self.on_toggle = Some(Rc::new(move |ctx, id, open| ctx.dispatch(f(id, open))));
        self
    }

    /// Handles branch toggle intent with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().on_toggle_ctx(|_ctx, _id, _open| {});
    /// let _ = tree;
    /// ```
    pub fn on_toggle_ctx(mut self, f: impl Fn(&mut EventCtx<A>, T, bool) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(f));
        self
    }

    /// Dispatches the action returned by an enabled selected row's trailing icon.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8, u8> = TreeView::new().on_trailing_action(|id| id);
    /// let _ = tree;
    /// ```
    pub fn on_trailing_action(mut self, f: impl Fn(T) -> A + 'static) -> Self {
        self.on_trailing_action = Some(Rc::new(move |ctx, id| ctx.dispatch(f(id))));
        self
    }

    /// Handles a trailing action with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().on_trailing_action_ctx(|_ctx, _id| {});
    /// let _ = tree;
    /// ```
    pub fn on_trailing_action_ctx(mut self, f: impl Fn(&mut EventCtx<A>, T) + 'static) -> Self {
        self.on_trailing_action = Some(Rc::new(f));
        self
    }

    /// Dispatches the action returned after a valid drag/drop move intent.
    ///
    /// `ApplyLocal` mutates bound snapshot nodes before emitting; `IntentOnly`
    /// emits without source mutation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeMove, TreeView};
    /// let tree: TreeView<u8, TreeMove<u8>> = TreeView::new().on_move(|event| event);
    /// let _ = tree;
    /// ```
    pub fn on_move(mut self, f: impl Fn(TreeMove<T>) -> A + 'static) -> Self {
        self.on_move = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    /// Handles drag/drop move intent with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().on_move_ctx(|_ctx, _event| {});
    /// let _ = tree;
    /// ```
    pub fn on_move_ctx(mut self, f: impl Fn(&mut EventCtx<A>, TreeMove<T>) + 'static) -> Self {
        self.on_move = Some(Rc::new(f));
        self
    }

    /// Dispatches the action returned for a nonempty committed rename.
    ///
    /// Installing a rename handler is required for ordinary rename commits. In
    /// local mode bound snapshot labels are changed before emission; intent mode
    /// leaves the source unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeRename, TreeView};
    /// let tree: TreeView<u8, TreeRename<u8>> = TreeView::new().on_rename(|event| event);
    /// let _ = tree;
    /// ```
    pub fn on_rename(mut self, f: impl Fn(TreeRename<T>) -> A + 'static) -> Self {
        self.on_rename = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    /// Handles a committed rename with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().on_rename_ctx(|_ctx, _event| {});
    /// let _ = tree;
    /// ```
    pub fn on_rename_ctx(mut self, f: impl Fn(&mut EventCtx<A>, TreeRename<T>) + 'static) -> Self {
        self.on_rename = Some(Rc::new(f));
        self
    }

    /// Dispatches the action returned for a committed create intent.
    ///
    /// Creation also requires `creatable`, structural mutation capability, and a
    /// factory. External create commands emit the trimmed final editor label.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeCreate, TreeView};
    /// let tree: TreeView<u8, TreeCreate<u8>> = TreeView::new().on_create(|event| event);
    /// let _ = tree;
    /// ```
    pub fn on_create(mut self, f: impl Fn(TreeCreate<T>) -> A + 'static) -> Self {
        self.on_create = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    /// Handles committed creation with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().on_create_ctx(|_ctx, _event| {});
    /// let _ = tree;
    /// ```
    pub fn on_create_ctx(mut self, f: impl Fn(&mut EventCtx<A>, TreeCreate<T>) + 'static) -> Self {
        self.on_create = Some(Rc::new(f));
        self
    }

    /// Handles cancellation of a create editor with direct context access.
    ///
    /// Cancellation removes a locally inserted provisional snapshot/draft and
    /// carries the original request.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().on_create_cancel_ctx(|_ctx, _event| {});
    /// let _ = tree;
    /// ```
    pub fn on_create_cancel_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, TreeCreateCancel<T>) + 'static,
    ) -> Self {
        self.on_create_cancel = Some(Rc::new(f));
        self
    }

    /// Dispatches the action returned for deletion of an enabled subtree.
    ///
    /// Local mode removes bound snapshot nodes first; intent mode emits only.
    /// A configured shortcut handler intercepts Delete before this behavior.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeDelete, TreeView};
    /// let tree: TreeView<u8, TreeDelete<u8>> = TreeView::new().on_delete(|event| event);
    /// let _ = tree;
    /// ```
    pub fn on_delete(mut self, f: impl Fn(TreeDelete<T>) -> A + 'static) -> Self {
        self.on_delete = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    /// Handles deletion intent with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().on_delete_ctx(|_ctx, _event| {});
    /// let _ = tree;
    /// ```
    pub fn on_delete_ctx(mut self, f: impl Fn(&mut EventCtx<A>, TreeDelete<T>) + 'static) -> Self {
        self.on_delete = Some(Rc::new(f));
        self
    }

    /// Dispatches the action returned for right-click row/blank context.
    ///
    /// An enabled row is made active and selected before its callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().on_context_menu(|_event| ());
    /// let _ = tree;
    /// ```
    pub fn on_context_menu(mut self, f: impl Fn(TreeContextMenu<T>) -> A + 'static) -> Self {
        self.on_context_menu = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    /// Handles right-click context with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().on_context_menu_ctx(|_ctx, _event| {});
    /// let _ = tree;
    /// ```
    pub fn on_context_menu_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, TreeContextMenu<T>) + 'static,
    ) -> Self {
        self.on_context_menu = Some(Rc::new(f));
        self
    }

    /// Dispatches supported Delete/Control-C/X/V shortcut intent.
    ///
    /// Installing this handler consumes matching shortcuts before built-in
    /// Delete. Alt/Meta-modified keys are ignored; Paste may carry no target.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().on_shortcut(|_event| ());
    /// let _ = tree;
    /// ```
    pub fn on_shortcut(mut self, f: impl Fn(TreeShortcut<T>) -> A + 'static) -> Self {
        self.on_shortcut = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    /// Handles supported shortcut intent with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeView;
    /// let tree: TreeView<u8> = TreeView::new().on_shortcut_ctx(|_ctx, _event| {});
    /// let _ = tree;
    /// ```
    pub fn on_shortcut_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, TreeShortcut<T>) + 'static,
    ) -> Self {
        self.on_shortcut = Some(Rc::new(f));
        self
    }
}

/// Component-stage configuration copied into the retained tree widget.
struct TreeViewComponent<T, A> {
    /// Root layout declarations.
    layout: LayoutStyle,
    /// Static snapshot roots.
    nodes: Vec<TreeNode<T>>,
    /// Optional writable snapshot roots.
    bound_nodes: Option<Signal<Vec<TreeNode<T>>>>,
    /// Optional retained source.
    model: Option<Rc<dyn RetainedTreeSource<T>>>,
    /// Optional readable selection.
    selected: Option<Binding<T>>,
    /// Optional writable selection.
    bound_selected: Option<Signal<T>>,
    /// Optional readable snapshot expansion IDs.
    expanded: Option<Binding<Vec<T>>>,
    /// Optional writable snapshot expansion IDs.
    bound_expanded: Option<Signal<Vec<T>>>,
    /// Optional one-shot editor command signal.
    command: Option<Signal<Option<TreeViewCommand<T>>>>,
    /// Fallback uncontrolled expansion IDs.
    default_expanded: Vec<T>,
    /// Whole-tree disabled state.
    disabled: Binding<bool>,
    /// Drag enablement.
    draggable: Binding<bool>,
    /// Structural mutation policy.
    mutation_mode: Binding<TreeMutationMode>,
    /// Rename editor enablement.
    editable: Binding<bool>,
    /// Delete enablement.
    deletable: Binding<bool>,
    /// Create enablement.
    creatable: Binding<bool>,
    /// Optional create factory.
    create_node: Option<TreeCreateFactory<T>>,
    /// Changed-selection callback.
    on_select: Option<TreeSelectHandler<T, A>>,
    /// Explicit activation callback.
    on_activate: Option<TreeActivateHandler<T, A>>,
    /// Toggle callback.
    on_toggle: Option<TreeToggleHandler<T, A>>,
    /// Trailing action callback.
    on_trailing_action: Option<TreeTrailingActionHandler<T, A>>,
    /// Move callback.
    on_move: Option<TreeMoveHandler<T, A>>,
    /// Rename callback.
    on_rename: Option<TreeRenameHandler<T, A>>,
    /// Create callback.
    on_create: Option<TreeCreateHandler<T, A>>,
    /// Create cancellation callback.
    on_create_cancel: Option<TreeCreateCancelHandler<T, A>>,
    /// Delete callback.
    on_delete: Option<TreeDeleteHandler<T, A>>,
    /// Context-menu callback.
    on_context_menu: Option<TreeContextMenuHandler<T, A>>,
    /// Keyboard shortcut callback.
    on_shortcut: Option<TreeShortcutHandler<T, A>>,
    /// Appearance and geometry tokens.
    style: TreeViewStyle,
    /// Row-range virtualization flag.
    virtualized: bool,
    /// Optional structural diagnostics.
    diagnostics: Option<TreeViewDiagnostics>,
}

impl<T: Clone + PartialEq + 'static, A: 'static> ComponentNode<A> for TreeViewComponent<T, A> {
    /// Allocates uncontrolled expansion, model invalidation, editor, and cache state.
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let internal_expanded = context.signal_with_invalidation(
            unique_vec(self.default_expanded.clone()),
            Invalidation::Layout,
        );
        let expanded = self
            .expanded
            .clone()
            .unwrap_or_else(|| Binding::Signal(internal_expanded.clone()));
        let mutable_expanded = self
            .bound_expanded
            .clone()
            .or_else(|| self.expanded.is_none().then_some(internal_expanded));
        let model_callback = self.model.as_ref().map(|_| {
            let invalidate = context.invalidation_target(Invalidation::Layout);
            Rc::new(move |_revision: u64| invalidate()) as Rc<dyn Fn(u64)>
        });
        let model_subscription = self
            .model
            .as_ref()
            .zip(model_callback.as_ref())
            .map(|(model, callback)| model.subscribe(callback));
        View::leaf(TreeViewWidget {
            layout: self.layout,
            nodes: self.nodes.clone(),
            bound_nodes: self.bound_nodes.clone(),
            model: self.model.clone(),
            _model_callback: model_callback,
            _model_subscription: model_subscription,
            selected: self.selected.clone(),
            bound_selected: self.bound_selected.clone(),
            expanded,
            mutable_expanded,
            command: self.command.clone(),
            pending_command: RefCell::new(None),
            disabled: self.disabled.clone(),
            draggable: self.draggable.clone(),
            mutation_mode: self.mutation_mode.clone(),
            editable: self.editable.clone(),
            deletable: self.deletable.clone(),
            creatable: self.creatable.clone(),
            create_node: self.create_node.clone(),
            on_select: self.on_select.clone(),
            on_activate: self.on_activate.clone(),
            on_toggle: self.on_toggle.clone(),
            on_trailing_action: self.on_trailing_action.clone(),
            on_move: self.on_move.clone(),
            on_rename: self.on_rename.clone(),
            on_create: self.on_create.clone(),
            on_create_cancel: self.on_create_cancel.clone(),
            on_delete: self.on_delete.clone(),
            on_context_menu: self.on_context_menu.clone(),
            on_shortcut: self.on_shortcut.clone(),
            style: self.style.clone(),
            virtualized: self.virtualized,
            diagnostics: self.diagnostics.clone(),
            active_index: context.signal_with_invalidation(None, Invalidation::Paint),
            drag: context.signal_with_invalidation(None, Invalidation::Paint),
            editing: context.signal_with_invalidation(None, Invalidation::Paint),
            draft: context.signal_with_invalidation(None, Invalidation::Layout),
            edit_value: context.signal_with_invalidation(String::new(), Invalidation::Paint),
            edit_buffer: context.signal_with_invalidation(
                TextBuffer::from_string(String::new()),
                Invalidation::Paint,
            ),
            edit_state: context.signal_with_invalidation(TextEditState::new(), Invalidation::Paint),
            last_click: context.signal_with_invalidation(None, Invalidation::Paint),
            observed_max_width: Rc::new(Cell::new(160.0)),
            snapshot_flat_cache: Rc::new(RefCell::new(SnapshotFlatCache::default())),
        })
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> IntoView<A> for TreeView<T, A> {
    /// Builds the retained component and preserves flex/size metadata.
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(TreeViewComponent {
                layout: self.layout,
                nodes: self.nodes,
                bound_nodes: self.bound_nodes,
                model: self.model,
                selected: self.selected,
                bound_selected: self.bound_selected,
                expanded: self.expanded,
                bound_expanded: self.bound_expanded,
                command: self.command,
                default_expanded: self.default_expanded,
                disabled: self.disabled,
                draggable: self.draggable,
                mutation_mode: self.mutation_mode,
                editable: self.editable,
                deletable: self.deletable,
                creatable: self.creatable,
                create_node: self.create_node,
                on_select: self.on_select,
                on_activate: self.on_activate,
                on_toggle: self.on_toggle,
                on_trailing_action: self.on_trailing_action,
                on_move: self.on_move,
                on_rename: self.on_rename,
                on_create: self.on_create,
                on_create_cancel: self.on_create_cancel,
                on_delete: self.on_delete,
                on_context_menu: self.on_context_menu,
                on_shortcut: self.on_shortcut,
                style: self.style,
                virtualized: self.virtualized,
                diagnostics: self.diagnostics,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

#[derive(Clone)]
/// Paint-ready visible node projected from snapshot or retained sources.
struct FlatNode<T> {
    /// Application/retained stable ID.
    id: T,
    /// Display label.
    label: String,
    /// Zero-based visible hierarchy depth.
    depth: usize,
    /// Whether to paint/toggle a branch chevron.
    branch: bool,
    /// Snapshot of disabled state.
    disabled: bool,
    /// Optional leading icon.
    leading_icon: Option<IconId>,
    /// Optional leading-icon tint override.
    leading_icon_tint: Option<Color>,
    /// Optional selected-row trailing action.
    trailing_action: Option<TreeNodeTrailingAction>,
    /// Parent ID, or `None` for a root.
    parent: Option<T>,
}

#[derive(Clone)]
/// Validated drag/drop target row and relation.
struct TreeDropTarget<T> {
    /// Target row ID.
    target: T,
    /// Before/after/inside relation.
    position: TreeDropPosition,
}

#[derive(Clone)]
/// Pointer drag candidate/active state.
struct TreeDragState<T> {
    /// Dragged source ID.
    source: T,
    /// Pointer-down location.
    start: Point,
    /// Whether movement passed the activation threshold.
    active: bool,
    /// Current enabled non-self drop target.
    target: Option<TreeDropTarget<T>>,
}

#[derive(Clone)]
/// Last row click used to detect double-click activation.
struct TreeClickState<T> {
    /// Event or legacy monotonic timestamp.
    at: TreeClickTimestamp,
    /// Pointer location used for distance threshold.
    pos: Point,
    /// Clicked row ID.
    id: T,
    /// Saturating click sequence count capped at three.
    count: u8,
}

#[derive(Clone, Copy)]
/// Comparable click timestamp within one timing source.
enum TreeClickTimestamp {
    /// Runtime event timestamp.
    Event(Duration),
    /// Monotonic fallback when event metadata is absent.
    Legacy(Instant),
}

impl TreeClickTimestamp {
    /// Returns checked elapsed time only for matching timestamp sources.
    fn elapsed_since(self, earlier: Self) -> Option<Duration> {
        match (self, earlier) {
            (Self::Event(now), Self::Event(earlier)) => now.checked_sub(earlier),
            (Self::Legacy(now), Self::Legacy(earlier)) => now.checked_duration_since(earlier),
            _ => None,
        }
    }
}

#[derive(Clone)]
/// Active inline editor identity, original label, and optional create request.
struct TreeEditing<T> {
    /// Edited node/draft ID.
    id: T,
    /// Label before editing began.
    original: String,
    /// Present for external/retained create-editor flows.
    create: Option<TreeCreateRequest<T>>,
}

#[derive(Clone)]
/// Retained intent-only provisional row not yet present in its model.
struct TreeDraft<T> {
    /// Paint-ready provisional node.
    node: FlatNode<T>,
    /// Visible row insertion index.
    insert_index: usize,
}

/// Persistent flattened cache for recursive snapshot trees.
struct SnapshotFlatCache<T> {
    /// Bound root-signal revision represented by the cache.
    nodes_revision: u64,
    /// Whether an initial flatten has occurred.
    initialized: bool,
    /// Deduplicated expansion IDs represented by the cache.
    expanded: Vec<T>,
    /// Visible paint-ready rows.
    rows: Vec<FlatNode<T>>,
    /// Saturating full-cache rebuild count.
    rebuilds: u64,
}

impl<T> Default for SnapshotFlatCache<T> {
    fn default() -> Self {
        Self {
            nodes_revision: 0,
            initialized: false,
            expanded: Vec::new(),
            rows: Vec::new(),
            rebuilds: 0,
        }
    }
}

/// Per-row state passed to the node painter.
struct TreeNodePaint<'a, T> {
    /// Row rectangle.
    row: Rect,
    /// Paint-ready node.
    node: &'a FlatNode<T>,
    /// Whether controlled selection matches the node ID.
    selected: bool,
    /// Disabled/drag alpha multiplier.
    opacity: f32,
    /// Current widget layout used by inline text editing.
    layout: &'a LayoutResult,
    /// Whether tree keyboard focus is active.
    focused: bool,
    /// Matching inline editor, if any.
    editing: Option<&'a TreeEditing<T>>,
}

/// Retained interactive tree widget and all reactive/cache state.
struct TreeViewWidget<T, A> {
    /// Root layout declarations.
    layout: LayoutStyle,
    /// Static snapshot roots.
    nodes: Vec<TreeNode<T>>,
    /// Optional writable snapshot roots.
    bound_nodes: Option<Signal<Vec<TreeNode<T>>>>,
    /// Optional retained source.
    model: Option<Rc<dyn RetainedTreeSource<T>>>,
    /// Strong callback kept alive for weak model subscription.
    _model_callback: Option<Rc<dyn Fn(u64)>>,
    /// RAII model revision subscription.
    _model_subscription: Option<TreeModelSubscription>,
    /// Optional readable selection.
    selected: Option<Binding<T>>,
    /// Optional writable selection.
    bound_selected: Option<Signal<T>>,
    /// Snapshot expanded-ID binding or internal uncontrolled binding.
    expanded: Binding<Vec<T>>,
    /// Optional writable snapshot expansion signal.
    mutable_expanded: Option<Signal<Vec<T>>>,
    /// Optional one-shot external command signal.
    command: Option<Signal<Option<TreeViewCommand<T>>>>,
    /// Command observed by one exact authoritative layout attempt.
    pending_command: RefCell<Option<TransactionalLayoutPending<TreeViewCommand<T>>>>,
    /// Whole-tree disabled state.
    disabled: Binding<bool>,
    /// Drag enablement.
    draggable: Binding<bool>,
    /// Structural mutation policy.
    mutation_mode: Binding<TreeMutationMode>,
    /// Rename editor enablement.
    editable: Binding<bool>,
    /// Delete enablement.
    deletable: Binding<bool>,
    /// Create enablement.
    creatable: Binding<bool>,
    /// Optional create-node factory.
    create_node: Option<TreeCreateFactory<T>>,
    /// Changed-selection callback.
    on_select: Option<TreeSelectHandler<T, A>>,
    /// Explicit activation callback.
    on_activate: Option<TreeActivateHandler<T, A>>,
    /// Branch-toggle callback.
    on_toggle: Option<TreeToggleHandler<T, A>>,
    /// Selected-row trailing-action callback.
    on_trailing_action: Option<TreeTrailingActionHandler<T, A>>,
    /// Drag/drop callback.
    on_move: Option<TreeMoveHandler<T, A>>,
    /// Rename callback.
    on_rename: Option<TreeRenameHandler<T, A>>,
    /// Create callback.
    on_create: Option<TreeCreateHandler<T, A>>,
    /// Create cancellation callback.
    on_create_cancel: Option<TreeCreateCancelHandler<T, A>>,
    /// Delete callback.
    on_delete: Option<TreeDeleteHandler<T, A>>,
    /// Context-menu callback.
    on_context_menu: Option<TreeContextMenuHandler<T, A>>,
    /// Keyboard shortcut callback.
    on_shortcut: Option<TreeShortcutHandler<T, A>>,
    /// Appearance and geometry tokens.
    style: TreeViewStyle,
    /// Whether layout/paint use visible row ranges.
    virtualized: bool,
    /// Current explicit keyboard-active row.
    active_index: Signal<Option<usize>>,
    /// Current drag candidate/state.
    drag: Signal<Option<TreeDragState<T>>>,
    /// Current inline editor.
    editing: Signal<Option<TreeEditing<T>>>,
    /// Retained intent-only provisional create row.
    draft: Signal<Option<TreeDraft<T>>>,
    /// Editor's reactive string value.
    edit_value: Signal<String>,
    /// Editor's text buffer.
    edit_buffer: Signal<TextBuffer>,
    /// Editor caret/selection state.
    edit_state: Signal<TextEditState>,
    /// Last click used for activation counting.
    last_click: Signal<Option<TreeClickState<T>>>,
    /// Monotonic intrinsic-width observation, floored at 160 pixels.
    observed_max_width: Rc<Cell<f32>>,
    /// Persistent flattened snapshot cache.
    snapshot_flat_cache: Rc<RefCell<SnapshotFlatCache<T>>>,
    /// Optional shared diagnostics counters.
    diagnostics: Option<TreeViewDiagnostics>,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Widget<A> for TreeViewWidget<T, A> {
    /// Returns the stable diagnostic widget name.
    fn debug_name(&self) -> &'static str {
        "TreeView"
    }

    /// Consumes commands, visits the selected row range, and resolves intrinsic size.
    ///
    /// Intrinsic width never shrinks below the greatest previously observed width
    /// or 160 logical pixels; intrinsic height covers every source row.
    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        if ctx.layout_pass().is_committed() {
            let command = self.command.as_ref().and_then(Signal::read);
            self.pending_command
                .replace(command.and_then(|command| TransactionalLayoutPending::new(ctx, command)));
        }
        let total_rows = self.visible_len();
        let layout_range = self.layout_row_range(ctx, constraints, total_rows);
        let layout_rows = layout_range.len();
        let nodes = self.visible_nodes_range(layout_range);
        if let Some(diagnostics) = &self.diagnostics {
            let flatten_rebuilds = self
                .model
                .as_ref()
                .map_or(0, |model| model.flatten_rebuilds());
            let fallback = self.virtualized
                && ctx.virtual_viewport().is_none()
                && !constraints.max_h.is_finite();
            diagnostics.update(|snapshot| {
                snapshot.layout_calls = snapshot.layout_calls.saturating_add(1);
                snapshot.loaded_rows = total_rows;
                snapshot.visible_rows = layout_rows;
                snapshot.layout_rows_visited = snapshot
                    .layout_rows_visited
                    .saturating_add(layout_rows as u64);
                snapshot.text_measurements = snapshot
                    .text_measurements
                    .saturating_add(layout_rows as u64);
                snapshot.flatten_rebuilds = flatten_rebuilds.max(snapshot.flatten_rebuilds);
                snapshot.virtualization_fallbacks = snapshot
                    .virtualization_fallbacks
                    .saturating_add(u64::from(fallback));
            });
        }
        let mut max_w = self.observed_max_width.get().max(160.0);
        for node in &nodes {
            let text_w = measure_text(ctx, &node.label, self.text_style(node)).unwrap_or(80.0);
            let icon_w = node
                .leading_icon
                .as_ref()
                .map(|_| self.style.icon_size + self.style.gap)
                .unwrap_or(0.0);
            let action_w = node
                .trailing_action
                .as_ref()
                .map(|_| self.trailing_action_size() + self.style.gap)
                .unwrap_or(0.0);
            max_w = max_w.max(
                self.style.padding_x * 2.0
                    + node.depth as f32 * self.style.indent
                    + self.style.chevron_size
                    + self.style.gap
                    + icon_w
                    + text_w
                    + action_w,
            );
        }
        self.observed_max_width.set(max_w);
        let intrinsic = Size::new(
            max_w,
            self.style.padding_y * 2.0 + total_rows as f32 * self.style.row_height,
        );
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    /// Publishes a staged editor command only after its observing layout won.
    ///
    /// A create command inserts a row and therefore deliberately schedules a
    /// second authoritative layout. Paint fails closed between those frames
    /// instead of drawing that fresh draft against older geometry.
    fn layout_committed(&self, ctx: &mut LayoutCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        let Some(command) = self
            .pending_command
            .borrow_mut()
            .take()
            .and_then(|pending| pending.into_committed(ctx))
        else {
            return;
        };
        if let Some(command_signal) = &self.command {
            command_signal.set(None);
        }
        self.apply_pending_command(command);
    }

    /// Paints the current row range, selection/active/drop/editor state, and focus.
    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, layout: &LayoutResult) {
        let disabled = self.disabled.read();
        if self.style.background.a > 0.0 {
            ctx.push(DrawCmd::RRect(DrawRRect {
                rect: bounds,
                radius: self.style.radius.tl,
                color: self.style.background,
            }));
        }

        let total_rows = self.visible_len();
        let paint_range = self.paint_row_range(ctx, bounds, total_rows);
        let paint_rows = paint_range.len();
        let nodes = self.visible_nodes_range(paint_range.clone());
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.update(|snapshot| {
                snapshot.paint_calls = snapshot.paint_calls.saturating_add(1);
                snapshot.loaded_rows = total_rows;
                snapshot.visible_rows = paint_rows;
                snapshot.paint_rows_visited = snapshot
                    .paint_rows_visited
                    .saturating_add(paint_rows as u64);
            });
        }
        let selected = self.selected_value();
        let active = self.normalized_active_index_for_source();
        let drag = self.drag.read();
        let editing = self.editing.read();
        for (local_index, node) in nodes.iter().enumerate() {
            let idx = paint_range.start + local_index;
            let row = self.row_rect(bounds, idx);
            let is_selected = selected.as_ref().is_some_and(|value| value == &node.id);
            let is_active = ctx.is_focused() && active == Some(idx);
            let is_drag_source = drag
                .as_ref()
                .is_some_and(|drag| drag.active && drag.source == node.id);
            let opacity = if disabled || node.disabled {
                self.style.disabled_opacity
            } else if is_drag_source {
                0.42
            } else {
                1.0
            };
            let bg = if is_selected {
                self.style.selected_background
            } else if is_active {
                self.style.active_background
            } else {
                Color::TRANSPARENT
            };
            if bg.a > 0.0 {
                ctx.push(DrawCmd::RRect(DrawRRect {
                    rect: row,
                    radius: self.style.radius.tl,
                    color: bg.with_alpha(bg.a * opacity),
                }));
            }

            if let Some(drop) = drag
                .as_ref()
                .and_then(|drag| drag.active.then_some(drag))
                .and_then(|drag| drag.target.as_ref())
                .filter(|drop| drop.target == node.id)
            {
                self.paint_drop_target(ctx, row, drop);
            }

            self.paint_node(
                ctx,
                TreeNodePaint {
                    row,
                    node,
                    selected: is_selected,
                    opacity,
                    layout,
                    focused: ctx.is_focused(),
                    editing: editing.as_ref().filter(|edit| edit.id == node.id),
                },
            );
            if is_active && !disabled && !node.disabled {
                ctx.push(DrawCmd::Border(DrawBorder {
                    rect: row,
                    radius: self.style.radius,
                    border: self.style.focus_ring,
                }));
            }
        }
    }

    /// Consumes queued commands, then routes enabled input to widget logic.
    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, layout: &LayoutResult) {
        // Events run outside build/layout/paint traversal. Applying the command
        // here lets a first typed character reach the freshly opened editor;
        // geometry-changing commands still request Layout through their state.
        self.consume_pending_command_for_event();
        if self.disabled.read() {
            return;
        }
        if self.handle_editing_event(ctx, event, bounds, layout) {
            return;
        }
        match event {
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: true,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                self.handle_pointer_press(ctx, bounds, *pos);
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Right,
                pressed: true,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                self.handle_context_menu(ctx, bounds, *pos);
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) => {
                self.handle_pointer_release(ctx, bounds, *pos);
            }
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                self.handle_pointer_move(ctx, bounds, *pos);
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                self.handle_keyboard(ctx, key);
            }
            _ => {}
        }
    }

    /// Is focusable only when the whole tree and at least one visible row are enabled.
    fn focus_policy(&self) -> FocusPolicy {
        let has_enabled = self.draft.read().is_some_and(|draft| !draft.node.disabled)
            || self.model.as_ref().map_or_else(
                || self.visible_nodes().iter().any(|node| !node.disabled),
                |model| model.first_enabled_row().is_some(),
            );
        if self.disabled.read() || !has_enabled {
            FocusPolicy::NotFocusable
        } else {
            FocusPolicy::Focusable
        }
    }

    /// Exposes single-line text input only while an inline editor is active.
    fn input_role(&self) -> InputRole {
        if self.editing.read().is_some() {
            InputRole::TextSingleLine
        } else {
            InputRole::None
        }
    }

    /// Returns inline editor IME caret geometry, or `None` outside editing.
    fn ime_cursor_rect(&self, bounds: Rect, layout: &LayoutResult) -> Option<Rect> {
        let editing = self.editing.read()?;
        let nodes = self.visible_nodes();
        let idx = nodes.iter().position(|node| node.id == editing.id)?;
        let row = self.row_rect(bounds, idx);
        let edit_rect = self.edit_rect(row, &nodes[idx]);
        ime_cursor_rect(
            edit_rect,
            layout,
            &self.edit_value,
            &self.edit_buffer,
            &self.edit_state,
            self.edit_text_style(),
        )
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> TreeViewWidget<T, A> {
    /// Applies one queued command at the start of an event traversal.
    fn consume_pending_command_for_event(&self) {
        let Some(command_signal) = &self.command else {
            return;
        };
        let Some(command) = command_signal.read() else {
            return;
        };
        command_signal.set(None);
        self.apply_pending_command(command);
    }

    /// Applies one command after the layout attempt that observed it committed.
    fn apply_pending_command(&self, command: TreeViewCommand<T>) {
        match command {
            TreeViewCommand::BeginRename(id) => {
                let nodes = self.visible_nodes();
                if let Some((idx, node)) = nodes.iter().enumerate().find(|(_, node)| node.id == id)
                {
                    if let Some(bound) = &self.bound_selected {
                        bound.set(node.id.clone());
                    }
                    self.active_index.set(Some(idx));
                    self.begin_rename(node);
                }
            }
            TreeViewCommand::BeginCreate(request) => {
                self.begin_create_from_request(request);
            }
        }
    }

    /// Clones bound snapshot roots when present, otherwise static roots.
    fn current_nodes(&self) -> Vec<TreeNode<T>> {
        self.bound_nodes
            .as_ref()
            .map(Signal::read)
            .unwrap_or_else(|| self.nodes.clone())
    }

    /// Returns retained row count or synchronizes/reads snapshot cache length.
    fn source_visible_len(&self) -> usize {
        self.model.as_ref().map_or_else(
            || {
                self.sync_snapshot_flat_cache();
                self.snapshot_flat_cache.borrow().rows.len()
            },
            |model| model.visible_len(),
        )
    }

    /// Returns source row count plus an optional provisional draft row.
    fn visible_len(&self) -> usize {
        self.source_visible_len() + usize::from(self.draft.read().is_some())
    }

    /// Clones one visible row, inserting a retained draft at its virtual position.
    fn visible_node_at(&self, index: usize) -> Option<FlatNode<T>> {
        let draft = self.draft.read();
        if let Some(draft) = &draft {
            if index == draft.insert_index {
                return Some(draft.node.clone());
            }
        }
        let source_index = draft
            .as_ref()
            .filter(|draft| index > draft.insert_index)
            .map_or(index, |_| index.saturating_sub(1));
        self.model.as_ref().map_or_else(
            || {
                self.sync_snapshot_flat_cache();
                self.snapshot_flat_cache
                    .borrow()
                    .rows
                    .get(source_index)
                    .cloned()
            },
            |model| model.flat_node_at(source_index),
        )
    }

    /// Clones existing visible rows in the requested half-open range.
    fn visible_nodes_range(&self, range: std::ops::Range<usize>) -> Vec<FlatNode<T>> {
        range
            .filter_map(|index| self.visible_node_at(index))
            .collect()
    }

    /// Replaces bound snapshot roots and reports whether a binding existed.
    fn set_current_nodes(&self, nodes: Vec<TreeNode<T>>) -> bool {
        let Some(bound) = &self.bound_nodes else {
            return false;
        };
        bound.set(nodes);
        true
    }

    /// Reports structural capability: bound snapshot or retained intent-only mode.
    fn can_mutate_nodes(&self) -> bool {
        self.bound_nodes.is_some()
            || (self.model.is_some() && self.mutation_mode.read() == TreeMutationMode::IntentOnly)
    }

    /// Adjusts a source row index for an inserted provisional draft.
    fn source_index_to_visible(&self, source_index: usize) -> usize {
        self.draft.read().map_or(source_index, |draft| {
            source_index + usize::from(source_index >= draft.insert_index)
        })
    }

    /// Resolves a retained draft's visible insertion row and depth.
    fn retained_create_position(&self, request: &TreeCreateRequest<T>) -> Option<(usize, usize)> {
        let rows = (0..self.source_visible_len())
            .filter_map(|index| {
                self.model
                    .as_ref()
                    .and_then(|model| model.flat_node_at(index))
            })
            .collect::<Vec<_>>();
        match request.kind {
            TreeCreateKind::SiblingAfter => {
                let Some(after) = &request.after else {
                    return Some((rows.len(), 0));
                };
                let row = rows.iter().position(|node| &node.id == after)?;
                let depth = rows[row].depth;
                Some((visible_subtree_end(&rows, row), depth))
            }
            TreeCreateKind::Child => {
                let parent = request.parent.as_ref()?;
                let row = rows.iter().position(|node| &node.id == parent)?;
                let depth = rows[row].depth.saturating_add(1);
                Some((visible_subtree_end(&rows, row), depth))
            }
        }
    }

    /// Installs one intent-only retained draft and begins its create editor.
    ///
    /// Rejects concurrent draft/editing and unresolved insertion positions.
    fn begin_retained_create(&self, node: TreeNode<T>, request: TreeCreateRequest<T>) -> bool {
        if self.draft.read().is_some() || self.editing.read().is_some() {
            return false;
        }
        let Some((insert_index, depth)) = self.retained_create_position(&request) else {
            return false;
        };
        let flat = FlatNode {
            id: node.id,
            label: node.label,
            depth,
            branch: node.branch,
            disabled: node.disabled.read(),
            leading_icon: node.leading_icon,
            leading_icon_tint: node.leading_icon_tint,
            trailing_action: node.trailing_action,
            parent: request.parent.clone(),
        };
        let created_id = flat.id.clone();
        self.draft.set(Some(TreeDraft {
            node: flat.clone(),
            insert_index,
        }));
        if let Some(bound) = &self.bound_selected {
            bound.set(created_id);
        }
        self.active_index.set(Some(insert_index));
        self.begin_create_rename(&flat, request)
    }

    /// Reads snapshot expansion IDs and removes later duplicates stably.
    fn expanded_values(&self) -> Vec<T> {
        unique_vec(self.expanded.read())
    }

    /// Clones readable controlled selection when configured.
    fn selected_value(&self) -> Option<T> {
        self.selected.as_ref().map(Binding::read)
    }

    /// Clones every visible source/draft row.
    fn visible_nodes(&self) -> Vec<FlatNode<T>> {
        (0..self.visible_len())
            .filter_map(|index| self.visible_node_at(index))
            .collect()
    }

    /// Synchronizes then clones snapshot-tree cached rows.
    fn visible_nodes_snapshot(&self) -> Vec<FlatNode<T>> {
        self.sync_snapshot_flat_cache();
        self.snapshot_flat_cache.borrow().rows.clone()
    }

    /// Rebuilds snapshot rows when bound-node revision or expansion IDs change.
    ///
    /// Static unbound nodes use revision zero and rebuild only on first access or
    /// expansion change.
    fn sync_snapshot_flat_cache(&self) {
        let expanded = self.expanded_values();
        let nodes_revision = self.bound_nodes.as_ref().map_or(0, Signal::revision);
        {
            let cache = self.snapshot_flat_cache.borrow();
            if cache.initialized
                && cache.nodes_revision == nodes_revision
                && cache.expanded == expanded
            {
                return;
            }
        }
        let mut out = Vec::new();
        for node in &self.current_nodes() {
            flatten_node(node, 0, None, &expanded, &mut out);
        }
        let mut cache = self.snapshot_flat_cache.borrow_mut();
        cache.nodes_revision = nodes_revision;
        cache.initialized = true;
        cache.expanded = expanded;
        cache.rows = out;
        cache.rebuilds = cache.rebuilds.saturating_add(1);
        if let Some(diagnostics) = &self.diagnostics {
            let rows = cache.rows.len() as u64;
            let rebuilds = cache.rebuilds;
            diagnostics.update(|snapshot| {
                snapshot.flatten_rebuilds = rebuilds;
                snapshot.snapshot_rows_cloned = snapshot.snapshot_rows_cloned.saturating_add(rows);
            });
        }
    }

    /// Selects all rows or a viewport/finite-height range plus eight-row overscan.
    fn layout_row_range(
        &self,
        ctx: &LayoutCtx<'_>,
        constraints: Constraints,
        len: usize,
    ) -> std::ops::Range<usize> {
        if !self.virtualized || len == 0 {
            return 0..len;
        }
        if let Some(viewport) = ctx.virtual_viewport() {
            return row_range_with_overscan(
                viewport.rect.y,
                viewport.rect.h,
                self.style.padding_y,
                self.style.row_height,
                len,
                TREE_VIRTUAL_OVERSCAN_ROWS,
            );
        }
        if constraints.max_h.is_finite() {
            return row_range_with_overscan(
                0.0,
                constraints.max_h,
                self.style.padding_y,
                self.style.row_height,
                len,
                TREE_VIRTUAL_OVERSCAN_ROWS,
            );
        }
        0..len
    }

    /// Normalizes active row against draft, retained, or snapshot sources.
    fn normalized_active_index_for_source(&self) -> Option<usize> {
        if self.draft.read().is_some() {
            let nodes = self.visible_nodes();
            return self.normalized_active_index(&nodes);
        }
        if let Some(model) = &self.model {
            if let Some(index) = self.active_index.read() {
                if model.flat_node_at(index).is_some_and(|node| !node.disabled) {
                    return Some(index);
                }
            }
            if let Some(selected) = self.selected_value() {
                if let Some(index) = model.row_of(&selected) {
                    if model.flat_node_at(index).is_some_and(|node| !node.disabled) {
                        return Some(index);
                    }
                }
            }
            return model.first_enabled_row();
        }
        let nodes = self.visible_nodes_snapshot();
        self.normalized_active_index(&nodes)
    }

    /// Computes one padded logical-pixel row rectangle.
    fn row_rect(&self, bounds: Rect, index: usize) -> Rect {
        Rect::new(
            bounds.x + self.style.padding_x,
            bounds.y + self.style.padding_y + index as f32 * self.style.row_height,
            (bounds.w - self.style.padding_x * 2.0).max(0.0),
            self.style.row_height,
        )
    }

    /// Maps a y coordinate to one row and records bounded hit-test diagnostics.
    fn row_index_at(&self, bounds: Rect, y: f32, len: usize) -> Option<usize> {
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.update(|snapshot| {
                snapshot.hit_tests = snapshot.hit_tests.saturating_add(1);
            });
        }
        let local_y = y - bounds.y - self.style.padding_y;
        if local_y < 0.0 {
            return None;
        }
        let idx = (local_y / self.style.row_height).floor() as usize;
        let result = (idx < len).then_some(idx);
        if result.is_some() {
            if let Some(diagnostics) = &self.diagnostics {
                diagnostics.update(|snapshot| {
                    snapshot.hit_test_rows_visited =
                        snapshot.hit_test_rows_visited.saturating_add(1);
                });
            }
        }
        result
    }

    /// Selects all rows or current clip range plus eight-row overscan for paint.
    fn paint_row_range(
        &self,
        ctx: &PaintCtx<'_>,
        bounds: Rect,
        len: usize,
    ) -> std::ops::Range<usize> {
        if !self.virtualized || len == 0 {
            return 0..len;
        }
        let clip = ctx.current_clip_bbox().unwrap_or(bounds);
        row_range_with_overscan(
            clip.y - bounds.y,
            clip.h,
            self.style.padding_y,
            self.style.row_height,
            len,
            TREE_VIRTUAL_OVERSCAN_ROWS,
        )
    }

    /// Computes depth-indented branch-chevron geometry.
    fn chevron_rect(&self, row: Rect, node: &FlatNode<T>) -> Rect {
        let x = row.x + node.depth as f32 * self.style.indent;
        Rect::new(
            x,
            row.y + (row.h - self.style.chevron_size) * 0.5,
            self.style.chevron_size,
            self.style.chevron_size,
        )
    }

    /// Computes text origin after chevron and optional leading icon.
    fn label_x(&self, row: Rect, node: &FlatNode<T>) -> f32 {
        let chevron = self.chevron_rect(row, node);
        let mut x = chevron.right() + self.style.gap;
        if node.leading_icon.is_some() {
            x += self.style.icon_size + self.style.gap;
        }
        x
    }

    /// Computes inline editor bounds with 32x18-pixel minimums.
    fn edit_rect(&self, row: Rect, node: &FlatNode<T>) -> Rect {
        let x = self.label_x(row, node);
        Rect::new(
            x - 4.0,
            row.y + 2.0,
            (row.right() - x + 4.0).max(32.0),
            (row.h - 4.0).max(18.0),
        )
    }

    /// Resolves action hit size from row/icon metrics, capped at 28 pixels.
    fn trailing_action_size(&self) -> f32 {
        self.style
            .row_height
            .min(28.0)
            .max(self.style.icon_size + 6.0)
    }

    /// Places the selected-row trailing action at the row's right edge.
    fn trailing_action_rect(&self, row: Rect) -> Rect {
        let size = self.trailing_action_size();
        Rect::new(row.right() - size, row.y + (row.h - size) * 0.5, size, size)
    }

    /// Centers the configured icon size inside an action hit rectangle.
    fn trailing_action_icon_rect(&self, action: Rect) -> Rect {
        Rect::new(
            action.x + (action.w - self.style.icon_size) * 0.5,
            action.y + (action.h - self.style.icon_size) * 0.5,
            self.style.icon_size,
            self.style.icon_size,
        )
    }

    /// Derives single-line editor style from tree tokens and default theme.
    fn edit_text_style(&self) -> TextInputStyle {
        let theme = Theme::default();
        let palette = theme.palette();
        TextInputStyle {
            bg: self.style.editing_background,
            border: self.style.editing_border,
            border_focused: self.style.editing_border,
            caret: palette.text,
            placeholder: palette.text_muted,
            selection_bg: palette.accent.with_alpha(0.34),
            radius: self.style.radius.tl,
            pad_x: 4.0,
            pad_y: 2.0,
            text: self.style.text,
            caret_w: 1.0,
            caret_blink_ms: 500,
        }
    }

    /// Paints a two-pixel sibling line or inside surface/border.
    fn paint_drop_target(&self, ctx: &mut PaintCtx<'_>, row: Rect, drop: &TreeDropTarget<T>) {
        match drop.position {
            TreeDropPosition::Before | TreeDropPosition::After => {
                let y = if drop.position == TreeDropPosition::Before {
                    row.y
                } else {
                    row.bottom() - 2.0
                };
                ctx.push(DrawCmd::Rect(DrawRect {
                    rect: Rect::new(row.x, y, row.w, 2.0),
                    color: self.style.drop_indicator,
                }));
            }
            TreeDropPosition::Inside => {
                ctx.push(DrawCmd::RRect(DrawRRect {
                    rect: row,
                    radius: self.style.radius.tl,
                    color: self.style.drop_inside_background,
                }));
                ctx.push(DrawCmd::Border(DrawBorder {
                    rect: row,
                    radius: self.style.radius,
                    border: Border::new(1.0, self.style.drop_indicator),
                }));
            }
        }
    }

    /// Paints chevron, leading icon, label/editor, and selected trailing action.
    fn paint_node(&self, ctx: &mut PaintCtx<'_>, paint: TreeNodePaint<'_, T>) {
        let TreeNodePaint {
            row,
            node,
            selected,
            opacity,
            layout,
            focused,
            editing,
        } = paint;
        let chevron = self.chevron_rect(row, node);
        if node.branch {
            let icon = if self.is_expanded(&node.id) {
                IconId::Lucide(LucideIcon::ChevronDown)
            } else {
                IconId::Lucide(LucideIcon::ChevronRight)
            };
            ctx.push(DrawCmd::Image(DrawImage {
                rect: chevron,
                icon,
                tint: self
                    .style
                    .chevron_tint
                    .with_alpha(self.style.chevron_tint.a * opacity),
                rotation_rad: 0.0,
            }));
        }

        let mut x = chevron.right() + self.style.gap;
        if let Some(icon) = &node.leading_icon {
            let rect = Rect::new(
                x,
                row.y + (row.h - self.style.icon_size) * 0.5,
                self.style.icon_size,
                self.style.icon_size,
            );
            let tint = if let Some(tint) = node.leading_icon_tint {
                tint
            } else if selected {
                self.style.selected_icon_tint
            } else {
                self.style.icon_tint
            };
            ctx.push(DrawCmd::Image(DrawImage {
                rect,
                icon: icon.clone(),
                tint: tint.with_alpha(tint.a * opacity),
                rotation_rad: 0.0,
            }));
            x += self.style.icon_size + self.style.gap;
        }

        if editing.is_some() {
            let edit_rect = self.edit_rect(row, node);
            let style = self.edit_text_style();
            ctx.push(DrawCmd::RRect(DrawRRect {
                rect: edit_rect,
                radius: style.radius,
                color: style.bg,
            }));
            ctx.push(DrawCmd::Border(DrawBorder {
                rect: edit_rect,
                radius: Radius::uniform(style.radius),
                border: Border::new(
                    1.0,
                    if focused {
                        style.border_focused
                    } else {
                        style.border
                    },
                ),
            }));
            paint_single_line_text(
                ctx,
                edit_rect,
                layout,
                &self.edit_value,
                &self.edit_buffer,
                &self.edit_state,
                None,
                style,
                focused,
            );
        } else {
            paint_text_centered(ctx, &node.label, self.text_style(node), row, x, opacity);
        }

        if selected {
            if let Some(action) = &node.trailing_action {
                let action_rect = self.trailing_action_rect(row);
                let bg = self
                    .style
                    .selected_background
                    .with_alpha((self.style.selected_background.a + 0.10).min(0.42) * opacity);
                if bg.a > 0.0 {
                    ctx.push(DrawCmd::RRect(DrawRRect {
                        rect: action_rect,
                        radius: self.style.radius.tl,
                        color: bg,
                    }));
                }
                let tint = self
                    .style
                    .selected_icon_tint
                    .with_alpha(self.style.selected_icon_tint.a * opacity);
                ctx.push(DrawCmd::Image(DrawImage {
                    rect: self.trailing_action_icon_rect(action_rect),
                    icon: action.icon.clone(),
                    tint,
                    rotation_rad: 0.0,
                }));
            }
        }
    }

    /// Chooses disabled, selected, or normal text style in precedence order.
    fn text_style(&self, node: &FlatNode<T>) -> TextStyle {
        if node.disabled || self.disabled.read() {
            self.style.disabled_text
        } else if self
            .selected_value()
            .as_ref()
            .is_some_and(|selected| selected == &node.id)
        {
            self.style.selected_text
        } else {
            self.style.text
        }
    }

    /// Returns valid enabled active row, selected match, or first enabled row.
    fn normalized_active_index(&self, nodes: &[FlatNode<T>]) -> Option<usize> {
        let active = self.active_index.read();
        if let Some(idx) = active {
            if idx < nodes.len() && !nodes[idx].disabled {
                return Some(idx);
            }
        }
        if let Some(selected) = self.selected_value() {
            if let Some(idx) = nodes
                .iter()
                .position(|node| !node.disabled && node.id == selected)
            {
                return Some(idx);
            }
        }
        nodes.iter().position(|node| !node.disabled)
    }

    /// Handles enabled row release: chevron toggle or selection/double activation.
    fn handle_pointer(&self, ctx: &mut EventCtx<A>, bounds: Rect, pos: Point) {
        let Some(idx) = self.row_index_at(bounds, pos.y, self.visible_len()) else {
            return;
        };
        let Some(node) = self.visible_node_at(idx) else {
            return;
        };
        if node.disabled {
            return;
        }
        self.active_index.set(Some(idx));
        let row = self.row_rect(bounds, idx);
        if node.branch && self.chevron_rect(row, &node).contains(pos.x, pos.y) {
            self.toggle_node(ctx, &node);
        } else {
            let click_count = self.register_row_click(ctx, &node, pos);
            self.select_node(ctx, &node);
            if click_count == 2 {
                self.activate_node(ctx, &node);
            }
        }
    }

    /// Emits row/blank context, selecting an enabled row first when needed.
    fn handle_context_menu(&self, ctx: &mut EventCtx<A>, bounds: Rect, pos: Point) {
        let Some(on_context_menu) = &self.on_context_menu else {
            return;
        };
        if let Some(idx) = self.row_index_at(bounds, pos.y, self.visible_len()) {
            let Some(node) = self.visible_node_at(idx) else {
                return;
            };
            if node.disabled {
                return;
            }
            self.active_index.set(Some(idx));
            let row = self.row_rect(bounds, idx);
            let selected = self.selected_value();
            if selected
                .as_ref()
                .is_none_or(|selected| selected != &node.id)
            {
                self.select_node(ctx, &node);
            }
            on_context_menu(
                ctx,
                TreeContextMenu::Row {
                    row_id: node.id.clone(),
                    pointer_position: pos,
                    row_rect: row,
                },
            );
            ctx.request_repaint();
            ctx.stop_propagation();
            return;
        }
        on_context_menu(
            ctx,
            TreeContextMenu::Blank {
                pointer_position: pos,
            },
        );
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Arms trailing-action release or a structurally capable drag candidate.
    fn handle_pointer_press(&self, ctx: &mut EventCtx<A>, bounds: Rect, pos: Point) {
        let Some(idx) = self.row_index_at(bounds, pos.y, self.visible_len()) else {
            return;
        };
        let Some(node) = self.visible_node_at(idx) else {
            return;
        };
        if node.disabled {
            return;
        }
        self.active_index.set(Some(idx));
        let row = self.row_rect(bounds, idx);
        if self.trailing_action_hit(row, &node, pos) {
            ctx.stop_propagation();
            ctx.request_repaint();
            return;
        }
        if self.draggable.read()
            && self.can_mutate_nodes()
            && !self.chevron_rect(row, &node).contains(pos.x, pos.y)
        {
            self.drag.set(Some(TreeDragState {
                source: node.id.clone(),
                start: pos,
                active: false,
                target: None,
            }));
            ctx.stop_propagation();
        }
    }

    /// Activates drag after four pixels and updates current drop target.
    fn handle_pointer_move(&self, ctx: &mut EventCtx<A>, bounds: Rect, pos: Point) {
        let Some(mut drag) = self.drag.read() else {
            return;
        };
        if !drag.active {
            let dx = pos.x - drag.start.x;
            let dy = pos.y - drag.start.y;
            if (dx * dx + dy * dy).sqrt() > 4.0 {
                drag.active = true;
            }
        }
        if drag.active {
            drag.target = self.drop_target_at(bounds, pos, &drag.source);
            ctx.request_repaint();
        }
        self.drag.set(Some(drag));
        ctx.stop_propagation();
    }

    /// Commits active drag, emits selected trailing action, or handles row click.
    fn handle_pointer_release(&self, ctx: &mut EventCtx<A>, bounds: Rect, pos: Point) {
        if let Some(drag) = self.drag.read() {
            self.drag.set(None);
            if drag.active {
                if let Some(drop) = drag.target {
                    self.apply_drop(ctx, drag.source, drop);
                } else {
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
                return;
            }
        }
        if let Some(idx) = self.row_index_at(bounds, pos.y, self.visible_len()) {
            let Some(node) = self.visible_node_at(idx) else {
                return;
            };
            let row = self.row_rect(bounds, idx);
            if self.trailing_action_hit(row, &node, pos) {
                self.active_index.set(Some(idx));
                self.emit_trailing_action(ctx, &node);
                return;
            }
        }
        if bounds.contains(pos.x, pos.y) {
            self.handle_pointer(ctx, bounds, pos);
        }
    }

    /// Tests an enabled selected row's configured trailing-action rectangle.
    fn trailing_action_hit(&self, row: Rect, node: &FlatNode<T>, pos: Point) -> bool {
        if node.disabled || node.trailing_action.is_none() {
            return false;
        }
        let Some(selected) = self.selected_value() else {
            return false;
        };
        if selected != node.id {
            return false;
        }
        self.trailing_action_rect(row).contains(pos.x, pos.y)
    }

    /// Invokes configured trailing action and consumes the event when eligible.
    fn emit_trailing_action(&self, ctx: &mut EventCtx<A>, node: &FlatNode<T>) {
        if node.disabled || node.trailing_action.is_none() {
            return;
        }
        if let Some(on_trailing_action) = &self.on_trailing_action {
            on_trailing_action(ctx, node.id.clone());
            ctx.request_repaint();
            ctx.stop_propagation();
        }
    }

    /// Resolves an enabled non-self target using vertical thirds of its row.
    fn drop_target_at(&self, bounds: Rect, pos: Point, source: &T) -> Option<TreeDropTarget<T>> {
        let nodes = self.visible_nodes();
        let idx = self.row_index_at(bounds, pos.y, nodes.len())?;
        let node = &nodes[idx];
        if node.disabled || &node.id == source {
            return None;
        }
        let row = self.row_rect(bounds, idx);
        let local_y = ((pos.y - row.y) / row.h.max(1.0)).clamp(0.0, 1.0);
        let position = if local_y < 0.33 {
            TreeDropPosition::Before
        } else if local_y > 0.66 {
            TreeDropPosition::After
        } else {
            TreeDropPosition::Inside
        };
        Some(TreeDropTarget {
            target: node.id.clone(),
            position,
        })
    }

    /// Routes navigation, expansion, activation, edit/create/delete, and shortcuts.
    fn handle_keyboard(&self, ctx: &mut EventCtx<A>, key: &KeyEvent) {
        let nodes = self.visible_nodes();
        if nodes.is_empty() || self.editing.read().is_some() {
            return;
        }
        let active = self.normalized_active_index(&nodes);
        let shortcut_target = self.shortcut_target_node(&nodes, active);
        if self.handle_keyboard_shortcut(ctx, key, shortcut_target) {
            return;
        }
        match &key.key {
            Key::Named(NamedKey::F(2)) => {
                if let Some(idx) = active {
                    self.start_rename(ctx, &nodes[idx]);
                }
            }
            Key::Named(NamedKey::Delete) => {
                if let Some(idx) = active {
                    self.delete_node(ctx, &nodes, idx);
                }
            }
            Key::Named(NamedKey::Insert) if key.modifiers.ctrl => {
                if let Some(idx) = active {
                    self.create_node(ctx, &nodes, idx, TreeCreateKind::Child);
                }
            }
            Key::Named(NamedKey::Insert) => {
                if let Some(idx) = active {
                    self.create_node(ctx, &nodes, idx, TreeCreateKind::SiblingAfter);
                }
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.move_active(ctx, next_enabled(&nodes, active, 1));
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.move_active(ctx, next_enabled(&nodes, active, -1));
            }
            Key::Named(NamedKey::Home) => {
                self.move_active(ctx, nodes.iter().position(|node| !node.disabled));
            }
            Key::Named(NamedKey::End) => {
                self.move_active(ctx, nodes.iter().rposition(|node| !node.disabled));
            }
            Key::Named(NamedKey::ArrowRight) => {
                if let Some(idx) = active {
                    let node = &nodes[idx];
                    if node.branch && !self.is_expanded(&node.id) {
                        self.toggle_node(ctx, node);
                    } else if node.branch {
                        self.move_active(ctx, first_visible_child_index(&nodes, idx));
                    }
                }
            }
            Key::Named(NamedKey::ArrowLeft) => {
                if let Some(idx) = active {
                    let node = &nodes[idx];
                    if node.branch && self.is_expanded(&node.id) {
                        self.toggle_node(ctx, node);
                    } else if let Some(parent) = &node.parent {
                        self.move_active(ctx, nodes.iter().position(|row| &row.id == parent));
                    }
                }
            }
            Key::Named(NamedKey::Enter) => {
                if let Some(idx) = active {
                    if self.on_activate.is_some() {
                        self.activate_node(ctx, &nodes[idx]);
                    } else {
                        self.select_node(ctx, &nodes[idx]);
                    }
                }
            }
            Key::Named(NamedKey::Space) => {
                if let Some(idx) = active {
                    let node = &nodes[idx];
                    if node.branch {
                        self.toggle_node(ctx, node);
                    } else {
                        self.select_node(ctx, node);
                    }
                }
            }
            _ => {}
        }
    }

    /// Emits configured Delete or Control-C/X/V shortcut before built-ins.
    ///
    /// Returns whether a shortcut was emitted and consumed.
    fn handle_keyboard_shortcut(
        &self,
        ctx: &mut EventCtx<A>,
        key: &KeyEvent,
        active: Option<&FlatNode<T>>,
    ) -> bool {
        let Some(on_shortcut) = &self.on_shortcut else {
            return false;
        };
        if key.modifiers.alt || key.modifiers.meta {
            return false;
        }
        let shortcut = match &key.key {
            Key::Named(NamedKey::Delete) => {
                let Some(active) = active else {
                    return false;
                };
                Some(TreeShortcut::Delete {
                    id: active.id.clone(),
                })
            }
            Key::Character(_) if key.modifiers.ctrl => match key_character_upper(key).as_deref() {
                Some("C") => active.map(|node| TreeShortcut::Copy {
                    id: node.id.clone(),
                }),
                Some("X") => active.map(|node| TreeShortcut::Cut {
                    id: node.id.clone(),
                }),
                Some("V") => Some(TreeShortcut::Paste {
                    id: active.map(|node| node.id.clone()),
                }),
                _ => None,
            },
            _ => None,
        };
        let Some(shortcut) = shortcut else {
            return false;
        };
        on_shortcut(ctx, shortcut);
        ctx.request_repaint();
        ctx.stop_propagation();
        true
    }

    /// Prefers enabled controlled selection, then enabled active row as target.
    fn shortcut_target_node<'a>(
        &self,
        nodes: &'a [FlatNode<T>],
        active: Option<usize>,
    ) -> Option<&'a FlatNode<T>> {
        if let Some(selected) = self.selected_value() {
            if let Some(node) = nodes
                .iter()
                .find(|node| !node.disabled && node.id == selected)
            {
                return Some(node);
            }
        }
        active.and_then(|idx| nodes.get(idx).filter(|node| !node.disabled))
    }

    /// Stores a changed active row and consumes the navigation event.
    fn move_active(&self, ctx: &mut EventCtx<A>, next: Option<usize>) {
        if next != self.active_index.read() {
            self.active_index.set(next);
            ctx.request_repaint();
            ctx.stop_propagation();
        }
    }

    /// Writes/emits only a changed enabled selection.
    fn select_node(&self, ctx: &mut EventCtx<A>, node: &FlatNode<T>) {
        if node.disabled {
            return;
        }
        let changed = self
            .selected_value()
            .as_ref()
            .is_none_or(|selected| selected != &node.id);
        if changed {
            if let Some(bound) = &self.bound_selected {
                bound.set(node.id.clone());
            }
            if let Some(on_select) = &self.on_select {
                on_select(ctx, node.id.clone());
            }
            ctx.request_repaint();
            ctx.stop_propagation();
        }
    }

    /// Emits explicit activation only for enabled nodes with a handler.
    fn activate_node(&self, ctx: &mut EventCtx<A>, node: &FlatNode<T>) {
        if node.disabled {
            return;
        }
        if let Some(on_activate) = &self.on_activate {
            on_activate(ctx, node.id.clone());
            ctx.request_repaint();
            ctx.stop_propagation();
        }
    }

    /// Registers a click and returns a 1..=3 count under time/distance/ID rules.
    fn register_row_click(&self, ctx: &EventCtx<A>, node: &FlatNode<T>, pos: Point) -> u8 {
        let now = ctx
            .event_meta()
            .map(|meta| TreeClickTimestamp::Event(meta.timestamp().duration()))
            .unwrap_or_else(|| TreeClickTimestamp::Legacy(Instant::now()));
        let next = self
            .last_click
            .read()
            .filter(|last| {
                last.id == node.id
                    && now
                        .elapsed_since(last.at)
                        .is_some_and(|elapsed| elapsed <= TREE_ACTIVATE_MAX_DELAY)
                    && point_distance_sq(last.pos, pos)
                        <= TREE_ACTIVATE_MAX_DISTANCE * TREE_ACTIVATE_MAX_DISTANCE
            })
            .map(|last| last.count.saturating_add(1).min(3))
            .unwrap_or(1);
        self.last_click.set(Some(TreeClickState {
            at: now,
            pos,
            id: node.id.clone(),
            count: next,
        }));
        next
    }

    /// Applies/emits a valid branch's requested next expansion state.
    ///
    /// Retained `ApplyLocal` must successfully mutate the model; retained
    /// `IntentOnly` emits without mutation. Snapshot mutation uses a writable
    /// expansion signal when available.
    fn toggle_node(&self, ctx: &mut EventCtx<A>, node: &FlatNode<T>) {
        if node.disabled || !node.branch {
            return;
        }
        let next_open = !self.is_expanded(&node.id);
        if let Some(model) = &self.model {
            if self.mutation_mode.read() == TreeMutationMode::ApplyLocal
                && !model.set_expanded(node.id.clone(), next_open)
            {
                return;
            }
        } else {
            let mut expanded = self.expanded_values();
            if next_open {
                if !expanded.iter().any(|id| id == &node.id) {
                    expanded.push(node.id.clone());
                }
            } else {
                expanded.retain(|id| id != &node.id);
            }
            if let Some(bound) = &self.mutable_expanded {
                bound.set(expanded);
            }
        }
        if let Some(on_toggle) = &self.on_toggle {
            on_toggle(ctx, node.id.clone(), next_open);
        }
        let active = self.model.as_ref().map_or_else(
            || {
                self.visible_nodes_snapshot()
                    .iter()
                    .position(|row| !row.disabled && row.id == node.id)
            },
            |model| {
                let row = model.row_of(&node.id)?;
                Some(self.source_index_to_visible(row))
            },
        );
        self.active_index.set(active);
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Reads expansion from retained model or deduplicated snapshot IDs.
    fn is_expanded(&self, id: &T) -> bool {
        self.model.as_ref().map_or_else(
            || self.expanded_values().iter().any(|expanded| expanded == id),
            |model| model.is_expanded(id),
        )
    }

    /// Handles focus loss, Escape/Enter, outside click, or single-line editing.
    ///
    /// Returns whether an active editor consumed/routed the event.
    fn handle_editing_event(
        &self,
        ctx: &mut EventCtx<A>,
        event: &Event,
        bounds: Rect,
        layout: &LayoutResult,
    ) -> bool {
        let Some(editing) = self.editing.read() else {
            return false;
        };
        let nodes = self.visible_nodes();
        let Some(idx) = nodes.iter().position(|node| node.id == editing.id) else {
            self.cancel_rename(ctx);
            return true;
        };
        let row = self.row_rect(bounds, idx);
        let edit_rect = self.edit_rect(row, &nodes[idx]);
        match event {
            Event::Focus(focus) if !focus.focused => {
                if !self.commit_rename(ctx) {
                    self.cancel_rename(ctx);
                }
                true
            }
            Event::Keyboard(key)
                if key.state == KeyState::Pressed && key.key == Key::Named(NamedKey::Escape) =>
            {
                self.cancel_rename(ctx);
                ctx.stop_propagation();
                true
            }
            Event::Keyboard(key)
                if key.state == KeyState::Pressed && key.key == Key::Named(NamedKey::Enter) =>
            {
                self.commit_rename(ctx);
                ctx.stop_propagation();
                true
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: true,
                ..
            }) if !edit_rect.contains(pos.x, pos.y) => {
                self.commit_rename(ctx);
                ctx.stop_propagation();
                true
            }
            _ => handle_single_line_text_event(
                ctx,
                event,
                edit_rect,
                layout,
                &self.edit_value,
                &self.edit_buffer,
                &self.edit_state,
                self.edit_text_style(),
                TextFieldEventOptions {
                    consume_handled_events: true,
                },
            ),
        }
    }

    /// Begins an ordinary rename and consumes input on success.
    fn start_rename(&self, ctx: &mut EventCtx<A>, node: &FlatNode<T>) {
        if self.begin_rename(node) {
            ctx.request_repaint();
            ctx.stop_propagation();
        }
    }

    /// Initializes full-label selection for an eligible ordinary rename.
    fn begin_rename(&self, node: &FlatNode<T>) -> bool {
        if !self.editable.read() || !self.can_mutate_nodes() || node.disabled {
            return false;
        }
        let mut edit_state = TextEditState::new();
        edit_state.caret_byte = node.label.len();
        edit_state.selection = Some(TextSelection {
            anchor: 0,
            caret: node.label.len(),
        });
        self.editing.set(Some(TreeEditing {
            id: node.id.clone(),
            original: node.label.clone(),
            create: None,
        }));
        self.edit_value.set(node.label.clone());
        self.edit_buffer
            .set(TextBuffer::from_string(node.label.clone()));
        self.edit_state.set(edit_state);
        true
    }

    /// Runs the factory and begins external-command create editing when eligible.
    fn begin_create_from_request(&self, request: TreeCreateRequest<T>) -> bool {
        if !self.creatable.read() || !self.can_mutate_nodes() {
            return false;
        }
        let Some(factory) = &self.create_node else {
            return false;
        };
        let Some(node) = factory(request.clone()) else {
            return false;
        };
        if self.model.is_some() && self.mutation_mode.read() == TreeMutationMode::IntentOnly {
            return self.begin_retained_create(node, request);
        }
        let created_id = node.id.clone();
        let mut nodes = self.current_nodes();
        if !insert_created_node(&mut nodes, &request, node) {
            return false;
        }
        self.set_current_nodes(nodes);
        if request.kind == TreeCreateKind::Child {
            if let Some(parent) = &request.parent {
                self.ensure_expanded(parent.clone());
            }
        }
        if let Some(bound) = &self.bound_selected {
            bound.set(created_id.clone());
        }
        self.active_index.set(
            self.visible_nodes()
                .iter()
                .position(|node| !node.disabled && node.id == created_id),
        );
        let nodes = self.visible_nodes();
        if let Some(node) = nodes.iter().find(|node| node.id == created_id) {
            return self.begin_create_rename(node, request);
        }
        true
    }

    /// Initializes full-label selection and retains create cancellation metadata.
    fn begin_create_rename(&self, node: &FlatNode<T>, request: TreeCreateRequest<T>) -> bool {
        if !self.editable.read() || !self.can_mutate_nodes() || node.disabled {
            return false;
        }
        let mut edit_state = TextEditState::new();
        edit_state.caret_byte = node.label.len();
        edit_state.selection = Some(TextSelection {
            anchor: 0,
            caret: node.label.len(),
        });
        self.editing.set(Some(TreeEditing {
            id: node.id.clone(),
            original: node.label.clone(),
            create: Some(request),
        }));
        self.edit_value.set(node.label.clone());
        self.edit_buffer
            .set(TextBuffer::from_string(node.label.clone()));
        self.edit_state.set(edit_state);
        true
    }

    /// Clears editor/draft state and removes/callbacks a cancelled create draft.
    fn cancel_rename(&self, ctx: &mut EventCtx<A>) {
        if let Some(editing) = self.editing.read() {
            if let Some(request) = editing.create.clone() {
                if let Some(on_create_cancel) = &self.on_create_cancel {
                    on_create_cancel(
                        ctx,
                        TreeCreateCancel {
                            id: editing.id.clone(),
                            request,
                        },
                    );
                }
            }
            if editing.create.is_some() {
                let mut nodes = self.current_nodes();
                if delete_node_by_id(&mut nodes, &editing.id).is_some() {
                    let _ = self.set_current_nodes(nodes);
                    self.remove_expanded_ids(&[editing.id]);
                }
            }
        }
        self.draft.set(None);
        self.editing.set(None);
        self.edit_value.set(String::new());
        self.edit_buffer.set(TextBuffer::from_string(String::new()));
        self.edit_state.set(TextEditState::new());
        ctx.request_repaint();
    }

    /// Trims and commits a nonempty editor value according to create/mutation mode.
    ///
    /// Empty input returns `false` and keeps editing. An unchanged ordinary rename
    /// closes successfully without callback. Ordinary changed rename requires a
    /// handler; external create emits final label and removes provisional state.
    fn commit_rename(&self, ctx: &mut EventCtx<A>) -> bool {
        let Some(editing) = self.editing.read() else {
            return false;
        };
        let new_label = self.edit_value.read().trim().to_string();
        if new_label.is_empty() {
            return false;
        }
        self.editing.set(None);
        let is_create = editing.create.is_some();
        if new_label == editing.original && !is_create {
            ctx.request_repaint();
            return true;
        }
        if let Some(request) = editing.create {
            if self.model.is_some() && self.mutation_mode.read() == TreeMutationMode::IntentOnly {
                self.draft.set(None);
                if let Some(on_create) = &self.on_create {
                    on_create(
                        ctx,
                        TreeCreate {
                            id: editing.id,
                            parent: request.parent,
                            after: request.after,
                            kind: request.kind,
                            label: new_label,
                        },
                    );
                }
                ctx.request_repaint();
                ctx.stop_propagation();
                return true;
            }
            let mut nodes = self.current_nodes();
            let _ = delete_node_by_id(&mut nodes, &editing.id);
            if !self.set_current_nodes(nodes) {
                return false;
            }
            if let Some(on_create) = &self.on_create {
                on_create(
                    ctx,
                    TreeCreate {
                        id: editing.id,
                        parent: request.parent,
                        after: request.after,
                        kind: request.kind,
                        label: new_label,
                    },
                );
            }
        } else if let Some(on_rename) = &self.on_rename {
            if self.model.is_some() && self.mutation_mode.read() == TreeMutationMode::IntentOnly {
                on_rename(
                    ctx,
                    TreeRename {
                        id: editing.id,
                        old_label: editing.original,
                        new_label,
                    },
                );
                ctx.request_repaint();
                ctx.stop_propagation();
                return true;
            }
            let mut nodes = self.current_nodes();
            let old_label = if new_label == editing.original {
                editing.original.clone()
            } else {
                let Some(old_label) = rename_node_label(&mut nodes, &editing.id, new_label.clone())
                else {
                    return false;
                };
                old_label
            };
            if !self.set_current_nodes(nodes) {
                return false;
            }
            on_rename(
                ctx,
                TreeRename {
                    id: editing.id,
                    old_label,
                    new_label,
                },
            );
        }
        ctx.request_repaint();
        ctx.stop_propagation();
        true
    }

    /// Applies local snapshot move or emits intent-only move, then reselects source.
    fn apply_drop(&self, ctx: &mut EventCtx<A>, source: T, drop: TreeDropTarget<T>) {
        if !self.draggable.read() || !self.can_mutate_nodes() {
            return;
        }
        let apply_local = self.mutation_mode.read() == TreeMutationMode::ApplyLocal;
        if apply_local {
            let mut nodes = self.current_nodes();
            if !move_node(&mut nodes, &source, &drop.target, drop.position) {
                ctx.request_repaint();
                ctx.stop_propagation();
                return;
            }
            self.set_current_nodes(nodes);
            if drop.position == TreeDropPosition::Inside {
                self.ensure_expanded(drop.target.clone());
            }
        }
        if let Some(bound) = &self.bound_selected {
            bound.set(source.clone());
        }
        if let Some(on_move) = &self.on_move {
            on_move(
                ctx,
                TreeMove {
                    source,
                    target: drop.target,
                    position: drop.position,
                },
            );
        }
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Handles keyboard create request, local insertion/intent, callback, and edit.
    fn create_node(
        &self,
        ctx: &mut EventCtx<A>,
        visible: &[FlatNode<T>],
        idx: usize,
        kind: TreeCreateKind,
    ) {
        if !self.creatable.read() || !self.can_mutate_nodes() {
            return;
        }
        let Some(factory) = &self.create_node else {
            return;
        };
        let target = &visible[idx];
        if target.disabled {
            return;
        }
        let request = match kind {
            TreeCreateKind::SiblingAfter => TreeCreateRequest {
                parent: target.parent.clone(),
                after: Some(target.id.clone()),
                kind,
                default_label: "New item".to_string(),
            },
            TreeCreateKind::Child => TreeCreateRequest {
                parent: Some(target.id.clone()),
                after: None,
                kind,
                default_label: "New item".to_string(),
            },
        };
        let Some(node) = factory(request.clone()) else {
            return;
        };
        if self.model.is_some() && self.mutation_mode.read() == TreeMutationMode::IntentOnly {
            if self.begin_retained_create(node, request) {
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            return;
        }
        let created_id = node.id.clone();
        let created_label = node.label.clone();
        let mut nodes = self.current_nodes();
        if !insert_created_node(&mut nodes, &request, node) {
            return;
        }
        self.set_current_nodes(nodes);
        if kind == TreeCreateKind::Child {
            self.ensure_expanded(target.id.clone());
        }
        if let Some(bound) = &self.bound_selected {
            bound.set(created_id.clone());
        }
        if let Some(on_create) = &self.on_create {
            on_create(
                ctx,
                TreeCreate {
                    id: created_id.clone(),
                    parent: request.parent.clone(),
                    after: request.after.clone(),
                    kind,
                    label: created_label.clone(),
                },
            );
        }
        self.start_rename(
            ctx,
            &FlatNode {
                id: created_id,
                label: created_label,
                depth: target.depth,
                branch: false,
                disabled: false,
                leading_icon: None,
                leading_icon_tint: None,
                trailing_action: None,
                parent: request.parent,
            },
        );
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Deletes locally or emits intent, then chooses the next enabled selection.
    fn delete_node(&self, ctx: &mut EventCtx<A>, visible: &[FlatNode<T>], idx: usize) {
        if !self.deletable.read() || !self.can_mutate_nodes() {
            return;
        }
        let target = &visible[idx];
        if target.disabled {
            return;
        }
        if self.mutation_mode.read() == TreeMutationMode::IntentOnly {
            if let Some(on_delete) = &self.on_delete {
                on_delete(
                    ctx,
                    TreeDelete {
                        id: target.id.clone(),
                        parent: target.parent.clone(),
                    },
                );
            }
            ctx.request_repaint();
            ctx.stop_propagation();
            return;
        }
        let mut nodes = self.current_nodes();
        let Some((deleted, parent)) = delete_node_by_id(&mut nodes, &target.id) else {
            return;
        };
        let deleted_ids = collect_node_ids(&deleted);
        self.set_current_nodes(nodes);
        self.remove_expanded_ids(&deleted_ids);
        let next_idx = next_enabled_after_delete(visible, idx, &deleted_ids);
        if let Some(bound) = &self.bound_selected {
            if let Some(next) = next_idx.and_then(|idx| visible.get(idx)) {
                bound.set(next.id.clone());
            }
        }
        self.active_index.set(next_idx);
        if let Some(on_delete) = &self.on_delete {
            on_delete(
                ctx,
                TreeDelete {
                    id: deleted.id,
                    parent,
                },
            );
        }
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Adds an ID to writable snapshot expansion when absent.
    fn ensure_expanded(&self, id: T) {
        let Some(bound) = &self.mutable_expanded else {
            return;
        };
        let mut expanded = self.expanded_values();
        if !expanded.iter().any(|value| value == &id) {
            expanded.push(id);
            bound.set(expanded);
        }
    }

    /// Removes deleted IDs from writable snapshot expansion.
    fn remove_expanded_ids(&self, ids: &[T]) {
        let Some(bound) = &self.mutable_expanded else {
            return;
        };
        let mut expanded = self.expanded_values();
        expanded.retain(|id| !ids.iter().any(|deleted| deleted == id));
        bound.set(expanded);
    }
}

/// Converts viewport geometry to a clamped row range with symmetric overscan.
///
/// Empty sources, nonpositive row height, or nonpositive viewport height return
/// `0..0`; arithmetic uses saturating integer bounds.
fn row_range_with_overscan(
    viewport_y: f32,
    viewport_height: f32,
    padding_y: f32,
    row_height: f32,
    len: usize,
    overscan_rows: usize,
) -> std::ops::Range<usize> {
    if len == 0 || row_height <= 0.0 || viewport_height <= 0.0 {
        return 0..0;
    }
    let top = viewport_y - padding_y;
    let bottom = viewport_y + viewport_height - padding_y;
    let first = (top / row_height).floor().max(0.0) as usize;
    let last = (bottom / row_height).ceil().max(0.0) as usize;
    let start = first.saturating_sub(overscan_rows).min(len);
    let end = last
        .saturating_add(overscan_rows)
        .saturating_add(1)
        .min(len);
    start..end.max(start)
}

/// Recursively appends a snapshot node and expanded descendants in display order.
fn flatten_node<T: Clone + PartialEq>(
    node: &TreeNode<T>,
    depth: usize,
    parent: Option<T>,
    expanded: &[T],
    out: &mut Vec<FlatNode<T>>,
) {
    let branch = node.branch || !node.children.is_empty();
    let is_expanded = expanded.iter().any(|id| id == &node.id);
    out.push(FlatNode {
        id: node.id.clone(),
        label: node.label.clone(),
        depth,
        branch,
        disabled: node.disabled.read(),
        leading_icon: node.leading_icon.clone(),
        leading_icon_tint: node.leading_icon_tint,
        trailing_action: node.trailing_action.clone(),
        parent: parent.clone(),
    });
    if branch && is_expanded {
        for child in &node.children {
            flatten_node(child, depth + 1, Some(node.id.clone()), expanded, out);
        }
    }
}

/// Finds the first depth-first matching snapshot ID as child indices.
fn find_path<T: PartialEq>(nodes: &[TreeNode<T>], id: &T) -> Option<Vec<usize>> {
    for (idx, node) in nodes.iter().enumerate() {
        if &node.id == id {
            return Some(vec![idx]);
        }
        if let Some(mut child) = find_path(&node.children, id) {
            child.insert(0, idx);
            return Some(child);
        }
    }
    None
}

/// Reports whether `path` starts at `ancestor`, including equality.
fn path_is_descendant_or_self(path: &[usize], ancestor: &[usize]) -> bool {
    path.len() >= ancestor.len() && path.starts_with(ancestor)
}

/// Resolves an immutable snapshot node from a nonempty index path.
fn node_at_path<'a, T>(nodes: &'a [TreeNode<T>], path: &[usize]) -> Option<&'a TreeNode<T>> {
    let (first, rest) = path.split_first()?;
    let node = nodes.get(*first)?;
    if rest.is_empty() {
        Some(node)
    } else {
        node_at_path(&node.children, rest)
    }
}

/// Resolves a mutable snapshot node from a nonempty index path.
fn node_mut_at_path<'a, T>(
    nodes: &'a mut [TreeNode<T>],
    path: &[usize],
) -> Option<&'a mut TreeNode<T>> {
    let (first, rest) = path.split_first()?;
    let node = nodes.get_mut(*first)?;
    if rest.is_empty() {
        Some(node)
    } else {
        node_mut_at_path(&mut node.children, rest)
    }
}

/// Removes and returns a snapshot node at a valid nonempty index path.
fn remove_node_at_path<T>(nodes: &mut Vec<TreeNode<T>>, path: &[usize]) -> Option<TreeNode<T>> {
    if path.len() == 1 {
        return (path[0] < nodes.len()).then(|| nodes.remove(path[0]));
    }
    let parent_path = &path[..path.len() - 1];
    let child_idx = *path.last()?;
    let parent = node_mut_at_path(nodes, parent_path)?;
    (child_idx < parent.children.len()).then(|| parent.children.remove(child_idx))
}

/// Inserts before/after a target sibling or appends inside the target.
fn insert_node_relative<T: Clone + PartialEq>(
    nodes: &mut Vec<TreeNode<T>>,
    target: &T,
    position: TreeDropPosition,
    node: TreeNode<T>,
) -> bool {
    let Some(target_path) = find_path(nodes, target) else {
        return false;
    };
    match position {
        TreeDropPosition::Inside => {
            if let Some(target_node) = node_mut_at_path(nodes, &target_path) {
                target_node.children.push(node);
                true
            } else {
                false
            }
        }
        TreeDropPosition::Before | TreeDropPosition::After => {
            let target_idx = *target_path.last().unwrap_or(&0);
            let insert_idx = if position == TreeDropPosition::Before {
                target_idx
            } else {
                target_idx + 1
            };
            if target_path.len() == 1 {
                nodes.insert(insert_idx.min(nodes.len()), node);
                true
            } else {
                let parent_path = &target_path[..target_path.len() - 1];
                if let Some(parent) = node_mut_at_path(nodes, parent_path) {
                    parent
                        .children
                        .insert(insert_idx.min(parent.children.len()), node);
                    true
                } else {
                    false
                }
            }
        }
    }
}

/// Moves one enabled snapshot subtree while rejecting missing/disabled/cyclic drops.
///
/// The original roots are restored when insertion unexpectedly fails.
fn move_node<T: Clone + PartialEq>(
    nodes: &mut Vec<TreeNode<T>>,
    source: &T,
    target: &T,
    position: TreeDropPosition,
) -> bool {
    let original = nodes.clone();
    let Some(source_path) = find_path(nodes, source) else {
        return false;
    };
    let Some(target_path) = find_path(nodes, target) else {
        return false;
    };
    if path_is_descendant_or_self(&target_path, &source_path) {
        return false;
    }
    let source_disabled = node_at_path(nodes, &source_path)
        .map(|node| node.disabled.read())
        .unwrap_or(true);
    let target_disabled = node_at_path(nodes, &target_path)
        .map(|node| node.disabled.read())
        .unwrap_or(true);
    if source_disabled || target_disabled {
        return false;
    }
    let Some(moved) = remove_node_at_path(nodes, &source_path) else {
        return false;
    };
    if insert_node_relative(nodes, target, position, moved) {
        true
    } else {
        *nodes = original;
        false
    }
}

/// Replaces the first enabled matching snapshot label and returns its old value.
fn rename_node_label<T: PartialEq>(
    nodes: &mut [TreeNode<T>],
    id: &T,
    new_label: String,
) -> Option<String> {
    for node in nodes {
        if &node.id == id {
            if node.disabled.read() {
                return None;
            }
            let old = std::mem::replace(&mut node.label, new_label);
            return Some(old);
        }
        if let Some(old) = rename_node_label(&mut node.children, id, new_label.clone()) {
            return Some(old);
        }
    }
    None
}

/// Inserts a factory node according to a validated create request relation.
fn insert_created_node<T: Clone + PartialEq>(
    nodes: &mut Vec<TreeNode<T>>,
    request: &TreeCreateRequest<T>,
    node: TreeNode<T>,
) -> bool {
    match request.kind {
        TreeCreateKind::SiblingAfter => {
            if let Some(after) = &request.after {
                insert_node_relative(nodes, after, TreeDropPosition::After, node)
            } else {
                nodes.push(node);
                true
            }
        }
        TreeCreateKind::Child => {
            let Some(parent) = &request.parent else {
                return false;
            };
            insert_node_relative(nodes, parent, TreeDropPosition::Inside, node)
        }
    }
}

/// Returns the first visible row after the indexed subtree.
fn visible_subtree_end<T>(rows: &[FlatNode<T>], row: usize) -> usize {
    let depth = rows[row].depth;
    rows.iter()
        .enumerate()
        .skip(row + 1)
        .find_map(|(index, candidate)| (candidate.depth <= depth).then_some(index))
        .unwrap_or(rows.len())
}

/// Removes the first enabled matching snapshot subtree and returns its parent ID.
fn delete_node_by_id<T: Clone + PartialEq>(
    nodes: &mut Vec<TreeNode<T>>,
    id: &T,
) -> Option<(TreeNode<T>, Option<T>)> {
    let path = find_path(nodes, id)?;
    let node = node_at_path(nodes, &path)?;
    if node.disabled.read() {
        return None;
    }
    let parent = if path.len() > 1 {
        node_at_path(nodes, &path[..path.len() - 1]).map(|node| node.id.clone())
    } else {
        None
    };
    remove_node_at_path(nodes, &path).map(|node| (node, parent))
}

/// Collects a snapshot subtree's IDs in depth-first pre-order.
fn collect_node_ids<T: Clone>(node: &TreeNode<T>) -> Vec<T> {
    let mut out = vec![node.id.clone()];
    for child in &node.children {
        out.extend(collect_node_ids(child));
    }
    out
}

/// Finds next enabled surviving row, falling back to the preceding range.
fn next_enabled_after_delete<T: PartialEq>(
    visible: &[FlatNode<T>],
    deleted_idx: usize,
    deleted_ids: &[T],
) -> Option<usize> {
    visible
        .iter()
        .enumerate()
        .skip(deleted_idx + 1)
        .find(|(_, node)| !node.disabled && !deleted_ids.iter().any(|id| id == &node.id))
        .map(|(idx, _)| idx)
        .or_else(|| {
            visible
                .iter()
                .enumerate()
                .take(deleted_idx)
                .rev()
                .find(|(_, node)| !node.disabled && !deleted_ids.iter().any(|id| id == &node.id))
                .map(|(idx, _)| idx)
        })
}

/// Removes later duplicates while preserving first-occurrence order.
fn unique_vec<T: PartialEq>(values: Vec<T>) -> Vec<T> {
    let mut out = Vec::new();
    for value in values {
        if !out.iter().any(|existing| existing == &value) {
            out.push(value);
        }
    }
    out
}

/// Finds the next enabled row with wraparound, or `None` when none exists.
fn next_enabled<T>(
    nodes: &[FlatNode<T>],
    active: Option<usize>,
    direction: isize,
) -> Option<usize> {
    if nodes.is_empty() {
        return None;
    }
    let start = active.unwrap_or(if direction >= 0 { 0 } else { nodes.len() - 1 });
    for step in 1..=nodes.len() {
        let idx = if direction >= 0 {
            (start + step) % nodes.len()
        } else {
            (start + nodes.len() - (step % nodes.len())) % nodes.len()
        };
        if !nodes[idx].disabled {
            return Some(idx);
        }
    }
    None
}

/// Finds the first enabled visible direct child after a parent row.
fn first_visible_child_index<T>(nodes: &[FlatNode<T>], index: usize) -> Option<usize> {
    let parent_depth = nodes.get(index)?.depth;
    nodes
        .iter()
        .enumerate()
        .skip(index + 1)
        .find(|(_, node)| node.depth == parent_depth + 1 && !node.disabled)
        .map(|(idx, _)| idx)
}

/// Returns squared logical-pixel Euclidean distance without a square root.
fn point_distance_sq(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}

/// ASCII-uppercases a character key for shortcut comparison.
fn key_character_upper(key: &KeyEvent) -> Option<String> {
    match &key.key {
        Key::Character(ch) => Some(ch.to_ascii_uppercase()),
        _ => None,
    }
}

/// Measures unwrapped text when layout text services are available.
fn measure_text(ctx: &mut LayoutCtx<'_>, text: &str, style: TextStyle) -> Option<f32> {
    ctx.text_system.as_deref_mut().map(|text_system| {
        text_system
            .layout_cached(TextLayoutParams {
                text,
                style,
                max_width: None,
                wrap_mode: WrapMode::NoWrap,
            })
            .metrics
            .width
    })
}

/// Obtains a cached unwrapped layout when paint text services are available.
fn layout_text(
    ctx: &mut PaintCtx<'_>,
    text: &str,
    style: TextStyle,
) -> Option<Arc<PreparedTextLayout>> {
    ctx.text_system.as_deref_mut().map(|text_system| {
        text_system.layout_cached(TextLayoutParams {
            text,
            style,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        })
    })
}

/// Paints one unwrapped label vertically centered at a fixed x origin.
fn paint_text_centered(
    ctx: &mut PaintCtx<'_>,
    text: &str,
    style: TextStyle,
    bounds: Rect,
    x: f32,
    opacity: f32,
) {
    let Some(layout) = layout_text(ctx, text, style) else {
        return;
    };
    let baseline = layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0);
    let y = bounds.y + (bounds.h - layout.metrics.height) * 0.5 + baseline;
    ctx.push(DrawCmd::Text(DrawText {
        pos: [x, y],
        color: style.color.with_alpha(style.color.a * opacity),
        decoration: ailloli_ui_core::TextDecoration::None,
        layout,
    }));
}
