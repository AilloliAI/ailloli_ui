//! VR-friendly window affordance frame.
//!
//! This widget is a framework-level slate/window surface: it owns title-bar,
//! chrome button, resize edge/corner hit zones, and visual feedback. Hosts can
//! map emitted [`WindowAffordanceEvent`] values to desktop window operations or
//! to OpenXR slate transforms.

use std::rc::Rc;

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{ClipShape, Constraints, Point, Rect, Size};
use ailloli_ui_core::style::{
    Border, BorderStyle, BoxShadow, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius,
};
use ailloli_ui_core::{Color, FontId, Offset, TextStyle, Theme};
use ailloli_ui_runtime::component::{ComponentNode, Context, IntoView, Signal, View, Widget};
use ailloli_ui_runtime::input::{EventCtx, HoverCursorRole, ResizeEdge};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawBoxShadow, DrawCmd, DrawRRect, DrawRect};

use super::{hit_resize_frame, hit_window_drag_region};
use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use crate::text::Text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAffordanceKind {
    Move,
    ResizeEdge(ResizeEdge),
    ResizeCorner(ResizeEdge),
    Close,
    Minimize,
    Follow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAffordanceState {
    Idle,
    Hovered,
    Active,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAffordanceDragPhase {
    Start,
    Drag,
    End,
    Click,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowAffordanceEvent {
    pub kind: WindowAffordanceKind,
    pub phase: WindowAffordanceDragPhase,
    pub position: Point,
    pub delta: Offset,
    pub total_delta: Offset,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowAffordanceStyle {
    pub titlebar_height: f32,
    pub control_size: f32,
    pub control_gap: f32,
    pub control_margin: f32,
    pub resize_hit_thickness: f32,
    pub resize_handle_length: f32,
    pub resize_handle_thickness: f32,
    pub radius: f32,
    pub background: Color,
    pub titlebar_background: Color,
    pub border: Color,
    pub shadow: BoxShadow,
    pub control_idle: Color,
    pub control_hover: Color,
    pub control_active: Color,
    pub close_hover: Color,
    pub handle_idle: Color,
    pub handle_hover: Color,
    pub handle_active: Color,
}

impl Default for WindowAffordanceStyle {
    fn default() -> Self {
        let theme = Theme::default();
        let palette = theme.palette();
        Self {
            titlebar_height: 38.0,
            control_size: 24.0,
            control_gap: 6.0,
            control_margin: 8.0,
            resize_hit_thickness: 12.0,
            resize_handle_length: 56.0,
            resize_handle_thickness: 3.0,
            radius: 12.0,
            background: palette.surface,
            titlebar_background: Color::rgba(20, 28, 44, 0.96),
            border: palette.border,
            shadow: BoxShadow::new(0.0, 10.0, 26.0, 0.0, Color::rgba(0, 0, 0, 0.34)),
            control_idle: Color::rgba(148, 163, 184, 0.24),
            control_hover: Color::rgba(148, 163, 184, 0.42),
            control_active: palette.accent,
            close_hover: Color::rgba(239, 68, 68, 0.78),
            handle_idle: Color::rgba(148, 163, 184, 0.18),
            handle_hover: Color::rgba(45, 212, 191, 0.72),
            handle_active: Color::rgba(45, 212, 191, 0.95),
        }
    }
}

type AffordanceHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, WindowAffordanceEvent)>;

pub struct WindowAffordanceFrame<A = ()> {
    layout: LayoutStyle,
    flex_item: FlexItemStyle,
    logical_window_id: String,
    title: String,
    content: Option<View<A>>,
    movable: bool,
    resizable: bool,
    show_controls: bool,
    style: WindowAffordanceStyle,
    on_affordance: Option<AffordanceHandler<A>>,
}

crate::impl_layout_builders!(WindowAffordanceFrame);

impl<A: 'static> WindowAffordanceFrame<A> {
    pub fn new(title: impl Into<String>) -> Self {
        let title = title.into();
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            logical_window_id: "window-affordance".to_string(),
            title,
            content: None,
            movable: true,
            resizable: true,
            show_controls: true,
            style: WindowAffordanceStyle::default(),
            on_affordance: None,
        }
    }

    pub fn logical_window_id(mut self, value: impl Into<String>) -> Self {
        self.logical_window_id = value.into();
        self
    }

    pub fn content(mut self, content: impl IntoView<A>) -> Self {
        self.content = Some(content.into_view());
        self
    }

    pub fn movable(mut self, value: bool) -> Self {
        self.movable = value;
        self
    }

    pub fn resizable(mut self, value: bool) -> Self {
        self.resizable = value;
        self
    }

    pub fn show_controls(mut self, value: bool) -> Self {
        self.show_controls = value;
        self
    }

    pub fn window_affordance_style(mut self, style: WindowAffordanceStyle) -> Self {
        self.style = style;
        self
    }

    pub fn on_affordance(mut self, f: impl Fn(WindowAffordanceEvent) -> A + 'static) -> Self {
        self.on_affordance = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    pub fn on_affordance_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, WindowAffordanceEvent) + 'static,
    ) -> Self {
        self.on_affordance = Some(Rc::new(f));
        self
    }
}

