use std::rc::Rc;

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Point, Rect, Size};
use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_core::{Offset, Theme};
use ailloli_ui_runtime::component::{ComponentNode, Context, IntoView, Signal, View, Widget};
use ailloli_ui_runtime::input::{EventCtx, HoverCursorRole};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;

use super::layout_ext::{apply_layout_size, finish_view_sized};
use super::resize_bar::{
    axis_value, paint_resize_line, ResizeAxis, ResizeBarStyle, ResizeDragPhase, ResizeDragState,
    SplitResizeEvent,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitPaneStyle {
    pub resize_bar: ResizeBarStyle,
}

impl Default for SplitPaneStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl SplitPaneStyle {
    pub fn from_theme(theme: Theme) -> Self {
        Self {
            resize_bar: ResizeBarStyle::from_theme(theme),
        }
    }
}

type ResizeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, SplitResizeEvent)>;

#[derive(Clone, Copy, Debug, PartialEq)]
enum SplitPaneInitialPosition {
    Start(f32),
    End(f32),
}

pub struct SplitPane<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    axis: ResizeAxis,
    start: View<A>,
    end: View<A>,
    initial_position: Option<SplitPaneInitialPosition>,
    bound_position: Option<Signal<f32>>,
    min_start: f32,
    min_end: f32,
    style: SplitPaneStyle,
    on_resize: Option<ResizeHandler<A>>,
}

crate::impl_layout_builders!(SplitPane);

impl<A: 'static> SplitPane<A> {
    pub fn columns(start: impl IntoView<A>, end: impl IntoView<A>) -> Self {
        Self::new(ResizeAxis::X, start, end)
    }

    pub fn rows(start: impl IntoView<A>, end: impl IntoView<A>) -> Self {
        Self::new(ResizeAxis::Y, start, end)
    }

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

    pub fn initial_position(mut self, position: f32) -> Self {
        self.initial_position = Some(SplitPaneInitialPosition::Start(position.max(0.0)));
        self
    }

    pub fn initial_end_position(mut self, position: f32) -> Self {
        self.initial_position = Some(SplitPaneInitialPosition::End(position.max(0.0)));
        self
    }

    pub fn bind_position(mut self, position: impl Into<Signal<f32>>) -> Self {
        self.bound_position = Some(position.into());
        self
    }

    pub fn min_start(mut self, value: f32) -> Self {
        self.min_start = value.max(0.0);
        self
    }

    pub fn min_end(mut self, value: f32) -> Self {
        self.min_end = value.max(0.0);
        self
    }

    pub fn split_pane_style(mut self, style: SplitPaneStyle) -> Self {
        self.style = style;
        self
    }

    pub fn on_resize(mut self, f: impl Fn(SplitResizeEvent) -> A + 'static) -> Self {
        self.on_resize = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    pub fn on_resize_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, SplitResizeEvent) + 'static,
    ) -> Self {
        self.on_resize = Some(Rc::new(f));
        self
    }
}

struct SplitPaneComponent<A> {
    layout: LayoutStyle,
    axis: ResizeAxis,
    start: View<A>,
    end: View<A>,
    initial_position: Option<SplitPaneInitialPosition>,
    bound_position: Option<Signal<f32>>,
    min_start: f32,
    min_end: f32,
    style: SplitPaneStyle,
    on_resize: Option<ResizeHandler<A>>,
}

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
struct SplitPaneDragState {
    pointer: ResizeDragState,
    start_position: f32,
    last_position: f32,
}

struct SplitPaneWidget<A> {
    layout: LayoutStyle,
    axis: ResizeAxis,
    initial_position: Option<SplitPaneInitialPosition>,
    bound_position: Option<Signal<f32>>,
    local_position: Signal<Option<f32>>,
    min_start: f32,
    min_end: f32,
    style: SplitPaneStyle,
    on_resize: Option<ResizeHandler<A>>,
    drag: Signal<Option<SplitPaneDragState>>,
    hover_seam: Signal<bool>,
}

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
            _ => {}
        }
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

impl<A: 'static> SplitPaneWidget<A> {
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

    fn clamp_position(&self, available: f32, position: f32) -> f32 {
        let max = (available - self.min_end).max(self.min_start);
        position.clamp(self.min_start.min(max), max)
    }

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

    fn seam_rect_abs(&self, bounds: Rect, layout: &LayoutResult) -> Rect {
        let available = main_extent(self.axis, layout.size);
        self.seam_rect_for(layout.size, self.current_position(available))
            .translate(Offset::new(bounds.x, bounds.y))
    }
}

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

fn main_extent(axis: ResizeAxis, size: Size) -> f32 {
    match axis {
        ResizeAxis::X => size.w,
        ResizeAxis::Y => size.h,
    }
}

fn size_with_main(axis: ResizeAxis, container: Size, main: f32) -> Size {
    match axis {
        ResizeAxis::X => Size::new(main.max(0.0), container.h),
        ResizeAxis::Y => Size::new(container.w, main.max(0.0)),
    }
}
