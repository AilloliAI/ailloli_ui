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

const TREE_ACTIVATE_MAX_DELAY: Duration = Duration::from_millis(500);
const TREE_ACTIVATE_MAX_DISTANCE: f32 = 4.0;
const TREE_VIRTUAL_OVERSCAN_ROWS: usize = 8;

/// Permanent structural counters for a [`TreeView`]. The handle is UI-local
/// and intentionally cheap to clone into a benchmark or diagnostics panel.
#[derive(Clone, Default)]
pub struct TreeViewDiagnostics {
    inner: Rc<RefCell<TreeViewDiagnosticsSnapshot>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TreeViewDiagnosticsSnapshot {
    pub layout_calls: u64,
    pub paint_calls: u64,
    pub hit_tests: u64,
    pub loaded_rows: usize,
    pub visible_rows: usize,
    pub layout_rows_visited: u64,
    pub paint_rows_visited: u64,
    pub hit_test_rows_visited: u64,
    pub flatten_rebuilds: u64,
    pub snapshot_rows_cloned: u64,
    pub text_measurements: u64,
    pub virtualization_fallbacks: u64,
}

impl TreeViewDiagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> TreeViewDiagnosticsSnapshot {
        *self.inner.borrow()
    }

    fn update(&self, update: impl FnOnce(&mut TreeViewDiagnosticsSnapshot)) {
        update(&mut self.inner.borrow_mut());
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TreeViewSize {
    Compact,
    #[default]
    Default,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TreeViewStyle {
    pub background: Color,
    pub selected_background: Color,
    pub active_background: Color,
    pub hover_background: Color,
    pub pressed_background: Color,
    pub drop_indicator: Color,
    pub drop_inside_background: Color,
    pub editing_background: Color,
    pub editing_border: Color,
    pub text: TextStyle,
    pub selected_text: TextStyle,
    pub disabled_text: TextStyle,
    pub icon_tint: Color,
    pub selected_icon_tint: Color,
    pub chevron_tint: Color,
    pub focus_ring: Border,
    pub radius: Radius,
    pub row_height: f32,
    pub padding_x: f32,
    pub padding_y: f32,
    pub indent: f32,
    pub gap: f32,
    pub icon_size: f32,
    pub chevron_size: f32,
    pub disabled_opacity: f32,
}

impl Default for TreeViewStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), TreeViewSize::Default)
    }
}

