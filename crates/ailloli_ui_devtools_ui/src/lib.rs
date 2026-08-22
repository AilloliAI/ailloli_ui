//! Ailloli UI widget frontend for DevTools snapshots.
//!
//! The builder is pure retained-view composition: it does not mutate the
//! snapshot or state. Overlay/right modes use a 560-logical-pixel panel;
//! bottom mode uses a 280-logical-pixel panel. All interactions emit typed
//! [`DevToolsAction`] values for the host to reduce.

use ailloli_ui_core::style::Length;
use ailloli_ui_core::{Color, FontId, TextStyle};
#[cfg(feature = "terminal")]
use ailloli_ui_devtools_core::TerminalDebugSnapshot;
use ailloli_ui_devtools_core::{DebugNode, DebugSnapshot, DebugWarning, DevToolsMode};
use ailloli_ui_runtime::component::{IntoView, IntoViewKeyExt, View};
use ailloli_ui_widgets::controls::Button;
use ailloli_ui_widgets::layout::{Column, Container, FlexItemExt, Row, ScrollView};
use ailloli_ui_widgets::text::Text;

/// Stable retained key for the panel container.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_ui::DEVTOOLS_PANEL_KEY;
/// assert_eq!(DEVTOOLS_PANEL_KEY, "ailloli_ui-devtools-panel");
/// ```
pub const DEVTOOLS_PANEL_KEY: &str = "ailloli_ui-devtools-panel";
/// Stable retained key for the overlay-mode button.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_ui::DEVTOOLS_MODE_OVERLAY_KEY;
/// assert!(DEVTOOLS_MODE_OVERLAY_KEY.ends_with("overlay"));
/// ```
pub const DEVTOOLS_MODE_OVERLAY_KEY: &str = "ailloli_ui-devtools-mode-overlay";
/// Stable retained key for the right-dock button.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_ui::DEVTOOLS_MODE_RIGHT_KEY;
/// assert!(DEVTOOLS_MODE_RIGHT_KEY.ends_with("right"));
/// ```
pub const DEVTOOLS_MODE_RIGHT_KEY: &str = "ailloli_ui-devtools-mode-right";
/// Stable retained key for the bottom-dock button.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_ui::DEVTOOLS_MODE_BOTTOM_KEY;
/// assert!(DEVTOOLS_MODE_BOTTOM_KEY.ends_with("bottom"));
/// ```
pub const DEVTOOLS_MODE_BOTTOM_KEY: &str = "ailloli_ui-devtools-mode-bottom";
/// Stable retained key for the picker toggle button.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_ui::DEVTOOLS_PICK_KEY;
/// assert_eq!(DEVTOOLS_PICK_KEY, "ailloli_ui-devtools-pick");
/// ```
pub const DEVTOOLS_PICK_KEY: &str = "ailloli_ui-devtools-pick";

/// User intent emitted by the retained DevTools view.
///
/// The UI never applies these actions itself; the host owns state reduction and
/// snapshot refresh.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_ui::DevToolsAction;
/// let action = DevToolsAction::Select(Some(42));
/// assert_eq!(action, DevToolsAction::Select(Some(42)));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum DevToolsAction {
    /// Selects an element ID, or clears selection with `None`.
    Select(Option<u64>),
    /// Sets an element hover ID, or clears hover with `None`.
    Hover(Option<u64>),
    /// Requests one core presentation mode.
    SetMode(DevToolsMode),
    /// Toggles host-controlled element-picking mode.
    TogglePicker,
    /// Replaces the case-insensitive tree filter; empty/whitespace shows all nodes.
    SetFilter(String),
}

