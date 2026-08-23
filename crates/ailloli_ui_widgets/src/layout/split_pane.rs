//! Two-child split layout with a draggable, bindable seam.

use std::rc::Rc;

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Point, Rect, Size};
use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_core::{Offset, Theme};
use ailloli_ui_runtime::component::{ComponentNode, Context, IntoView, Signal, View, Widget};
use ailloli_ui_runtime::input::{ActivationPolicy, EventCtx, HoverCursorRole};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;

use super::layout_ext::{apply_layout_size, finish_view_sized};
use super::resize_bar::{
    axis_value, paint_resize_line, ResizeAxis, ResizeBarStyle, ResizeDragPhase, ResizeDragState,
    SplitResizeEvent,
};

#[derive(Clone, Copy, Debug, PartialEq)]
/// Styling for the seam shared with [`super::ResizeBar`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::layout::SplitPaneStyle;
/// let style = SplitPaneStyle::default();
/// assert_eq!(style.resize_bar.hit_thickness, 8.0);
/// ```
pub struct SplitPaneStyle {
    /// Seam hit geometry, line geometry, and interaction colors.
    pub resize_bar: ResizeBarStyle,
}

/// Derives split-pane styling from [`Theme::default`].
impl Default for SplitPaneStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl SplitPaneStyle {
    /// Derives seam colors from `theme` and standard resize geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::layout::SplitPaneStyle;
    /// assert_eq!(SplitPaneStyle::from_theme(Theme::dark()).resize_bar.line_thickness, 2.0);
    /// ```
    pub fn from_theme(theme: Theme) -> Self {
        Self {
            resize_bar: ResizeBarStyle::from_theme(theme),
        }
    }
}

/// Shared retained callback invoked for seam drag events.
type ResizeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, SplitResizeEvent)>;

#[derive(Clone, Copy, Debug, PartialEq)]
/// Initial seam distance measured from the start or end edge.
enum SplitPaneInitialPosition {
    /// Logical-pixel distance measured from the leading edge.
    Start(f32),
    /// Logical-pixel distance measured from the trailing edge.
    End(f32),
}

/// Two-child layout split on x or y with a draggable seam.
///
/// Columns split on x; rows split on y. The default seam is centered, both
/// minimum extents are zero, the pane fills both axes, and position is held in
/// component-local state unless [`Self::bind_position`] supplies a signal.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::{layout::SplitPane, text::Text};
/// let split: SplitPane<()> = SplitPane::columns(Text::new("left"), Text::new("right"));
/// let _ = split;
/// ```
pub struct SplitPane<A = ()> {
    /// Outer logical sizing policy inherited by the retained view.
    pub(crate) layout: LayoutStyle,
    /// Parent-flex participation metadata preserved during conversion.
    pub(crate) flex_item: FlexItemStyle,
    /// Physical axis along which the two children are divided.
    axis: ResizeAxis,
    /// Child occupying the leading side of the seam.
    start: View<A>,
    /// Child occupying the trailing side of the seam.
    end: View<A>,
    /// Optional initial seam distance used until retained state exists.
    initial_position: Option<SplitPaneInitialPosition>,
    /// Optional caller-owned seam distance in logical pixels from the start.
    bound_position: Option<Signal<f32>>,
    /// Minimum logical-pixel extent reserved for the leading child.
    min_start: f32,
    /// Minimum logical-pixel extent reserved for the trailing child.
    min_end: f32,
    /// Seam hit target and visual style.
    style: SplitPaneStyle,
    /// Optional callback receiving start, drag, and end resize events.
    on_resize: Option<ResizeHandler<A>>,
}

crate::impl_layout_builders!(SplitPane);