impl<A: 'static> IntoView<A> for WindowAffordanceFrame<A> {
    fn into_view(self) -> View<A> {
        View::component(WindowAffordanceFrameComponent {
            layout: self.layout,
            flex_item: self.flex_item,
            logical_window_id: self.logical_window_id,
            title: self.title,
            content: self.content,
            movable: self.movable,
            resizable: self.resizable,
            show_controls: self.show_controls,
            style: self.style,
            on_affordance: self.on_affordance,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AffordanceDragState {
    kind: WindowAffordanceKind,
    start: Point,
    last: Point,
}

struct WindowAffordanceFrameComponent<A> {
    layout: LayoutStyle,
    flex_item: FlexItemStyle,
    logical_window_id: String,
    title: String,
    content: Option<View<A>>,
    movable: bool,
    resizable: bool,
    show_controls: bool,
    style: WindowAffordanceStyle,
    on_affordance: Option<AffordanceHandler<A>>,
}

impl<A: 'static> ComponentNode<A> for WindowAffordanceFrameComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let mut children = Vec::new();
        children.push(
            Text::new(self.title.clone())
                .style(TextStyle::new(
                    FontId::Ui,
                    15,
                    Color::rgba(236, 243, 255, 0.96),
                ))
                .nowrap()
                .into_view(),
        );
        if let Some(content) = self.content.clone() {
            children.push(content);
        }
        let widget = WindowAffordanceFrameWidget {
            layout: self.layout,
            logical_window_id: self.logical_window_id.clone(),
            movable: self.movable,
            resizable: self.resizable,
            show_controls: self.show_controls,
            style: self.style,
            on_affordance: self.on_affordance.clone(),
            hover: context.signal(None),
            press: context.signal(None),
            drag: context.signal(None),
        };
        finish_view_sized(
            View::node(widget, children),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

struct WindowAffordanceFrameWidget<A> {
    layout: LayoutStyle,
    logical_window_id: String,
    movable: bool,
    resizable: bool,
    show_controls: bool,
    style: WindowAffordanceStyle,
    on_affordance: Option<AffordanceHandler<A>>,
    hover: Signal<Option<WindowAffordanceKind>>,
    press: Signal<Option<WindowAffordanceKind>>,
    drag: Signal<Option<AffordanceDragState>>,
}

impl<A: 'static> Widget<A> for WindowAffordanceFrameWidget<A> {
    fn debug_name(&self) -> &'static str {
        "WindowAffordanceFrame"
    }

    fn layout(
        &self,
        engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = apply_layout_size(Size::new(520.0, 340.0), self.layout, constraints);
        let titlebar_h = self.style.titlebar_height.min(size.h).max(0.0);
        let content_h = (size.h - titlebar_h).max(0.0);
        let mut child_layouts = Vec::new();

        if let Some(title) = children.first_mut() {
            let max_title_w = (size.w
                - self.style.control_margin * 2.0
                - self.style.control_size * 3.0
                - self.style.control_gap * 2.0
                - 20.0)
                .max(0.0);
            let title_layout = title.layout(
                engine,
                ctx,
                Constraints {
                    min_w: 0.0,
                    max_w: max_title_w,
                    min_h: 0.0,
                    max_h: titlebar_h,
                },
            );
            let title_y = ((titlebar_h - title_layout.size.h) * 0.5).max(0.0);
            child_layouts.push(ChildLayout {
                offset: Offset::new(16.0, title_y),
                size: title_layout.size,
                paint_bounds: Rect::new(16.0, title_y, title_layout.size.w, title_layout.size.h),
                visual_bounds: Rect::new(16.0, title_y, title_layout.size.w, title_layout.size.h),
            });
        }

        if let Some(content) = children.get_mut(1) {
            let _ = content.layout(engine, ctx, Constraints::tight(size.w, content_h));
            child_layouts.push(ChildLayout {
                offset: Offset::new(0.0, titlebar_h),
                size: Size::new(size.w, content_h),
                paint_bounds: Rect::new(0.0, titlebar_h, size.w, content_h),
                visual_bounds: Rect::new(0.0, titlebar_h, size.w, content_h),
            });
        }

        let bounds = Rect::new(0.0, 0.0, size.w, size.h);
        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds: bounds,
            visual_bounds: self.style.shadow.paint_bounds(bounds),
            overlay_hit_bounds: Vec::new(),
            clip: Some(ClipShape::round_rect(bounds, self.style.radius)),
            is_window_root_clip: true,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        if !self.style.shadow.inset && self.style.shadow.color.a > 0.0 {
            ctx.push(DrawCmd::BoxShadow(DrawBoxShadow {
                rect: bounds,
                radius: Radius::uniform(self.style.radius),
                shadow: self.style.shadow,
            }));
        }
        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: bounds,
            radius: self.style.radius,
            color: self.style.background,
        }));
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: titlebar_rect(bounds, &self.style),
            color: self.style.titlebar_background,
        }));
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: bounds,
            radius: Radius::uniform(self.style.radius),
            border: Border {
                widths: ailloli_ui_core::EdgeInsets::all(1.0),
                colors: ailloli_ui_core::style::EdgeColors::all(self.style.border),
                style: BorderStyle::Solid,
            },
        }));

        if self.show_controls {
            for (kind, rect) in chrome_control_rects(bounds, &self.style) {
                let state =
                    affordance_state(kind, self.hover.read(), self.press.read(), self.drag.read());
                let color = control_color(kind, state, &self.style);
                ctx.push(DrawCmd::RRect(DrawRRect {
                    rect,
                    radius: 5.0,
                    color,
                }));
            }
        }

        if self.resizable {
            for (kind, rect) in resize_handle_rects(bounds, &self.style) {
                let state =
                    affordance_state(kind, self.hover.read(), self.press.read(), self.drag.read());
                let color = match state {
                    WindowAffordanceState::Active => self.style.handle_active,
                    WindowAffordanceState::Hovered => self.style.handle_hover,
                    WindowAffordanceState::Disabled => Color::TRANSPARENT,
                    WindowAffordanceState::Idle => self.style.handle_idle,
                };
                if color.a > 0.0 {
                    ctx.push(DrawCmd::RRect(DrawRRect {
                        rect,
                        radius: self.style.resize_handle_thickness,
                        color,
                    }));
                }
            }
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        match event {
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                if let Some(drag) = self.drag.read() {
                    let event = drag_event(
                        drag.kind,
                        WindowAffordanceDragPhase::Drag,
                        drag.start,
                        drag.last,
                        *pos,
                    );
                    self.emit(ctx, event);
                    self.drag.set(Some(AffordanceDragState {
                        kind: drag.kind,
                        start: drag.start,
                        last: *pos,
                    }));
                    ctx.request_repaint();
                    ctx.stop_propagation();
                    return;
                }

                let next = classify_window_affordance_hit(
                    bounds,
                    &self.style,
                    *pos,
                    self.movable,
                    self.resizable,
                    self.show_controls,
                );
                if self.hover.read() != next {
                    self.hover.set(next);
                    ctx.request_repaint();
                }
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: true,
                ..
            }) => {
                let Some(kind) = classify_window_affordance_hit(
                    bounds,
                    &self.style,
                    *pos,
                    self.movable,
                    self.resizable,
                    self.show_controls,
                ) else {
                    return;
                };
                self.press.set(Some(kind));
                match kind {
                    WindowAffordanceKind::Move
                    | WindowAffordanceKind::ResizeEdge(_)
                    | WindowAffordanceKind::ResizeCorner(_) => {
                        self.drag.set(Some(AffordanceDragState {
                            kind,
                            start: *pos,
                            last: *pos,
                        }));
                        self.emit(
                            ctx,
                            drag_event(kind, WindowAffordanceDragPhase::Start, *pos, *pos, *pos),
                        );
                    }
                    WindowAffordanceKind::Close
                    | WindowAffordanceKind::Minimize
                    | WindowAffordanceKind::Follow => {}
                }
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
                    self.emit(
                        ctx,
                        drag_event(
                            drag.kind,
                            WindowAffordanceDragPhase::End,
                            drag.start,
                            drag.last,
                            *pos,
                        ),
                    );
                    self.drag.set(None);
                    self.press.set(None);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                    return;
                }

                let pressed = self.press.read();
                self.press.set(None);
                if let Some(kind) = pressed {
                    let released = classify_window_affordance_hit(
                        bounds,
                        &self.style,
                        *pos,
                        self.movable,
                        self.resizable,
                        self.show_controls,
                    );
                    if released == Some(kind) {
                        self.default_chrome_action(ctx, kind);
                        self.emit(
                            ctx,
                            WindowAffordanceEvent {
                                kind,
                                phase: WindowAffordanceDragPhase::Click,
                                position: *pos,
                                delta: Offset::new(0.0, 0.0),
                                total_delta: Offset::new(0.0, 0.0),
                            },
                        );
                    }
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
        _layout: &LayoutResult,
        pos: Point,
    ) -> HoverCursorRole {
        match classify_window_affordance_hit(
            bounds,
            &self.style,
            pos,
            self.movable,
            self.resizable,
            self.show_controls,
        ) {
            Some(WindowAffordanceKind::ResizeEdge(ResizeEdge::E | ResizeEdge::W))
            | Some(WindowAffordanceKind::ResizeCorner(ResizeEdge::NE | ResizeEdge::SW)) => {
                HoverCursorRole::ResizeX
            }
            Some(WindowAffordanceKind::ResizeEdge(ResizeEdge::N | ResizeEdge::S))
            | Some(WindowAffordanceKind::ResizeCorner(ResizeEdge::NW | ResizeEdge::SE)) => {
                HoverCursorRole::ResizeY
            }
            _ => HoverCursorRole::Inherit,
        }
    }
}

