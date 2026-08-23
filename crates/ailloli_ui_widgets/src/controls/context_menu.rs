//! Right-click menus rendered through retained popup portals.

use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{Color, FontId, IconId, Offset, Point, TextStyle, Theme};
use ailloli_ui_runtime::app::{PresentationGeneration, RuntimeHandle};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{
    ActivationPolicy, ClickAction, EventCtx, FocusPolicy, HoverCursorRole, IntoClickAction,
};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::popup::{
    PopupContent, PopupDismissReason, PopupId, PopupMountPolicy, PopupOwner, PopupPlacementSpec,
    PopupRequest, HEADLESS_POPUP_WINDOW_ID,
};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawCmd, DrawImage, DrawRect};
use lucide_icons::Icon;

use super::popup::{
    apply_opacity, measure_text, menu_popup_semantics, paint_overlay_text_in_rect,
    paint_popup_border, paint_popup_shell, PopupAlignment, PopupPlacement, PopupPortalBridge,
};
use super::select::{SelectSize, SelectStyle};

#[derive(Clone, Debug, PartialEq)]
/// Resolved colors, typography, and logical-pixel geometry for context menus.
///
/// Menus are clipped to `popup.popup_max_height`; this control does not add a
/// scrollbar, so callers should keep the complete entry stack within that cap.
/// Custom non-positive or non-finite geometry is not normalized.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::ContextMenuStyle;
/// let style = ContextMenuStyle::from_theme(Theme::dark());
/// assert_eq!((style.width, style.row_height), (252.0, 28.0));
/// assert_eq!(style.popup.popup_max_height, 420.0);
/// ```
pub struct ContextMenuStyle {
    /// Shared popup shell, active-row, item-text, and height-cap style.
    pub popup: SelectStyle,
    /// Right-aligned shortcut hint typography.
    pub shortcut_text: TextStyle,
    /// One-pixel separator-line color.
    pub separator: Color,
    /// Item row height in logical pixels.
    pub row_height: f32,
    /// Vertical space reserved for a separator in logical pixels.
    pub separator_height: f32,
    /// Root menu width in logical pixels.
    pub width: f32,
    /// First-level submenu width in logical pixels.
    pub submenu_width: f32,
    /// Horizontal distance between root menu and submenu in logical pixels.
    pub submenu_gap: f32,
    /// Leading and submenu-chevron icon size in logical pixels.
    pub icon_size: f32,
    /// Gap after icons and around shortcut/chevron regions in logical pixels.
    pub icon_gap: f32,
    /// Horizontal content inset in logical pixels.
    pub padding_x: f32,
    /// Alpha multiplier applied to disabled item visuals.
    pub disabled_opacity: f32,
}

impl Default for ContextMenuStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl ContextMenuStyle {
    /// Resolves compact menu styling from a theme.
    ///
    /// The root defaults to 252 logical pixels wide, submenus to 228, item
    /// rows to 28, and the clipped height cap to 420.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::ContextMenuStyle;
    /// let style = ContextMenuStyle::from_theme(Theme::default());
    /// assert_eq!((style.submenu_width, style.submenu_gap), (228.0, 4.0));
    /// assert_eq!((style.icon_size, style.padding_x), (14.0, 10.0));
    /// ```
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        let mut popup = SelectStyle::from_theme(theme, SelectSize::Compact);
        popup.width = 252.0;
        popup.option_height = 28.0;
        popup.popup_max_height = 420.0;
        popup.radius = Radius::uniform(theme.radius().md);
        popup.popup_border = Border::new(1.0, palette.border);
        Self {
            shortcut_text: TextStyle::new(FontId::Mono, 11, palette.text_muted),
            separator: palette.border.with_alpha(0.86),
            row_height: 28.0,
            separator_height: 9.0,
            width: 252.0,
            submenu_width: 228.0,
            submenu_gap: 4.0,
            icon_size: 14.0,
            icon_gap: 8.0,
            padding_x: 10.0,
            disabled_opacity: 0.45,
            popup,
        }
    }
}

/// One row in a root menu or first-level submenu.
///
/// Separators occupy `separator_height`, are skipped by navigation, and cannot
/// activate. Items occupy `row_height`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{ContextMenuEntry, ContextMenuItem};
/// let entries: Vec<ContextMenuEntry<()>> = vec![
///     ContextMenuEntry::Item(ContextMenuItem::new("Copy")),
///     ContextMenuEntry::Separator,
/// ];
/// assert!(matches!(entries[1], ContextMenuEntry::Separator));
/// ```
pub enum ContextMenuEntry<A = ()> {
    /// Selectable or submenu-bearing row.
    Item(ContextMenuItem<A>),
    /// Non-interactive horizontal divider.
    Separator,
}

impl<A> Clone for ContextMenuEntry<A> {
    fn clone(&self) -> Self {
        match self {
            Self::Item(item) => Self::Item(item.clone()),
            Self::Separator => Self::Separator,
        }
    }
}

/// A context-menu row with optional metadata, action, or submenu.
///
/// A leaf item closes the complete menu after activation even when it has no
/// action. Disabled items remain visible but cannot be highlighted or activated.
/// The current controller opens one submenu tier; submenu entries that themselves
/// contain a submenu are displayed with a chevron but are not opened.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{ContextMenuEntry, ContextMenuItem};
/// let item: ContextMenuItem<()> = ContextMenuItem::new("More")
///     .submenu([ContextMenuEntry::Item(ContextMenuItem::new("Details"))]);
/// assert_eq!(item.label(), "More");
/// assert_eq!(item.submenu_entries().len(), 1);
/// ```
pub struct ContextMenuItem<A = ()> {
    /// Primary row label.
    label: String,
    /// Optional non-semantic shortcut hint.
    shortcut: Option<String>,
    /// Optional leading icon.
    icon: Option<IconId>,
    /// Live disabled state.
    disabled: Binding<bool>,
    /// Optional leaf action.
    action: Option<Rc<ClickAction<A>>>,
    /// Owned first-level submenu entries.
    submenu: Vec<ContextMenuEntry<A>>,
}

impl<A> Clone for ContextMenuItem<A> {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            shortcut: self.shortcut.clone(),
            icon: self.icon.clone(),
            disabled: self.disabled.clone(),
            action: self.action.clone(),
            submenu: self.submenu.clone(),
        }
    }
}