impl<A: 'static> SplitPane<A> {
    /// Creates left/right panes separated by an x-axis seam.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::SplitPane, text::Text};
    /// let split: SplitPane<()> = SplitPane::columns(Text::new("left"), Text::new("right"));
    /// let _ = split;
    /// ```
    pub fn columns(start: impl IntoView<A>, end: impl IntoView<A>) -> Self {
        Self::new(ResizeAxis::X, start, end)
    }

    /// Creates top/bottom panes separated by a y-axis seam.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::SplitPane, text::Text};
    /// let split: SplitPane<()> = SplitPane::rows(Text::new("top"), Text::new("bottom"));
    /// let _ = split;
    /// ```
    pub fn rows(start: impl IntoView<A>, end: impl IntoView<A>) -> Self {
        Self::new(ResizeAxis::Y, start, end)
    }

    /// Creates a fill-sized split for an explicit axis and two required children.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::{ResizeAxis, SplitPane}, text::Text};
    /// let split: SplitPane<()> = SplitPane::new(ResizeAxis::X, Text::new("a"), Text::new("b"));
    /// let _ = split;
    /// ```
    pub fn new(axis: ResizeAxis, start: impl IntoView<A>, end: impl IntoView<A>) -> Self {
        Self {
            layout: LayoutStyle::default().fill(),
            flex_item: FlexItemStyle::default(),
            axis,
            start: start.into_view(),
            end: end.into_view(),
            initial_position: None,
            bound_position: None,
            min_start: 0.0,
            min_end: 0.0,
            style: SplitPaneStyle::default(),
            on_resize: None,
        }
    }

    /// Sets initial seam distance from the start edge in logical pixels.
    ///
    /// Negative and `NaN` values become zero. A bound signal takes precedence,
    /// and the resolved value is clamped by pane minima on layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::SplitPane, text::Text};
    /// let split: SplitPane<()> = SplitPane::columns(Text::new("a"), Text::new("b")).initial_position(160.0);
    /// let _ = split;
    /// ```
    pub fn initial_position(mut self, position: f32) -> Self {
        self.initial_position = Some(SplitPaneInitialPosition::Start(position.max(0.0)));
        self
    }

    /// Sets initial end-pane extent in logical pixels.
    ///
    /// The seam is resolved as `available - position`, floored at zero, then
    /// clamped by pane minima. Negative and `NaN` inputs become zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::SplitPane, text::Text};
    /// let split: SplitPane<()> = SplitPane::columns(Text::new("a"), Text::new("b")).initial_end_position(240.0);
    /// let _ = split;
    /// ```
    pub fn initial_end_position(mut self, position: f32) -> Self {
        self.initial_position = Some(SplitPaneInitialPosition::End(position.max(0.0)));
        self
    }

    /// Binds seam position, measured from the start edge, to shared state.
    ///
    /// Binding has priority over initial and local positions and is updated
    /// synchronously during drag. Its value is clamped for layout but is not
    /// rewritten merely because constraints clamp it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::{layout::SplitPane, text::Text};
    /// let position = State::new(120.0);
    /// let split: SplitPane<()> = SplitPane::columns(Text::new("a"), Text::new("b")).bind_position(position);
    /// let _ = split;
    /// ```
    pub fn bind_position(mut self, position: impl Into<Signal<f32>>) -> Self {
        self.bound_position = Some(position.into());
        self
    }

    /// Sets the minimum start-pane main extent in logical pixels.
    ///
    /// Negative and `NaN` inputs become zero. If both minima cannot fit, the
    /// start minimum wins and the end pane may collapse to zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::SplitPane, text::Text};
    /// let split: SplitPane<()> = SplitPane::columns(Text::new("a"), Text::new("b")).min_start(80.0);
    /// let _ = split;
    /// ```
    pub fn min_start(mut self, value: f32) -> Self {
        self.min_start = value.max(0.0);
        self
    }

    /// Sets the minimum end-pane main extent in logical pixels.
    ///
    /// Negative and `NaN` inputs become zero. Impossible combined minima use
    /// the start-minimum precedence described by [`Self::min_start`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::SplitPane, text::Text};
    /// let split: SplitPane<()> = SplitPane::columns(Text::new("a"), Text::new("b")).min_end(100.0);
    /// let _ = split;
    /// ```
    pub fn min_end(mut self, value: f32) -> Self {
        self.min_end = value.max(0.0);
        self
    }

    /// Replaces all seam style values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::{SplitPane, SplitPaneStyle}, text::Text};
    /// let style = SplitPaneStyle::default();
    /// let split: SplitPane<()> = SplitPane::columns(Text::new("a"), Text::new("b")).split_pane_style(style);
    /// let _ = split;
    /// ```
    pub fn split_pane_style(mut self, style: SplitPaneStyle) -> Self {
        self.style = style;
        self
    }

    /// Maps each seam event into an application action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::{SplitPane, SplitResizeEvent}, text::Text};
    /// enum Action { Resize(SplitResizeEvent) }
    /// let split: SplitPane<Action> = SplitPane::columns(Text::new("a"), Text::new("b")).on_resize(Action::Resize);
    /// let _ = split;
    /// ```
    pub fn on_resize(mut self, f: impl Fn(SplitResizeEvent) -> A + 'static) -> Self {
        self.on_resize = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    /// Handles seam events with mutable runtime event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::SplitPane, text::Text};
    /// let split: SplitPane<()> = SplitPane::columns(Text::new("a"), Text::new("b")).on_resize_ctx(|_ctx, _event| {});
    /// let _ = split;
    /// ```
    pub fn on_resize_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, SplitResizeEvent) + 'static,
    ) -> Self {
        self.on_resize = Some(Rc::new(f));
        self
    }
}

