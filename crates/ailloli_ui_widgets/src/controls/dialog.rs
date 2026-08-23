//! Modal confirmation dialogs painted over an optional host child.

use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{
    AlignItems, Border, BoxShadow, FlexItemStyle, JustifyContent, LayoutSizeHint, LayoutStyle,
    Radius,
};
use ailloli_ui_core::{Color, FontId, Offset, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{ClickAction, EventCtx, FocusPolicy, IntoClickAction};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawBoxShadow, DrawCmd, DrawRRect, DrawRect};

use ailloli_ui_text::WrapMode;

use super::popup::{
    paint_overlay_text_in_rect, paint_overlay_text_in_rect_aligned, OverlayTextOptions,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Semantic treatment for a dialog's confirm action.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::DialogTone;
/// assert_eq!(DialogTone::default(), DialogTone::Neutral);
/// ```
pub enum DialogTone {
    /// Accent-colored primary confirmation.
    #[default]
    Neutral,
    /// Danger-colored confirmation for destructive operations.
    Danger,
}

#[derive(Clone, Debug, PartialEq)]
/// Resolved paint, typography, and logical-pixel geometry for a [`Dialog`].
///
/// `primary_background_pressed`, `cancel_background_pressed`, and
/// `danger_background_pressed` are retained for style compatibility but the
/// current overlay painter does not yet resolve per-button pressed state.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::{DialogStyle, DialogTone};
/// let style = DialogStyle::from_theme(Theme::dark(), DialogTone::Neutral);
/// assert_eq!(style.panel_width, 360.0);
/// assert_eq!(style.button_height, 34.0);
/// ```
pub struct DialogStyle {
    /// Full-host overlay fill.
    pub backdrop: Color,
    /// Panel fill.
    pub panel_background: Color,
    /// Panel border.
    pub border: Border,
    /// Panel shadows; inset entries are skipped by the current painter.
    pub shadows: Vec<BoxShadow>,
    /// Title text style.
    pub title_text: TextStyle,
    /// Wrapping body text style.
    pub body_text: TextStyle,
    /// Shared cancel and confirm label style.
    pub button_text: TextStyle,
    /// Neutral confirm-button fill.
    pub primary_background: Color,
    /// Reserved neutral pressed fill; currently not painted.
    pub primary_background_pressed: Color,
    /// Cancel-button fill.
    pub cancel_background: Color,
    /// Reserved cancel pressed fill; currently not painted.
    pub cancel_background_pressed: Color,
    /// Danger confirm-button fill.
    pub danger_background: Color,
    /// Reserved danger pressed fill; currently not painted.
    pub danger_background_pressed: Color,
    /// Shared button border.
    pub button_border: Border,
    /// Panel corner radii.
    pub radius: Radius,
    /// Button corner radii.
    pub button_radius: Radius,
    /// Preferred panel width in logical pixels.
    pub panel_width: f32,
    /// Minimum panel height in logical pixels.
    pub panel_min_height: f32,
    /// Panel and button edge inset in logical pixels.
    pub padding: f32,
    /// Cancel and confirm button height in logical pixels.
    pub button_height: f32,
    /// Width of each button in logical pixels.
    pub button_width: f32,
    /// Horizontal gap between cancel and confirm buttons.
    pub gap: f32,
}

impl Default for DialogStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), DialogTone::Neutral)
    }
}

