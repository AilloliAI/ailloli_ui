//! Retained single- or multi-open accordion sections.

use std::rc::Rc;
use std::sync::Arc;

use crate::layout::layout_ext::finish_view_sized;
use crate::layout::{Column, Container};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{
    AlignItems, Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius,
};
use ailloli_ui_core::{Color, FontId, IconId, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, IntoViewKeyExt, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawImage, DrawRRect, DrawText};
use ailloli_ui_text::{PreparedTextLayout, TextLayoutParams, WrapMode};
use lucide_icons::Icon as LucideIcon;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Built-in density choices for an [`Accordion`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::AccordionSize;
/// assert_eq!(AccordionSize::default(), AccordionSize::Default);
/// ```
pub enum AccordionSize {
    /// 30-pixel headers with smaller padding and typography.
    Compact,
    /// 36-pixel headers with standard padding and typography.
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Cardinality allowed for open accordion IDs.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::AccordionMode;
/// assert_eq!(AccordionMode::default(), AccordionMode::Single);
/// ```
pub enum AccordionMode {
    /// At most the first distinct ID is considered open.
    #[default]
    Single,
    /// Every distinct supplied ID may remain open.
    Multiple,
}

#[derive(Clone, Debug, PartialEq)]
/// Resolved colors, typography, and logical-pixel metrics for an accordion.
///
/// `content_padding_x` and `content_indent` are reserved compatibility fields;
/// current content uses `content_padding_y` uniformly on all four edges.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::{AccordionSize, AccordionStyle};
/// let style = AccordionStyle::from_theme(Theme::dark(), AccordionSize::Compact);
/// assert_eq!(style.header_height, 30.0);
/// assert_eq!(style.title_text.px_size, 12);
/// ```
pub struct AccordionStyle {
    /// Outer container fill.
    pub background: Color,
    /// Outer container border.
    pub border: Border,
    /// Border painted around focused enabled headers.
    pub focus_ring: Border,
    /// Closed idle header fill.
    pub header_background: Color,
    /// Closed hovered header fill.
    pub header_hovered: Color,
    /// Closed pressed header fill.
    pub header_pressed: Color,
    /// Open header fill, taking precedence over interaction fills.
    pub header_open: Color,
    /// Enabled header title style.
    pub title_text: TextStyle,
    /// Disabled header title style.
    pub disabled_text: TextStyle,
    /// Enabled chevron tint.
    pub icon_tint: Color,
    /// Disabled chevron tint before opacity multiplication.
    pub disabled_icon_tint: Color,
    /// Outer/header corner radii.
    pub radius: Radius,
    /// Header intrinsic height.
    pub header_height: f32,
    /// Outer container padding on every edge.
    pub padding: f32,
    /// Vertical gap between headers and mounted content.
    pub gap: f32,
    /// Header horizontal inset.
    pub header_padding_x: f32,
    /// Reserved horizontal content padding; currently unused.
    pub content_padding_x: f32,
    /// Current content padding applied uniformly on every edge.
    pub content_padding_y: f32,
    /// Reserved content indentation; currently unused.
    pub content_indent: f32,
    /// Chevron width and height.
    pub icon_size: f32,
    /// Alpha multiplier applied to disabled header paint.
    pub disabled_opacity: f32,
}

impl Default for AccordionStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), AccordionSize::Default)
    }
}

