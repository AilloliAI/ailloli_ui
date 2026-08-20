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
use ailloli_ui_runtime::{DrawCmd, DrawRRect};

use super::layout_ext::{apply_layout_size, finish_view_sized};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeAxis {
    X,
    Y,
}

impl ResizeAxis {
    pub fn cursor_role(self) -> HoverCursorRole {
        match self {
            Self::X => HoverCursorRole::ResizeX,
            Self::Y => HoverCursorRole::ResizeY,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeDragPhase {
    Start,
    Drag,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitResizeEvent {
    pub axis: ResizeAxis,
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
pub struct ResizeBarStyle {
    pub hit_thickness: f32,
    pub line_thickness: f32,
    pub idle_color: Color,
    pub hover_color: Color,
    pub active_color: Color,
    pub radius: f32,
    pub vertical_extent: f32,
    pub horizontal_extent: f32,
}

impl Default for ResizeBarStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl ResizeBarStyle {
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

    pub fn intrinsic_size(&self, axis: ResizeAxis) -> Size {
        match axis {
            ResizeAxis::X => Size::new(self.hit_thickness, self.vertical_extent),
            ResizeAxis::Y => Size::new(self.horizontal_extent, self.hit_thickness),
        }
    }
}

type ResizeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, SplitResizeEvent)>;

pub struct ResizeBar<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    axis: ResizeAxis,
    style: ResizeBarStyle,
    on_resize: Option<ResizeHandler<A>>,
}

crate::impl_layout_builders!(ResizeBar);

impl<A: 'static> ResizeBar<A> {
    pub fn vertical() -> Self {
        Self::new(ResizeAxis::X)
    }

    pub fn horizontal() -> Self {
        Self::new(ResizeAxis::Y)
    }

    pub fn new(axis: ResizeAxis) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            axis,
            style: ResizeBarStyle::default(),
            on_resize: None,
        }
    }

    pub fn resize_bar_style(mut self, style: ResizeBarStyle) -> Self {
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

impl<A: 'static> Default for ResizeBar<A> {
    fn default() -> Self {
        Self::vertical()
    }
}

struct ResizeBarComponent<A> {
    layout: LayoutStyle,
    axis: ResizeAxis,
    style: ResizeBarStyle,
    on_resize: Option<ResizeHandler<A>>,
}

impl<A: 'static> ComponentNode<A> for ResizeBarComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        View::leaf(ResizeBarWidget {
            layout: self.layout,
            axis: self.axis,
            style: self.style,
            on_resize: self.on_resize.clone(),
            drag: context.signal(None),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ResizeDragState {
    pub start: Point,
    pub last: Point,
}

struct ResizeBarWidget<A> {
    layout: LayoutStyle,
    axis: ResizeAxis,
    style: ResizeBarStyle,
    on_resize: Option<ResizeHandler<A>>,
    drag: Signal<Option<ResizeDragState>>,
}

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

impl<A: 'static> ResizeBarWidget<A> {
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

pub(crate) fn axis_value(axis: ResizeAxis, point: Point) -> f32 {
    match axis {
        ResizeAxis::X => point.x,
        ResizeAxis::Y => point.y,
    }
}

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
