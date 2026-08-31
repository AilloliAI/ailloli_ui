//! Standalone draggable split-resize handle and provider-neutral event model.

use std::rc::Rc;

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Point, Rect, Size};
use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{Color, Theme};
use ailloli_ui_runtime::component::{ComponentNode, Context, IntoView, Signal, View, Widget};
use ailloli_ui_runtime::input::{ActivationPolicy, EventCtx, HoverCursorRole};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawCmd, DrawRRect, Invalidation};

use super::layout_ext::{apply_layout_size, finish_view_sized};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Main axis changed by a resize handle.
///
/// `X` represents a vertical divider with horizontal movement; `Y` represents
/// a horizontal divider with vertical movement.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::input::HoverCursorRole;
/// use ailloli_ui_widgets::layout::ResizeAxis;
/// assert_eq!(ResizeAxis::X.cursor_role(), HoverCursorRole::ResizeX);
/// ```
pub enum ResizeAxis {
    /// Horizontal position changed by a vertical bar.
    X,
    /// Vertical position changed by a horizontal bar.
    Y,
}

impl ResizeAxis {
    /// Returns the matching horizontal or vertical resize cursor role.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::HoverCursorRole;
    /// use ailloli_ui_widgets::layout::ResizeAxis;
    /// assert_eq!(ResizeAxis::Y.cursor_role(), HoverCursorRole::ResizeY);
    /// ```
    pub fn cursor_role(self) -> HoverCursorRole {
        match self {
            Self::X => HoverCursorRole::ResizeX,
            Self::Y => HoverCursorRole::ResizeY,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Emitted pointer-drag lifecycle for a resize interaction.
///
/// Pointer cancellation clears internal state without emitting `End`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::layout::ResizeDragPhase;
/// assert_ne!(ResizeDragPhase::Start, ResizeDragPhase::End);
/// ```
pub enum ResizeDragPhase {
    /// Initial left-button press inside the bar.
    Start,
    /// Pointer movement while captured.
    Drag,
    /// Left-button release after capture.
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// Resize event expressed along one logical-pixel main axis.
///
/// `delta` is relative to the previous event; `total_delta` is relative to the
/// initial press. `position` is the current split or bar-local pointer position.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::layout::{ResizeAxis, ResizeDragPhase, SplitResizeEvent};
/// let event = SplitResizeEvent { axis: ResizeAxis::X, phase: ResizeDragPhase::Drag, position: 120.0, delta: 4.0, total_delta: 12.0 };
/// assert_eq!(event.total_delta, 12.0);
/// ```
pub struct SplitResizeEvent {
    /// Horizontal (`X`) or vertical (`Y`) split coordinate.
    pub axis: ResizeAxis,
    /// Start, incremental drag, or end phase.
    pub phase: ResizeDragPhase,
    /// Current split position, in logical pixels on the main axis.
    /// For standalone `ResizeBar`, this is the current main-axis pointer
    /// coordinate inside the bar bounds.
    pub position: f32,
    /// Delta since the previous emitted event, in logical pixels.
    pub delta: f32,
    /// Delta since drag start, in logical pixels.
    pub total_delta: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
/// Hit geometry, visible line geometry, and colors for [`ResizeBar`].
///
/// All dimensions are logical pixels and are consumed without global
/// normalization. Defaults use an 8-pixel hit target, a 2-pixel line, and a
/// 64-pixel intrinsic extent along the bar.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::layout::ResizeBarStyle;
/// let style = ResizeBarStyle::default();
/// assert_eq!(style.hit_thickness, 8.0);
/// assert_eq!(style.line_thickness, 2.0);
/// ```
pub struct ResizeBarStyle {
    /// Hit target thickness on the resize axis.
    pub hit_thickness: f32,
    /// Centered visible line thickness.
    pub line_thickness: f32,
    /// Line color when neither hovered nor dragging.
    pub idle_color: Color,
    /// Line color while hovered.
    pub hover_color: Color,
    /// Line color while dragging.
    pub active_color: Color,
    /// Visible line corner radius.
    pub radius: f32,
    /// Default height of a vertical (`X`) bar.
    pub vertical_extent: f32,
    /// Default width of a horizontal (`Y`) bar.
    pub horizontal_extent: f32,
}

/// Derives resize-bar defaults from [`Theme::default`].
impl Default for ResizeBarStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl ResizeBarStyle {
    /// Derives colors from `theme` and uses the standard geometry defaults.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::layout::ResizeBarStyle;
    /// assert_eq!(ResizeBarStyle::from_theme(Theme::dark()).vertical_extent, 64.0);
    /// ```
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        Self {
            hit_thickness: 8.0,
            line_thickness: 2.0,
            idle_color: Color::TRANSPARENT,
            hover_color: palette.focus.with_alpha(0.72),
            active_color: palette.accent,
            radius: 2.0,
            vertical_extent: 64.0,
            horizontal_extent: 64.0,
        }
    }

    /// Returns the axis-oriented intrinsic size in logical pixels.
    ///
    /// Values are passed through without clamping.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Size;
    /// use ailloli_ui_widgets::layout::{ResizeAxis, ResizeBarStyle};
    /// let style = ResizeBarStyle::default();
    /// assert_eq!(style.intrinsic_size(ResizeAxis::X), Size::new(8.0, 64.0));
    /// assert_eq!(style.intrinsic_size(ResizeAxis::Y), Size::new(64.0, 8.0));
    /// ```
    pub fn intrinsic_size(&self, axis: ResizeAxis) -> Size {
        match axis {
            ResizeAxis::X => Size::new(self.hit_thickness, self.vertical_extent),
            ResizeAxis::Y => Size::new(self.horizontal_extent, self.hit_thickness),
        }
    }
}

/// Shared retained callback invoked for emitted resize events.
type ResizeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, SplitResizeEvent)>;

/// Declarative draggable resize bar.
///
/// The default is vertical (`ResizeAxis::X`), uses theme-derived styling, and
/// has no handler. Drag capture starts only on a left press inside its bounds.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::layout::ResizeBar;
/// let bar: ResizeBar<()> = ResizeBar::vertical();
/// let _ = bar;
/// ```
pub struct ResizeBar<A = ()> {
    /// Outer logical sizing policy inherited by the retained view.
    pub(crate) layout: LayoutStyle,
    /// Parent-flex participation metadata preserved during conversion.
    pub(crate) flex_item: FlexItemStyle,
    /// Coordinate axis changed by a drag.
    axis: ResizeAxis,
    /// Hit target and visual line geometry in logical pixels.
    style: ResizeBarStyle,
    /// Optional callback receiving start, drag, and end events.
    on_resize: Option<ResizeHandler<A>>,
}

crate::impl_layout_builders!(ResizeBar);

impl<A: 'static> ResizeBar<A> {
    /// Creates a vertical bar that changes the x coordinate.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::ResizeBar;
    /// let bar: ResizeBar<()> = ResizeBar::vertical();
    /// let _ = bar;
    /// ```
    pub fn vertical() -> Self {
        Self::new(ResizeAxis::X)
    }

    /// Creates a horizontal bar that changes the y coordinate.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::ResizeBar;
    /// let bar: ResizeBar<()> = ResizeBar::horizontal();
    /// let _ = bar;
    /// ```
    pub fn horizontal() -> Self {
        Self::new(ResizeAxis::Y)
    }

    /// Creates a resize bar for the explicit main axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::{ResizeAxis, ResizeBar};
    /// let bar: ResizeBar<()> = ResizeBar::new(ResizeAxis::Y);
    /// let _ = bar;
    /// ```
    pub fn new(axis: ResizeAxis) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            axis,
            style: ResizeBarStyle::default(),
            on_resize: None,
        }
    }