impl AccordionStyle {
    /// Resolves accordion colors and geometry from `theme` and `size`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{AccordionSize, AccordionStyle};
    /// let style = AccordionStyle::from_theme(Theme::dark(), AccordionSize::Default);
    /// assert_eq!(style.header_height, 36.0);
    /// assert_eq!(style.disabled_opacity, 0.42);
    /// ```
    pub fn from_theme(theme: Theme, size: AccordionSize) -> Self {
        let palette = theme.palette();
        let (header_height, padding, gap, header_padding_x, text_size) = match size {
            AccordionSize::Compact => (30.0, 6.0, 3.0, 8.0, 12),
            AccordionSize::Default => (36.0, 8.0, 4.0, 10.0, 13),
        };
        Self {
            background: palette.surface,
            border: Border::new(1.0, palette.border),
            focus_ring: Border::new(1.0, palette.focus),
            header_background: Color::TRANSPARENT,
            header_hovered: palette.surface_elevated,
            header_pressed: Color::hex_rgb(0x20252A),
            header_open: palette.accent.with_alpha(0.16),
            title_text: TextStyle::new(FontId::Ui, text_size, palette.text),
            disabled_text: TextStyle::new(
                FontId::Ui,
                text_size,
                palette.text_muted.with_alpha(0.70),
            ),
            icon_tint: palette.text_muted,
            disabled_icon_tint: palette.text_muted.with_alpha(0.58),
            radius: Radius::uniform(theme.radius().md),
            header_height,
            padding,
            gap,
            header_padding_x,
            content_padding_x: 12.0,
            content_padding_y: 8.0,
            content_indent: 18.0,
            icon_size: 16.0,
            disabled_opacity: 0.42,
        }
    }
}

/// Shared callback receiving `(item_id, requested_open_state)`.
type AccordionToggleHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, String, bool)>;

#[derive(Clone)]
/// One identified accordion header and its retained content view.
///
/// IDs should be unique. They drive open-state comparison and retained view
/// keys; duplicates are not rejected and can produce duplicate content/keys.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::AccordionItem;
/// use ailloli_ui_widgets::text::Text;
/// let item = AccordionItem::<()>::new("general", "General").child(Text::new("Settings"));
/// let _ = item;
/// ```
pub struct AccordionItem<A = ()> {
    /// Identity used by open-state lists and retained keys.
    id: String,
    /// Owned unwrapped header title.
    title: String,
    /// Live disabled state for the header.
    disabled: Binding<bool>,
    /// Retained content mounted only while the ID is open.
    content: View<A>,
}

impl<A: 'static> AccordionItem<A> {
    /// Creates an enabled item with empty content.
    ///
    /// Empty IDs and titles are accepted; callers should still use unique,
    /// stable IDs for correct retained identity.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::AccordionItem;
    /// let item: AccordionItem<()> = AccordionItem::new("advanced", "Advanced");
    /// let _ = item;
    /// ```
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            disabled: Binding::Static(false),
            content: View::empty(),
        }
    }

    /// Sets static or reactive disabled state for the header.
    ///
    /// Disabled headers ignore events and leave focus traversal. Existing open
    /// content remains mounted; disabling does not close the item.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::AccordionItem;
    /// let item: AccordionItem<()> = AccordionItem::new("locked", "Locked").disabled(true);
    /// let _ = item;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Replaces the item's sole retained content view.
    ///
    /// Content is mounted only while the item ID appears in sanitized open state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::AccordionItem;
    /// use ailloli_ui_widgets::text::Text;
    /// let item = AccordionItem::<()>::new("about", "About").child(Text::new("Ailloli"));
    /// let _ = item;
    /// ```
    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.content = child.into_view();
        self
    }
}

/// A controlled, bound, or internally managed set of collapsible sections.
///
/// Open lists are de-duplicated in order; single mode keeps only the first ID.
/// Unknown IDs are retained in state but mount no content. Headers activate on
/// left-button release or pressed Enter/Space. Bound/internal mode writes the
/// new sanitized list; controlled mode only reports through `on_toggle` and
/// requires the consumer to update its binding.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{Accordion, AccordionItem};
/// let accordion = Accordion::<()>::new()
///     .item(AccordionItem::new("one", "One"))
///     .default_open("one");
/// let _ = accordion;
/// ```
pub struct Accordion<A = ()> {
    /// Layout applied to the outer container.
    pub(crate) layout: LayoutStyle,
    /// Flex-item behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Single- or multiple-open sanitation/toggle mode.
    mode: AccordionMode,
    /// Items in insertion order; no capacity bound.
    items: Vec<AccordionItem<A>>,
    /// Optional controlled or bound open-ID source.
    open_ids: Option<Binding<Vec<String>>>,
    /// Writable open-ID signal in bound mode.
    bound_open_ids: Option<Signal<Vec<String>>>,
    /// Initial internal open IDs, sanitized on component build.
    default_open_ids: Vec<String>,
    /// Optional callback for requested toggles.
    on_toggle: Option<AccordionToggleHandler<A>>,
    /// Resolved paint and geometry.
    style: AccordionStyle,
}