impl<A: 'static> ContextMenuItem<A> {
    /// Creates an enabled leaf item without icon, shortcut, or action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ContextMenuItem;
    /// let item: ContextMenuItem<()> = ContextMenuItem::new("Copy");
    /// assert_eq!(item.label(), "Copy");
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            shortcut: None,
            icon: None,
            disabled: Binding::Static(false),
            action: None,
            submenu: Vec::new(),
        }
    }

    /// Sets the right-aligned display-only shortcut hint.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ContextMenuItem;
    /// let item: ContextMenuItem<()> = ContextMenuItem::new("Copy").shortcut("Ctrl+C");
    /// assert_eq!(item.shortcut_label(), Some("Ctrl+C"));
    /// ```
    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// Sets the leading icon, replacing any previous icon.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::ContextMenuItem;
    /// let item: ContextMenuItem<()> = ContextMenuItem::new("History").leading_icon(IconId::History);
    /// let _ = item;
    /// ```
    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets static or reactive disabled state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ContextMenuItem;
    /// let item: ContextMenuItem<()> = ContextMenuItem::new("Unavailable").disabled(true);
    /// assert!(item.is_disabled());
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Sets the leaf action, replacing any previous action.
    ///
    /// Items with non-empty submenus open the submenu instead of running this
    /// action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ContextMenuItem;
    /// #[derive(Clone)]
    /// enum Action { Copy }
    /// let item = ContextMenuItem::new("Copy").on_select(Action::Copy);
    /// let _ = item;
    /// ```
    pub fn on_select(mut self, action: impl IntoClickAction<A>) -> Self {
        self.action = Some(Rc::new(action.into_click_action()));
        self
    }

    /// Replaces the submenu entries with values collected in iterator order.
    ///
    /// An empty iterator restores leaf behavior. Only one interactive submenu
    /// tier is currently supported.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ContextMenuEntry, ContextMenuItem};
    /// let item: ContextMenuItem<()> = ContextMenuItem::new("More").submenu([
    ///     ContextMenuEntry::Item(ContextMenuItem::new("Details")),
    ///     ContextMenuEntry::Separator,
    /// ]);
    /// assert_eq!(item.submenu_entries().len(), 2);
    /// ```
    pub fn submenu(mut self, entries: impl IntoIterator<Item = ContextMenuEntry<A>>) -> Self {
        self.submenu = entries.into_iter().collect();
        self
    }

    /// Reads the current static or reactive disabled value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ContextMenuItem;
    /// let item: ContextMenuItem<()> = ContextMenuItem::new("Delete").disabled(true);
    /// assert!(item.is_disabled());
    /// ```
    pub fn is_disabled(&self) -> bool {
        self.disabled.read()
    }

    /// Returns the label exactly as supplied.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ContextMenuItem;
    /// let item: ContextMenuItem<()> = ContextMenuItem::new("");
    /// assert_eq!(item.label(), "");
    /// ```
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the optional shortcut hint; empty text remains `Some("")`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ContextMenuItem;
    /// let item: ContextMenuItem<()> = ContextMenuItem::new("Copy").shortcut("");
    /// assert_eq!(item.shortcut_label(), Some(""));
    /// ```
    pub fn shortcut_label(&self) -> Option<&str> {
        self.shortcut.as_deref()
    }

    /// Borrows submenu entries in their display order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ContextMenuItem;
    /// let item: ContextMenuItem<()> = ContextMenuItem::new("Leaf");
    /// assert!(item.submenu_entries().is_empty());
    /// ```
    pub fn submenu_entries(&self) -> &[ContextMenuEntry<A>] {
        &self.submenu
    }
}

/// A retained right-click menu attached to an optional host child.
///
/// Visibility can be controlled, two-way bound, or internal. A right click can
/// open/re-anchor bound and internal menus; a controlled `false` value prevents
/// opening, while controlled `true` permits re-anchoring. When no pointer or
/// explicit anchor exists, the menu starts at the host's bottom-left corner.
/// Escape/outside dismissal can write only bound or internal state.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{ContextMenu, ContextMenuEntry, ContextMenuItem};
/// use ailloli_ui_widgets::text::Text;
/// let menu = ContextMenu::<()>::new(Text::new("File"))
///     .entries(vec![ContextMenuEntry::Item(ContextMenuItem::new("Open"))]);
/// let _ = menu;
/// ```
pub struct ContextMenu<A = ()> {
    /// Layout configuration for the host slot.
    pub(crate) layout: LayoutStyle,
    /// Flex behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Optional controlled or bound visibility source.
    open: Option<Binding<bool>>,
    /// Writable visibility signal in bound mode.
    bound_open: Option<Signal<bool>>,
    /// Initial internal visibility without an external source.
    default_open: bool,
    /// Static or reactive programmatic anchor in host coordinates.
    anchor: Binding<Point>,
    /// Whether the programmatic anchor was explicitly configured.
    anchor_explicit: bool,
    /// Static or reactive complete entry collection.
    entries: Binding<Vec<ContextMenuEntry<A>>>,
    /// Live disabled state.
    disabled: Binding<bool>,
    /// Resolved popup and row style.
    style: ContextMenuStyle,
    /// Optional sole host child painted below the portal.
    child: Option<View<A>>,
}

crate::impl_layout_builders!(ContextMenu);

impl<A: 'static> Default for ContextMenu<A> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<A: 'static> ContextMenu<A> {
    /// Creates a closed menu around one host child.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ContextMenu;
    /// use ailloli_ui_widgets::text::Text;
    /// let menu: ContextMenu<()> = ContextMenu::new(Text::new("Right-click"));
    /// let _ = menu;
    /// ```
    pub fn new(child: impl IntoView<A>) -> Self {
        Self::empty().child(child)
    }

    /// Creates a closed menu without a child or entries.
    ///
    /// In an unbounded host it intrinsically measures as zero by zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ContextMenu;
    /// let menu: ContextMenu<()> = ContextMenu::empty();
    /// let _ = menu;
    /// ```
    pub fn empty() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            open: None,
            bound_open: None,
            default_open: false,
            anchor: Binding::Static(Point::default()),
            anchor_explicit: false,
            entries: Binding::Static(Vec::new()),
            disabled: Binding::Static(false),
            style: ContextMenuStyle::default(),
            child: None,
        }
    }

    /// Sets controlled visibility and clears two-way bound mode.
    ///
    /// Portal dismissal cannot mutate this binding. A controlled `false` value
    /// also prevents right-click opening.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ContextMenu;
    /// let menu: ContextMenu<()> = ContextMenu::empty().open(true);
    /// let _ = menu;
    /// ```
    pub fn open(mut self, open: impl Into<Binding<bool>>) -> Self {
        self.open = Some(open.into());
        self.bound_open = None;
        self
    }

    /// Installs a writable visibility signal for opening and dismissal.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// use ailloli_ui_widgets::controls::ContextMenu;
    /// let open = Signal::new(Rc::new(RefCell::new(false)), Rc::new(|| {}));
    /// let menu: ContextMenu<()> = ContextMenu::empty().bind_open(open);
    /// let _ = menu;
    /// ```
    pub fn bind_open(mut self, open: impl Into<Signal<bool>>) -> Self {
        let signal = open.into();
        self.open = Some(Binding::Signal(signal.clone()));
        self.bound_open = Some(signal);
        self
    }

    /// Sets initial internal visibility when no external source exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ContextMenu;
    /// let menu: ContextMenu<()> = ContextMenu::empty().default_open(true);
    /// let _ = menu;
    /// ```
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Sets a static or reactive programmatic anchor in host coordinates.
    ///
    /// A later right click temporarily takes precedence until the menu closes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Point;
    /// use ailloli_ui_widgets::controls::ContextMenu;
    /// let menu: ContextMenu<()> = ContextMenu::empty().anchor(Point::new(12.0, 24.0));
    /// let _ = menu;
    /// ```
    pub fn anchor(mut self, anchor: impl Into<Binding<Point>>) -> Self {
        self.anchor = anchor.into();
        self.anchor_explicit = true;
        self
    }

    /// Installs a writable anchor signal and marks the anchor explicit.
    ///
    /// The menu reads it reactively but does not write it during pointer opens.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_core::Point;
    /// use ailloli_ui_runtime::component::Signal;
    /// use ailloli_ui_widgets::controls::ContextMenu;
    /// let anchor = Signal::new(Rc::new(RefCell::new(Point::new(1.0, 2.0))), Rc::new(|| {}));
    /// let menu: ContextMenu<()> = ContextMenu::empty().bind_anchor(anchor);
    /// let _ = menu;
    /// ```
    pub fn bind_anchor(mut self, anchor: impl Into<Signal<Point>>) -> Self {
        self.anchor = Binding::Signal(anchor.into());
        self.anchor_explicit = true;
        self
    }

    /// Sets the static or reactive complete entry collection.
    ///
    /// Empty collections keep open state but publish no visible menu content.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ContextMenu, ContextMenuEntry, ContextMenuItem};
    /// let entries = vec![ContextMenuEntry::Item(ContextMenuItem::new("Open"))];
    /// let menu: ContextMenu<()> = ContextMenu::empty().entries(entries);
    /// let _ = menu;
    /// ```
    pub fn entries(mut self, entries: impl Into<Binding<Vec<ContextMenuEntry<A>>>>) -> Self {
        self.entries = entries.into();
        self
    }

    /// Installs a writable signal as the live complete entry collection.
    ///
    /// The menu only reads the signal; it never mutates its entries.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// use ailloli_ui_widgets::controls::{ContextMenu, ContextMenuEntry};
    /// let entries = Signal::new(
    ///     Rc::new(RefCell::new(Vec::<ContextMenuEntry<()>>::new())),
    ///     Rc::new(|| {}),
    /// );
    /// let menu: ContextMenu<()> = ContextMenu::empty().bind_entries(entries);
    /// let _ = menu;
    /// ```
    pub fn bind_entries(mut self, entries: impl Into<Signal<Vec<ContextMenuEntry<A>>>>) -> Self {
        self.entries = Binding::Signal(entries.into());
        self
    }

    /// Sets static or reactive disabled state.
    ///
    /// Disabled state prevents right-click opening/focus and closes an existing
    /// portal programmatically without changing controlled visibility.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ContextMenu;
    /// let menu: ContextMenu<()> = ContextMenu::empty().disabled(true);
    /// let _ = menu;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Replaces the complete popup and row style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ContextMenu, ContextMenuStyle};
    /// let menu: ContextMenu<()> = ContextMenu::empty().context_menu_style(ContextMenuStyle::default());
    /// let _ = menu;
    /// ```
    pub fn context_menu_style(mut self, style: ContextMenuStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets root-menu width in both the menu and nested popup styles.
    ///
    /// This does not change `submenu_width`. The value is in logical pixels and
    /// is not clamped.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ContextMenu;
    /// let menu: ContextMenu<()> = ContextMenu::empty().menu_width(300.0);
    /// let _ = menu;
    /// ```
    pub fn menu_width(mut self, width: f32) -> Self {
        self.style.width = width;
        self.style.popup.width = width;
        self
    }

    /// Sets the sole host child, replacing any previous child.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ContextMenu;
    /// use ailloli_ui_widgets::text::Text;
    /// let menu: ContextMenu<()> = ContextMenu::empty().child(Text::new("Target"));
    /// let _ = menu;
    /// ```
    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.child = Some(child.into_view());
        self
    }
}