/// Host-owned presentation and interaction state consumed by the view builder.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::DevToolsMode;
/// use ailloli_ui_devtools_ui::DevToolsState;
/// let state = DevToolsState::default();
/// assert!(!state.enabled);
/// assert_eq!(state.mode, DevToolsMode::Overlay);
/// assert!(state.filter.is_empty());
/// ```
#[derive(Debug, Clone)]
pub struct DevToolsState {
    /// Master visibility switch; `false` produces an empty view.
    pub enabled: bool,
    /// Overlay/docking mode; `Hidden` also produces an empty view.
    pub mode: DevToolsMode,
    /// Whether the header reports active host picking.
    pub picker_active: bool,
    /// Selected element ID used by the tree marker and details panel.
    pub selected: Option<u64>,
    /// Hovered element ID used by the tree marker when not selected.
    pub hovered: Option<u64>,
    /// Case-insensitive widget-name/key/decimal-ID filter.
    pub filter: String,
}

/// Starts disabled in overlay mode with no interaction state or filter.
impl Default for DevToolsState {
    /// Returns the conservative host-controlled defaults.
    fn default() -> Self {
        Self {
            enabled: false,
            mode: DevToolsMode::Overlay,
            picker_active: false,
            selected: None,
            hovered: None,
            filter: String::new(),
        }
    }
}

/// Builds an overlay or docked panel from immutable snapshot and UI state.
///
/// Disabled/hidden state returns [`View::empty`]. Overlay mode right-aligns a
/// 560-pixel panel with 12 pixels of top spacing; right docking reserves 560
/// pixels; bottom docking reserves 280 pixels. The tree filter does not change
/// the details fallback, which uses the selected node or first snapshot node.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_devtools_core::DebugSnapshot;
/// use ailloli_ui_devtools_ui::{build_devtools_overlay, DevToolsAction, DevToolsState};
/// use ailloli_ui_runtime::component::View;
/// fn build(snapshot: &DebugSnapshot) {
///     let view: View<DevToolsAction> = build_devtools_overlay(snapshot, &DevToolsState::default());
///     let _ = view;
/// }
/// ```
pub fn build_devtools_overlay(
    snapshot: &DebugSnapshot,
    state: &DevToolsState,
) -> View<DevToolsAction> {
    if !state.enabled || matches!(state.mode, DevToolsMode::Hidden) {
        return View::empty();
    }

    let panel = Container::new()
        .width(panel_width(state.mode))
        .height(panel_height(state.mode))
        .background(Color::rgba(20, 22, 28, 0.94))
        .radius(8.0)
        .child(
            Column::new().fill().child(header(snapshot, state)).child(
                Row::new()
                    .fill()
                    .child(tree_panel(snapshot, state).flex_grow_by(1.2))
                    .child(details_panel(snapshot, state).flex_grow()),
            ),
        )
        .key(DEVTOOLS_PANEL_KEY)
        .into_view();

    match state.mode {
        DevToolsMode::Overlay => Container::new()
            .fill()
            .child(
                Row::new()
                    .fill()
                    .child(Container::new().flex_grow().into_view())
                    .child(
                        Column::new()
                            .fill_height()
                            .child(Container::new().height(Length::px(12.0)).into_view())
                            .child(panel)
                            .child(Container::new().flex_grow().into_view()),
                    ),
            )
            .into_view(),
        DevToolsMode::DockRight => Row::new()
            .fill()
            .child(Container::new().flex_grow().into_view())
            .child(panel)
            .into_view(),
        DevToolsMode::DockBottom => Column::new()
            .fill()
            .child(Container::new().flex_grow().into_view())
            .child(panel)
            .into_view(),
        DevToolsMode::Hidden => View::empty(),
    }
}

/// Builds counts, picker status, three mode buttons, and picker toggle.
fn header(snapshot: &DebugSnapshot, state: &DevToolsState) -> View<DevToolsAction> {
    Row::new()
        .fill_width()
        .height(Length::px(34.0))
        .child(label(format!(
            "Ailloli UI DevTools - nodes {} - warnings {}{}",
            snapshot.nodes.len(),
            snapshot.warnings.len(),
            if state.picker_active { " - picker" } else { "" }
        )))
        .child(mode_button("Overlay", DevToolsMode::Overlay))
        .child(mode_button("Right", DevToolsMode::DockRight))
        .child(mode_button("Bottom", DevToolsMode::DockBottom))
        .child(
            Button::new()
                .child(label("Pick"))
                .on_click(DevToolsAction::TogglePicker)
                .key(DEVTOOLS_PICK_KEY),
        )
        .into_view()
}

