//! Overlay toast values and a non-focusable stacking host.

use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{
    Border, BoxShadow, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius,
};
use ailloli_ui_core::{Color, FontId, IconId, Offset, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawBoxShadow, DrawCmd, DrawImage, DrawRRect};

use super::popup::{apply_opacity, paint_overlay_text_in_rect};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Semantic accent color for a [`Toast`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ToastTone;
/// assert_eq!(ToastTone::default(), ToastTone::Neutral);
/// ```
pub enum ToastTone {
    /// De-emphasized neutral accent.
    #[default]
    Neutral,
    /// Successful outcome.
    Success,
    /// Warning outcome.
    Warning,
    /// Failed or destructive outcome.
    Danger,
    /// Informational outcome.
    Info,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Corner from which a [`ToastHost`] stacks notifications.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ToastPosition;
/// assert_eq!(ToastPosition::default(), ToastPosition::TopRight);
/// ```
pub enum ToastPosition {
    /// Stack downward from the top-left inset.
    TopLeft,
    /// Stack downward from the top-right inset.
    #[default]
    TopRight,
    /// Stack upward from the bottom-left inset.
    BottomLeft,
    /// Stack upward from the bottom-right inset.
    BottomRight,
}

#[derive(Clone, Debug, PartialEq)]
/// Resolved toast colors, typography, and logical-pixel geometry.
///
/// Toast heights are fixed at 52 pixels without a description and 72 pixels
/// with one. `padding_y` is retained for style compatibility but those fixed
/// vertical offsets currently do not read it.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::ToastStyle;
/// let style = ToastStyle::from_theme(Theme::dark());
/// assert_eq!(style.width, 330.0);
/// assert_eq!(style.inset, 18.0);
/// ```
pub struct ToastStyle {
    /// Card fill.
    pub background: Color,
    /// Card border.
    pub border: Border,
    /// Card shadows; inset entries are skipped.
    pub shadows: Vec<BoxShadow>,
    /// Title text style.
    pub title_text: TextStyle,
    /// Description text style.
    pub description_text: TextStyle,
    /// Close-icon tint before the fixed 0.82 opacity multiplier.
    pub close_tint: Color,
    /// Neutral tone-strip and icon color.
    pub neutral: Color,
    /// Success tone-strip and icon color.
    pub success: Color,
    /// Warning tone-strip and icon color.
    pub warning: Color,
    /// Danger tone-strip and icon color.
    pub danger: Color,
    /// Info tone-strip and icon color.
    pub info: Color,
    /// Card corner radii.
    pub radius: Radius,
    /// Toast width in logical pixels.
    pub width: f32,
    /// Horizontal text/icon inset in logical pixels.
    pub padding_x: f32,
    /// Reserved vertical padding; fixed toast offsets currently ignore it.
    pub padding_y: f32,
    /// Gap between leading icon/text and text/close regions.
    pub gap: f32,
    /// Leading icon width and height.
    pub icon_size: f32,
    /// Close hit/icon width and height.
    pub close_size: f32,
    /// Gap between stacked toast rectangles.
    pub stack_gap: f32,
    /// Distance from the selected host edges.
    pub inset: f32,
}

impl Default for ToastStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl ToastStyle {
    /// Resolves toast colors, typography, and geometry from `theme`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::ToastStyle;
    /// let style = ToastStyle::from_theme(Theme::dark());
    /// assert_eq!(style.icon_size, 16.0);
    /// assert_eq!(style.stack_gap, 10.0);
    /// ```
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        Self {
            background: palette.surface_elevated,
            border: Border::new(1.0, palette.border),
            shadows: vec![theme.shadows().md],
            title_text: TextStyle::new(FontId::Ui, 13, palette.text),
            description_text: TextStyle::new(FontId::Ui, 12, palette.text_muted),
            close_tint: palette.text_muted,
            neutral: palette.text_muted,
            success: palette.success,
            warning: palette.warning,
            danger: palette.danger,
            info: palette.info,
            radius: Radius::uniform(theme.radius().lg),
            width: 330.0,
            padding_x: 12.0,
            padding_y: 10.0,
            gap: 8.0,
            icon_size: 16.0,
            close_size: 16.0,
            stack_gap: 10.0,
            inset: 18.0,
        }
    }

    /// Returns the configured accent color for `tone`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ToastStyle, ToastTone};
    /// let style = ToastStyle::default();
    /// assert_eq!(style.tone_color(ToastTone::Success), style.success);
    /// ```
    pub fn tone_color(&self, tone: ToastTone) -> Color {
        match tone {
            ToastTone::Neutral => self.neutral,
            ToastTone::Success => self.success,
            ToastTone::Warning => self.warning,
            ToastTone::Danger => self.danger,
            ToastTone::Info => self.info,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
/// Owned data for one overlay notification.
///
/// IDs route dismissal and should be unique. Duplicate IDs are accepted; a
/// bound-host dismissal removes every matching value while invoking its callback
/// once. `Some("")` description still selects the taller 72-pixel layout.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{Toast, ToastTone};
/// let toast = Toast::new("saved", "Saved").tone(ToastTone::Success);
/// assert_eq!(toast.id(), "saved");
/// ```
pub struct Toast {
    /// Dismissal identity.
    id: String,
    /// Primary text.
    title: String,
    /// Optional secondary line; presence controls height even when empty.
    description: Option<String>,
    /// Semantic accent tone.
    tone: ToastTone,
    /// Optional tone-tinted leading icon.
    leading_icon: Option<IconId>,
    /// Whether a pointer-accessible close affordance is painted.
    closable: bool,
}

impl Toast {
    /// Creates a neutral, closable toast with no description or icon.
    ///
    /// Empty and duplicate IDs are accepted but unique stable IDs are recommended.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Toast;
    /// let toast = Toast::new("connected", "Connected");
    /// assert_eq!(toast.title(), "Connected");
    /// ```
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
            tone: ToastTone::Neutral,
            leading_icon: None,
            closable: true,
        }
    }

    /// Borrows the exact dismissal identity.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Toast;
    /// let toast = Toast::new("job-1", "Started");
    /// assert_eq!(toast.id(), "job-1");
    /// ```
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Borrows the exact primary title.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Toast;
    /// let toast = Toast::new("job-1", "Started");
    /// assert_eq!(toast.title(), "Started");
    /// ```
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Sets the secondary line, replacing any previous description.
    ///
    /// An empty string remains `Some` and selects the taller toast layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Toast;
    /// let toast = Toast::new("saved", "Saved").description("All changes are on disk");
    /// let _ = toast;
    /// ```
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the semantic accent tone.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Toast, ToastTone};
    /// let toast = Toast::new("failed", "Failed").tone(ToastTone::Danger);
    /// let _ = toast;
    /// ```
    pub fn tone(mut self, tone: ToastTone) -> Self {
        self.tone = tone;
        self
    }

    /// Sets a tone-tinted leading icon, replacing any prior icon.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::Toast;
    /// let toast = Toast::new("done", "Done").leading_icon(IconId::Check);
    /// let _ = toast;
    /// ```
    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    /// Controls whether the close icon, hit bound, and dismiss path are present.
    ///
    /// Non-closable toasts can still be removed by changing the host's source.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Toast;
    /// let toast = Toast::new("sync", "Syncing").closable(false);
    /// let _ = toast;
    /// ```
    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }
}