/// Component properties used to allocate retained menu and submenu state.
struct ContextMenuComponent<A> {
    /// Outer logical sizing policy for the underlying host child.
    layout: LayoutStyle,
    /// Optional readable external open state.
    open: Option<Binding<bool>>,
    /// Optional writable external open state.
    bound_open: Option<Signal<bool>>,
    /// Initial open state used by an uncontrolled menu.
    default_open: bool,
    /// Static or reactive preferred anchor in logical window coordinates.
    anchor: Binding<Point>,
    /// Whether the caller explicitly supplied the anchor.
    anchor_explicit: bool,
    /// Reactive root entry collection.
    entries: Binding<Vec<ContextMenuEntry<A>>>,
    /// Reactive interaction-disable flag.
    disabled: Binding<bool>,
    /// Popup, row, and submenu styling.
    style: ContextMenuStyle,
    /// Optional underlying host content painted below the menu.
    child: Option<View<A>>,
}

impl<A: 'static> ComponentNode<A> for ContextMenuComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let mut children = Vec::new();
        if let Some(child) = self.child.clone() {
            children.push(child);
        }
        let initially_open = self
            .open
            .as_ref()
            .map(Binding::read)
            .unwrap_or(self.default_open);
        let runtime = context.runtime();
        let root_popup_id = runtime.popup_id_for_element(context.element_id()).ok();
        let internal_open = context.signal(self.default_open);
        let last_requested_open =
            context.signal(root_popup_id.is_some_and(|popup_id| runtime.popup_is_open(popup_id)));
        let controller = ContextMenuController {
            runtime,
            open: self.open.clone(),
            bound_open: self.bound_open.clone(),
            internal_open,
            last_requested_open,
            anchor: self.anchor.clone(),
            anchor_explicit: self.anchor_explicit,
            pointer_anchor: context.signal(None),
            entries: self.entries.clone(),
            disabled: self.disabled.clone(),
            style: self.style.clone(),
            active_index: context.signal(None),
            submenu_parent_index: context.signal(None),
            submenu_active_index: context.signal(None),
            pressed_entry: context.signal(None),
            geometry: context.signal(None),
            root_popup_id,
            submenu_popup_id: context.signal(None),
        };
        let popup_content = context_menu_root_content(controller.clone());
        View::node(
            ContextMenuWidget {
                layout: self.layout,
                controller,
                popup: PopupPortalBridge::new_retained_with_content(
                    context,
                    menu_popup_semantics(true),
                    initially_open,
                    popup_content,
                ),
            },
            children,
        )
    }
}

impl<A: 'static> IntoView<A> for ContextMenu<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(ContextMenuComponent {
                layout: self.layout,
                open: self.open,
                bound_open: self.bound_open,
                default_open: self.default_open,
                anchor: self.anchor,
                anchor_explicit: self.anchor_explicit,
                entries: self.entries,
                disabled: self.disabled,
                style: self.style,
                child: self.child,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Host widget that opens and synchronizes the root popup portal.
struct ContextMenuWidget<A> {
    /// Outer logical sizing policy for the underlying host child.
    layout: LayoutStyle,
    /// Shared retained controller for root and submenu content.
    controller: ContextMenuController<A>,
    /// Bridge synchronizing popup registry content and lifecycle.
    popup: PopupPortalBridge<A>,
}

/// Shared state machine used by the host and both retained popup widgets.
struct ContextMenuController<A> {
    /// UI-local runtime used to allocate and close popup registrations.
    runtime: RuntimeHandle<A>,
    /// Optional readable external open state.
    open: Option<Binding<bool>>,
    /// Optional writable external open state.
    bound_open: Option<Signal<bool>>,
    /// Retained open state used by an uncontrolled menu.
    internal_open: Signal<bool>,
    /// Last open state reconciled into the popup registry.
    last_requested_open: Signal<bool>,
    /// Static or reactive preferred anchor in logical window coordinates.
    anchor: Binding<Point>,
    /// Whether the caller explicitly supplied the anchor.
    anchor_explicit: bool,
    /// Pointer-derived anchor captured for implicit context-menu opening.
    pointer_anchor: Signal<Option<Point>>,
    /// Reactive root entry collection.
    entries: Binding<Vec<ContextMenuEntry<A>>>,
    /// Reactive interaction-disable flag.
    disabled: Binding<bool>,
    /// Popup, row, and submenu styling.
    style: ContextMenuStyle,
    /// Keyboard-active root entry index.
    active_index: Signal<Option<usize>>,
    /// Root entry whose submenu is currently open.
    submenu_parent_index: Signal<Option<usize>>,
    /// Keyboard-active entry within the open submenu.
    submenu_active_index: Signal<Option<usize>>,
    /// Entry and depth captured by the active pointer press.
    pressed_entry: Signal<Option<ContextMenuPressedEntry>>,
    /// Last resolved root/submenu rectangles and entry rows.
    geometry: Signal<Option<ContextMenuGeometry>>,
    /// Popup identity reserved for root menu content.
    root_popup_id: Option<PopupId>,
    /// Popup identity reserved for the current submenu, if any.
    submenu_popup_id: Signal<Option<PopupId>>,
}

