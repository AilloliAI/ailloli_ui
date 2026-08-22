//! Single-child clipped scrolling, virtualization hints, follow-end, and bars.

use ailloli_ui_core::event::{Event, PointerEvent};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::{ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollState};
use ailloli_ui_core::style::FlexItemStyle;
use ailloli_ui_core::{Color, Offset};
use ailloli_ui_runtime::component::{ComponentNode, Context, IntoView, Signal, View, Widget};
use ailloli_ui_runtime::input::EventCtx;
use ailloli_ui_runtime::layout::LayoutEngine;
use ailloli_ui_runtime::layout::{
    ChildLayout, LayoutChild, LayoutCtx, LayoutResult, VirtualViewport,
};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawCmd, DrawRRect, Invalidation};

/// Visual style for [`ScrollView`] scrollbars.
///
/// All dimensions are logical pixels. The default is a six-pixel bar inset by
/// three pixels with a minimum 24-pixel thumb. Custom values are not normalized
/// globally; unusable track geometry simply suppresses that scrollbar.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::layout::ScrollbarStyle;
/// let style = ScrollbarStyle::default();
/// assert_eq!(style.thickness, 6.0);
/// assert_eq!(style.min_thumb_len, 24.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarStyle {
    /// Scrollbar track fill.
    pub track_color: Color,
    /// Scrollbar thumb fill.
    pub thumb_color: Color,
    /// Track/thumb cross-axis thickness in logical pixels.
    pub thickness: f32,
    /// Minimum main-axis thumb length in logical pixels, capped to the track.
    pub min_thumb_len: f32,
    /// Distance from viewport edges in logical pixels.
    pub inset: f32,
    /// Track and thumb corner radius in logical pixels.
    pub radius: f32,
}

/// Supplies the standard neutral scrollbar palette and geometry.
impl Default for ScrollbarStyle {
    fn default() -> Self {
        Self {
            track_color: Color::rgba(148, 163, 184, 0.16),
            thumb_color: Color::rgba(148, 163, 184, 0.56),
            thickness: 6.0,
            min_thumb_len: 24.0,
            inset: 3.0,
            radius: 3.0,
        }
    }
}

/// Single-child viewport for content larger than its visible bounds.
///
/// The default scrolls vertically, begins at zero, shows scrollbars, maps one
/// wheel line to 48 logical pixels, and does not follow the content end. The
/// content is clipped and receives a virtual viewport hint containing the
/// current offset. Only wheel input is handled by this widget.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::{layout::ScrollView, text::Text};
/// let scroll: ScrollView<()> = ScrollView::vertical().child(Text::new("content"));
/// let _ = scroll;
/// ```
pub struct ScrollView<A = ()> {
    axes: ScrollAxes,
    initial_offset: Offset,
    behavior: ScrollBehavior,
    follow_end: bool,
    scrollbars: bool,
    scrollbar_style: ScrollbarStyle,
    child: Option<View<A>>,
}

