//! Single-child clipped scrolling, virtualization hints, follow-end, and bars.

use std::cell::Cell;

use ailloli_ui_core::event::{Event, PointerEvent};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::{
    ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometry,
    ScrollbarGeometrySpec,
};
use ailloli_ui_core::style::FlexItemStyle;
use ailloli_ui_core::{Color, Offset};
use ailloli_ui_runtime::component::reactive::with_untracked_reads;
use ailloli_ui_runtime::component::{ComponentNode, Context, IntoView, Signal, View, Widget};
use ailloli_ui_runtime::input::EventCtx;
use ailloli_ui_runtime::layout::LayoutEngine;
use ailloli_ui_runtime::layout::{
    ChildLayout, LayoutChild, LayoutCtx, LayoutResult, VirtualViewport,
};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawCmd, DrawRRect, Invalidation};

use crate::scrollbar::{thumb_color_for_state, ScrollbarInteraction};
use crate::transactional_layout::TransactionalLayoutPending;

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
/// current offset. Visible bars support thumb drag and centered track clicks;
/// wheel events bubble when the viewport is already at its requested limit.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::{layout::ScrollView, text::Text};
/// let scroll: ScrollView<()> = ScrollView::vertical().child(Text::new("content"));
/// let _ = scroll;
/// ```
pub struct ScrollView<A = ()> {
    /// Axes on which content may exceed the viewport and scroll.
    axes: ScrollAxes,
    /// Initial content offset in logical pixels.
    initial_offset: Offset,
    /// Wheel scaling and axis-filtering policy.
    behavior: ScrollBehavior,
    /// Whether content growth should remain pinned to its end edge.
    follow_end: bool,
    /// Whether overflow paints visual scrollbars.
    scrollbars: bool,
    /// Scrollbar colors and logical-pixel geometry.
    scrollbar_style: ScrollbarStyle,
    /// Optional sole content child.
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

    /// Shows or hides interactive overflow scrollbars.
    ///
    /// Hidden bars expose no pointer hit regions; wheel scrolling and clipping
    /// remain active.
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
    /// Axes on which content may scroll.
    axes: ScrollAxes,
    /// Initial retained content offset in logical pixels.
    initial_offset: Offset,
    /// Wheel scaling and axis-filtering policy.
    behavior: ScrollBehavior,
    /// Whether content growth should remain pinned to its end edge.
    follow_end: bool,
    /// Whether overflow paints visual scrollbars.
    scrollbars: bool,
    /// Scrollbar colors and logical-pixel geometry.
    scrollbar_style: ScrollbarStyle,
    /// Optional sole content child.
    child: Option<View<A>>,
}

/// Builds the viewport widget and preserves its optional content child.
impl<A: 'static> ComponentNode<A> for ScrollViewComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let state = context.signal_with_invalidation(
            ScrollState::with_offset(self.initial_offset),
            Invalidation::Paint,
        );
        let follow_end_active =
            context.signal_with_invalidation(self.follow_end, Invalidation::Paint);
        let scrollbar_interaction =
            context.signal_with_invalidation(ScrollbarInteraction::default(), Invalidation::Paint);
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
                scrollbar_interaction,
                pending_layout: Cell::new(None),
            },
            children,
        )
    }
}

/// Stateful retained viewport implementation.
struct ScrollViewWidget {
    /// Axes on which content may scroll.
    axes: ScrollAxes,
    /// Retained content offset in logical pixels.
    state: Signal<ScrollState>,
    /// Wheel scaling and axis-filtering policy.
    behavior: ScrollBehavior,
    /// Whether end-following is enabled for this viewport.
    follow_end: bool,
    /// Whether the viewport currently remains attached to the content end.
    follow_end_active: Signal<bool>,
    /// Whether overflow paints visual scrollbars.
    scrollbars: bool,
    /// Scrollbar colors and logical-pixel geometry.
    scrollbar_style: ScrollbarStyle,
    /// Retained hover and captured scrollbar gesture.
    scrollbar_interaction: Signal<ScrollbarInteraction>,
    /// Geometry-derived state owned by one exact authoritative attempt.
    pending_layout: Cell<Option<TransactionalLayoutPending<PendingScrollViewLayout>>>,
}