impl DialogStyle {
    /// Resolves `tone` through `theme` into complete dialog style values.
    ///
    /// Tone affects the primary and primary-pressed colors; danger-specific
    /// fields are populated for either tone.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{DialogStyle, DialogTone};
    /// let neutral = DialogStyle::from_theme(Theme::dark(), DialogTone::Neutral);
    /// let danger = DialogStyle::from_theme(Theme::dark(), DialogTone::Danger);
    /// assert_ne!(neutral.primary_background, danger.primary_background);
    /// ```
    pub fn from_theme(theme: Theme, tone: DialogTone) -> Self {
        let palette = theme.palette();
        let danger = palette.danger;
        Self {
            backdrop: Color::BLACK.with_alpha(0.56),
            panel_background: palette.surface_elevated,
            border: Border::new(1.0, palette.border),
            shadows: vec![theme.shadows().lg],
            title_text: TextStyle::new(FontId::Ui, 16, palette.text),
            body_text: TextStyle::new(FontId::Ui, 13, palette.text_muted),
            button_text: TextStyle::new(FontId::Ui, 13, palette.text),
            primary_background: match tone {
                DialogTone::Neutral => palette.accent,
                DialogTone::Danger => danger,
            },
            primary_background_pressed: match tone {
                DialogTone::Neutral => Color::hex_rgb(0xD94800),
                DialogTone::Danger => Color::hex_rgb(0xB91C1C),
            },
            cancel_background: palette.surface,
            cancel_background_pressed: Color::hex_rgb(0x20252A),
            danger_background: danger,
            danger_background_pressed: Color::hex_rgb(0xB91C1C),
            button_border: Border::new(1.0, palette.border),
            radius: Radius::uniform(theme.radius().lg),
            button_radius: Radius::uniform(theme.radius().md),
            panel_width: 360.0,
            panel_min_height: 184.0,
            padding: 18.0,
            button_height: 34.0,
            button_width: 96.0,
            gap: 10.0,
        }
    }
}

/// A modal confirmation overlay with controlled, bound, or internal visibility.
///
/// Confirm activation runs the confirm action and requests close. Cancel-button,
/// backdrop, and Escape activation run the cancel action and request close.
/// In controlled mode, close cannot mutate the supplied binding; the consumer
/// must update it. Bound mode writes `false`, and internal mode updates retained
/// state. Disabled state hides the dialog without changing visibility state.
/// The optional child remains the underlying host content.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::Dialog;
/// let dialog: Dialog<()> = Dialog::new().title("Unsaved changes?").default_open(true);
/// let _ = dialog;
/// ```
pub struct Dialog<A = ()> {
    /// Layout configuration for the full host slot.
    pub(crate) layout: LayoutStyle,
    /// Flex-item behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Optional controlled or bound visibility source.
    open: Option<Binding<bool>>,
    /// Writable visibility signal in bound mode only.
    bound_open: Option<Signal<bool>>,
    /// Initial internal visibility used only without an external binding.
    default_open: bool,
    /// Live disabled state; disabled open dialogs are hidden but remain open.
    disabled: Binding<bool>,
    /// Live title text.
    title: Binding<String>,
    /// Live wrapping body text.
    body: Binding<String>,
    /// Live confirm-button label.
    confirm_label: Binding<String>,
    /// Live cancel-button label.
    cancel_label: Binding<String>,
    /// Semantic confirm treatment.
    tone: DialogTone,
    /// Resolved paint and geometry.
    style: DialogStyle,
    /// Optional confirm action.
    on_confirm: Option<Rc<ClickAction<A>>>,
    /// Optional cancellation action.
    on_cancel: Option<Rc<ClickAction<A>>>,
    /// Optional sole host child painted under the overlay.
    child: Option<View<A>>,
}

crate::impl_layout_builders!(Dialog);