/// Creates the same vertical viewport as [`ScrollView::new`].
impl<A: 'static> Default for ScrollView<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> ScrollView<A> {
    /// Creates a vertical viewport.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::ScrollView;
    /// let scroll: ScrollView<()> = ScrollView::new();
    /// let _ = scroll;
    /// ```
    pub fn new() -> Self {
        Self::vertical()
    }

    /// Creates a vertical-only viewport.
    ///
    /// Horizontal initial/wheel offsets are filtered to zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::ScrollView;
    /// let scroll: ScrollView<()> = ScrollView::vertical();
    /// let _ = scroll;
    /// ```
    pub fn vertical() -> Self {
        Self {
            axes: ScrollAxes::VERTICAL,
            initial_offset: Offset::default(),
            behavior: ScrollBehavior::new(ScrollAxes::VERTICAL),
            follow_end: false,
            scrollbars: true,
            scrollbar_style: ScrollbarStyle::default(),
            child: None,
        }
    }

    /// Creates a horizontal-only viewport.
    ///
    /// Vertical initial/wheel offsets are filtered to zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::ScrollView;
    /// let scroll: ScrollView<()> = ScrollView::horizontal();
    /// let _ = scroll;
    /// ```
    pub fn horizontal() -> Self {
        Self {
            axes: ScrollAxes::HORIZONTAL,
            initial_offset: Offset::default(),
            behavior: ScrollBehavior::new(ScrollAxes::HORIZONTAL),
            follow_end: false,
            scrollbars: true,
            scrollbar_style: ScrollbarStyle::default(),
            child: None,
        }
    }

    /// Creates a viewport scrollable on both axes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::ScrollView;
    /// let scroll: ScrollView<()> = ScrollView::both();
    /// let _ = scroll;
    /// ```
    pub fn both() -> Self {
        Self {
            axes: ScrollAxes::BOTH,
            initial_offset: Offset::default(),
            behavior: ScrollBehavior::new(ScrollAxes::BOTH),
            follow_end: false,
            scrollbars: true,
            scrollbar_style: ScrollbarStyle::default(),
            child: None,
        }
    }

    /// Sets the initial non-persisted logical-pixel content offset.
    ///
    /// Disabled axes are filtered to zero. The first bounded layout clamps the
    /// retained state to content limits.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Offset;
    /// use ailloli_ui_widgets::layout::ScrollView;
    /// let scroll: ScrollView<()> = ScrollView::both().initial_offset(Offset::new(10.0, 20.0));
    /// let _ = scroll;
    /// ```
    pub fn initial_offset(mut self, offset: Offset) -> Self {
        self.initial_offset = self.axes.filter_offset(offset);
        self
    }

    /// Sets a non-negative initial vertical offset in logical pixels.
    ///
    /// Negative and `NaN` inputs become zero via floating-point `max`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::ScrollView;
    /// let scroll: ScrollView<()> = ScrollView::vertical().initial_scroll_y(120.0);
    /// let _ = scroll;
    /// ```
    pub fn initial_scroll_y(self, scroll_y: f32) -> Self {
        self.initial_offset(Offset::new(0.0, scroll_y.max(0.0)))
    }

    /// Legacy builder kept for old tests/apps where this value represented child paint offset.
    ///
    /// The legacy value is negated: `-40.0` becomes a retained y offset of
    /// `40.0`, while positive and `NaN` values become zero. Prefer
    /// [`Self::initial_scroll_y`] for new code.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::ScrollView;
    /// let legacy: ScrollView<()> = ScrollView::vertical().scroll_y(-40.0);
    /// let _ = legacy;
    /// ```
    pub fn scroll_y(mut self, scroll_y: f32) -> Self {
        self.initial_offset.y = (-scroll_y).max(0.0);
        self
    }

    /// Sets logical pixels represented by one wheel line.
    ///
    /// Values are clamped to at least one; `NaN` and negative infinity become
    /// one, while positive infinity remains infinite.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::ScrollView;
    /// let scroll: ScrollView<()> = ScrollView::vertical().wheel_line_px(24.0);
    /// let _ = scroll;
    /// ```
    pub fn wheel_line_px(mut self, line_px: f32) -> Self {
        self.behavior = self.behavior.with_line_px(line_px);
        self
    }

    /// Enables or disables sticky scrolling at the enabled-axis end.
    ///
    /// When enabled, content growth follows the end until a manual wheel scroll
    /// moves farther than one logical pixel away. Reaching the end reactivates it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::ScrollView;
    /// let log: ScrollView<()> = ScrollView::vertical().follow_end(true);
    /// let _ = log;
    /// ```
    pub fn follow_end(mut self, enabled: bool) -> Self {
        self.follow_end = enabled;
        self
    }

    /// Shows or hides non-interactive overflow indicators.
    ///
    /// Wheel scrolling and clipping remain active when hidden.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::ScrollView;
    /// let scroll: ScrollView<()> = ScrollView::vertical().scrollbars(false);
    /// let _ = scroll;
    /// ```
    pub fn scrollbars(mut self, enabled: bool) -> Self {
        self.scrollbars = enabled;
        self
    }

    /// Replaces scrollbar colors and geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::{ScrollbarStyle, ScrollView};
    /// let style = ScrollbarStyle { thickness: 8.0, ..Default::default() };
    /// let scroll: ScrollView<()> = ScrollView::vertical().scrollbar_style(style);
    /// let _ = scroll;
    /// ```
    pub fn scrollbar_style(mut self, style: ScrollbarStyle) -> Self {
        self.scrollbar_style = style;
        self
    }

    /// Sets the single scroll content child, replacing any previous child.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::ScrollView, text::Text};
    /// let scroll: ScrollView<()> = ScrollView::vertical().child(Text::new("content"));
    /// let _ = scroll;
    /// ```
    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.child = Some(child.into_view());
        self
    }
}