/// State published only by the successful attempt that computed it.
#[derive(Clone, Copy)]
struct PendingScrollViewLayout {
    /// Final authoritative clamp.
    state: Option<ScrollState>,
    /// Final authoritative follow-end transition.
    follow_end_active: Option<bool>,
    /// Geometry-dependent gesture cleanup.
    scrollbar_interaction: Option<ScrollbarInteraction>,
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
        // `state` is owned by this exact retained widget. Its pre-clamp value is
        // administrative because only the final authoritative pass may publish
        // the geometry-dependent clamp in `layout_committed`.
        let mut state = with_untracked_reads(|| self.state.read());
        let mut next_follow_end_active = None;
        let mut persist_state = false;
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
                let (next_state, next_follow) = self.sync_scroll_state_for_layout(state, metrics);
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
                next_follow_end_active = next_follow;
                persist_state = true;
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

        let scrollbar_geometries = if self.scrollbars {
            let metrics = child_layouts
                .first()
                .map(|child| ScrollMetrics::new(size, child.size));
            metrics
                .map(|metrics| {
                    scrollbars_for(viewport, metrics, state, self.axes, self.scrollbar_style)
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        // Reconciliation can clear a gesture against authoritative geometry.
        // Its context-owned Paint invalidator is the static ownership edge;
        // observing the pre-reconcile value would supersede this layout when
        // the cleanup writes the new value below.
        let mut interaction = with_untracked_reads(|| self.scrollbar_interaction.read());
        let interaction_changed = interaction.reconcile(ctx.layout_pass(), &scrollbar_geometries);
        if ctx.layout_pass().is_committed() {
            let retained_state = with_untracked_reads(|| self.state.read());
            self.pending_layout.set(TransactionalLayoutPending::new(
                ctx,
                PendingScrollViewLayout {
                    state: (persist_state && !same_offset(retained_state.offset, state.offset))
                        .then_some(state),
                    follow_end_active: next_follow_end_active,
                    scrollbar_interaction: interaction_changed.then_some(interaction),
                },
            ));
        }

        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds: viewport,
            visual_bounds: viewport,
            overlay_hit_bounds: scrollbar_geometries
                .iter()
                .map(|geometry| geometry.hit_track)
                .collect(),
            clip: Some(ailloli_ui_core::ClipShape::Rect(viewport)),
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn layout_committed(&self, ctx: &mut LayoutCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        let Some(pending) = self
            .pending_layout
            .take()
            .and_then(|pending| pending.into_committed(ctx))
        else {
            return;
        };
        if let Some(state) = pending.state {
            self.state.set(state);
        }
        if let Some(active) = pending.follow_end_active {
            self.follow_end_active.set(active);
        }
        if let Some(interaction) = pending.scrollbar_interaction {
            self.scrollbar_interaction.set(interaction);
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
        let geometries = scrollbars_for(bounds, metrics, state, self.axes, self.scrollbar_style);
        paint_scrollbars(
            ctx,
            &geometries,
            self.scrollbar_style,
            self.scrollbar_interaction.read(),
        );
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, layout: &LayoutResult) {
        let Some(child) = layout.children.first() else {
            return;
        };

        let metrics = ScrollMetrics::new(layout.size, child.size);
        if self.scrollbars {
            let geometries = scrollbars_for(
                bounds,
                metrics,
                self.state.read(),
                self.axes,
                self.scrollbar_style,
            );
            let mut interaction = self.scrollbar_interaction.read();
            let response = interaction.handle_event(ctx, event, &geometries);
            if response.state_changed {
                self.scrollbar_interaction.set(interaction);
            }
            let mut scrolled = false;
            if let Some((axis, target)) = response.scroll_to {
                let state = self.state.read();
                let target = match axis {
                    ScrollbarAxis::Horizontal => Offset::new(target, state.offset.y),
                    ScrollbarAxis::Vertical => Offset::new(state.offset.x, target),
                };
                let outcome = state.scroll_to(target, metrics, self.axes);
                if outcome.changed {
                    self.state.set(outcome.state());
                    self.sync_follow_end_after_manual_scroll(outcome.after, outcome.max_offset);
                    scrolled = true;
                }
            }
            if response.repaint || scrolled {
                if scrolled {
                    ctx.request_layout();
                } else {
                    ctx.request_repaint();
                }
            }
            if response.consumed {
                ctx.stop_propagation();
                return;
            }
        }

        if let Event::Pointer(PointerEvent::Wheel {
            delta, modifiers, ..
        }) = event
        {
            let state = self.state.read();
            let outcome = state.scroll_by(
                self.behavior.wheel_delta_with_modifiers(*delta, *modifiers),
                metrics,
                self.axes,
            );
            if outcome.changed {
                self.state.set(outcome.state());
                self.sync_follow_end_after_manual_scroll(outcome.after, outcome.max_offset);
                ctx.request_layout();
                ctx.stop_propagation();
            }
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
    /// Returns whether every enabled scroll axis has a finite viewport extent.
    fn has_bounded_scroll_viewport(&self, viewport: Size) -> bool {
        (!self.axes.horizontal || viewport.w.is_finite())
            && (!self.axes.vertical || viewport.h.is_finite())
    }

    /// Clamps offset and applies the retained follow-end policy during layout.
    fn sync_scroll_state_for_layout(
        &self,
        state: ScrollState,
        metrics: ScrollMetrics,
    ) -> (ScrollState, Option<bool>) {
        if !self.follow_end {
            let clamped = state.clamp_to(metrics, self.axes);
            return (clamped.state(), None);
        }

        let max_offset = self.axes.filter_offset(metrics.max_offset());
        let follow_active = with_untracked_reads(|| self.follow_end_active.read());
        let mut next_follow_active = follow_active;
        let target = if follow_active {
            ScrollState::with_offset(max_offset)
        } else {
            let clamped = state.clamp_to(metrics, self.axes).state();
            if offset_at_end(clamped.offset, max_offset, self.axes) {
                next_follow_active = true;
            }
            clamped
        };
        (
            target,
            (next_follow_active != follow_active).then_some(next_follow_active),
        )
    }

    /// Updates end-following after a wheel or other manual offset change.
    fn sync_follow_end_after_manual_scroll(&self, offset: Offset, max_offset: Offset) {
        if !self.follow_end {
            return;
        }
        self.set_follow_end_active(offset_at_end(offset, max_offset, self.axes));
    }

    /// Writes the follow-end flag only when its value changes.
    fn set_follow_end_active(&self, active: bool) {
        if with_untracked_reads(|| self.follow_end_active.read()) != active {
            self.follow_end_active.set(active);
        }
    }

    /// Makes enabled axes unbounded while preserving the viewport on other axes.
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
    geometries: &[ScrollbarGeometry],
    style: ScrollbarStyle,
    interaction: ScrollbarInteraction,
) {
    for geometry in geometries.iter().copied() {
        push_scrollbar(ctx, geometry, style, interaction);
    }
}

/// Appends track then thumb rounded-rectangle commands.
fn push_scrollbar(
    ctx: &mut PaintCtx<'_>,
    geometry: ScrollbarGeometry,
    style: ScrollbarStyle,
    interaction: ScrollbarInteraction,
) {
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: geometry.track,
        radius: style.radius,
        color: style.track_color,
    }));
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: geometry.thumb,
        radius: style.radius,
        color: thumb_color_for_state(
            style.thumb_color,
            interaction.visual_state(geometry.axis, ctx.is_hovered()),
        ),
    }));
}