crate::impl_layout_builders!(Accordion);

impl<A: 'static> Default for Accordion<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Accordion<A> {
    /// Creates an empty single-open accordion with no initially open IDs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Accordion;
    /// let accordion: Accordion<()> = Accordion::new();
    /// let _ = accordion;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            mode: AccordionMode::Single,
            items: Vec::new(),
            open_ids: None,
            bound_open_ids: None,
            default_open_ids: Vec::new(),
            on_toggle: None,
            style: AccordionStyle::default(),
        }
    }

    /// Selects single-open mode and truncates default IDs to the first.
    ///
    /// External controlled/bound lists are sanitized when read; this builder
    /// does not mutate their source.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Accordion;
    /// let accordion: Accordion<()> = Accordion::new().multiple().single();
    /// let _ = accordion;
    /// ```
    pub fn single(mut self) -> Self {
        self.mode = AccordionMode::Single;
        self.default_open_ids.truncate(1);
        self
    }

    /// Selects multiple-open mode without changing current default IDs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Accordion;
    /// let accordion: Accordion<()> = Accordion::new().multiple();
    /// let _ = accordion;
    /// ```
    pub fn multiple(mut self) -> Self {
        self.mode = AccordionMode::Multiple;
        self
    }

    /// Sets open cardinality and truncates defaults when selecting single mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Accordion, AccordionMode};
    /// let accordion: Accordion<()> = Accordion::new().mode(AccordionMode::Multiple);
    /// let _ = accordion;
    /// ```
    pub fn mode(mut self, mode: AccordionMode) -> Self {
        self.mode = mode;
        if self.mode == AccordionMode::Single {
            self.default_open_ids.truncate(1);
        }
        self
    }

    /// Appends one item in display order.
    ///
    /// IDs are not validated for uniqueness or membership in existing open lists.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Accordion, AccordionItem};
    /// let accordion = Accordion::<()>::new().item(AccordionItem::new("one", "One"));
    /// let _ = accordion;
    /// ```
    pub fn item(mut self, item: AccordionItem<A>) -> Self {
        self.items.push(item);
        self
    }

    /// Adds an initially open ID according to the current mode.
    ///
    /// Single mode replaces the list with `id`; multiple mode appends only when
    /// absent. The ID need not match an item. External open state overrides it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Accordion;
    /// let accordion: Accordion<()> = Accordion::new().default_open("general");
    /// let _ = accordion;
    /// ```
    pub fn default_open(mut self, id: impl Into<String>) -> Self {
        let id = id.into();
        if self.mode == AccordionMode::Single {
            self.default_open_ids = vec![id];
        } else if !self.default_open_ids.iter().any(|open| open == &id) {
            self.default_open_ids.push(id);
        }
        self
    }

    /// Replaces initial open IDs after stable de-duplication.
    ///
    /// Input order is preserved. Single mode retains at most the first distinct
    /// ID; multiple mode retains every distinct ID, including unknown ones.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Accordion;
    /// let accordion: Accordion<()> =
    ///     Accordion::new().multiple().default_open_many(["one", "one", "two"]);
    /// let _ = accordion;
    /// ```
    pub fn default_open_many(mut self, ids: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut out = Vec::new();
        for id in ids {
            let id = id.into();
            if !out.iter().any(|open| open == &id) {
                out.push(id);
            }
        }
        if self.mode == AccordionMode::Single {
            out.truncate(1);
        }
        self.default_open_ids = out;
        self
    }

    /// Sets controlled static or reactive open IDs.
    ///
    /// IDs are sanitized only when read. Toggle requests cannot mutate a purely
    /// controlled source and should be handled through [`Self::on_toggle`]. This
    /// method does not clear a writable signal installed by an earlier
    /// [`Self::bind_open_ids`] call; choose one ownership mode per builder.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Accordion;
    /// let accordion: Accordion<()> = Accordion::new().open_ids(vec!["one".to_string()]);
    /// let _ = accordion;
    /// ```
    pub fn open_ids(mut self, ids: impl Into<Binding<Vec<String>>>) -> Self {
        self.open_ids = Some(ids.into());
        self
    }

    /// Installs a writable signal for two-way open-ID state.
    ///
    /// Toggles write a sanitized list to the signal before invoking the optional
    /// callback. The source itself is not sanitized until a toggle writes it.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// use ailloli_ui_widgets::controls::Accordion;
    /// let ids = Signal::new(Rc::new(RefCell::new(vec!["one".to_string()])), Rc::new(|| {}));
    /// let accordion: Accordion<()> = Accordion::new().bind_open_ids(ids);
    /// let _ = accordion;
    /// ```
    pub fn bind_open_ids(mut self, ids: impl Into<Signal<Vec<String>>>) -> Self {
        let signal = ids.into();
        self.open_ids = Some(Binding::Signal(signal.clone()));
        self.bound_open_ids = Some(signal);
        self
    }

    /// Replaces the complete resolved style without changing mode or state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{Accordion, AccordionSize, AccordionStyle};
    /// let style = AccordionStyle::from_theme(Theme::dark(), AccordionSize::Compact);
    /// let accordion: Accordion<()> = Accordion::new().accordion_style(style);
    /// let _ = accordion;
    /// ```
    pub fn accordion_style(mut self, style: AccordionStyle) -> Self {
        self.style = style;
        self
    }

    /// Replaces the complete style with the default-theme built-in size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Accordion, AccordionSize};
    /// let accordion: Accordion<()> = Accordion::new().accordion_size(AccordionSize::Compact);
    /// let _ = accordion;
    /// ```
    pub fn accordion_size(mut self, size: AccordionSize) -> Self {
        self.style = AccordionStyle::from_theme(Theme::default(), size);
        self
    }

    /// Maps each requested `(id, open)` state to an action and dispatches it.
    ///
    /// The callback runs after bound/internal state is updated. In controlled
    /// mode it is the consumer's opportunity to update open IDs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Accordion;
    /// enum Action { Toggled(String, bool) }
    /// let accordion = Accordion::new().on_toggle(|id, open| Action::Toggled(id, open));
    /// let _ = accordion;
    /// ```
    pub fn on_toggle(mut self, f: impl Fn(String, bool) -> A + 'static) -> Self {
        self.on_toggle = Some(Rc::new(move |ctx, id, open| ctx.dispatch(f(id, open))));
        self
    }

    /// Installs a context-aware toggle callback.
    ///
    /// A later toggle-handler builder replaces it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Accordion;
    /// let accordion = Accordion::<()>::new()
    ///     .on_toggle_ctx(|ctx, _id, _open| ctx.request_repaint());
    /// let _ = accordion;
    /// ```
    pub fn on_toggle_ctx(mut self, f: impl Fn(&mut EventCtx<A>, String, bool) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(f));
        self
    }
}