    /// Replaces all hit, line, extent, radius, and color values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::{ResizeBar, ResizeBarStyle};
    /// let style = ResizeBarStyle { hit_thickness: 12.0, ..Default::default() };
    /// let bar: ResizeBar<()> = ResizeBar::vertical().resize_bar_style(style);
    /// let _ = bar;
    /// ```
    pub fn resize_bar_style(mut self, style: ResizeBarStyle) -> Self {
        self.style = style;
        self
    }

    /// Maps each emitted resize event into an application action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::{ResizeBar, SplitResizeEvent};
    /// enum Action { Resize(SplitResizeEvent) }
    /// let bar: ResizeBar<Action> = ResizeBar::vertical().on_resize(Action::Resize);
    /// let _ = bar;
    /// ```
    pub fn on_resize(mut self, f: impl Fn(SplitResizeEvent) -> A + 'static) -> Self {
        self.on_resize = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    /// Handles resize events with mutable runtime event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::ResizeBar;
    /// let bar: ResizeBar<()> = ResizeBar::vertical().on_resize_ctx(|_ctx, _event| {});
    /// let _ = bar;
    /// ```
    pub fn on_resize_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, SplitResizeEvent) + 'static,
    ) -> Self {
        self.on_resize = Some(Rc::new(f));
        self
    }
}