/// Shared callback receiving the dismissed toast ID.
type ToastDismissHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, String)>;

/// A full-slot host that overlays a stack of toasts over one optional child.
///
/// The host itself is never focusable. Close activation is pointer-only and
/// checks visible values in reverse order. Bound mode removes all matching IDs;
/// controlled/static mode reports dismissal but cannot mutate its source.
/// Toasts are not clipped or virtualized and may extend outside a small host.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{Toast, ToastHost};
/// let host: ToastHost<()> = ToastHost::new().toast(Toast::new("saved", "Saved"));
/// let _ = host;
/// ```
pub struct ToastHost<A = ()> {
    /// Layout configuration for the full host slot.
    pub(crate) layout: LayoutStyle,
    /// Flex-item behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Static, controlled, or bound toast values in stack order.
    toasts: Binding<Vec<Toast>>,
    /// Writable source in bound mode only.
    bound_toasts: Option<Signal<Vec<Toast>>>,
    /// Corner and stack direction.
    position: ToastPosition,
    /// Resolved paint and geometry.
    style: ToastStyle,
    /// Optional dismissal callback.
    on_dismiss: Option<ToastDismissHandler<A>>,
    /// Optional sole host child below the overlay.
    child: Option<View<A>>,
}

crate::impl_layout_builders!(ToastHost);