/// Builds filtered pre-order node buttons inside a scroll view.
///
/// Selected nodes use `>`; otherwise hovered nodes use `~`. Indentation repeats
/// two spaces per depth and `!` marks any per-node warning.
fn tree_panel(snapshot: &DebugSnapshot, state: &DevToolsState) -> View<DevToolsAction> {
    let mut list = Column::new().fill_width();
    for node in snapshot
        .nodes
        .iter()
        .filter(|node| node_matches(node, state))
    {
        let marker = if Some(node.id) == state.selected {
            ">"
        } else if Some(node.id) == state.hovered {
            "~"
        } else {
            " "
        };
        let warning = if node.warnings.is_empty() { "" } else { " !" };
        let key = node
            .key
            .as_ref()
            .map(|key| format!(" key={key}"))
            .unwrap_or_default();
        let text = format!(
            "{marker} {}{}#{} {}{}",
            "  ".repeat(node.depth),
            node.widget_name,
            node.id,
            key,
            warning
        );
        list = list.child(
            Button::new()
                .fill_width()
                .child(label(text))
                .on_click(DevToolsAction::Select(Some(node.id))),
        );
    }

    section("Tree", ScrollView::new().child(list).into_view())
}

/// Builds property/layout, optional terminal, and warning sections.
///
/// Details prefer the selected ID when present in the snapshot, otherwise the
/// first node. The warning section is always present.
fn details_panel(snapshot: &DebugSnapshot, state: &DevToolsState) -> View<DevToolsAction> {
    let selected = state
        .selected
        .and_then(|id| snapshot.node(id))
        .or_else(|| snapshot.nodes.first());

    let mut content = Column::new().fill_width();
    if let Some(node) = selected {
        content = content
            .child(section("Properties", node_properties(node)))
            .child(section("Layout / Flex / Paint", node_layout(node)));
    }
    #[cfg(feature = "terminal")]
    {
        if !snapshot.terminal_inspections.is_empty() {
            content = content.child(section("Terminal inspector", terminal_inspector(snapshot)));
        }
    }
    content = content.child(section("Warnings", warnings_view(snapshot)));

    ScrollView::new().child(content).into_view()
}

/// Formats stable identity, key, parent, and child values for one node.
fn node_properties(node: &DebugNode) -> View<DevToolsAction> {
    Column::new()
        .fill_width()
        .child(label(format!("id: {}", node.id)))
        .child(label(format!("widget: {}", node.widget_name)))
        .child(label(format!(
            "key: {}",
            node.key.as_deref().unwrap_or("-")
        )))
        .child(label(format!("parent: {:?}", node.parent)))
        .child(label(format!("children: {:?}", node.children)))
        .into_view()
}