/// Component properties used to allocate internal open-ID state and keyed rows.
struct AccordionComponent<A> {
    layout: LayoutStyle,
    mode: AccordionMode,
    items: Vec<AccordionItem<A>>,
    open_ids: Option<Binding<Vec<String>>>,
    bound_open_ids: Option<Signal<Vec<String>>>,
    default_open_ids: Vec<String>,
    on_toggle: Option<AccordionToggleHandler<A>>,
    style: AccordionStyle,
}

impl<A: 'static> ComponentNode<A> for AccordionComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let internal_open = context.signal(sanitize_open_ids(&self.default_open_ids, self.mode));
        let open_binding = self
            .open_ids
            .clone()
            .unwrap_or_else(|| Binding::Signal(internal_open.clone()));
        let mutable_open = self
            .bound_open_ids
            .clone()
            .or_else(|| self.open_ids.is_none().then_some(internal_open));
        let open_ids = sanitize_open_ids(&open_binding.read(), self.mode);

        let mut content = Column::new()
            .gap(self.style.gap)
            .align_items(AlignItems::Stretch);

        for item in &self.items {
            let is_open = open_ids.iter().any(|open| open == &item.id);
            content = content.child(
                AccordionHeader {
                    id: item.id.clone(),
                    title: item.title.clone(),
                    disabled: item.disabled.clone(),
                    open_ids: open_binding.clone(),
                    mutable_open: mutable_open.clone(),
                    mode: self.mode,
                    open: is_open,
                    on_toggle: self.on_toggle.clone(),
                    style: self.style.clone(),
                }
                .into_view()
                .key(format!("accordion-header-{}", item.id)),
            );
            if is_open {
                content = content.child(
                    Container::new()
                        .fill_width()
                        .padding(0.0)
                        .child(
                            Container::new()
                                .fill_width()
                                .padding(self.style.content_padding_y)
                                .child(item.content.clone()),
                        )
                        .key(format!("accordion-content-{}", item.id)),
                );
            }
        }

        let mut container = Container::<A>::new()
            .background(self.style.background)
            .border(self.style.border.widths.top, self.style.border.colors.top)
            .radius(self.style.radius.tl)
            .padding(self.style.padding)
            .child(content);
        container.layout = self.layout;
        container.into_view()
    }
}