impl<A: 'static> Default for ToastHost<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> ToastHost<A> {
    /// Creates an empty top-right host with no child or dismissal callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ToastHost;
    /// let host: ToastHost<()> = ToastHost::new();
    /// let _ = host;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            toasts: Binding::Static(Vec::new()),
            bound_toasts: None,
            position: ToastPosition::TopRight,
            style: ToastStyle::default(),
            on_dismiss: None,
            child: None,
        }
    }

    /// Appends one toast to a snapshot of the current binding.
    ///
    /// This converts the result to static mode and clears a writable binding.
    /// Repeated calls preserve order and grow an unbounded vector.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Toast, ToastHost};
    /// let host: ToastHost<()> = ToastHost::new().toast(Toast::new("one", "First"));
    /// let _ = host;
    /// ```
    pub fn toast(mut self, toast: Toast) -> Self {
        let mut toasts = self.toasts.read();
        toasts.push(toast);
        self.toasts = Binding::Static(toasts);
        self.bound_toasts = None;
        self
    }

    /// Replaces the static or reactive controlled toast list.
    ///
    /// This clears bound mode, so close activation cannot mutate the source.
    /// The callback can be used to ask the consumer to update it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Toast, ToastHost};
    /// let host: ToastHost<()> = ToastHost::new().toasts(vec![Toast::new("one", "First")]);
    /// let _ = host;
    /// ```
    pub fn toasts(mut self, toasts: impl Into<Binding<Vec<Toast>>>) -> Self {
        self.toasts = toasts.into();
        self.bound_toasts = None;
        self
    }

    /// Installs a writable signal for two-way dismissal.
    ///
    /// Close activation removes every toast whose ID equals the activated ID,
    /// then invokes the optional callback once.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// use ailloli_ui_widgets::controls::{Toast, ToastHost};
    /// let toasts = Signal::new(Rc::new(RefCell::new(vec![Toast::new("one", "First")])), Rc::new(|| {}));
    /// let host: ToastHost<()> = ToastHost::new().bind_toasts(toasts);
    /// let _ = host;
    /// ```
    pub fn bind_toasts(mut self, toasts: impl Into<Signal<Vec<Toast>>>) -> Self {
        let signal = toasts.into();
        self.toasts = Binding::Signal(signal.clone());
        self.bound_toasts = Some(signal);
        self
    }

    /// Selects the corner and vertical stacking direction.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ToastHost, ToastPosition};
    /// let host: ToastHost<()> = ToastHost::new().position(ToastPosition::BottomLeft);
    /// let _ = host;
    /// ```
    pub fn position(mut self, position: ToastPosition) -> Self {
        self.position = position;
        self
    }

    /// Replaces complete toast-stack style and geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{ToastHost, ToastStyle};
    /// let style = ToastStyle::from_theme(Theme::dark());
    /// let host: ToastHost<()> = ToastHost::new().toast_style(style);
    /// let _ = host;
    /// ```
    pub fn toast_style(mut self, style: ToastStyle) -> Self {
        self.style = style;
        self
    }

    /// Maps each dismissed ID to an application action and dispatches it.
    ///
    /// The mapper runs after bound values are removed. A later handler replaces it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ToastHost;
    /// enum Action { Dismissed(String) }
    /// let host = ToastHost::new().on_dismiss(Action::Dismissed);
    /// let _ = host;
    /// ```
    pub fn on_dismiss(mut self, f: impl Fn(String) -> A + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(move |ctx, id| ctx.dispatch(f(id))));
        self
    }

    /// Installs a context-aware dismissal callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ToastHost;
    /// let host = ToastHost::<()>::new().on_dismiss_ctx(|ctx, _id| ctx.request_repaint());
    /// let _ = host;
    /// ```
    pub fn on_dismiss_ctx(mut self, f: impl Fn(&mut EventCtx<A>, String) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(f));
        self
    }

    /// Sets the sole underlying host child, replacing any previous child.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ToastHost;
    /// use ailloli_ui_widgets::text::Text;
    /// let host = ToastHost::<()>::new().child(Text::new("Workspace"));
    /// let _ = host;
    /// ```
    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.child = Some(child.into_view());
        self
    }
}