impl<A: 'static> WindowAffordanceFrameWidget<A> {
    fn emit(&self, ctx: &mut EventCtx<A>, event: WindowAffordanceEvent) {
        if let Some(handler) = &self.on_affordance {
            handler(ctx, event);
        }
    }

    fn default_chrome_action(&self, ctx: &EventCtx<A>, kind: WindowAffordanceKind) {
        match kind {
            WindowAffordanceKind::Close => ctx.request_close(),
            WindowAffordanceKind::Minimize => {
                ctx.request_minimize_window(self.logical_window_id.clone())
            }
            _ => {}
        }
    }
}

pub fn classify_window_affordance_hit(
    bounds: Rect,
    style: &WindowAffordanceStyle,
    pos: Point,
    movable: bool,
    resizable: bool,
    show_controls: bool,
) -> Option<WindowAffordanceKind> {
    if !bounds.contains(pos.x, pos.y) {
        return None;
    }
    if show_controls {
        for (kind, rect) in chrome_control_rects(bounds, style) {
            if rect.contains(pos.x, pos.y) {
                return Some(kind);
            }
        }
    }
    if let Some(edge) = hit_resize_frame(bounds, style.resize_hit_thickness, pos, resizable) {
        return Some(match edge {
            ResizeEdge::NE | ResizeEdge::NW | ResizeEdge::SE | ResizeEdge::SW => {
                WindowAffordanceKind::ResizeCorner(edge)
            }
            edge => WindowAffordanceKind::ResizeEdge(edge),
        });
    }
    if hit_window_drag_region(titlebar_rect(bounds, style), pos, movable) {
        return Some(WindowAffordanceKind::Move);
    }
    None
}