/// Formats node geometry, hints, flex inputs, and optional constraints.
///
/// Floating-point display uses one decimal place; absent values use `-`.
fn node_layout(node: &DebugNode) -> View<DevToolsAction> {
    Column::new()
        .fill_width()
        .child(label(format!(
            "layout size: {:.1} x {:.1}",
            node.layout_size.w, node.layout_size.h
        )))
        .child(label(format_rect("assigned slot", node.assigned_slot)))
        .child(label(format_rect("absolute", Some(node.absolute_bounds))))
        .child(label(format_rect("paint", node.paint_bounds)))
        .child(label(format_rect("clip", node.clip_bounds)))
        .child(label(format!(
            "size hint: width={:?} height={:?}",
            node.size_hint.width, node.size_hint.height
        )))
        .child(label(format!(
            "flex: grow={:.1} shrink={:.1} basis={:?} align={:?}",
            node.flex_item.flex_grow,
            node.flex_item.flex_shrink,
            node.flex_item.flex_basis,
            node.flex_item.align_self
        )))
        .child(label(format!(
            "constraints in: {}",
            node.layout_debug
                .as_ref()
                .and_then(|debug| debug.constraints_in)
                .map(format_constraints)
                .unwrap_or_else(|| "-".to_string())
        )))
        .child(label(format!(
            "constraints final: {}",
            node.layout_debug
                .as_ref()
                .and_then(|debug| debug.constraints_final)
                .map(format_constraints)
                .unwrap_or_else(|| "-".to_string())
        )))
        .into_view()
}

/// Builds warning rows or the explicit `No warnings` empty state.
fn warnings_view(snapshot: &DebugSnapshot) -> View<DevToolsAction> {
    let mut list = Column::new().fill_width();
    if snapshot.warnings.is_empty() {
        list = list.child(label("No warnings"));
    } else {
        for warning in &snapshot.warnings {
            list = list.child(warning_row(warning));
        }
    }
    list.into_view()
}

/// Builds a warning button that selects its optional affected node.
fn warning_row(warning: &DebugWarning) -> View<DevToolsAction> {
    Button::new()
        .fill_width()
        .child(label(format!(
            "{:?} node={:?}: {}",
            warning.kind, warning.node, warning.message
        )))
        .on_click(DevToolsAction::Select(warning.node))
        .into_view()
}

#[cfg(feature = "terminal")]
/// Builds one terminal block per inspection in snapshot order.
fn terminal_inspector(snapshot: &DebugSnapshot) -> View<DevToolsAction> {
    let mut list = Column::new().fill_width();
    for terminal in &snapshot.terminal_inspections {
        list = list.child(terminal_debug_block(terminal));
    }
    list.into_view()
}

#[cfg(feature = "terminal")]
/// Formats bounded terminal state without exposing hyperlink targets.
///
/// It shows the newest three commands in reverse order, the oldest three
/// retained warnings, and the first four captured latest-output lines.
fn terminal_debug_block(terminal: &TerminalDebugSnapshot) -> View<DevToolsAction> {
    let snapshot = &terminal.snapshot;
    let mut list = Column::new()
        .fill_width()
        .child(label(format!(
            "session: {} ({})",
            terminal.title, terminal.id
        )))
        .child(label(format!(
            "screen: {:?} {}x{} scrollback={} pushed={}",
            snapshot.active_screen,
            snapshot.size.cols,
            snapshot.size.rows,
            snapshot.scrollback_len,
            snapshot.scrollback_total_pushed
        )))
        .child(label(format!(
            "cursor: row={} col={} visible={} shape={:?}",
            snapshot.cursor.row,
            snapshot.cursor.col,
            snapshot.cursor.visible,
            snapshot.cursor.shape
        )))
        .child(label(format!(
            "modes: alt={} wrap={} bracketed={} mouse={:?}/sgr={}",
            snapshot.modes.alternate_screen,
            snapshot.modes.wraparound,
            snapshot.modes.bracketed_paste,
            snapshot.modes.mouse_tracking,
            snapshot.modes.sgr_mouse
        )))
        .child(label(format!(
            "damage: full={} dirty={:?}",
            snapshot.damage_full, snapshot.dirty_lines
        )))
        .child(label(format!(
            "diagnostics={} warnings={} commands={} events={} stable_ids={}",
            snapshot.diagnostics.len(),
            snapshot.warnings.len(),
            snapshot.commands.len(),
            snapshot.event_log.len(),
            terminal.stable_ids.len()
        )));
    for command in snapshot.commands.iter().rev().take(3) {
        list = list.child(label(format!(
            "cmd #{} {:?}: {}",
            command.id.0, command.status, command.command_line
        )));
    }
    for warning in snapshot.warnings.iter().take(3) {
        list = list.child(label(format!(
            "warning: {:?} {}",
            warning.kind, warning.reason
        )));
    }
    for line in snapshot.latest_output_lines.iter().take(4) {
        list = list.child(label(format!("out: {}", line.trim_end())));
    }
    list.into_view()
}