impl<A: 'static> IntoView<A> for Accordion<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(AccordionComponent {
                layout: self.layout,
                mode: self.mode,
                items: self.items,
                open_ids: self.open_ids,
                bound_open_ids: self.bound_open_ids,
                default_open_ids: self.default_open_ids,
                on_toggle: self.on_toggle,
                style: self.style,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Builder-to-view bridge for one keyed accordion header.
struct AccordionHeader<A> {
    id: String,
    title: String,
    disabled: Binding<bool>,
    open_ids: Binding<Vec<String>>,
    mutable_open: Option<Signal<Vec<String>>>,
    mode: AccordionMode,
    open: bool,
    on_toggle: Option<AccordionToggleHandler<A>>,
    style: AccordionStyle,
}

impl<A: 'static> IntoView<A> for AccordionHeader<A> {
    fn into_view(self) -> View<A> {
        View::leaf(AccordionHeaderWidget {
            id: self.id,
            title: self.title,
            disabled: self.disabled,
            open_ids: self.open_ids,
            mutable_open: self.mutable_open,
            mode: self.mode,
            open: self.open,
            on_toggle: self.on_toggle,
            style: self.style,
        })
    }
}

/// Retained focusable header that reads state and applies toggle requests.
struct AccordionHeaderWidget<A> {
    id: String,
    title: String,
    disabled: Binding<bool>,
    open_ids: Binding<Vec<String>>,
    mutable_open: Option<Signal<Vec<String>>>,
    mode: AccordionMode,
    open: bool,
    on_toggle: Option<AccordionToggleHandler<A>>,
    style: AccordionStyle,
}