impl<A> Clone for ContextMenuController<A> {
    fn clone(&self) -> Self {
        Self {
            runtime: self.runtime.clone(),
            open: self.open.clone(),
            bound_open: self.bound_open.clone(),
            internal_open: self.internal_open.clone(),
            last_requested_open: self.last_requested_open.clone(),
            anchor: self.anchor.clone(),
            anchor_explicit: self.anchor_explicit,
            pointer_anchor: self.pointer_anchor.clone(),
            entries: self.entries.clone(),
            disabled: self.disabled.clone(),
            style: self.style.clone(),
            active_index: self.active_index.clone(),
            submenu_parent_index: self.submenu_parent_index.clone(),
            submenu_active_index: self.submenu_active_index.clone(),
            pressed_entry: self.pressed_entry.clone(),
            geometry: self.geometry.clone(),
            root_popup_id: self.root_popup_id,
            submenu_popup_id: self.submenu_popup_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Pointer-pressed row identity used to require same-row release.
struct ContextMenuPressedEntry {
    /// `None` for root rows or the root parent index for submenu rows.
    parent: Option<usize>,
    /// Absolute index within the root or submenu entry slice.
    index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// Resolved viewport and root menu rectangles used to place a submenu.
struct ContextMenuGeometry {
    /// Complete presentation viewport.
    viewport: Rect,
    /// Root popup rectangle after host placement resolution.
    menu: Rect,
}

impl<A: 'static> Widget<A> for ContextMenuWidget<A> {
    fn debug_name(&self) -> &'static str {
        "ContextMenu"
    }

    fn layout(
        &self,
        engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = host_slot_size(engine, ctx, children, constraints, self.layout);
        let mut child_layouts = Vec::new();
        if let Some(child) = children.first_mut() {
            let r = child.layout(engine, ctx, Constraints::tight(size.w, size.h));
            child_layouts.push(ChildLayout {
                offset: Offset::default(),
                size: r.size,
                paint_bounds: Rect::new(0.0, 0.0, r.size.w, r.size.h),
                visual_bounds: r.visual_bounds,
            });
        }

        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        if self.controller.sync_portal_visibility(&self.popup) && !self.controller.disabled.read() {
            // Semantic placement is published during paint; the host then
            // resolves it against the complete presentation viewport.
        } else {
            self.popup.close(PopupDismissReason::Programmatic);
        }

        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds,
            visual_bounds: paint_bounds,
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        if !self.controller.is_open() || self.controller.disabled.read() {
            return;
        }
        let entries = self.controller.entries.read();
        if entries.is_empty() {
            return;
        }
        self.controller
            .publish_root_popup_without_event(&self.popup, bounds, &entries);
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        self.popup.refresh_owner(ctx);
        if self.controller.disabled.read() {
            return;
        }

        if let Event::Pointer(PointerEvent::Button {
            pos,
            button: MouseButton::Right,
            pressed: true,
            ..
        }) = event
        {
            if bounds.contains(pos.x, pos.y) && self.controller.open_at(*pos) {
                self.controller.sync_portal(&self.popup, ctx, bounds);
                ctx.request_repaint();
                ctx.stop_propagation();
                return;
            }
        }

        if !self.controller.is_open() {
            return;
        }
        self.controller.sync_portal(&self.popup, ctx, bounds);
    }

    fn focus_policy(&self) -> FocusPolicy {
        if self.controller.is_open() && !self.controller.disabled.read() {
            FocusPolicy::Focusable
        } else {
            FocusPolicy::NotFocusable
        }
    }

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }
}

impl<A: 'static> ContextMenuController<A> {
    /// Reconciles requested state with host-owned portal state and reports visibility.
    fn is_open(&self) -> bool {
        let requested = self.requested_open();
        let portal_open = self
            .root_popup_id
            .is_some_and(|popup_id| self.runtime.popup_is_open(popup_id));
        if requested && !portal_open {
            if !self.last_requested_open.read() {
                // A bound signal may request focus before the next layout has
                // synchronized its rising edge into the popup portal. Keep
                // that pending request intact; only a request already known
                // to the portal can have been dismissed by the host.
                return false;
            }
            // Host-level outside/Escape dismissal is consumed before the
            // owner widget sees the event. Synchronize mutable state during
            // the next paint/event query as well as layout, otherwise a
            // focus-restoration repaint can rebuild and reopen the menu.
            if let Some(bound) = &self.bound_open {
                bound.set(false);
                self.last_requested_open.set(false);
            } else if self.open.is_none() {
                self.internal_open.set(false);
                self.last_requested_open.set(false);
            }
            return false;
        }
        requested && portal_open
    }

    /// Reads controlled/bound visibility or retained internal visibility.
    fn requested_open(&self) -> bool {
        self.open
            .as_ref()
            .map(Binding::read)
            .unwrap_or_else(|| self.internal_open.read())
    }

    /// Applies visibility edges to the root portal and mirrors host dismissals.
    fn sync_portal_visibility(&self, popup: &PopupPortalBridge<A>) -> bool {
        let requested = self.requested_open();
        let previously_requested = self.last_requested_open.read();
        if previously_requested != requested {
            self.last_requested_open.set(requested);
        }
        if !requested {
            if let Some(popup_id) = self.root_popup_id {
                if self.runtime.popup_is_open(popup_id) {
                    self.runtime
                        .close_popup(popup_id, PopupDismissReason::Programmatic);
                }
            }
            return false;
        }

        if !previously_requested
            && self
                .root_popup_id
                .is_some_and(|popup_id| !self.runtime.popup_is_open(popup_id))
        {
            popup.open_unpositioned(None);
        }
        if self
            .root_popup_id
            .is_some_and(|popup_id| self.runtime.popup_is_open(popup_id))
        {
            return true;
        }

        // A portal-level Escape/outside/stale dismissal must also update the
        // mutable public/internal state, otherwise layout would reopen it.
        if let Some(bound) = &self.bound_open {
            bound.set(false);
            self.last_requested_open.set(false);
        } else if self.open.is_none() {
            self.internal_open.set(false);
            self.last_requested_open.set(false);
        }
        false
    }

    /// Re-anchors and opens a bound/internal menu after a right-button press.
    fn open_at(&self, anchor: Point) -> bool {
        let can_open = self.bound_open.is_some() || self.open.is_none() || self.is_open();
        if !can_open {
            return false;
        }
        self.pointer_anchor.set(Some(anchor));
        self.geometry.set(None);
        if let Some(bound) = &self.bound_open {
            bound.set(true);
        } else if self.open.is_none() {
            self.internal_open.set(true);
        }
        self.active_index.set(None);
        self.submenu_parent_index.set(None);
        self.submenu_active_index.set(None);
        self.pressed_entry.set(None);
        self.last_requested_open.set(true);
        true
    }

    /// Publishes the root popup placement using event-scoped owner metadata.
    fn sync_portal(&self, popup: &PopupPortalBridge<A>, ctx: &EventCtx<A>, bounds: Rect) {
        let entries = self.entries.read();
        if entries.is_empty() {
            popup.open_unpositioned(Some(ctx));
            return;
        }
        popup.open_placed(ctx, self.root_placement(bounds, &entries));
    }

    /// Publishes placement during paint when no event context is available.
    fn publish_root_popup_without_event(
        &self,
        popup: &PopupPortalBridge<A>,
        bounds: Rect,
        entries: &[ContextMenuEntry<A>],
    ) {
        popup.open_placed_without_event(self.root_placement(bounds, entries));
    }

    /// Builds a flip-enabled bottom/start placement request for the root menu.
    fn root_placement(&self, owner: Rect, entries: &[ContextMenuEntry<A>]) -> PopupPlacementSpec {
        let anchor = self.effective_anchor(owner);
        PopupPlacementSpec::new(
            Rect::new(anchor.x, anchor.y, 0.0, 0.0),
            Size::new(
                self.style.width,
                self.menu_height(entries)
                    .min(self.style.popup.popup_max_height),
            ),
        )
        .with_placement(PopupPlacement::Bottom)
        .with_alignment(PopupAlignment::Start)
        .with_gap(0.0)
        .with_flip(true)
    }