/// Creates a vertical resize bar.
impl<A: 'static> Default for ResizeBar<A> {
    fn default() -> Self {
        Self::vertical()
    }
}

/// Retained builder snapshot used to allocate drag state.
struct ResizeBarComponent<A> {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Coordinate axis changed by a drag.
    axis: ResizeAxis,
    /// Hit target and visual line geometry in logical pixels.
    style: ResizeBarStyle,
    /// Optional retained resize callback.
    on_resize: Option<ResizeHandler<A>>,
}

/// Builds a leaf widget with persistent optional drag state.
impl<A: 'static> ComponentNode<A> for ResizeBarComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        View::leaf(ResizeBarWidget {
            layout: self.layout,
            axis: self.axis,
            style: self.style,
            on_resize: self.on_resize.clone(),
            drag: context.signal_with_invalidation(None, Invalidation::Paint),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// Captured initial and previous pointer positions in logical pixels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Point;
/// use ailloli_ui_widgets::layout::{ResizeAxis, ResizeDragPhase, SplitResizeEvent};
/// let event = SplitResizeEvent { axis: ResizeAxis::X, phase: ResizeDragPhase::Start, position: Point::new(3.0, 4.0).x, delta: 0.0, total_delta: 0.0 };
/// assert_eq!(event.position, 3.0);
/// ```
pub(crate) struct ResizeDragState {
    /// Pointer at the initial press.
    pub start: Point,
    /// Pointer used as the next incremental-delta origin.
    pub last: Point,
}

/// Retained leaf implementing layout, paint, pointer capture, and cursor role.
struct ResizeBarWidget<A> {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Coordinate axis changed by a drag.
    axis: ResizeAxis,
    /// Hit target and visual line geometry in logical pixels.
    style: ResizeBarStyle,
    /// Optional retained resize callback.
    on_resize: Option<ResizeHandler<A>>,
    /// Captured pointer state, or `None` outside an active drag.
    drag: Signal<Option<ResizeDragState>>,
}

/// Implements the standalone resize-bar interaction contract.
impl<A: 'static> Widget<A> for ResizeBarWidget<A> {
    fn debug_name(&self) -> &'static str {
        "ResizeBar"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = apply_layout_size(
            self.style.intrinsic_size(self.axis),
            self.layout,
            constraints,
        );
        let bounds = Rect::new(0.0, 0.0, size.w, size.h);
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: bounds,
            visual_bounds: bounds,
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let dragging = self.drag.read().is_some();
        let color = if dragging {
            self.style.active_color
        } else if ctx.interaction().hovered {
            self.style.hover_color
        } else {
            self.style.idle_color
        };
        paint_resize_line(ctx, bounds, self.axis, &self.style, color);
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        match event {
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: true,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                self.drag.set(Some(ResizeDragState {
                    start: *pos,
                    last: *pos,
                }));
                self.emit(ctx, ResizeDragPhase::Start, *pos, *pos, *pos, bounds);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                if let Some(drag) = self.drag.read() {
                    self.emit(
                        ctx,
                        ResizeDragPhase::Drag,
                        *pos,
                        drag.last,
                        drag.start,
                        bounds,
                    );
                    self.drag.set(Some(ResizeDragState {
                        start: drag.start,
                        last: *pos,
                    }));
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) => {
                if let Some(drag) = self.drag.read() {
                    self.emit(
                        ctx,
                        ResizeDragPhase::End,
                        *pos,
                        drag.last,
                        drag.start,
                        bounds,
                    );
                    self.drag.set(None);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Cancelled { .. }) if self.drag.read().is_some() => {
                self.drag.set(None);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            _ => {}
        }
    }

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::AllowOnFocusOnly
    }

    fn hover_cursor_role(&self) -> HoverCursorRole {
        self.axis.cursor_role()
    }
}