/// Retained builder snapshot used to allocate scroll/follow signals.
struct ScrollViewComponent<A> {
    axes: ScrollAxes,
    initial_offset: Offset,
    behavior: ScrollBehavior,
    follow_end: bool,
    scrollbars: bool,
    scrollbar_style: ScrollbarStyle,
    child: Option<View<A>>,
}

/// Builds the viewport widget and preserves its optional content child.
impl<A: 'static> ComponentNode<A> for ScrollViewComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let state = context.signal_with_invalidation(
            ScrollState::with_offset(self.initial_offset),
            Invalidation::Layout,
        );
        let follow_end_active =
            context.signal_with_invalidation(self.follow_end, Invalidation::Layout);
        let mut children = Vec::new();
        if let Some(child) = self.child.clone() {
            children.push(child);
        }

        View::node(
            ScrollViewWidget {
                axes: self.axes,
                state,
                behavior: self.behavior,
                follow_end: self.follow_end,
                follow_end_active,
                scrollbars: self.scrollbars,
                scrollbar_style: self.scrollbar_style,
            },
            children,
        )
    }
}

/// Stateful retained viewport implementation.
struct ScrollViewWidget {
    axes: ScrollAxes,
    state: Signal<ScrollState>,
    behavior: ScrollBehavior,
    follow_end: bool,
    follow_end_active: Signal<bool>,
    scrollbars: bool,
    scrollbar_style: ScrollbarStyle,
}

/// Implements bounded/unbounded layout passes, clipping, bars, and wheel input.
impl<A: 'static> Widget<A> for ScrollViewWidget {
    fn debug_name(&self) -> &'static str {
        "ScrollView"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let mut child_layouts = Vec::new();
        let mut state = self.state.read();
        let constraints_max = constraints.max_size();
        let mut size = finite_or_zero_size(constraints_max);

        if let Some(child) = children.first_mut() {
            let child_constraints = self.child_constraints(constraints_max);
            let viewport_hint_size = finite_or_zero_size(constraints_max);
            let previous_viewport = ctx.replace_virtual_viewport(Some(VirtualViewport::new(
                Rect::new(
                    state.offset.x,
                    state.offset.y,
                    viewport_hint_size.w,
                    viewport_hint_size.h,
                ),
                0.0,
            )));
            let mut r = child.layout(engine, ctx, child_constraints);
            ctx.replace_virtual_viewport(previous_viewport);
            size = viewport_size(constraints_max, r.size);
            if self.has_bounded_scroll_viewport(constraints_max) {
                let metrics = ScrollMetrics::new(size, r.size);
                let next_state = self.sync_scroll_state_for_layout(state, metrics);
                if !same_offset(state.offset, next_state.offset) {
                    let previous_viewport =
                        ctx.replace_virtual_viewport(Some(VirtualViewport::new(
                            Rect::new(next_state.offset.x, next_state.offset.y, size.w, size.h),
                            0.0,
                        )));
                    r = child.layout(engine, ctx, child_constraints);
                    ctx.replace_virtual_viewport(previous_viewport);
                }
                state = next_state;
            } else {
                // Flex containers probe growing children with an unbounded
                // main axis before assigning their final slot. That probe is
                // not a real viewport: clamping here would erase a retained
                // deep scroll offset and then expose every virtualized row to
                // layout. Keep the persistent state untouched and render the
                // intrinsic probe from the origin; the following bounded pass
                // applies and clamps the real offset.
                state = ScrollState::new();
            }
            child_layouts.push(ChildLayout {
                offset: Offset::new(-state.offset.x, -state.offset.y),
                size: r.size,
                paint_bounds: Rect::new(0.0, 0.0, r.size.w, r.size.h),
                visual_bounds: r.visual_bounds,
            });
        }
        let viewport = Rect::new(0.0, 0.0, size.w, size.h);

        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds: viewport,
            visual_bounds: viewport,
            overlay_hit_bounds: Vec::new(),
            clip: Some(ailloli_ui_core::ClipShape::Rect(viewport)),
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, layout: &LayoutResult) {
        if !self.scrollbars {
            return;
        }
        let Some(child) = layout.children.first() else {
            return;
        };

        let metrics = ScrollMetrics::new(layout.size, child.size);
        let state = self.state.read();
        paint_scrollbars(ctx, bounds, metrics, state, self.axes, self.scrollbar_style);
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, _bounds: Rect, layout: &LayoutResult) {
        let Event::Pointer(PointerEvent::Wheel { delta, .. }) = event else {
            return;
        };
        let Some(child) = layout.children.first() else {
            return;
        };

        let metrics = ScrollMetrics::new(layout.size, child.size);
        let state = self.state.read();
        let outcome = state.scroll_by(self.behavior.wheel_delta(*delta), metrics, self.axes);
        if outcome.changed {
            self.state.set(outcome.state());
            self.sync_follow_end_after_manual_scroll(outcome.after, outcome.max_offset);
            ctx.request_repaint();
            ctx.stop_propagation();
        }
    }
}