/// Retained builder snapshot used to allocate local position/drag state.
struct SplitPaneComponent<A> {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Physical split axis.
    axis: ResizeAxis,
    /// Leading retained child.
    start: View<A>,
    /// Trailing retained child.
    end: View<A>,
    /// Initial seam distance before local or bound state takes precedence.
    initial_position: Option<SplitPaneInitialPosition>,
    /// Optional caller-owned seam distance.
    bound_position: Option<Signal<f32>>,
    /// Minimum leading-child extent in logical pixels.
    min_start: f32,
    /// Minimum trailing-child extent in logical pixels.
    min_end: f32,
    /// Seam hit target and visual style.
    style: SplitPaneStyle,
    /// Optional retained resize callback.
    on_resize: Option<ResizeHandler<A>>,
}

/// Builds the split widget with exactly two retained children.
impl<A: 'static> ComponentNode<A> for SplitPaneComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        View::node(
            SplitPaneWidget {
                layout: self.layout,
                axis: self.axis,
                initial_position: self.initial_position,
                bound_position: self.bound_position.clone(),
                local_position: context.signal(None),
                min_start: self.min_start,
                min_end: self.min_end,
                style: self.style,
                on_resize: self.on_resize.clone(),
                drag: context.signal(None),
                hover_seam: context.signal(false),
            },
            vec![self.start.clone(), self.end.clone()],
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// Captured pointer and seam positions for incremental drag events.
struct SplitPaneDragState {
    /// Pointer start/current samples shared with standalone resize bars.
    pointer: ResizeDragState,
    /// Seam distance in logical pixels captured at press time.
    start_position: f32,
    /// Most recently emitted clamped seam distance.
    last_position: f32,
}

/// Retained layout, seam-state, hover, and drag implementation.
struct SplitPaneWidget<A> {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Physical split axis.
    axis: ResizeAxis,
    /// Initial seam distance before retained state takes precedence.
    initial_position: Option<SplitPaneInitialPosition>,
    /// Optional caller-owned seam distance.
    bound_position: Option<Signal<f32>>,
    /// Uncontrolled retained seam distance, absent before first write.
    local_position: Signal<Option<f32>>,
    /// Minimum leading-child extent in logical pixels.
    min_start: f32,
    /// Minimum trailing-child extent in logical pixels.
    min_end: f32,
    /// Seam hit target and visual style.
    style: SplitPaneStyle,
    /// Optional retained resize callback.
    on_resize: Option<ResizeHandler<A>>,
    /// Active pointer drag state, or `None` outside a captured gesture.
    drag: Signal<Option<SplitPaneDragState>>,
    /// Whether the pointer currently intersects the seam hit target.
    hover_seam: Signal<bool>,
}