    /// Resolves pointer, explicit, or host-bottom-left anchor precedence.
    fn effective_anchor(&self, owner: Rect) -> Point {
        self.pointer_anchor.read().unwrap_or_else(|| {
            if self.anchor_explicit {
                self.anchor.read()
            } else {
                Point::new(owner.x, owner.bottom())
            }
        })
    }

    /// Stores changed resolved geometry without redundant signal writes.
    fn update_geometry(&self, geometry: ContextMenuGeometry) {
        if self.geometry.read() != Some(geometry) {
            self.geometry.set(Some(geometry));
        }
    }

    /// Refreshes root menu and viewport rectangles from the popup portal.
    fn refresh_resolved_geometry(&self) -> Option<ContextMenuGeometry> {
        let popup_id = self.root_popup_id?;
        let portal = self.runtime.popup_portal();
        let portal = portal.borrow();
        let geometry = ContextMenuGeometry {
            viewport: portal.resolved_viewport(popup_id)?,
            menu: portal.bounds(popup_id)?,
        };
        drop(portal);
        self.update_geometry(geometry);
        Some(geometry)
    }

    /// Places a first-level submenu to the right and clamps it to the viewport.
    fn submenu_rect(
        &self,
        bounds: Rect,
        menu: Rect,
        parent_row: Rect,
        entries: &[ContextMenuEntry<A>],
    ) -> Rect {
        let desired = Rect::new(
            menu.right() + self.style.submenu_gap,
            parent_row.y,
            self.style.submenu_width,
            self.menu_height(entries)
                .min(self.style.popup.popup_max_height),
        );
        super::popup::clamp_rect_to_bounds(desired, bounds)
    }

    /// Sums item and separator heights without applying the popup height cap.
    fn menu_height(&self, entries: &[ContextMenuEntry<A>]) -> f32 {
        entries.iter().map(|entry| self.entry_height(entry)).sum()
    }

    /// Resolves an entry's configured item-row or separator height.
    fn entry_height(&self, entry: &ContextMenuEntry<A>) -> f32 {
        match entry {
            ContextMenuEntry::Item(_) => self.style.row_height,
            ContextMenuEntry::Separator => self.style.separator_height,
        }
    }

    /// Returns an entry rectangle by accumulating all preceding row heights.
    fn entry_row(&self, menu: Rect, entries: &[ContextMenuEntry<A>], index: usize) -> Option<Rect> {
        if index >= entries.len() {
            return None;
        }
        let mut y = menu.y;
        for entry in &entries[..index] {
            y += self.entry_height(entry);
        }
        Some(Rect::new(
            menu.x,
            y,
            menu.w,
            self.entry_height(&entries[index]),
        ))
    }

    /// Hit-tests a position and returns item indices while excluding separators.
    fn item_index_at(
        &self,
        menu: Rect,
        entries: &[ContextMenuEntry<A>],
        pos: Point,
    ) -> Option<usize> {
        if !menu.contains(pos.x, pos.y) {
            return None;
        }
        let mut y = menu.y;
        for (idx, entry) in entries.iter().enumerate() {
            let height = self.entry_height(entry);
            let row = Rect::new(menu.x, y, menu.w, height);
            if row.contains(pos.x, pos.y) {
                return matches!(entry, ContextMenuEntry::Item(_)).then_some(idx);
            }
            y += height;
        }
        None
    }

    /// Returns the first enabled item index, skipping separators.
    fn first_enabled_index(&self, entries: &[ContextMenuEntry<A>]) -> Option<usize> {
        entries.iter().enumerate().find_map(|(idx, entry)| {
            let ContextMenuEntry::Item(item) = entry else {
                return None;
            };
            (!item.is_disabled()).then_some(idx)
        })
    }

    /// Returns the last enabled item index, skipping separators.
    fn last_enabled_index(&self, entries: &[ContextMenuEntry<A>]) -> Option<usize> {
        entries.iter().enumerate().rev().find_map(|(idx, entry)| {
            let ContextMenuEntry::Item(item) = entry else {
                return None;
            };
            (!item.is_disabled()).then_some(idx)
        })
    }