/// Constructs and dispatches one resize event when a handler exists.
impl<A: 'static> ResizeBarWidget<A> {
    /// Builds and dispatches one axis-projected resize event when configured.
    fn emit(
        &self,
        ctx: &mut EventCtx<A>,
        phase: ResizeDragPhase,
        current: Point,
        last: Point,
        start: Point,
        bounds: Rect,
    ) {
        let event = resize_event(self.axis, phase, current, last, start, bounds, 0.0);
        if let Some(on_resize) = &self.on_resize {
            on_resize(ctx, event);
        }
    }
}

/// Converts the builder into a retained component and preserves flex metadata.
impl<A: 'static> IntoView<A> for ResizeBar<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(ResizeBarComponent {
                layout: self.layout,
                axis: self.axis,
                style: self.style,
                on_resize: self.on_resize,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Selects x for [`ResizeAxis::X`] or y for [`ResizeAxis::Y`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Point;
/// use ailloli_ui_widgets::layout::{ResizeAxis, ResizeDragPhase, SplitResizeEvent};
/// let point = Point::new(7.0, 9.0);
/// let event = SplitResizeEvent { axis: ResizeAxis::Y, phase: ResizeDragPhase::Drag, position: point.y, delta: 0.0, total_delta: 0.0 };
/// assert_eq!(event.position, 9.0);
/// ```
pub(crate) fn axis_value(axis: ResizeAxis, point: Point) -> f32 {
    match axis {
        ResizeAxis::X => point.x,
        ResizeAxis::Y => point.y,
    }
}

/// Builds axis-relative deltas and resolves the split-position sentinel.
///
/// A strictly positive explicit `position` wins. Zero, negative, or `NaN`
/// instead uses the pointer position relative to `bounds`, clamped to the
/// corresponding bound extent.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::layout::{ResizeAxis, ResizeDragPhase, SplitResizeEvent};
/// let event = SplitResizeEvent { axis: ResizeAxis::X, phase: ResizeDragPhase::Drag, position: 20.0, delta: 3.0, total_delta: 8.0 };
/// assert_eq!((event.position, event.delta), (20.0, 3.0));
/// ```
pub(crate) fn resize_event(
    axis: ResizeAxis,
    phase: ResizeDragPhase,
    current: Point,
    last: Point,
    start: Point,
    bounds: Rect,
    position: f32,
) -> SplitResizeEvent {
    let current_main = axis_value(axis, current);
    let last_main = axis_value(axis, last);
    let start_main = axis_value(axis, start);
    let fallback_position = match axis {
        ResizeAxis::X => (current.x - bounds.x).clamp(0.0, bounds.w),
        ResizeAxis::Y => (current.y - bounds.y).clamp(0.0, bounds.h),
    };
    SplitResizeEvent {
        axis,
        phase,
        position: if position > 0.0 {
            position
        } else {
            fallback_position
        },
        delta: current_main - last_main,
        total_delta: current_main - start_main,
    }
}

/// Centers the visible line across the complete bar extent.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Size;
/// use ailloli_ui_widgets::layout::{ResizeAxis, ResizeBarStyle};
/// assert_eq!(ResizeBarStyle::default().intrinsic_size(ResizeAxis::X), Size::new(8.0, 64.0));
/// ```
pub(crate) fn line_rect(bounds: Rect, axis: ResizeAxis, style: &ResizeBarStyle) -> Rect {
    match axis {
        ResizeAxis::X => Rect::new(
            bounds.x + (bounds.w - style.line_thickness) * 0.5,
            bounds.y,
            style.line_thickness,
            bounds.h,
        ),
        ResizeAxis::Y => Rect::new(
            bounds.x,
            bounds.y + (bounds.h - style.line_thickness) * 0.5,
            bounds.w,
            style.line_thickness,
        ),
    }
}

/// Appends one rounded line unless color alpha or bounds dimensions are non-positive.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::layout::{ResizeAxis, ResizeBar, ResizeBarStyle};
/// let bar: ResizeBar<()> = ResizeBar::new(ResizeAxis::X).resize_bar_style(ResizeBarStyle::default());
/// let _ = bar;
/// ```
pub(crate) fn paint_resize_line(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    axis: ResizeAxis,
    style: &ResizeBarStyle,
    color: Color,
) {
    if color.a <= 0.0 || bounds.w <= 0.0 || bounds.h <= 0.0 {
        return;
    }
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: line_rect(bounds, axis, style),
        radius: Radius::uniform(style.radius).tl,
        color,
    }));
}