impl<A: 'static> Default for Dialog<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Dialog<A> {
    /// Creates a closed, enabled, internally managed neutral dialog.
    ///
    /// Default labels are `"Dialog"`, `"Confirm"`, and `"Cancel"`; body text is
    /// empty and no actions or host child are installed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Dialog;
    /// let dialog: Dialog<()> = Dialog::new();
    /// let _ = dialog;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            open: None,
            bound_open: None,
            default_open: false,
            disabled: Binding::Static(false),
            title: Binding::Static("Dialog".to_string()),
            body: Binding::Static(String::new()),
            confirm_label: Binding::Static("Confirm".to_string()),
            cancel_label: Binding::Static("Cancel".to_string()),
            tone: DialogTone::Neutral,
            style: DialogStyle::default(),
            on_confirm: None,
            on_cancel: None,
            child: None,
        }
    }

    /// Sets controlled static or reactive visibility.
    ///
    /// This clears writable bound mode. Confirm/cancel still run and consume
    /// their input, but the dialog remains open until the external source reads
    /// `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Dialog;
    /// let dialog: Dialog<()> = Dialog::new().open(true);
    /// let _ = dialog;
    /// ```
    pub fn open(mut self, open: impl Into<Binding<bool>>) -> Self {
        self.open = Some(open.into());
        self.bound_open = None;
        self
    }

    /// Installs a writable visibility signal for two-way close behavior.
    ///
    /// Confirm, cancel, backdrop, and Escape paths set the signal to `false`.
    /// A later [`Self::open`] call returns to controlled mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// use ailloli_ui_widgets::controls::Dialog;
    /// let open = Signal::new(Rc::new(RefCell::new(true)), Rc::new(|| {}));
    /// let dialog: Dialog<()> = Dialog::new().bind_open(open);
    /// let _ = dialog;
    /// ```
    pub fn bind_open(mut self, open: impl Into<Signal<bool>>) -> Self {
        let signal = open.into();
        self.open = Some(Binding::Signal(signal.clone()));
        self.bound_open = Some(signal);
        self
    }

    /// Sets initial retained visibility for internal mode.
    ///
    /// This value is ignored when [`Self::open`] or [`Self::bind_open`] supplies
    /// external visibility. It initializes state during first component build;
    /// it is not a recurring reopen command.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Dialog;
    /// let dialog: Dialog<()> = Dialog::new().default_open(true);
    /// let _ = dialog;
    /// ```
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Sets a static or reactive disabled binding.
    ///
    /// Disabled state hides overlay paint/hit bounds, ignores events, and
    /// removes focusability without closing or resetting visibility state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Dialog;
    /// let dialog: Dialog<()> = Dialog::new().open(true).disabled(true);
    /// let _ = dialog;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Replaces static or reactive title text.
    ///
    /// Empty text is valid and paints no glyphs while retaining title geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Dialog;
    /// let dialog: Dialog<()> = Dialog::new().title("Delete project?");
    /// let _ = dialog;
    /// ```
    pub fn title(mut self, title: impl Into<Binding<String>>) -> Self {
        self.title = title.into();
        self
    }

    /// Replaces static or reactive wrapping body text.
    ///
    /// Empty body reserves one nominal line; any non-empty body reserves two
    /// nominal lines before layout constraints, independently of actual wraps.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Dialog;
    /// let dialog: Dialog<()> = Dialog::new().body("This operation cannot be undone.");
    /// let _ = dialog;
    /// ```
    pub fn body(mut self, body: impl Into<Binding<String>>) -> Self {
        self.body = body.into();
        self
    }

    /// Replaces static or reactive confirm-button text.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Dialog;
    /// let dialog: Dialog<()> = Dialog::new().confirm_label("Delete");
    /// let _ = dialog;
    /// ```
    pub fn confirm_label(mut self, label: impl Into<Binding<String>>) -> Self {
        self.confirm_label = label.into();
        self
    }

    /// Replaces static or reactive cancel-button text.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Dialog;
    /// let dialog: Dialog<()> = Dialog::new().cancel_label("Keep editing");
    /// let _ = dialog;
    /// ```
    pub fn cancel_label(mut self, label: impl Into<Binding<String>>) -> Self {
        self.cancel_label = label.into();
        self
    }

    /// Sets semantic tone and replaces the complete style from the default theme.
    ///
    /// This discards custom style values installed earlier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Dialog, DialogTone};
    /// let dialog: Dialog<()> = Dialog::new().tone(DialogTone::Danger);
    /// let _ = dialog;
    /// ```
    pub fn tone(mut self, tone: DialogTone) -> Self {
        self.tone = tone;
        self.style = DialogStyle::from_theme(Theme::default(), tone);
        self
    }

    /// Replaces the complete resolved style without changing semantic tone.
    ///
    /// A later [`Self::tone`] call replaces this custom style. Geometry values
    /// are otherwise accepted as-is.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{Dialog, DialogStyle, DialogTone};
    /// let style = DialogStyle::from_theme(Theme::dark(), DialogTone::Neutral);
    /// let dialog: Dialog<()> = Dialog::new().dialog_style(style);
    /// let _ = dialog;
    /// ```
    pub fn dialog_style(mut self, style: DialogStyle) -> Self {
        self.style = style;
        self
    }

    /// Installs an action run before a confirm close request.
    ///
    /// A later call replaces it. Clicking confirm closes even without an action.
    /// Keyboard Enter does not currently synthesize confirmation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Dialog;
    /// #[derive(Clone)]
    /// enum Action { Confirm }
    /// let dialog = Dialog::new().on_confirm(Action::Confirm);
    /// let _ = dialog;
    /// ```
    pub fn on_confirm(mut self, action: impl IntoClickAction<A>) -> Self {
        self.on_confirm = Some(Rc::new(action.into_click_action()));
        self
    }

    /// Installs an action run before cancellation closes are requested.
    ///
    /// The action covers cancel-button, backdrop, and Escape paths. A later call
    /// replaces it; cancellation closes even without an action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Dialog;
    /// #[derive(Clone)]
    /// enum Action { Cancel }
    /// let dialog = Dialog::new().on_cancel(Action::Cancel);
    /// let _ = dialog;
    /// ```
    pub fn on_cancel(mut self, action: impl IntoClickAction<A>) -> Self {
        self.on_cancel = Some(Rc::new(action.into_click_action()));
        self
    }

    /// Sets the sole underlying host child, replacing any previous child.
    ///
    /// The child fills the resolved host slot and remains painted while the
    /// dialog overlay is open.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Dialog;
    /// use ailloli_ui_widgets::text::Text;
    /// let dialog = Dialog::<()>::new().child(Text::new("Workspace"));
    /// let _ = dialog;
    /// ```
    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.child = Some(child.into_view());
        self
    }
}