    /// Advances with wraparound through enabled item indices.
    fn next_enabled_index(
        &self,
        entries: &[ContextMenuEntry<A>],
        current: Option<usize>,
    ) -> Option<usize> {
        let enabled = entries
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| {
                let ContextMenuEntry::Item(item) = entry else {
                    return None;
                };
                (!item.is_disabled()).then_some(idx)
            })
            .collect::<Vec<_>>();
        if enabled.is_empty() {
            return None;
        }
        let next = current
            .and_then(|current| enabled.iter().position(|idx| *idx == current))
            .map_or(0, |position| (position + 1) % enabled.len());
        Some(enabled[next])
    }

    /// Moves backward with wraparound through enabled item indices.
    fn previous_enabled_index(
        &self,
        entries: &[ContextMenuEntry<A>],
        current: Option<usize>,
    ) -> Option<usize> {
        let enabled: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| {
                let ContextMenuEntry::Item(item) = entry else {
                    return None;
                };
                (!item.is_disabled()).then_some(idx)
            })
            .collect();
        if enabled.is_empty() {
            return None;
        }
        let pos = current
            .and_then(|current| enabled.iter().position(|idx| *idx == current))
            .unwrap_or(0);
        Some(enabled[(pos + enabled.len() - 1) % enabled.len()])
    }

    /// Handles root Escape, navigation, submenu, and activation keys.
    fn handle_root_keyboard(&self, ctx: &mut EventCtx<A>, key: &Key) {
        let entries = self.entries.read();
        match key {
            Key::Named(NamedKey::Escape) => {
                self.close_with_runtime(ctx, PopupDismissReason::Escape);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.set_active(
                    ctx,
                    self.next_enabled_index(&entries, self.active_index.read()),
                );
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.set_active(
                    ctx,
                    self.previous_enabled_index(&entries, self.active_index.read()),
                );
            }
            Key::Named(NamedKey::Home) => {
                self.set_active(ctx, self.first_enabled_index(&entries));
            }
            Key::Named(NamedKey::End) => {
                self.set_active(ctx, self.last_enabled_index(&entries));
            }
            Key::Named(NamedKey::ArrowRight) => {
                if let Some(index) = self.active_index.read() {
                    self.open_submenu(ctx, &entries, index);
                }
            }
            Key::Named(NamedKey::ArrowLeft) => {
                self.close_submenu(ctx, PopupDismissReason::Escape);
            }
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                if let Some(index) = self
                    .active_index
                    .read()
                    .or_else(|| self.first_enabled_index(&entries))
                {
                    self.activate_entry(ctx, &entries, index);
                }
            }
            _ => {}
        }
    }

    /// Highlights the hovered root item and opens or closes its submenu.
    fn handle_root_pointer_move(&self, ctx: &mut EventCtx<A>, bounds: Rect, pos: Point) {
        let entries = self.entries.read();
        if let Some(index) = self.item_index_at(bounds, &entries, pos) {
            self.active_index.set(Some(index));
            if self.has_submenu(&entries, index) {
                self.open_submenu(ctx, &entries, index);
            } else {
                self.close_submenu(ctx, PopupDismissReason::Programmatic);
            }
            ctx.request_repaint();
        }
    }

    /// Records an enabled leaf press or opens a pressed submenu parent.
    fn handle_root_pointer_press(&self, ctx: &mut EventCtx<A>, bounds: Rect, pos: Point) {
        let entries = self.entries.read();
        self.pressed_entry.set(None);
        if let Some(index) = self.item_index_at(bounds, &entries, pos) {
            if self.has_submenu(&entries, index) {
                self.open_submenu(ctx, &entries, index);
            } else if matches!(entries.get(index), Some(ContextMenuEntry::Item(item)) if !item.is_disabled())
            {
                self.active_index.set(Some(index));
                self.pressed_entry.set(Some(ContextMenuPressedEntry {
                    parent: None,
                    index,
                }));
                ctx.request_repaint();
            }
            return;
        }
        ctx.stop_propagation();
    }

    /// Activates only a left-button release matching the recorded root press.
    fn handle_root_pointer_release(
        &self,
        ctx: &mut EventCtx<A>,
        bounds: Rect,
        pos: Point,
        button: MouseButton,
    ) {
        let pressed = self.pressed_entry.read();
        self.pressed_entry.set(None);
        if button != MouseButton::Left {
            ctx.request_repaint();
            ctx.stop_propagation();
            return;
        }
        let entries = self.entries.read();
        let release = self
            .item_index_at(bounds, &entries, pos)
            .map(|index| (None, index));
        let Some(pressed) = pressed else {
            ctx.request_repaint();
            return;
        };
        if release != Some((pressed.parent, pressed.index)) {
            ctx.request_repaint();
            return;
        }
        if self.has_submenu(&entries, pressed.index) {
            self.open_submenu(ctx, &entries, pressed.index);
        } else {
            self.activate_entry(ctx, &entries, pressed.index);
        }
    }

    /// Runs an enabled leaf action and closes the complete menu.
    fn activate_entry(&self, ctx: &mut EventCtx<A>, entries: &[ContextMenuEntry<A>], index: usize) {
        let Some(ContextMenuEntry::Item(item)) = entries.get(index) else {
            return;
        };
        if item.is_disabled() {
            ctx.stop_propagation();
            return;
        }
        if !item.submenu.is_empty() {
            self.open_submenu(ctx, entries, index);
            return;
        }
        if let Some(action) = &item.action {
            action.run(ctx);
        }
        self.close_with_runtime(ctx, PopupDismissReason::Programmatic);
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Registers, places, and opens an enabled first-level submenu.
    fn open_submenu(&self, ctx: &mut EventCtx<A>, entries: &[ContextMenuEntry<A>], index: usize) {
        let Some(ContextMenuEntry::Item(item)) = entries.get(index) else {
            return;
        };
        if item.is_disabled() || item.submenu.is_empty() {
            return;
        }
        self.active_index.set(Some(index));
        self.submenu_parent_index.set(Some(index));
        self.submenu_active_index
            .set(self.first_enabled_index(&item.submenu));

        let Some(root_geometry) = self
            .refresh_resolved_geometry()
            .or_else(|| self.geometry.read())
        else {
            ctx.request_repaint();
            ctx.stop_propagation();
            return;
        };
        let Some(parent_row) = self.entry_row(root_geometry.menu, entries, index) else {
            return;
        };
        let submenu = self.submenu_rect(
            root_geometry.viewport,
            root_geometry.menu,
            parent_row,
            &item.submenu,
        );
        if self.ensure_submenu_registered(ctx) {
            if let Some(popup_id) = self.submenu_popup_id.read() {
                let _ = ctx.runtime().open_popup(popup_id, parent_row, submenu);
            }
        }
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Reports whether an indexed root item owns any submenu entries.
    fn has_submenu(&self, entries: &[ContextMenuEntry<A>], index: usize) -> bool {
        matches!(entries.get(index), Some(ContextMenuEntry::Item(item)) if !item.submenu.is_empty())
    }

    /// Changes root keyboard selection and closes the current submenu.
    fn set_active(&self, ctx: &mut EventCtx<A>, next: Option<usize>) {
        self.active_index.set(next);
        self.close_submenu(ctx, PopupDismissReason::Programmatic);
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Clones entries for the currently open submenu parent.
    fn submenu_entries(&self) -> Option<Vec<ContextMenuEntry<A>>> {
        let parent = self.submenu_parent_index.read()?;
        let entries = self.entries.read();
        let ContextMenuEntry::Item(item) = entries.get(parent)? else {
            return None;
        };
        Some(item.submenu.clone())
    }

    /// Ensures the submenu portal has the current owner, parent, and content.
    fn ensure_submenu_registered(&self, ctx: &EventCtx<A>) -> bool {
        let (Some(root_popup_id), Some(submenu_popup_id)) =
            (self.root_popup_id, self.submenu_popup_id.read())
        else {
            return false;
        };
        let runtime = ctx.runtime();
        let owner = popup_owner_for_event(&runtime, ctx);
        let current_matches = {
            let portal = runtime.popup_portal();
            let portal = portal.borrow();
            portal.request(submenu_popup_id).is_some_and(|request| {
                request.owner() == &owner
                    && request.parent() == Some(root_popup_id)
                    && request.mount_policy() == PopupMountPolicy::RetainedOverlay
            })
        };
        if current_matches {
            return true;
        }
        if runtime.popup_portal().borrow().contains(submenu_popup_id) {
            runtime.unregister_popup(submenu_popup_id);
        }
        let content = context_menu_submenu_content(self.clone());
        runtime
            .register_popup(
                PopupRequest::new(submenu_popup_id, owner, content)
                    .with_parent(root_popup_id)
                    .with_semantics(menu_popup_semantics(true))
                    .with_mount_policy(PopupMountPolicy::RetainedOverlay),
            )
            .is_ok()
    }

    /// Clears submenu interaction state and requests host dismissal.
    fn close_submenu(&self, ctx: &mut EventCtx<A>, reason: PopupDismissReason) {
        self.submenu_parent_index.set(None);
        self.submenu_active_index.set(None);
        self.pressed_entry.set(None);
        if let Some(popup_id) = self.submenu_popup_id.read() {
            ctx.runtime().close_popup(popup_id, reason);
        }
        ctx.request_repaint();
    }

    /// Clears complete menu state, writable visibility, and both portals.
    fn close_with_runtime(&self, ctx: &mut EventCtx<A>, reason: PopupDismissReason) {
        if let Some(bound) = &self.bound_open {
            bound.set(false);
        } else if self.open.is_none() {
            self.internal_open.set(false);
        }
        self.last_requested_open.set(false);
        self.active_index.set(None);
        self.submenu_parent_index.set(None);
        self.submenu_active_index.set(None);
        self.pressed_entry.set(None);
        self.pointer_anchor.set(None);
        self.geometry.set(None);
        if let Some(popup_id) = self.root_popup_id {
            ctx.runtime().close_popup(popup_id, reason);
        }
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Handles one-tier submenu Escape, navigation, and leaf activation.
    fn handle_submenu_keyboard(&self, ctx: &mut EventCtx<A>, key: &Key) {
        let Some(entries) = self.submenu_entries() else {
            self.close_submenu(ctx, PopupDismissReason::Programmatic);
            return;
        };
        match key {
            Key::Named(NamedKey::Escape | NamedKey::ArrowLeft) => {
                self.close_submenu(ctx, PopupDismissReason::Escape);
                ctx.stop_propagation();
            }
            Key::Named(NamedKey::ArrowDown) => {
                let next = self.next_enabled_index(&entries, self.submenu_active_index.read());
                self.submenu_active_index.set(next);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Key::Named(NamedKey::ArrowUp) => {
                let next = self.previous_enabled_index(&entries, self.submenu_active_index.read());
                self.submenu_active_index.set(next);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Key::Named(NamedKey::Home) => {
                self.submenu_active_index
                    .set(self.first_enabled_index(&entries));
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Key::Named(NamedKey::End) => {
                self.submenu_active_index
                    .set(self.last_enabled_index(&entries));
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Key::Named(NamedKey::Enter | NamedKey::Space) => {
                if let Some(index) = self
                    .submenu_active_index
                    .read()
                    .or_else(|| self.first_enabled_index(&entries))
                {
                    self.activate_submenu_entry(ctx, &entries, index);
                }
            }
            _ => {}
        }
    }

    /// Highlights the enabled submenu item under the pointer.
    fn handle_submenu_pointer_move(&self, ctx: &mut EventCtx<A>, bounds: Rect, pos: Point) {
        let Some(entries) = self.submenu_entries() else {
            return;
        };
        let next = self
            .item_index_at(bounds, &entries, pos)
            .filter(|index| matches!(entries.get(*index), Some(ContextMenuEntry::Item(item)) if !item.is_disabled()));
        if self.submenu_active_index.read() != next {
            self.submenu_active_index.set(next);
            ctx.request_repaint();
        }
    }

    /// Records an enabled submenu press with its root parent identity.
    fn handle_submenu_pointer_press(&self, ctx: &mut EventCtx<A>, bounds: Rect, pos: Point) {
        self.pressed_entry.set(None);
        let Some(parent) = self.submenu_parent_index.read() else {
            return;
        };
        let Some(entries) = self.submenu_entries() else {
            return;
        };
        if let Some(index) = self
            .item_index_at(bounds, &entries, pos)
            .filter(|index| matches!(entries.get(*index), Some(ContextMenuEntry::Item(item)) if !item.is_disabled()))
        {
            self.submenu_active_index.set(Some(index));
            self.pressed_entry.set(Some(ContextMenuPressedEntry {
                parent: Some(parent),
                index,
            }));
            ctx.request_repaint();
        }
        ctx.stop_propagation();
    }

    /// Activates only a left-button release matching the submenu press.
    fn handle_submenu_pointer_release(
        &self,
        ctx: &mut EventCtx<A>,
        bounds: Rect,
        pos: Point,
        button: MouseButton,
    ) {
        let pressed = self.pressed_entry.read();
        self.pressed_entry.set(None);
        if button != MouseButton::Left {
            ctx.stop_propagation();
            return;
        }
        let Some(parent) = self.submenu_parent_index.read() else {
            return;
        };
        let Some(entries) = self.submenu_entries() else {
            return;
        };
        let release = self.item_index_at(bounds, &entries, pos);
        if pressed
            == release.map(|index| ContextMenuPressedEntry {
                parent: Some(parent),
                index,
            })
        {
            if let Some(index) = release {
                self.activate_submenu_entry(ctx, &entries, index);
            }
        }
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Runs an enabled submenu leaf action and closes the complete menu.
    fn activate_submenu_entry(
        &self,
        ctx: &mut EventCtx<A>,
        entries: &[ContextMenuEntry<A>],
        index: usize,
    ) {
        let Some(ContextMenuEntry::Item(item)) = entries.get(index) else {
            return;
        };
        if item.is_disabled() || !item.submenu.is_empty() {
            ctx.stop_propagation();
            return;
        }
        if let Some(action) = &item.action {
            action.run(ctx);
        }
        self.close_with_runtime(ctx, PopupDismissReason::Programmatic);
    }

    /// Paints a clipped popup shell, all rows, and the final border.
    fn paint_entries(
        &self,
        ctx: &mut PaintCtx<'_>,
        menu: Rect,
        entries: &[ContextMenuEntry<A>],
        active: Option<usize>,
    ) {
        paint_popup_shell(ctx, menu, &self.style.popup);
        ctx.with_overlay_clip(menu, |ctx| {
            let mut y = menu.y;
            for (idx, entry) in entries.iter().enumerate() {
                let height = self.entry_height(entry);
                let row = Rect::new(menu.x, y, menu.w, height);
                match entry {
                    ContextMenuEntry::Item(item) => {
                        self.paint_item(ctx, row, item, active == Some(idx));
                    }
                    ContextMenuEntry::Separator => {
                        let line = Rect::new(
                            row.x + self.style.padding_x,
                            row.y + (row.h - 1.0) * 0.5,
                            (row.w - self.style.padding_x * 2.0).max(0.0),
                            1.0,
                        );
                        ctx.push_overlay(DrawCmd::Rect(DrawRect {
                            rect: line,
                            color: self.style.separator,
                        }));
                    }
                }
                y += height;
            }
        });
        paint_popup_border(ctx, menu, &self.style.popup);
    }

    /// Paints one item row with active, disabled, metadata, and submenu states.
    fn paint_item(
        &self,
        ctx: &mut PaintCtx<'_>,
        row: Rect,
        item: &ContextMenuItem<A>,
        active: bool,
    ) {
        let disabled = item.is_disabled();
        let opacity = if disabled {
            self.style.disabled_opacity
        } else {
            1.0
        };
        if active {
            ctx.push_overlay(DrawCmd::Rect(DrawRect {
                rect: row,
                color: apply_opacity(self.style.popup.option_active, opacity),
            }));
        }

        let mut x = row.x + self.style.padding_x;
        if let Some(icon) = &item.icon {
            ctx.push_overlay(DrawCmd::Image(DrawImage {
                rect: Rect::new(
                    x,
                    row.y + (row.h - self.style.icon_size) * 0.5,
                    self.style.icon_size,
                    self.style.icon_size,
                ),
                icon: icon.clone(),
                tint: apply_opacity(
                    if disabled {
                        self.style.popup.disabled_icon_tint
                    } else {
                        self.style.popup.icon_tint
                    },
                    opacity,
                ),
                rotation_rad: 0.0,
            }));
            x += self.style.icon_size + self.style.icon_gap;
        }

        let trailing_icon_w = if item.submenu.is_empty() {
            0.0
        } else {
            self.style.icon_size + self.style.icon_gap
        };
        let shortcut_w = item
            .shortcut
            .as_ref()
            .map(|shortcut| {
                measure_text(
                    ctx.text_system.as_deref_mut(),
                    shortcut,
                    self.style.shortcut_text,
                )
                .w
            })
            .unwrap_or(0.0);
        let text_style = if disabled {
            self.style.popup.disabled_text
        } else {
            self.style.popup.text
        };
        let text_right =
            row.right() - self.style.padding_x - shortcut_w - self.style.icon_gap - trailing_icon_w;
        paint_overlay_text_in_rect(
            ctx,
            &item.label,
            text_style,
            Rect::new(x, row.y, (text_right - x).max(0.0), row.h),
            opacity,
        );
        if let Some(shortcut) = &item.shortcut {
            paint_overlay_text_in_rect(
                ctx,
                shortcut,
                self.style.shortcut_text,
                Rect::new(
                    row.right() - self.style.padding_x - shortcut_w - trailing_icon_w,
                    row.y,
                    shortcut_w,
                    row.h,
                ),
                opacity,
            );
        }
        if !item.submenu.is_empty() {
            ctx.push_overlay(DrawCmd::Image(DrawImage {
                rect: Rect::new(
                    row.right() - self.style.padding_x - self.style.icon_size,
                    row.y + (row.h - self.style.icon_size) * 0.5,
                    self.style.icon_size,
                    self.style.icon_size,
                ),
                icon: IconId::Lucide(Icon::ChevronRight),
                tint: apply_opacity(self.style.popup.icon_tint, opacity),
                rotation_rad: 0.0,
            }));
        }
    }
}

/// Root popup component that obtains the child submenu popup identifier.
struct RetainedContextMenuRootComponent<A> {
    /// Shared controller used to build root popup content.
    controller: ContextMenuController<A>,
}

impl<A: 'static> ComponentNode<A> for RetainedContextMenuRootComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        self.controller.submenu_popup_id.set(
            context
                .runtime()
                .popup_id_for_element(context.element_id())
                .ok(),
        );
        View::leaf(RetainedContextMenuRoot {
            controller: self.controller.clone(),
        })
    }
}

/// Retained root popup widget for painting and dispatching menu interaction.
struct RetainedContextMenuRoot<A> {
    /// Shared controller handling root layout, paint, and input.
    controller: ContextMenuController<A>,
}

impl<A> Clone for RetainedContextMenuRoot<A> {
    fn clone(&self) -> Self {
        Self {
            controller: self.controller.clone(),
        }
    }
}

impl<A: 'static> Widget<A> for RetainedContextMenuRoot<A> {
    fn debug_name(&self) -> &'static str {
        "ContextMenuPopup"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        retained_context_menu_layout(
            constraints,
            self.controller.style.width,
            self.controller.menu_height(&self.controller.entries.read()),
            self.controller.style.popup.popup_max_height,
        )
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        self.controller.refresh_resolved_geometry();
        self.controller.paint_entries(
            ctx,
            bounds,
            &self.controller.entries.read(),
            self.controller.active_index.read(),
        );
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.controller.disabled.read() {
            self.controller
                .close_with_runtime(ctx, PopupDismissReason::Programmatic);
            return;
        }
        match event {
            Event::Keyboard(key) if key.state == KeyState::Pressed && !key.repeat => {
                self.controller.handle_root_keyboard(ctx, &key.key);
            }
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                self.controller.handle_root_pointer_move(ctx, bounds, *pos);
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left | MouseButton::Right,
                pressed: true,
                ..
            }) => self.controller.handle_root_pointer_press(ctx, bounds, *pos),
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left | MouseButton::Right,
                pressed: false,
                ..
            }) => self.controller.handle_root_pointer_release(
                ctx,
                bounds,
                *pos,
                event_pointer_button(event),
            ),
            Event::Pointer(PointerEvent::Cancelled { .. }) => {
                self.controller.pressed_entry.set(None);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::Focusable
    }

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }

    fn hover_cursor_role_at(
        &self,
        bounds: Rect,
        _layout: &LayoutResult,
        pos: Point,
    ) -> HoverCursorRole {
        let entries = self.controller.entries.read();
        self.controller
            .item_index_at(bounds, &entries, pos)
            .filter(|index| matches!(entries.get(*index), Some(ContextMenuEntry::Item(item)) if !item.is_disabled()))
            .map_or(HoverCursorRole::Default, |_| HoverCursorRole::Pointer)
    }
}