fn drag_event(
    kind: WindowAffordanceKind,
    phase: WindowAffordanceDragPhase,
    start: Point,
    last: Point,
    pos: Point,
) -> WindowAffordanceEvent {
    WindowAffordanceEvent {
        kind,
        phase,
        position: pos,
        delta: Offset::new(pos.x - last.x, pos.y - last.y),
        total_delta: Offset::new(pos.x - start.x, pos.y - start.y),
    }
}

fn affordance_state(
    kind: WindowAffordanceKind,
    hover: Option<WindowAffordanceKind>,
    press: Option<WindowAffordanceKind>,
    drag: Option<AffordanceDragState>,
) -> WindowAffordanceState {
    if drag.is_some_and(|state| state.kind == kind) || press == Some(kind) {
        WindowAffordanceState::Active
    } else if hover == Some(kind) {
        WindowAffordanceState::Hovered
    } else {
        WindowAffordanceState::Idle
    }
}

fn control_color(
    kind: WindowAffordanceKind,
    state: WindowAffordanceState,
    style: &WindowAffordanceStyle,
) -> Color {
    match (kind, state) {
        (_, WindowAffordanceState::Active) => style.control_active,
        (WindowAffordanceKind::Close, WindowAffordanceState::Hovered) => style.close_hover,
        (_, WindowAffordanceState::Hovered) => style.control_hover,
        (_, WindowAffordanceState::Disabled) => Color::TRANSPARENT,
        _ => style.control_idle,
    }
}