/// Component properties used to allocate internal visibility state.
struct DialogComponent<A> {
    /// Outer logical sizing policy for the underlying host child.
    layout: LayoutStyle,
    /// Optional readable external open state.
    open: Option<Binding<bool>>,
    /// Optional writable external open state.
    bound_open: Option<Signal<bool>>,
    /// Initial open state used by an uncontrolled dialog.
    default_open: bool,
    /// Reactive interaction-disable flag.
    disabled: Binding<bool>,
    /// Reactive user-visible dialog title.
    title: Binding<String>,
    /// Reactive user-visible dialog body.
    body: Binding<String>,
    /// Reactive confirm-button label.
    confirm_label: Binding<String>,
    /// Reactive cancel-button label.
    cancel_label: Binding<String>,
    /// Semantic color tone for the confirm action.
    tone: DialogTone,
    /// Surface, backdrop, and logical-pixel geometry.
    style: DialogStyle,
    /// Optional confirm action shared by pointer and keyboard activation.
    on_confirm: Option<Rc<ClickAction<A>>>,
    /// Optional cancel action shared by button, Escape, and outside dismissal.
    on_cancel: Option<Rc<ClickAction<A>>>,
    /// Optional underlying host content painted below the modal overlay.
    child: Option<View<A>>,
}

impl<A: 'static> ComponentNode<A> for DialogComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let mut children = Vec::new();
        if let Some(child) = self.child.clone() {
            children.push(child);
        }
        View::node(
            DialogWidget {
                layout: self.layout,
                open: self.open.clone(),
                bound_open: self.bound_open.clone(),
                internal_open: context.signal(self.default_open),
                disabled: self.disabled.clone(),
                title: self.title.clone(),
                body: self.body.clone(),
                confirm_label: self.confirm_label.clone(),
                cancel_label: self.cancel_label.clone(),
                tone: self.tone,
                style: self.style.clone(),
                on_confirm: self.on_confirm.clone(),
                on_cancel: self.on_cancel.clone(),
            },
            children,
        )
    }
}

impl<A: 'static> IntoView<A> for Dialog<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(DialogComponent {
                layout: self.layout,
                open: self.open,
                bound_open: self.bound_open,
                default_open: self.default_open,
                disabled: self.disabled,
                title: self.title,
                body: self.body,
                confirm_label: self.confirm_label,
                cancel_label: self.cancel_label,
                tone: self.tone,
                style: self.style,
                on_confirm: self.on_confirm,
                on_cancel: self.on_cancel,
                child: self.child,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Retained overlay widget resolving visibility and modal event paths.