/// Retained first-level submenu widget sharing the root controller.
struct RetainedContextSubmenu<A> {
    /// Shared controller handling submenu layout, paint, and input.
    controller: ContextMenuController<A>,
}

impl<A> Clone for RetainedContextSubmenu<A> {
    fn clone(&self) -> Self {
        Self {
            controller: self.controller.clone(),
        }
    }
}

impl<A: 'static> Widget<A> for RetainedContextSubmenu<A> {
    fn debug_name(&self) -> &'static str {
        "ContextSubmenuPopup"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let entries = self.controller.submenu_entries().unwrap_or_default();
        retained_context_menu_layout(
            constraints,
            self.controller.style.submenu_width,
            self.controller.menu_height(&entries),
            self.controller.style.popup.popup_max_height,
        )
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let Some(entries) = self.controller.submenu_entries() else {
            return;
        };
        self.controller.paint_entries(
            ctx,
            bounds,
            &entries,
            self.controller.submenu_active_index.read(),
        );
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        match event {
            Event::Keyboard(key) if key.state == KeyState::Pressed && !key.repeat => {
                self.controller.handle_submenu_keyboard(ctx, &key.key);
            }
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                self.controller
                    .handle_submenu_pointer_move(ctx, bounds, *pos);
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left | MouseButton::Right,
                pressed: true,
                ..
            }) => self
                .controller
                .handle_submenu_pointer_press(ctx, bounds, *pos),
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left | MouseButton::Right,
                pressed: false,
                ..
            }) => self.controller.handle_submenu_pointer_release(
                ctx,
                bounds,
                *pos,
                event_pointer_button(event),
            ),
            Event::Pointer(PointerEvent::Cancelled { .. }) => {
                self.controller.pressed_entry.set(None);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::Focusable
    }

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }

    fn hover_cursor_role_at(
        &self,
        bounds: Rect,
        _layout: &LayoutResult,
        pos: Point,
    ) -> HoverCursorRole {
        let Some(entries) = self.controller.submenu_entries() else {
            return HoverCursorRole::Default;
        };
        self.controller
            .item_index_at(bounds, &entries, pos)
            .filter(|index| matches!(entries.get(*index), Some(ContextMenuEntry::Item(item)) if !item.is_disabled()))
            .map_or(HoverCursorRole::Default, |_| HoverCursorRole::Pointer)
    }
}