/// Component properties used to build the overlay host node.
struct ToastHostComponent<A> {
    /// Outer logical sizing policy for the underlying host child.
    layout: LayoutStyle,
    /// Readable ordered toast collection.
    toasts: Binding<Vec<Toast>>,
    /// Optional writable collection used for built-in dismissal.
    bound_toasts: Option<Signal<Vec<Toast>>>,
    /// Viewport corner or edge used to stack toast overlays.
    position: ToastPosition,
    /// Toast colors, spacing, and logical-pixel geometry.
    style: ToastStyle,
    /// Optional callback receiving dismissed toast identities.
    on_dismiss: Option<ToastDismissHandler<A>>,
    /// Optional underlying host content painted below overlays.
    child: Option<View<A>>,
}

impl<A: 'static> ComponentNode<A> for ToastHostComponent<A> {
    fn build(&self, _context: &mut Context<A>) -> View<A> {
        let mut children = Vec::new();
        if let Some(child) = self.child.clone() {
            children.push(child);
        }
        View::node(
            ToastHostWidget {
                layout: self.layout,
                toasts: self.toasts.clone(),
                bound_toasts: self.bound_toasts.clone(),
                position: self.position,
                style: self.style.clone(),
                on_dismiss: self.on_dismiss.clone(),
            },
            children,
        )
    }
}

impl<A: 'static> IntoView<A> for ToastHost<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(ToastHostComponent {
                layout: self.layout,
                toasts: self.toasts,
                bound_toasts: self.bound_toasts,
                position: self.position,
                style: self.style,
                on_dismiss: self.on_dismiss,
                child: self.child,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Retained overlay widget resolving stack geometry and close activation.
struct ToastHostWidget<A> {
    /// Outer logical sizing policy for the underlying host child.
    layout: LayoutStyle,
    /// Readable ordered toast collection.
    toasts: Binding<Vec<Toast>>,
    /// Optional writable collection used for built-in dismissal.
    bound_toasts: Option<Signal<Vec<Toast>>>,
    /// Viewport corner or edge used to stack toast overlays.
    position: ToastPosition,
    /// Toast colors, spacing, and logical-pixel geometry.
    style: ToastStyle,
    /// Optional retained dismissal callback.
    on_dismiss: Option<ToastDismissHandler<A>>,
}

impl<A: 'static> Widget<A> for ToastHostWidget<A> {
    fn debug_name(&self) -> &'static str {
        "ToastHost"
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
        let overlay_hit_bounds = self
            .toast_rects(size)
            .into_iter()
            .filter_map(|(_, toast, rect)| toast.closable.then_some(self.close_rect(rect)))
            .collect();

        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds,
            visual_bounds: paint_bounds,
            overlay_hit_bounds,
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        for (_, toast, rect) in self.toast_rects(bounds.size()).into_iter() {
            self.paint_toast(ctx, rect.translate(Offset::new(bounds.x, bounds.y)), &toast);
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        let Event::Pointer(PointerEvent::Button {
            pos,
            button: MouseButton::Left,
            pressed: false,
            ..
        }) = event
        else {
            return;
        };

        for (_, toast, rect) in self.toast_rects(bounds.size()).into_iter().rev() {
            let rect = rect.translate(Offset::new(bounds.x, bounds.y));
            if toast.closable && self.close_rect(rect).contains(pos.x, pos.y) {
                self.dismiss(ctx, toast.id.clone());
                ctx.stop_propagation();
                return;
            }
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }
}