struct DialogWidget<A> {
    /// Outer logical sizing policy for the underlying host child.
    layout: LayoutStyle,
    /// Optional readable external open state.
    open: Option<Binding<bool>>,
    /// Optional writable external open state.
    bound_open: Option<Signal<bool>>,
    /// Retained open state used by an uncontrolled dialog.
    internal_open: Signal<bool>,
    /// Reactive interaction-disable flag.
    disabled: Binding<bool>,
    /// Reactive user-visible dialog title.
    title: Binding<String>,
    /// Reactive user-visible dialog body.
    body: Binding<String>,
    /// Reactive confirm-button label.
    confirm_label: Binding<String>,
    /// Reactive cancel-button label.
    cancel_label: Binding<String>,
    /// Semantic color tone for the confirm action.
    tone: DialogTone,
    /// Surface, backdrop, and logical-pixel geometry.
    style: DialogStyle,
    /// Optional retained confirm action.
    on_confirm: Option<Rc<ClickAction<A>>>,
    /// Optional retained cancel action.
    on_cancel: Option<Rc<ClickAction<A>>>,
}

impl<A: 'static> Widget<A> for DialogWidget<A> {
    fn debug_name(&self) -> &'static str {
        "Dialog"
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
        let overlay_hit_bounds = if self.is_open() && !self.disabled.read() {
            vec![paint_bounds]
        } else {
            Vec::new()
        };

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
        if !self.is_open() || self.disabled.read() {
            return;
        }
        self.paint_dialog(ctx, bounds);
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if !self.is_open() || self.disabled.read() {
            return;
        }

        match event {
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) => {
                let panel = self.panel_rect(bounds);
                if self.confirm_rect(panel).contains(pos.x, pos.y) {
                    if let Some(action) = &self.on_confirm {
                        action.run(ctx);
                    }
                    self.close();
                    ctx.request_repaint();
                    ctx.stop_propagation();
                } else if self.cancel_rect(panel).contains(pos.x, pos.y)
                    || !panel.contains(pos.x, pos.y)
                {
                    self.cancel(ctx);
                }
            }
            Event::Keyboard(key)
                if key.state == KeyState::Pressed
                    && matches!(
                        key.key,
                        ailloli_ui_core::event::Key::Named(NamedKey::Escape)
                    ) =>
            {
                self.cancel(ctx);
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        if self.is_open() && !self.disabled.read() {
            FocusPolicy::Focusable
        } else {
            FocusPolicy::NotFocusable
        }
    }
}