/// Implements tight two-pane layout and seam pointer capture/painting.
impl<A: 'static> Widget<A> for SplitPaneWidget<A> {
    fn debug_name(&self) -> &'static str {
        "SplitPane"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let fallback = constraints.max_size();
        let intrinsic = Size::new(fallback.w.max(0.0), fallback.h.max(0.0));
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        let available = main_extent(self.axis, size);
        let position = self.current_position(available);

        let start_size = size_with_main(self.axis, size, position);
        let end_size = size_with_main(self.axis, size, (available - position).max(0.0));
        let mut child_layouts = Vec::with_capacity(children.len());

        if let Some(child) = children.get_mut(0) {
            let _ = child.layout(engine, ctx, Constraints::tight(start_size.w, start_size.h));
            let bounds = Rect::new(0.0, 0.0, start_size.w, start_size.h);
            child_layouts.push(ChildLayout {
                offset: Offset::new(0.0, 0.0),
                size: start_size,
                paint_bounds: bounds,
                visual_bounds: bounds,
            });
        }
        if let Some(child) = children.get_mut(1) {
            let offset = match self.axis {
                ResizeAxis::X => Offset::new(position, 0.0),
                ResizeAxis::Y => Offset::new(0.0, position),
            };
            let _ = child.layout(engine, ctx, Constraints::tight(end_size.w, end_size.h));
            let bounds = Rect::new(offset.x, offset.y, end_size.w, end_size.h);
            child_layouts.push(ChildLayout {
                offset,
                size: end_size,
                paint_bounds: bounds,
                visual_bounds: bounds,
            });
        }

        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds,
            visual_bounds: paint_bounds,
            overlay_hit_bounds: vec![self.seam_rect_for(size, position)],
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, layout: &LayoutResult) {
        let dragging = self.drag.read().is_some();
        let color = if dragging {
            self.style.resize_bar.active_color
        } else if self.hover_seam.read() {
            self.style.resize_bar.hover_color
        } else {
            self.style.resize_bar.idle_color
        };
        paint_resize_line(
            ctx,
            self.seam_rect_abs(bounds, layout),
            self.axis,
            &self.style.resize_bar,
            color,
        );
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, layout: &LayoutResult) {
        match event {
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                let seam_hover = self.seam_rect_abs(bounds, layout).contains(pos.x, pos.y);
                if self.hover_seam.read() != seam_hover {
                    self.hover_seam.set(seam_hover);
                    ctx.request_repaint();
                }
                if let Some(drag) = self.drag.read() {
                    let available = main_extent(self.axis, layout.size);
                    let next = self.clamp_position(
                        available,
                        drag.start_position + axis_value(self.axis, *pos)
                            - axis_value(self.axis, drag.pointer.start),
                    );
                    self.set_position(next);
                    self.emit(
                        ctx,
                        ResizeDragPhase::Drag,
                        next,
                        next - drag.last_position,
                        next - drag.start_position,
                    );
                    self.drag.set(Some(SplitPaneDragState {
                        pointer: ResizeDragState {
                            start: drag.pointer.start,
                            last: *pos,
                        },
                        start_position: drag.start_position,
                        last_position: next,
                    }));
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: true,
                ..
            }) if self.seam_rect_abs(bounds, layout).contains(pos.x, pos.y) => {
                let available = main_extent(self.axis, layout.size);
                let position = self.current_position(available);
                self.drag.set(Some(SplitPaneDragState {
                    pointer: ResizeDragState {
                        start: *pos,
                        last: *pos,
                    },
                    start_position: position,
                    last_position: position,
                }));
                self.hover_seam.set(true);
                self.emit(ctx, ResizeDragPhase::Start, position, 0.0, 0.0);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) => {
                if let Some(drag) = self.drag.read() {
                    let available = main_extent(self.axis, layout.size);
                    let next = self.clamp_position(
                        available,
                        drag.start_position + axis_value(self.axis, *pos)
                            - axis_value(self.axis, drag.pointer.start),
                    );
                    self.set_position(next);
                    self.emit(
                        ctx,
                        ResizeDragPhase::End,
                        next,
                        next - drag.last_position,
                        next - drag.start_position,
                    );
                    self.drag.set(None);
                    self.hover_seam
                        .set(self.seam_rect_abs(bounds, layout).contains(pos.x, pos.y));
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Cancelled { .. }) if self.drag.read().is_some() => {
                self.drag.set(None);
                self.hover_seam.set(false);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            _ => {}
        }
    }

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::AllowOnFocusOnly
    }

    fn hover_cursor_role_at(
        &self,
        bounds: Rect,
        layout: &LayoutResult,
        pos: Point,
    ) -> HoverCursorRole {
        if self.drag.read().is_some() || self.seam_rect_abs(bounds, layout).contains(pos.x, pos.y) {
            self.axis.cursor_role()
        } else {
            HoverCursorRole::Inherit
        }
    }
}