fn titlebar_rect(bounds: Rect, style: &WindowAffordanceStyle) -> Rect {
    Rect::new(
        bounds.x,
        bounds.y,
        bounds.w,
        style.titlebar_height.min(bounds.h).max(0.0),
    )
}

fn chrome_control_rects(
    bounds: Rect,
    style: &WindowAffordanceStyle,
) -> [(WindowAffordanceKind, Rect); 3] {
    let size = style
        .control_size
        .max(1.0)
        .min(style.titlebar_height.max(1.0));
    let y = bounds.y + (style.titlebar_height - size).max(0.0) * 0.5;
    let right = bounds.right() - style.control_margin;
    let step = size + style.control_gap.max(0.0);
    [
        (
            WindowAffordanceKind::Close,
            Rect::new(right - size, y, size, size),
        ),
        (
            WindowAffordanceKind::Minimize,
            Rect::new(right - size - step, y, size, size),
        ),
        (
            WindowAffordanceKind::Follow,
            Rect::new(right - size - step * 2.0, y, size, size),
        ),
    ]
}

fn resize_handle_rects(
    bounds: Rect,
    style: &WindowAffordanceStyle,
) -> [(WindowAffordanceKind, Rect); 8] {
    let l = style.resize_handle_length.min(bounds.w * 0.3).max(8.0);
    let t = style.resize_handle_thickness.max(1.0);
    let mid_x = bounds.x + (bounds.w - l) * 0.5;
    let mid_y = bounds.y + (bounds.h - l) * 0.5;
    [
        (
            WindowAffordanceKind::ResizeEdge(ResizeEdge::N),
            Rect::new(mid_x, bounds.y + 2.0, l, t),
        ),
        (
            WindowAffordanceKind::ResizeEdge(ResizeEdge::S),
            Rect::new(mid_x, bounds.bottom() - t - 2.0, l, t),
        ),
        (
            WindowAffordanceKind::ResizeEdge(ResizeEdge::W),
            Rect::new(bounds.x + 2.0, mid_y, t, l),
        ),
        (
            WindowAffordanceKind::ResizeEdge(ResizeEdge::E),
            Rect::new(bounds.right() - t - 2.0, mid_y, t, l),
        ),
        (
            WindowAffordanceKind::ResizeCorner(ResizeEdge::NW),
            Rect::new(bounds.x + 2.0, bounds.y + 2.0, l * 0.45, t),
        ),
        (
            WindowAffordanceKind::ResizeCorner(ResizeEdge::NE),
            Rect::new(bounds.right() - l * 0.45 - 2.0, bounds.y + 2.0, l * 0.45, t),
        ),
        (
            WindowAffordanceKind::ResizeCorner(ResizeEdge::SW),
            Rect::new(bounds.x + 2.0, bounds.bottom() - t - 2.0, l * 0.45, t),
        ),
        (
            WindowAffordanceKind::ResizeCorner(ResizeEdge::SE),
            Rect::new(
                bounds.right() - l * 0.45 - 2.0,
                bounds.bottom() - t - 2.0,
                l * 0.45,
                t,
            ),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> WindowAffordanceStyle {
        WindowAffordanceStyle {
            titlebar_height: 40.0,
            control_size: 20.0,
            control_margin: 10.0,
            resize_hit_thickness: 10.0,
            ..Default::default()
        }
    }

    #[test]
    fn window_affordance_hit_titlebar_move() {
        let hit = classify_window_affordance_hit(
            Rect::new(0.0, 0.0, 320.0, 220.0),
            &style(),
            Point::new(80.0, 20.0),
            true,
            true,
            true,
        );
        assert_eq!(hit, Some(WindowAffordanceKind::Move));
    }

    #[test]
    fn window_affordance_controls_win_over_resize_border() {
        let bounds = Rect::new(0.0, 0.0, 320.0, 220.0);
        let close = chrome_control_rects(bounds, &style())[0].1;
        let hit = classify_window_affordance_hit(
            bounds,
            &style(),
            Point::new(close.x + 4.0, close.y + 4.0),
            true,
            true,
            true,
        );
        assert_eq!(hit, Some(WindowAffordanceKind::Close));
    }

    #[test]
    fn window_affordance_hit_edges_and_corners() {
        let bounds = Rect::new(0.0, 0.0, 320.0, 220.0);
        let style = style();
        assert_eq!(
            classify_window_affordance_hit(bounds, &style, Point::new(3.0, 90.0), true, true, true,),
            Some(WindowAffordanceKind::ResizeEdge(ResizeEdge::W))
        );
        assert_eq!(
            classify_window_affordance_hit(bounds, &style, Point::new(3.0, 3.0), true, true, true,),
            Some(WindowAffordanceKind::ResizeCorner(ResizeEdge::NW))
        );
    }

    #[test]
    fn window_affordance_content_has_no_frame_hit() {
        let hit = classify_window_affordance_hit(
            Rect::new(0.0, 0.0, 320.0, 220.0),
            &style(),
            Point::new(120.0, 80.0),
            true,
            true,
            true,
        );
        assert_eq!(hit, None);
    }

    #[test]
    fn window_affordance_disable_move_or_resize() {
        let bounds = Rect::new(0.0, 0.0, 320.0, 220.0);
        let style = style();
        assert_eq!(
            classify_window_affordance_hit(
                bounds,
                &style,
                Point::new(80.0, 20.0),
                false,
                true,
                true,
            ),
            None
        );
        assert_eq!(
            classify_window_affordance_hit(
                bounds,
                &style,
                Point::new(3.0, 90.0),
                true,
                false,
                true,
            ),
            None
        );
    }
}