/// Wraps the retained root component in reusable popup content.
fn context_menu_root_content<A: 'static>(controller: ContextMenuController<A>) -> PopupContent<A> {
    PopupContent::new(move || {
        View::component(RetainedContextMenuRootComponent {
            controller: controller.clone(),
        })
    })
}

/// Wraps the retained submenu widget in reusable popup content.
fn context_menu_submenu_content<A: 'static>(
    controller: ContextMenuController<A>,
) -> PopupContent<A> {
    PopupContent::new(move || {
        View::leaf(RetainedContextSubmenu {
            controller: controller.clone(),
        })
    })
}

/// Constrains a popup to requested width and capped content height.
fn retained_context_menu_layout(
    constraints: Constraints,
    width: f32,
    content_height: f32,
    max_height: f32,
) -> LayoutResult {
    let size = constraints.constrain(Size::new(width, content_height.min(max_height)));
    let bounds = Rect::new(0.0, 0.0, size.w, size.h);
    LayoutResult {
        size,
        children: Vec::new(),
        paint_bounds: bounds,
        visual_bounds: bounds,
        overlay_hit_bounds: Vec::new(),
        clip: Some(ailloli_ui_core::ClipShape::Rect(bounds)),
        is_window_root_clip: false,
        artifact: None,
    }
}

/// Resolves popup ownership from event, presentation, or headless context.
fn popup_owner_for_event<A>(runtime: &RuntimeHandle<A>, ctx: &EventCtx<A>) -> PopupOwner {
    if let Some(meta) = ctx.event_meta() {
        return PopupOwner::new(
            meta.logical_window_id().clone(),
            meta.presentation_generation(),
            runtime.element_tree_id(),
            ctx.target(),
        );
    }
    if let Some((window, generation)) = runtime.presentation_scope() {
        return PopupOwner::new(window, generation, runtime.element_tree_id(), ctx.target());
    }
    PopupOwner::new(
        HEADLESS_POPUP_WINDOW_ID,
        PresentationGeneration::INITIAL,
        runtime.element_tree_id(),
        ctx.target(),
    )
}

/// Extracts a pointer button and falls back to left for non-button events.
fn event_pointer_button(event: &Event) -> MouseButton {
    match event {
        Event::Pointer(PointerEvent::Button { button, .. }) => *button,
        _ => MouseButton::Left,
    }
}

/// Resolves host size from its child or finite maximum constraints.
fn host_slot_size<A: 'static>(
    engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
    ctx: &mut LayoutCtx<'_>,
    children: &mut [LayoutChild],
    constraints: Constraints,
    layout: LayoutStyle,
) -> Size {
    let intrinsic = if let Some(child) = children.first_mut() {
        child.layout(engine, ctx, constraints.loosen()).size
    } else {
        Size::new(
            finite_or(constraints.max_w, 0.0),
            finite_or(constraints.max_h, 0.0),
        )
    };
    apply_layout_size(intrinsic, layout, constraints)
}

/// Returns `value` when finite and `fallback` for NaN or either infinity.
fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}