/// Converts the builder into a component with default flex-shrink weight one.
impl<A: 'static> IntoView<A> for ScrollView<A> {
    fn into_view(self) -> View<A> {
        View::component(ScrollViewComponent {
            axes: self.axes,
            initial_offset: self.initial_offset,
            behavior: self.behavior,
            follow_end: self.follow_end,
            scrollbars: self.scrollbars,
            scrollbar_style: self.scrollbar_style,
            child: self.child,
        })
        // A scroll container is a viewport, not an intrinsically-sized list.
        // Let a flex parent shrink its probe result into the assigned slot;
        // callers can still opt out explicitly with `.flex_shrink(0.0)`.
        .with_flex_item(FlexItemStyle::new().flex_shrink(1.0))
    }
}

/// Scroll-state synchronization, follow-end policy, and child constraints.
impl ScrollViewWidget {
    fn has_bounded_scroll_viewport(&self, viewport: Size) -> bool {
        (!self.axes.horizontal || viewport.w.is_finite())
            && (!self.axes.vertical || viewport.h.is_finite())
    }

    fn sync_scroll_state_for_layout(
        &self,
        state: ScrollState,
        metrics: ScrollMetrics,
    ) -> ScrollState {
        if !self.follow_end {
            let clamped = state.clamp_to(metrics, self.axes);
            if clamped.changed {
                let next = clamped.state();
                self.state.set(next);
                return next;
            }
            return state;
        }

        let max_offset = self.axes.filter_offset(metrics.max_offset());
        let follow_active = self.follow_end_active.read();
        let target = if follow_active {
            ScrollState::with_offset(max_offset)
        } else {
            let clamped = state.clamp_to(metrics, self.axes).state();
            if offset_at_end(clamped.offset, max_offset, self.axes) {
                self.set_follow_end_active(true);
            }
            clamped
        };
        if !same_offset(state.offset, target.offset) {
            self.state.set(target);
        }
        target
    }

    fn sync_follow_end_after_manual_scroll(&self, offset: Offset, max_offset: Offset) {
        if !self.follow_end {
            return;
        }
        self.set_follow_end_active(offset_at_end(offset, max_offset, self.axes));
    }

    fn set_follow_end_active(&self, active: bool) {
        if self.follow_end_active.read() != active {
            self.follow_end_active.set(active);
        }
    }

    fn child_constraints(&self, viewport: Size) -> Constraints {
        Constraints::loose(
            if self.axes.horizontal {
                f32::INFINITY
            } else {
                viewport.w
            },
            if self.axes.vertical {
                f32::INFINITY
            } else {
                viewport.h
            },
        )
    }
}

/// Maximum logical-pixel distance considered to be at the scroll end.
const FOLLOW_END_EPSILON: f32 = 1.0;

/// Tests enabled axes against their maximum offsets within the end epsilon.
fn offset_at_end(offset: Offset, max_offset: Offset, axes: ScrollAxes) -> bool {
    (!axes.horizontal || offset.x >= max_offset.x - FOLLOW_END_EPSILON)
        && (!axes.vertical || offset.y >= max_offset.y - FOLLOW_END_EPSILON)
}

/// Compares offsets with a strict 0.001 logical-pixel per-axis tolerance.
fn same_offset(a: Offset, b: Offset) -> bool {
    (a.x - b.x).abs() < 0.001 && (a.y - b.y).abs() < 0.001
}

/// Replaces each non-finite extent with zero independently.
fn finite_or_zero_size(size: Size) -> Size {
    Size::new(
        if size.w.is_finite() { size.w } else { 0.0 },
        if size.h.is_finite() { size.h } else { 0.0 },
    )
}