/// Resolves every enabled overflowing axis with shared Core geometry.
fn scrollbars_for(
    bounds: Rect,
    metrics: ScrollMetrics,
    state: ScrollState,
    axes: ScrollAxes,
    style: ScrollbarStyle,
) -> Vec<ScrollbarGeometry> {
    let max = metrics.max_offset();
    let show_horizontal = axes.horizontal && max.x > 0.5;
    let show_vertical = axes.vertical && max.y > 0.5;
    let reserve = style.thickness + style.inset;
    let mut geometries = Vec::with_capacity(2);
    if show_vertical {
        let spec = ScrollbarGeometrySpec::new(ScrollbarAxis::Vertical, bounds, metrics, state)
            .with_paint_metrics(style.thickness, style.min_thumb_len, style.inset)
            .with_end_reserve(if show_horizontal { reserve } else { 0.0 });
        if let Some(geometry) = spec.resolve() {
            geometries.push(geometry);
        }
    }
    if show_horizontal {
        let spec = ScrollbarGeometrySpec::new(ScrollbarAxis::Horizontal, bounds, metrics, state)
            .with_paint_metrics(style.thickness, style.min_thumb_len, style.inset)
            .with_end_reserve(if show_vertical { reserve } else { 0.0 });
        if let Some(geometry) = spec.resolve() {
            geometries.push(geometry);
        }
    }
    geometries
}