impl<A: 'static> ToastHostWidget<A> {
    /// Resolves all toast rectangles in source order without clipping.
    fn toast_rects(&self, host_size: Size) -> Vec<(usize, Toast, Rect)> {
        let mut y = match self.position {
            ToastPosition::TopLeft | ToastPosition::TopRight => self.style.inset,
            ToastPosition::BottomLeft | ToastPosition::BottomRight => {
                host_size.h - self.style.inset
            }
        };
        let mut out = Vec::new();
        for (idx, toast) in self.toasts.read().into_iter().enumerate() {
            let h = self.toast_height(&toast);
            if matches!(
                self.position,
                ToastPosition::BottomLeft | ToastPosition::BottomRight
            ) {
                y -= h;
            }
            let x = match self.position {
                ToastPosition::TopLeft | ToastPosition::BottomLeft => self.style.inset,
                ToastPosition::TopRight | ToastPosition::BottomRight => {
                    host_size.w - self.style.inset - self.style.width
                }
            }
            .max(self.style.inset);
            out.push((idx, toast, Rect::new(x, y, self.style.width, h)));
            if matches!(
                self.position,
                ToastPosition::TopLeft | ToastPosition::TopRight
            ) {
                y += h + self.style.stack_gap;
            } else {
                y -= self.style.stack_gap;
            }
        }
        out
    }

    /// Returns 72 pixels when a description is present, otherwise 52.
    fn toast_height(&self, toast: &Toast) -> f32 {
        if toast.description.is_some() {
            72.0
        } else {
            52.0
        }
    }

    /// Returns the close hit/icon square aligned to the toast's right inset.
    fn close_rect(&self, rect: Rect) -> Rect {
        Rect::new(
            rect.right() - self.style.padding_x - self.style.close_size,
            rect.y + (rect.h - self.style.close_size) * 0.5,
            self.style.close_size,
            self.style.close_size,
        )
    }

    /// Removes matching bound values, reports once, and requests repaint.
    fn dismiss(&self, ctx: &mut EventCtx<A>, id: String) {
        if let Some(bound) = &self.bound_toasts {
            bound.update(|toasts| toasts.retain(|toast| toast.id != id));
        }
        if let Some(on_dismiss) = &self.on_dismiss {
            on_dismiss(ctx, id);
        }
        ctx.request_repaint();
    }

    /// Paints one card, four-pixel tone strip, content, close icon, and border.
    fn paint_toast(&self, ctx: &mut PaintCtx<'_>, rect: Rect, toast: &Toast) {
        for shadow in self.style.shadows.iter().copied().filter(|s| !s.inset) {
            ctx.push_overlay(DrawCmd::BoxShadow(DrawBoxShadow {
                rect,
                radius: self.style.radius,
                shadow,
            }));
        }
        ctx.push_overlay(DrawCmd::RRect(DrawRRect {
            rect,
            radius: self.style.radius.tl,
            color: self.style.background,
        }));

        let tone = self.style.tone_color(toast.tone);
        ctx.push_overlay(DrawCmd::RRect(DrawRRect {
            rect: Rect::new(rect.x, rect.y, 4.0, rect.h),
            radius: self.style.radius.tl.min(4.0),
            color: tone,
        }));

        let mut x = rect.x + self.style.padding_x + 4.0;
        if let Some(icon) = &toast.leading_icon {
            ctx.push_overlay(DrawCmd::Image(DrawImage {
                rect: Rect::new(
                    x,
                    rect.y + (rect.h - self.style.icon_size) * 0.5,
                    self.style.icon_size,
                    self.style.icon_size,
                ),
                icon: icon.clone(),
                tint: tone,
                rotation_rad: 0.0,
            }));
            x += self.style.icon_size + self.style.gap;
        }

        let right = if toast.closable {
            self.close_rect(rect).x - self.style.gap
        } else {
            rect.right() - self.style.padding_x
        };
        let text_rect = Rect::new(x, rect.y + 7.0, (right - x).max(0.0), 24.0);
        paint_overlay_text_in_rect(ctx, &toast.title, self.style.title_text, text_rect, 1.0);
        if let Some(description) = &toast.description {
            let desc = Rect::new(x, rect.y + 34.0, (right - x).max(0.0), 22.0);
            paint_overlay_text_in_rect(ctx, description, self.style.description_text, desc, 1.0);
        }

        if toast.closable {
            ctx.push_overlay(DrawCmd::Image(DrawImage {
                rect: self.close_rect(rect),
                icon: IconId::Close,
                tint: apply_opacity(self.style.close_tint, 0.82),
                rotation_rad: 0.0,
            }));
        }

        if self.style.border.is_visible() {
            ctx.push_overlay(DrawCmd::Border(DrawBorder {
                rect,
                radius: self.style.radius,
                border: self.style.border,
            }));
        }
    }
}

/// Resolves host size from its child or finite constraint maxima.
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

/// Local geometry extension used to pass host dimensions to stack layout.
trait RectExt {
    /// Copies rectangle width and height into a [`Size`].
    fn size(self) -> Size;
}

impl RectExt for Rect {
    fn size(self) -> Size {
        Size::new(self.w, self.h)
    }
}