/// Uses finite maximum constraints or falls back to content extent per axis.
fn viewport_size(constraints_max: Size, content: Size) -> Size {
    Size::new(
        finite_or_content(constraints_max.w, content.w),
        finite_or_content(constraints_max.h, content.h),
    )
}

/// Selects a finite maximum or the corresponding content extent.
fn finite_or_content(max: f32, content: f32) -> f32 {
    if max.is_finite() {
        max
    } else {
        content
    }
}

/// Paints bars only for enabled axes with more than 0.5 pixels of overflow.
fn paint_scrollbars(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    metrics: ScrollMetrics,
    state: ScrollState,
    axes: ScrollAxes,
    style: ScrollbarStyle,
) {
    let max_offset = metrics.max_offset();
    let show_vertical = axes.vertical && max_offset.y > 0.5;
    let show_horizontal = axes.horizontal && max_offset.x > 0.5;

    if show_vertical {
        let reserve = if show_horizontal {
            style.thickness + style.inset
        } else {
            0.0
        };
        if let Some((track, thumb)) =
            vertical_scrollbar_rects(bounds, metrics, state, style, max_offset.y, reserve)
        {
            push_scrollbar(ctx, track, thumb, style);
        }
    }

    if show_horizontal {
        let reserve = if show_vertical {
            style.thickness + style.inset
        } else {
            0.0
        };
        if let Some((track, thumb)) =
            horizontal_scrollbar_rects(bounds, metrics, state, style, max_offset.x, reserve)
        {
            push_scrollbar(ctx, track, thumb, style);
        }
    }
}

/// Appends track then thumb rounded-rectangle commands.
fn push_scrollbar(ctx: &mut PaintCtx<'_>, track: Rect, thumb: Rect, style: ScrollbarStyle) {
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: track,
        radius: style.radius,
        color: style.track_color,
    }));
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: thumb,
        radius: style.radius,
        color: style.thumb_color,
    }));
}

/// Resolves vertical track/thumb geometry or rejects an unusable track/content.
fn vertical_scrollbar_rects(
    bounds: Rect,
    metrics: ScrollMetrics,
    state: ScrollState,
    style: ScrollbarStyle,
    max_offset_y: f32,
    bottom_reserve: f32,
) -> Option<(Rect, Rect)> {
    let track_h = bounds.h - style.inset * 2.0 - bottom_reserve;
    if track_h <= style.thickness || metrics.content.h <= 0.0 {
        return None;
    }
    let track = Rect::new(
        bounds.right() - style.inset - style.thickness,
        bounds.y + style.inset,
        style.thickness,
        track_h,
    );
    let ratio = (metrics.viewport.h / metrics.content.h).clamp(0.0, 1.0);
    let thumb_h = (track.h * ratio)
        .max(style.min_thumb_len.min(track.h))
        .min(track.h);
    let travel = (track.h - thumb_h).max(0.0);
    let progress = if max_offset_y > 0.0 {
        (state.offset.y / max_offset_y).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb = Rect::new(track.x, track.y + travel * progress, track.w, thumb_h);
    Some((track, thumb))
}

/// Resolves horizontal track/thumb geometry or rejects an unusable track/content.
fn horizontal_scrollbar_rects(
    bounds: Rect,
    metrics: ScrollMetrics,
    state: ScrollState,
    style: ScrollbarStyle,
    max_offset_x: f32,
    right_reserve: f32,
) -> Option<(Rect, Rect)> {
    let track_w = bounds.w - style.inset * 2.0 - right_reserve;
    if track_w <= style.thickness || metrics.content.w <= 0.0 {
        return None;
    }
    let track = Rect::new(
        bounds.x + style.inset,
        bounds.bottom() - style.inset - style.thickness,
        track_w,
        style.thickness,
    );
    let ratio = (metrics.viewport.w / metrics.content.w).clamp(0.0, 1.0);
    let thumb_w = (track.w * ratio)
        .max(style.min_thumb_len.min(track.w))
        .min(track.w);
    let travel = (track.w - thumb_w).max(0.0);
    let progress = if max_offset_x > 0.0 {
        (state.offset.x / max_offset_x).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb = Rect::new(track.x + travel * progress, track.y, thumb_w, track.h);
    Some((track, thumb))
}