impl TreeViewStyle {
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
pub enum TreeDropPosition {
    Before,
    After,
    Inside,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeMove<T> {
    pub source: T,
    pub target: T,
    pub position: TreeDropPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeMutationMode {
    ApplyLocal,
    IntentOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeShortcut<T> {
    Delete { id: T },
    Copy { id: T },
    Cut { id: T },
    Paste { id: Option<T> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRename<T> {
    pub id: T,
    pub old_label: String,
    pub new_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeCreateKind {
    SiblingAfter,
    Child,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeCreateRequest<T> {
    pub parent: Option<T>,
    pub after: Option<T>,
    pub kind: TreeCreateKind,
    pub default_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeCreate<T> {
    pub id: T,
    pub parent: Option<T>,
    pub after: Option<T>,
    pub kind: TreeCreateKind,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeCreateCancel<T> {
    pub id: T,
    pub request: TreeCreateRequest<T>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeDelete<T> {
    pub id: T,
    pub parent: Option<T>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TreeContextMenu<T> {
    Row {
        row_id: T,
        pointer_position: Point,
        row_rect: Rect,
    },
    Blank {
        pointer_position: Point,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeViewCommand<T> {
    BeginRename(T),
    BeginCreate(TreeCreateRequest<T>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNodeTrailingAction {
    pub icon: IconId,
    pub tooltip: Option<String>,
}

impl TreeNodeTrailingAction {
    pub fn new(icon: IconId) -> Self {
        Self {
            icon,
            tooltip: None,
        }
    }

    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }
}

#[derive(Clone)]
pub struct TreeNode<T> {
    id: T,
    label: String,
    branch: bool,
    children: Vec<TreeNode<T>>,
    disabled: Binding<bool>,
    leading_icon: Option<IconId>,
    leading_icon_tint: Option<Color>,
    trailing_action: Option<TreeNodeTrailingAction>,
    transient: bool,
}

impl<T> TreeNode<T> {
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

    pub fn child(mut self, child: TreeNode<T>) -> Self {
        self.branch = true;
        self.children.push(child);
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = TreeNode<T>>) -> Self {
        let children = children.into_iter().collect::<Vec<_>>();
        if !children.is_empty() {
            self.branch = true;
        }
        self.children.extend(children);
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    pub fn leading_icon_tint(mut self, color: Color) -> Self {
        self.leading_icon_tint = Some(color);
        self
    }

    pub fn transient(mut self, transient: bool) -> Self {
        self.transient = transient;
        self
    }

    pub fn trailing_action(mut self, icon: IconId) -> Self {
        self.trailing_action = Some(TreeNodeTrailingAction::new(icon));
        self
    }

    pub fn trailing_action_with_tooltip(
        mut self,
        icon: IconId,
        tooltip: impl Into<String>,
    ) -> Self {
        self.trailing_action = Some(TreeNodeTrailingAction::new(icon).tooltip(tooltip));
        self
    }

    pub fn id(&self) -> &T {
        &self.id
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn child_nodes(&self) -> &[TreeNode<T>] {
        &self.children
    }

    pub fn is_transient(&self) -> bool {
        self.transient
    }

    pub fn trailing_action_ref(&self) -> Option<&TreeNodeTrailingAction> {
        self.trailing_action.as_ref()
    }
}

type TreeSelectHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T)>;
type TreeActivateHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T)>;
type TreeToggleHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T, bool)>;
type TreeTrailingActionHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T)>;
type TreeMoveHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, TreeMove<T>)>;
type TreeRenameHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, TreeRename<T>)>;
type TreeCreateHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, TreeCreate<T>)>;
type TreeCreateCancelHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, TreeCreateCancel<T>)>;
type TreeDeleteHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, TreeDelete<T>)>;
type TreeContextMenuHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, TreeContextMenu<T>)>;
type TreeShortcutHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, TreeShortcut<T>)>;
type TreeCreateFactory<T> = Rc<dyn Fn(TreeCreateRequest<T>) -> Option<TreeNode<T>>>;

trait RetainedTreeSource<T> {
    fn visible_len(&self) -> usize;
    fn flat_node_at(&self, index: usize) -> Option<FlatNode<T>>;
    fn row_of(&self, id: &T) -> Option<usize>;
    fn first_enabled_row(&self) -> Option<usize>;
    fn is_expanded(&self, id: &T) -> bool;
    fn set_expanded(&self, id: T, expanded: bool) -> bool;
    fn subscribe(&self, callback: &Rc<dyn Fn(u64)>) -> TreeModelSubscription;
    fn flatten_rebuilds(&self) -> u64;
}

impl<T> RetainedTreeSource<T> for TreeModelHandle<T>
where
    T: Clone + Eq + Hash + fmt::Debug + 'static,
{
    fn visible_len(&self) -> usize {
        self.read(|model| model.visible_len())
    }

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

    fn row_of(&self, id: &T) -> Option<usize> {
        self.read(|model| model.flat_index().row_of(id))
    }

    fn first_enabled_row(&self) -> Option<usize> {
        self.read(|model| model.flat_index().first_enabled_row())
    }

    fn is_expanded(&self, id: &T) -> bool {
        self.read(|model| model.is_expanded(id))
    }

    fn set_expanded(&self, id: T, expanded: bool) -> bool {
        self.apply(TreeMutation::SetExpanded { id, expanded })
            .is_ok()
    }

    fn subscribe(&self, callback: &Rc<dyn Fn(u64)>) -> TreeModelSubscription {
        TreeModelHandle::subscribe(self, callback)
    }

    fn flatten_rebuilds(&self) -> u64 {
        self.read(|model| model.flat_index().rebuilds())
    }
}

pub struct TreeView<T, A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    nodes: Vec<TreeNode<T>>,
    bound_nodes: Option<Signal<Vec<TreeNode<T>>>>,
    model: Option<Rc<dyn RetainedTreeSource<T>>>,
    selected: Option<Binding<T>>,
    bound_selected: Option<Signal<T>>,
    expanded: Option<Binding<Vec<T>>>,
    bound_expanded: Option<Signal<Vec<T>>>,
    command: Option<Signal<Option<TreeViewCommand<T>>>>,
    default_expanded: Vec<T>,
    disabled: Binding<bool>,
    draggable: Binding<bool>,
    mutation_mode: Binding<TreeMutationMode>,
    editable: Binding<bool>,
    deletable: Binding<bool>,
    creatable: Binding<bool>,
    create_node: Option<TreeCreateFactory<T>>,
    on_select: Option<TreeSelectHandler<T, A>>,
    on_activate: Option<TreeActivateHandler<T, A>>,
    on_toggle: Option<TreeToggleHandler<T, A>>,
    on_trailing_action: Option<TreeTrailingActionHandler<T, A>>,
    on_move: Option<TreeMoveHandler<T, A>>,
    on_rename: Option<TreeRenameHandler<T, A>>,
    on_create: Option<TreeCreateHandler<T, A>>,
    on_create_cancel: Option<TreeCreateCancelHandler<T, A>>,
    on_delete: Option<TreeDeleteHandler<T, A>>,
    on_context_menu: Option<TreeContextMenuHandler<T, A>>,
    on_shortcut: Option<TreeShortcutHandler<T, A>>,
    style: TreeViewStyle,
    virtualized: bool,
    diagnostics: Option<TreeViewDiagnostics>,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Default for TreeView<T, A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> LayoutExt for TreeView<T, A> {
    fn layout_mut(&mut self) -> &mut LayoutStyle {
        &mut self.layout
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> TreeView<T, A> {
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

    pub fn node(mut self, node: TreeNode<T>) -> Self {
        self.nodes.push(node);
        self
    }

    pub fn nodes(mut self, nodes: impl IntoIterator<Item = TreeNode<T>>) -> Self {
        self.nodes.extend(nodes);
        self
    }

    pub fn bind_nodes(mut self, nodes: impl Into<Signal<Vec<TreeNode<T>>>>) -> Self {
        self.bound_nodes = Some(nodes.into());
        self
    }

    /// Uses a retained, revisioned model. This is the recommended path for
    /// large or incrementally updated trees.
    pub fn model(mut self, model: TreeModelHandle<T>) -> Self
    where
        T: Eq + Hash + fmt::Debug,
    {
        self.model = Some(Rc::new(model));
        self
    }

    pub fn selected(mut self, selected: impl Into<Binding<T>>) -> Self {
        self.selected = Some(selected.into());
        self
    }

    pub fn bind_selected(mut self, selected: impl Into<Signal<T>>) -> Self {
        let signal = selected.into();
        self.selected = Some(Binding::Signal(signal.clone()));
        self.bound_selected = Some(signal);
        self
    }

    pub fn expanded(mut self, expanded: impl Into<Binding<Vec<T>>>) -> Self {
        self.expanded = Some(expanded.into());
        self
    }

    pub fn bind_expanded(mut self, expanded: impl Into<Signal<Vec<T>>>) -> Self {
        let signal = expanded.into();
        self.expanded = Some(Binding::Signal(signal.clone()));
        self.bound_expanded = Some(signal);
        self
    }

    pub fn bind_command(mut self, command: impl Into<Signal<Option<TreeViewCommand<T>>>>) -> Self {
        self.command = Some(command.into());
        self
    }

    pub fn default_expanded(mut self, id: T) -> Self {
        if !self.default_expanded.iter().any(|open| open == &id) {
            self.default_expanded.push(id);
        }
        self
    }

    pub fn default_expanded_many(mut self, ids: impl IntoIterator<Item = T>) -> Self {
        self.default_expanded.clear();
        for id in ids {
            if !self.default_expanded.iter().any(|open| open == &id) {
                self.default_expanded.push(id);
            }
        }
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn draggable(mut self, draggable: impl Into<Binding<bool>>) -> Self {
        self.draggable = draggable.into();
        self
    }

    pub fn mutation_mode(mut self, mode: impl Into<Binding<TreeMutationMode>>) -> Self {
        self.mutation_mode = mode.into();
        self
    }

    pub fn editable(mut self, editable: impl Into<Binding<bool>>) -> Self {
        self.editable = editable.into();
        self
    }

    pub fn deletable(mut self, deletable: impl Into<Binding<bool>>) -> Self {
        self.deletable = deletable.into();
        self
    }

    pub fn creatable(mut self, creatable: impl Into<Binding<bool>>) -> Self {
        self.creatable = creatable.into();
        self
    }

    pub fn create_node_with(
        mut self,
        factory: impl Fn(TreeCreateRequest<T>) -> Option<TreeNode<T>> + 'static,
    ) -> Self {
        self.create_node = Some(Rc::new(factory));
        self
    }

    pub fn tree_style(mut self, style: TreeViewStyle) -> Self {
        self.style = style;
        self
    }

    pub fn tree_size(mut self, size: TreeViewSize) -> Self {
        self.style = TreeViewStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn virtualized(mut self, virtualized: bool) -> Self {
        self.virtualized = virtualized;
        self
    }

    pub fn diagnostics(mut self, diagnostics: TreeViewDiagnostics) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    pub fn width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.width = value.into();
        self
    }

    pub fn height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.height = value.into();
        self
    }

    pub fn min_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.min_width = value.into();
        self
    }

    pub fn max_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.max_width = value.into();
        self
    }

    pub fn min_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.min_height = value.into();
        self
    }

    pub fn max_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.max_height = value.into();
        self
    }

    pub fn fill(mut self) -> Self {
        self.layout.width = ailloli_ui_core::style::Length::Fill;
        self.layout.height = ailloli_ui_core::style::Length::Fill;
        self
    }

    pub fn fill_width(mut self) -> Self {
        self.layout.width = ailloli_ui_core::style::Length::Fill;
        self
    }

    pub fn fill_height(mut self) -> Self {
        self.layout.height = ailloli_ui_core::style::Length::Fill;
        self
    }

    pub fn flex_grow(mut self) -> Self {
        self.flex_item = self.flex_item.flex_grow(1.0);
        self
    }

    pub fn on_select(mut self, f: impl Fn(T) -> A + 'static) -> Self {
        self.on_select = Some(Rc::new(move |ctx, id| ctx.dispatch(f(id))));
        self
    }

    pub fn on_select_ctx(mut self, f: impl Fn(&mut EventCtx<A>, T) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    pub fn on_activate(mut self, f: impl Fn(T) -> A + 'static) -> Self {
        self.on_activate = Some(Rc::new(move |ctx, id| ctx.dispatch(f(id))));
        self
    }

    pub fn on_activate_ctx(mut self, f: impl Fn(&mut EventCtx<A>, T) + 'static) -> Self {
        self.on_activate = Some(Rc::new(f));
        self
    }

    pub fn on_toggle(mut self, f: impl Fn(T, bool) -> A + 'static) -> Self {
        self.on_toggle = Some(Rc::new(move |ctx, id, open| ctx.dispatch(f(id, open))));
        self
    }

    pub fn on_toggle_ctx(mut self, f: impl Fn(&mut EventCtx<A>, T, bool) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(f));
        self
    }

    pub fn on_trailing_action(mut self, f: impl Fn(T) -> A + 'static) -> Self {
        self.on_trailing_action = Some(Rc::new(move |ctx, id| ctx.dispatch(f(id))));
        self
    }

    pub fn on_trailing_action_ctx(mut self, f: impl Fn(&mut EventCtx<A>, T) + 'static) -> Self {
        self.on_trailing_action = Some(Rc::new(f));
        self
    }

    pub fn on_move(mut self, f: impl Fn(TreeMove<T>) -> A + 'static) -> Self {
        self.on_move = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    pub fn on_move_ctx(mut self, f: impl Fn(&mut EventCtx<A>, TreeMove<T>) + 'static) -> Self {
        self.on_move = Some(Rc::new(f));
        self
    }

    pub fn on_rename(mut self, f: impl Fn(TreeRename<T>) -> A + 'static) -> Self {
        self.on_rename = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    pub fn on_rename_ctx(mut self, f: impl Fn(&mut EventCtx<A>, TreeRename<T>) + 'static) -> Self {
        self.on_rename = Some(Rc::new(f));
        self
    }

    pub fn on_create(mut self, f: impl Fn(TreeCreate<T>) -> A + 'static) -> Self {
        self.on_create = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    pub fn on_create_ctx(mut self, f: impl Fn(&mut EventCtx<A>, TreeCreate<T>) + 'static) -> Self {
        self.on_create = Some(Rc::new(f));
        self
    }

    pub fn on_create_cancel_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, TreeCreateCancel<T>) + 'static,
    ) -> Self {
        self.on_create_cancel = Some(Rc::new(f));
        self
    }

    pub fn on_delete(mut self, f: impl Fn(TreeDelete<T>) -> A + 'static) -> Self {
        self.on_delete = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    pub fn on_delete_ctx(mut self, f: impl Fn(&mut EventCtx<A>, TreeDelete<T>) + 'static) -> Self {
        self.on_delete = Some(Rc::new(f));
        self
    }

    pub fn on_context_menu(mut self, f: impl Fn(TreeContextMenu<T>) -> A + 'static) -> Self {
        self.on_context_menu = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    pub fn on_context_menu_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, TreeContextMenu<T>) + 'static,
    ) -> Self {
        self.on_context_menu = Some(Rc::new(f));
        self
    }

    pub fn on_shortcut(mut self, f: impl Fn(TreeShortcut<T>) -> A + 'static) -> Self {
        self.on_shortcut = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    pub fn on_shortcut_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, TreeShortcut<T>) + 'static,
    ) -> Self {
        self.on_shortcut = Some(Rc::new(f));
        self
    }
}

struct TreeViewComponent<T, A> {
    layout: LayoutStyle,
    nodes: Vec<TreeNode<T>>,
    bound_nodes: Option<Signal<Vec<TreeNode<T>>>>,
    model: Option<Rc<dyn RetainedTreeSource<T>>>,
    selected: Option<Binding<T>>,
    bound_selected: Option<Signal<T>>,
    expanded: Option<Binding<Vec<T>>>,
    bound_expanded: Option<Signal<Vec<T>>>,
    command: Option<Signal<Option<TreeViewCommand<T>>>>,
    default_expanded: Vec<T>,
    disabled: Binding<bool>,
    draggable: Binding<bool>,
    mutation_mode: Binding<TreeMutationMode>,
    editable: Binding<bool>,
    deletable: Binding<bool>,
    creatable: Binding<bool>,
    create_node: Option<TreeCreateFactory<T>>,
    on_select: Option<TreeSelectHandler<T, A>>,
    on_activate: Option<TreeActivateHandler<T, A>>,
    on_toggle: Option<TreeToggleHandler<T, A>>,
    on_trailing_action: Option<TreeTrailingActionHandler<T, A>>,
    on_move: Option<TreeMoveHandler<T, A>>,
    on_rename: Option<TreeRenameHandler<T, A>>,
    on_create: Option<TreeCreateHandler<T, A>>,
    on_create_cancel: Option<TreeCreateCancelHandler<T, A>>,
    on_delete: Option<TreeDeleteHandler<T, A>>,
    on_context_menu: Option<TreeContextMenuHandler<T, A>>,
    on_shortcut: Option<TreeShortcutHandler<T, A>>,
    style: TreeViewStyle,
    virtualized: bool,
    diagnostics: Option<TreeViewDiagnostics>,
}

impl<T: Clone + PartialEq + 'static, A: 'static> ComponentNode<A> for TreeViewComponent<T, A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let internal_expanded = context.signal(unique_vec(self.default_expanded.clone()));
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
            active_index: context.signal(None),
            drag: context.signal(None),
            editing: context.signal(None),
            draft: context.signal(None),
            edit_value: context.signal(String::new()),
            edit_buffer: context.signal(TextBuffer::from_string(String::new())),
            edit_state: context.signal(TextEditState::new()),
            last_click: context.signal(None),
            observed_max_width: Rc::new(Cell::new(160.0)),
            snapshot_flat_cache: Rc::new(RefCell::new(SnapshotFlatCache::default())),
        })
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> IntoView<A> for TreeView<T, A> {
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
struct FlatNode<T> {
    id: T,
    label: String,
    depth: usize,
    branch: bool,
    disabled: bool,
    leading_icon: Option<IconId>,
    leading_icon_tint: Option<Color>,
    trailing_action: Option<TreeNodeTrailingAction>,
    parent: Option<T>,
}

#[derive(Clone)]
struct TreeDropTarget<T> {
    target: T,
    position: TreeDropPosition,
}

#[derive(Clone)]
struct TreeDragState<T> {
    source: T,
    start: Point,
    active: bool,
    target: Option<TreeDropTarget<T>>,
}

#[derive(Clone)]
struct TreeClickState<T> {
    at: TreeClickTimestamp,
    pos: Point,
    id: T,
    count: u8,
}

#[derive(Clone, Copy)]
enum TreeClickTimestamp {
    Event(Duration),
    Legacy(Instant),
}

impl TreeClickTimestamp {
    fn elapsed_since(self, earlier: Self) -> Option<Duration> {
        match (self, earlier) {
            (Self::Event(now), Self::Event(earlier)) => now.checked_sub(earlier),
            (Self::Legacy(now), Self::Legacy(earlier)) => now.checked_duration_since(earlier),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct TreeEditing<T> {
    id: T,
    original: String,
    create: Option<TreeCreateRequest<T>>,
}

#[derive(Clone)]
struct TreeDraft<T> {
    node: FlatNode<T>,
    insert_index: usize,
}

struct SnapshotFlatCache<T> {
    nodes_revision: u64,
    initialized: bool,
    expanded: Vec<T>,
    rows: Vec<FlatNode<T>>,
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

struct TreeNodePaint<'a, T> {
    row: Rect,
    node: &'a FlatNode<T>,
    selected: bool,
    opacity: f32,
    layout: &'a LayoutResult,
    focused: bool,
    editing: Option<&'a TreeEditing<T>>,
}

struct TreeViewWidget<T, A> {
    layout: LayoutStyle,
    nodes: Vec<TreeNode<T>>,
    bound_nodes: Option<Signal<Vec<TreeNode<T>>>>,
    model: Option<Rc<dyn RetainedTreeSource<T>>>,
    _model_callback: Option<Rc<dyn Fn(u64)>>,
    _model_subscription: Option<TreeModelSubscription>,
    selected: Option<Binding<T>>,
    bound_selected: Option<Signal<T>>,
    expanded: Binding<Vec<T>>,
    mutable_expanded: Option<Signal<Vec<T>>>,
    command: Option<Signal<Option<TreeViewCommand<T>>>>,
    disabled: Binding<bool>,
    draggable: Binding<bool>,
    mutation_mode: Binding<TreeMutationMode>,
    editable: Binding<bool>,
    deletable: Binding<bool>,
    creatable: Binding<bool>,
    create_node: Option<TreeCreateFactory<T>>,
    on_select: Option<TreeSelectHandler<T, A>>,
    on_activate: Option<TreeActivateHandler<T, A>>,
    on_toggle: Option<TreeToggleHandler<T, A>>,
    on_trailing_action: Option<TreeTrailingActionHandler<T, A>>,
    on_move: Option<TreeMoveHandler<T, A>>,
    on_rename: Option<TreeRenameHandler<T, A>>,
    on_create: Option<TreeCreateHandler<T, A>>,
    on_create_cancel: Option<TreeCreateCancelHandler<T, A>>,
    on_delete: Option<TreeDeleteHandler<T, A>>,
    on_context_menu: Option<TreeContextMenuHandler<T, A>>,
    on_shortcut: Option<TreeShortcutHandler<T, A>>,
    style: TreeViewStyle,
    virtualized: bool,
    active_index: Signal<Option<usize>>,
    drag: Signal<Option<TreeDragState<T>>>,
    editing: Signal<Option<TreeEditing<T>>>,
    draft: Signal<Option<TreeDraft<T>>>,
    edit_value: Signal<String>,
    edit_buffer: Signal<TextBuffer>,
    edit_state: Signal<TextEditState>,
    last_click: Signal<Option<TreeClickState<T>>>,
    observed_max_width: Rc<Cell<f32>>,
    snapshot_flat_cache: Rc<RefCell<SnapshotFlatCache<T>>>,
    diagnostics: Option<TreeViewDiagnostics>,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Widget<A> for TreeViewWidget<T, A> {
    fn debug_name(&self) -> &'static str {
        "TreeView"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        self.consume_pending_command();
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

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, layout: &LayoutResult) {
        // A command may come from an externally-owned `State`, which has no
        // runtime invalidator of its own. Incremental layout can therefore
        // legitimately reuse this widget without calling `layout`. Consume
        // the command at the next routed event as well as during layout so
        // the legacy command binding remains usable without defeating the
        // layout cache.
        self.consume_pending_command();
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

    fn input_role(&self) -> InputRole {
        if self.editing.read().is_some() {
            InputRole::TextSingleLine
        } else {
            InputRole::None
        }
    }

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
    fn consume_pending_command(&self) {
        let Some(command_signal) = &self.command else {
            return;
        };
        let Some(command) = command_signal.read() else {
            return;
        };
        command_signal.set(None);
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

    fn current_nodes(&self) -> Vec<TreeNode<T>> {
        self.bound_nodes
            .as_ref()
            .map(Signal::read)
            .unwrap_or_else(|| self.nodes.clone())
    }

    fn source_visible_len(&self) -> usize {
        self.model.as_ref().map_or_else(
            || {
                self.sync_snapshot_flat_cache();
                self.snapshot_flat_cache.borrow().rows.len()
            },
            |model| model.visible_len(),
        )
    }

    fn visible_len(&self) -> usize {
        self.source_visible_len() + usize::from(self.draft.read().is_some())
    }

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

    fn visible_nodes_range(&self, range: std::ops::Range<usize>) -> Vec<FlatNode<T>> {
        range
            .filter_map(|index| self.visible_node_at(index))
            .collect()
    }

    fn set_current_nodes(&self, nodes: Vec<TreeNode<T>>) -> bool {
        let Some(bound) = &self.bound_nodes else {
            return false;
        };
        bound.set(nodes);
        true
    }

    fn can_mutate_nodes(&self) -> bool {
        self.bound_nodes.is_some()
            || (self.model.is_some() && self.mutation_mode.read() == TreeMutationMode::IntentOnly)
    }

    fn source_index_to_visible(&self, source_index: usize) -> usize {
        self.draft.read().map_or(source_index, |draft| {
            source_index + usize::from(source_index >= draft.insert_index)
        })
    }

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

    fn expanded_values(&self) -> Vec<T> {
        unique_vec(self.expanded.read())
    }

    fn selected_value(&self) -> Option<T> {
        self.selected.as_ref().map(Binding::read)
    }

    fn visible_nodes(&self) -> Vec<FlatNode<T>> {
        (0..self.visible_len())
            .filter_map(|index| self.visible_node_at(index))
            .collect()
    }

    fn visible_nodes_snapshot(&self) -> Vec<FlatNode<T>> {
        self.sync_snapshot_flat_cache();
        self.snapshot_flat_cache.borrow().rows.clone()
    }

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

    fn row_rect(&self, bounds: Rect, index: usize) -> Rect {
        Rect::new(
            bounds.x + self.style.padding_x,
            bounds.y + self.style.padding_y + index as f32 * self.style.row_height,
            (bounds.w - self.style.padding_x * 2.0).max(0.0),
            self.style.row_height,
        )
    }

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

    fn chevron_rect(&self, row: Rect, node: &FlatNode<T>) -> Rect {
        let x = row.x + node.depth as f32 * self.style.indent;
        Rect::new(
            x,
            row.y + (row.h - self.style.chevron_size) * 0.5,
            self.style.chevron_size,
            self.style.chevron_size,
        )
    }

    fn label_x(&self, row: Rect, node: &FlatNode<T>) -> f32 {
        let chevron = self.chevron_rect(row, node);
        let mut x = chevron.right() + self.style.gap;
        if node.leading_icon.is_some() {
            x += self.style.icon_size + self.style.gap;
        }
        x
    }

    fn edit_rect(&self, row: Rect, node: &FlatNode<T>) -> Rect {
        let x = self.label_x(row, node);
        Rect::new(
            x - 4.0,
            row.y + 2.0,
            (row.right() - x + 4.0).max(32.0),
            (row.h - 4.0).max(18.0),
        )
    }

    fn trailing_action_size(&self) -> f32 {
        self.style
            .row_height
            .min(28.0)
            .max(self.style.icon_size + 6.0)
    }

    fn trailing_action_rect(&self, row: Rect) -> Rect {
        let size = self.trailing_action_size();
        Rect::new(row.right() - size, row.y + (row.h - size) * 0.5, size, size)
    }

    fn trailing_action_icon_rect(&self, action: Rect) -> Rect {
        Rect::new(
            action.x + (action.w - self.style.icon_size) * 0.5,
            action.y + (action.h - self.style.icon_size) * 0.5,
            self.style.icon_size,
            self.style.icon_size,
        )
    }

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

    fn move_active(&self, ctx: &mut EventCtx<A>, next: Option<usize>) {
        if next != self.active_index.read() {
            self.active_index.set(next);
            ctx.request_repaint();
            ctx.stop_propagation();
        }
    }

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

    fn is_expanded(&self, id: &T) -> bool {
        self.model.as_ref().map_or_else(
            || self.expanded_values().iter().any(|expanded| expanded == id),
            |model| model.is_expanded(id),
        )
    }

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

    fn start_rename(&self, ctx: &mut EventCtx<A>, node: &FlatNode<T>) {
        if self.begin_rename(node) {
            ctx.request_repaint();
            ctx.stop_propagation();
        }
    }

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

    fn remove_expanded_ids(&self, ids: &[T]) {
        let Some(bound) = &self.mutable_expanded else {
            return;
        };
        let mut expanded = self.expanded_values();
        expanded.retain(|id| !ids.iter().any(|deleted| deleted == id));
        bound.set(expanded);
    }
}

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

fn path_is_descendant_or_self(path: &[usize], ancestor: &[usize]) -> bool {
    path.len() >= ancestor.len() && path.starts_with(ancestor)
}

fn node_at_path<'a, T>(nodes: &'a [TreeNode<T>], path: &[usize]) -> Option<&'a TreeNode<T>> {
    let (first, rest) = path.split_first()?;
    let node = nodes.get(*first)?;
    if rest.is_empty() {
        Some(node)
    } else {
        node_at_path(&node.children, rest)
    }
}

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

fn remove_node_at_path<T>(nodes: &mut Vec<TreeNode<T>>, path: &[usize]) -> Option<TreeNode<T>> {
    if path.len() == 1 {
        return (path[0] < nodes.len()).then(|| nodes.remove(path[0]));
    }
    let parent_path = &path[..path.len() - 1];
    let child_idx = *path.last()?;
    let parent = node_mut_at_path(nodes, parent_path)?;
    (child_idx < parent.children.len()).then(|| parent.children.remove(child_idx))
}

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

fn visible_subtree_end<T>(rows: &[FlatNode<T>], row: usize) -> usize {
    let depth = rows[row].depth;
    rows.iter()
        .enumerate()
        .skip(row + 1)
        .find_map(|(index, candidate)| (candidate.depth <= depth).then_some(index))
        .unwrap_or(rows.len())
}

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

fn collect_node_ids<T: Clone>(node: &TreeNode<T>) -> Vec<T> {
    let mut out = vec![node.id.clone()];
    for child in &node.children {
        out.extend(collect_node_ids(child));
    }
    out
}

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

fn unique_vec<T: PartialEq>(values: Vec<T>) -> Vec<T> {
    let mut out = Vec::new();
    for value in values {
        if !out.iter().any(|existing| existing == &value) {
            out.push(value);
        }
    }
    out
}

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

fn first_visible_child_index<T>(nodes: &[FlatNode<T>], index: usize) -> Option<usize> {
    let parent_depth = nodes.get(index)?.depth;
    nodes
        .iter()
        .enumerate()
        .skip(index + 1)
        .find(|(_, node)| node.depth == parent_depth + 1 && !node.disabled)
        .map(|(idx, _)| idx)
}

fn point_distance_sq(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}

fn key_character_upper(key: &KeyEvent) -> Option<String> {
    match &key.key {
        Key::Character(ch) => Some(ch.to_ascii_uppercase()),
        _ => None,
    }
}

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