/// Wraps titled content with eight logical pixels of padding and panel styling.
fn section(title: impl Into<String>, child: impl IntoView<DevToolsAction>) -> View<DevToolsAction> {
    Container::new()
        .fill_width()
        .padding(8.0)
        .background(Color::rgba(31, 34, 42, 0.96))
        .child(
            Column::new()
                .fill_width()
                .child(label(title.into()))
                .child(child),
        )
        .into_view()
}

/// Builds a keyed mode button; hidden mode uses an internal stable fallback key.
fn mode_button(label_text: &str, mode: DevToolsMode) -> View<DevToolsAction> {
    let key = match mode {
        DevToolsMode::Overlay => DEVTOOLS_MODE_OVERLAY_KEY,
        DevToolsMode::DockRight => DEVTOOLS_MODE_RIGHT_KEY,
        DevToolsMode::DockBottom => DEVTOOLS_MODE_BOTTOM_KEY,
        DevToolsMode::Hidden => "ailloli_ui-devtools-mode-hidden",
    };
    Button::new()
        .child(label(label_text))
        .on_click(DevToolsAction::SetMode(mode))
        .key(key)
        .into_view()
}

/// Builds 12-logical-pixel white UI text.
fn label(text: impl Into<String>) -> Text {
    Text::new(text.into()).style(TextStyle::new(FontId::Ui, 12, Color::WHITE))
}

/// Matches trimmed lowercase filter text against widget name, key, or decimal ID.
///
/// Unicode lowercase conversion allocates for each inspected node when the
/// filter is nonempty. Whitespace-only filters match every node.
fn node_matches(node: &DebugNode, state: &DevToolsState) -> bool {
    if state.filter.trim().is_empty() {
        return true;
    }
    let needle = state.filter.to_lowercase();
    node.widget_name.to_lowercase().contains(&needle)
        || node
            .key
            .as_ref()
            .is_some_and(|key| key.to_lowercase().contains(&needle))
        || node.id.to_string().contains(&needle)
}

/// Returns 560 logical pixels for overlay/right, fill for bottom, and zero when hidden.
fn panel_width(mode: DevToolsMode) -> Length {
    match mode {
        DevToolsMode::Overlay | DevToolsMode::DockRight => Length::px(560.0),
        DevToolsMode::DockBottom => Length::Fill,
        DevToolsMode::Hidden => Length::px(0.0),
    }
}

/// Returns fill for overlay/right, 280 logical pixels for bottom, and zero when hidden.
fn panel_height(mode: DevToolsMode) -> Length {
    match mode {
        DevToolsMode::Overlay | DevToolsMode::DockRight => Length::Fill,
        DevToolsMode::DockBottom => Length::px(280.0),
        DevToolsMode::Hidden => Length::px(0.0),
    }
}

/// Formats optional logical geometry with one decimal and derived bottom edge.
fn format_rect(label: &str, rect: Option<ailloli_ui_devtools_core::DebugRect>) -> String {
    match rect {
        Some(rect) => format!(
            "{label}: x={:.1} y={:.1} w={:.1} h={:.1} bottom={:.1}",
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            rect.bottom()
        ),
        None => format!("{label}: -"),
    }
}

/// Formats `(min_w,min_h)` and `(max_w,max_h)` with one decimal place.
fn format_constraints(c: ailloli_ui_devtools_core::DebugConstraints) -> String {
    format!(
        "min=({:.1},{:.1}) max=({:.1},{:.1})",
        c.min_w, c.min_h, c.max_w, c.max_h
    )
}