impl<A: 'static> DialogWidget<A> {
    /// Reads controlled/bound visibility, falling back to retained internal state.
    fn is_open(&self) -> bool {
        self.open
            .as_ref()
            .map(Binding::read)
            .unwrap_or_else(|| self.internal_open.read())
    }

    /// Writes `false` only in bound or internal mode.
    fn close(&self) {
        if let Some(bound) = &self.bound_open {
            bound.set(false);
        } else if self.open.is_none() {
            self.internal_open.set(false);
        }
    }

    /// Runs the optional cancel action, requests close/repaint, and consumes input.
    fn cancel(&self, ctx: &mut EventCtx<A>) {
        if let Some(action) = &self.on_cancel {
            action.run(ctx);
        }
        self.close();
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Centers the preferred panel, reserving 24 pixels per horizontal host edge.
    fn panel_rect(&self, bounds: Rect) -> Rect {
        let width = self.style.panel_width.min((bounds.w - 48.0).max(180.0));
        let body_lines = if self.body.read().is_empty() {
            1.0
        } else {
            2.0
        };
        let height = self
            .style
            .panel_min_height
            .max(self.style.padding * 2.0 + 28.0 + 18.0 * body_lines + 46.0);
        Rect::new(
            bounds.x + (bounds.w - width) * 0.5,
            bounds.y + (bounds.h - height) * 0.5,
            width,
            height,
        )
    }

    /// Places the confirm button at the panel's bottom-right padding inset.
    fn confirm_rect(&self, panel: Rect) -> Rect {
        Rect::new(
            panel.right() - self.style.padding - self.style.button_width,
            panel.bottom() - self.style.padding - self.style.button_height,
            self.style.button_width,
            self.style.button_height,
        )
    }

    /// Places the cancel button immediately left of confirm with the style gap.
    fn cancel_rect(&self, panel: Rect) -> Rect {
        let confirm = self.confirm_rect(panel);
        Rect::new(
            confirm.x - self.style.gap - self.style.button_width,
            confirm.y,
            self.style.button_width,
            self.style.button_height,
        )
    }

    /// Paints backdrop, non-inset shadows, panel, text, buttons, then border.
    fn paint_dialog(&self, ctx: &mut PaintCtx<'_>, bounds: Rect) {
        ctx.push_overlay(DrawCmd::Rect(DrawRect {
            rect: bounds,
            color: self.style.backdrop,
        }));

        let panel = self.panel_rect(bounds);
        for shadow in self.style.shadows.iter().copied().filter(|s| !s.inset) {
            ctx.push_overlay(DrawCmd::BoxShadow(DrawBoxShadow {
                rect: panel,
                radius: self.style.radius,
                shadow,
            }));
        }
        ctx.push_overlay(DrawCmd::RRect(DrawRRect {
            rect: panel,
            radius: self.style.radius.tl,
            color: self.style.panel_background,
        }));

        let title = Rect::new(
            panel.x + self.style.padding,
            panel.y + self.style.padding - 2.0,
            panel.w - self.style.padding * 2.0,
            28.0,
        );
        paint_overlay_text_in_rect(ctx, &self.title.read(), self.style.title_text, title, 1.0);

        let body_y = title.bottom() + 10.0;
        let button_top = self.cancel_rect(panel).y;
        let body = Rect::new(
            panel.x + self.style.padding,
            body_y,
            panel.w - self.style.padding * 2.0,
            (button_top - body_y - 10.0).max(0.0),
        );
        paint_overlay_text_in_rect_aligned(
            ctx,
            &self.body.read(),
            self.style.body_text,
            body,
            OverlayTextOptions {
                opacity: 1.0,
                wrap_mode: WrapMode::WordOrAnywhere,
                justify_content: JustifyContent::Start,
                align_items: AlignItems::Start,
            },
        );

        self.paint_button(
            ctx,
            self.cancel_rect(panel),
            &self.cancel_label.read(),
            self.style.cancel_background,
            self.style.cancel_background_pressed,
        );
        let confirm_bg = match self.tone {
            DialogTone::Neutral => self.style.primary_background,
            DialogTone::Danger => self.style.danger_background,
        };
        let confirm_pressed = match self.tone {
            DialogTone::Neutral => self.style.primary_background_pressed,
            DialogTone::Danger => self.style.danger_background_pressed,
        };
        self.paint_button(
            ctx,
            self.confirm_rect(panel),
            &self.confirm_label.read(),
            confirm_bg,
            confirm_pressed,
        );

        if self.style.border.is_visible() {
            ctx.push_overlay(DrawCmd::Border(DrawBorder {
                rect: panel,
                radius: self.style.radius,
                border: self.style.border,
            }));
        }
    }

    /// Paints one overlay button; the reserved pressed color is currently unused.
    fn paint_button(
        &self,
        ctx: &mut PaintCtx<'_>,
        rect: Rect,
        label: &str,
        color: Color,
        _pressed: Color,
    ) {
        ctx.push_overlay(DrawCmd::RRect(DrawRRect {
            rect,
            radius: self.style.button_radius.tl,
            color,
        }));
        if self.style.button_border.is_visible() {
            ctx.push_overlay(DrawCmd::Border(DrawBorder {
                rect,
                radius: self.style.button_radius,
                border: self.style.button_border,
            }));
        }
        paint_overlay_text_in_rect_aligned(
            ctx,
            label,
            self.style.button_text,
            rect,
            OverlayTextOptions {
                opacity: if color == self.style.cancel_background {
                    0.92
                } else {
                    1.0
                },
                wrap_mode: WrapMode::NoWrap,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
            },
        );
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