/// Position priority/clamping, state writes, event dispatch, and seam geometry.
impl<A: 'static> SplitPaneWidget<A> {
    /// Resolves bound, local, initial, or centered position and clamps it.
    fn current_position(&self, available: f32) -> f32 {
        let raw = self
            .bound_position
            .as_ref()
            .map(Signal::read)
            .or_else(|| self.local_position.read())
            .or_else(|| {
                self.initial_position.map(|position| match position {
                    SplitPaneInitialPosition::Start(value) => value,
                    SplitPaneInitialPosition::End(value) => (available - value).max(0.0),
                })
            })
            .unwrap_or(available * 0.5);
        self.clamp_position(available, raw)
    }

    /// Clamps the seam so both configured minimum extents remain satisfied.
    fn clamp_position(&self, available: f32, position: f32) -> f32 {
        let max = (available - self.min_end).max(self.min_start);
        position.clamp(self.min_start.min(max), max)
    }

    /// Writes a changed position to controlled or local retained state.
    fn set_position(&self, position: f32) {
        if let Some(bound) = &self.bound_position {
            if (bound.read() - position).abs() > f32::EPSILON {
                bound.set(position);
            }
        } else if self
            .local_position
            .read()
            .is_none_or(|value| (value - position).abs() > f32::EPSILON)
        {
            self.local_position.set(Some(position));
        }
    }

    /// Emits one resize phase with incremental and press-relative deltas.
    fn emit(
        &self,
        ctx: &mut EventCtx<A>,
        phase: ResizeDragPhase,
        position: f32,
        delta: f32,
        total_delta: f32,
    ) {
        if let Some(on_resize) = &self.on_resize {
            on_resize(
                ctx,
                SplitResizeEvent {
                    axis: self.axis,
                    phase,
                    position,
                    delta,
                    total_delta,
                },
            );
        }
    }

    /// Computes the origin-local seam hit rectangle for a laid-out size.
    fn seam_rect_for(&self, size: Size, position: f32) -> Rect {
        let half = self.style.resize_bar.hit_thickness * 0.5;
        match self.axis {
            ResizeAxis::X => Rect::new(
                position - half,
                0.0,
                self.style.resize_bar.hit_thickness,
                size.h,
            ),
            ResizeAxis::Y => Rect::new(
                0.0,
                position - half,
                size.w,
                self.style.resize_bar.hit_thickness,
            ),
        }
    }

    /// Computes the seam hit rectangle in logical window coordinates.
    fn seam_rect_abs(&self, bounds: Rect, layout: &LayoutResult) -> Rect {
        let available = main_extent(self.axis, layout.size);
        self.seam_rect_for(layout.size, self.current_position(available))
            .translate(Offset::new(bounds.x, bounds.y))
    }
}

/// Converts the builder into a retained component and preserves flex metadata.
impl<A: 'static> IntoView<A> for SplitPane<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(SplitPaneComponent {
                layout: self.layout,
                axis: self.axis,
                start: self.start,
                end: self.end,
                initial_position: self.initial_position,
                bound_position: self.bound_position,
                min_start: self.min_start,
                min_end: self.min_end,
                style: self.style,
                on_resize: self.on_resize,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Selects width for x-axis splits or height for y-axis splits.
fn main_extent(axis: ResizeAxis, size: Size) -> f32 {
    match axis {
        ResizeAxis::X => size.w,
        ResizeAxis::Y => size.h,
    }
}

/// Replaces the selected main extent, flooring it at zero.
fn size_with_main(axis: ResizeAxis, container: Size, main: f32) -> Size {
    match axis {
        ResizeAxis::X => Size::new(main.max(0.0), container.h),
        ResizeAxis::Y => Size::new(container.w, main.max(0.0)),
    }
}