impl<A: 'static> Widget<A> for AccordionHeaderWidget<A> {
    fn debug_name(&self) -> &'static str {
        "AccordionHeader"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let text_w = measure_text(ctx, &self.title, self.text_style()).unwrap_or(120.0);
        let intrinsic = Size::new(
            self.style.header_padding_x * 2.0 + self.style.icon_size + self.style.gap + text_w,
            self.style.header_height,
        );
        let size = constraints.constrain(intrinsic);
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

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let disabled = self.disabled.read();
        let interaction = ctx.interaction();
        let mut bg = self.style.header_background;
        if self.open {
            bg = self.style.header_open;
        } else if interaction.pressed && !disabled {
            bg = self.style.header_pressed;
        } else if interaction.hovered && !disabled {
            bg = self.style.header_hovered;
        }
        let opacity = if disabled {
            self.style.disabled_opacity
        } else {
            1.0
        };

        if bg.a > 0.0 {
            ctx.push(DrawCmd::RRect(DrawRRect {
                rect: bounds,
                radius: self.style.radius.tl,
                color: bg.with_alpha(bg.a * opacity),
            }));
        }

        let icon_rect = Rect::new(
            bounds.x + self.style.header_padding_x,
            bounds.y + (bounds.h - self.style.icon_size) * 0.5,
            self.style.icon_size,
            self.style.icon_size,
        );
        let icon = if self.open {
            IconId::Lucide(LucideIcon::ChevronDown)
        } else {
            IconId::Lucide(LucideIcon::ChevronRight)
        };
        let icon_tint = if disabled {
            self.style.disabled_icon_tint
        } else {
            self.style.icon_tint
        };
        ctx.push(DrawCmd::Image(DrawImage {
            rect: icon_rect,
            icon,
            tint: icon_tint.with_alpha(icon_tint.a * opacity),
            rotation_rad: 0.0,
        }));

        paint_text_centered(
            ctx,
            &self.title,
            self.text_style(),
            bounds,
            icon_rect.right() + self.style.gap,
            opacity,
        );

        if interaction.focused && !disabled {
            ctx.push(DrawCmd::Border(DrawBorder {
                rect: bounds,
                radius: self.style.radius,
                border: self.style.focus_ring,
            }));
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.disabled.read() {
            return;
        }
        match event {
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if bounds.contains(pos.x, pos.y) => self.toggle(ctx),
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                if matches!(
                    &key.key,
                    Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space)
                ) {
                    self.toggle(ctx);
                }
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        if self.disabled.read() {
            FocusPolicy::NotFocusable
        } else {
            FocusPolicy::Focusable
        }
    }
}

impl<A> AccordionHeaderWidget<A> {
    /// Resolves enabled or disabled header text style.
    fn text_style(&self) -> TextStyle {
        if self.disabled.read() {
            self.style.disabled_text
        } else {
            self.style.title_text
        }
    }

    /// Computes sanitized next state, writes when mutable, reports, and consumes.
    fn toggle(&self, ctx: &mut EventCtx<A>) {
        let current = sanitize_open_ids(&self.open_ids.read(), self.mode);
        let next_open = !current.iter().any(|id| id == &self.id);
        let next = toggled_open_ids(&current, &self.id, next_open, self.mode);
        let changed = next != current;
        if changed {
            if let Some(open) = &self.mutable_open {
                open.set(next);
            }
            ctx.request_repaint();
        }
        if let Some(on_toggle) = &self.on_toggle {
            on_toggle(ctx, self.id.clone(), next_open);
        }
        if changed || self.on_toggle.is_some() {
            ctx.stop_propagation();
        }
    }
}

/// Stable-de-duplicates IDs and truncates after the first in single mode.
fn sanitize_open_ids(ids: &[String], mode: AccordionMode) -> Vec<String> {
    let mut out = Vec::new();
    for id in ids {
        if !out.iter().any(|open| open == id) {
            out.push(id.clone());
        }
        if mode == AccordionMode::Single && !out.is_empty() {
            break;
        }
    }
    out
}

/// Returns the sanitized semantic result of opening or closing one ID.
fn toggled_open_ids(
    current: &[String],
    id: &str,
    next_open: bool,
    mode: AccordionMode,
) -> Vec<String> {
    match (mode, next_open) {
        (AccordionMode::Single, true) => vec![id.to_string()],
        (AccordionMode::Single, false) => Vec::new(),
        (AccordionMode::Multiple, true) => {
            let mut out = current.to_vec();
            if !out.iter().any(|open| open == id) {
                out.push(id.to_string());
            }
            out
        }
        (AccordionMode::Multiple, false) => current
            .iter()
            .filter(|open| open.as_str() != id)
            .cloned()
            .collect(),
    }
}

/// Measures one unwrapped header line, returning `None` without a text system.
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

/// Prepares one unwrapped header line, returning `None` without a text system.
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

/// Paints left-anchored header text vertically centered with alpha multiplication.
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
