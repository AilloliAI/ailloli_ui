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
/// Semantic operation represented by a chrome hit target.
///
/// Edge and corner variants retain the exact host [`ResizeEdge`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::input::ResizeEdge;
/// use ailloli_ui_widgets::chrome::WindowAffordanceKind;
/// assert_eq!(WindowAffordanceKind::ResizeCorner(ResizeEdge::NW), WindowAffordanceKind::ResizeCorner(ResizeEdge::NW));
/// ```
pub enum WindowAffordanceKind {
    /// Drag the complete window or spatial slate.
    Move,
    /// Resize from one cardinal edge.
    ResizeEdge(ResizeEdge),
    /// Resize from one diagonal corner.
    ResizeCorner(ResizeEdge),
    /// Request closure.
    Close,
    /// Request minimization.
    Minimize,
    /// Host-defined follow/pin operation.
    Follow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Visual interaction state of one affordance.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::chrome::WindowAffordanceState;
/// assert_ne!(WindowAffordanceState::Idle, WindowAffordanceState::Active);
/// ```
pub enum WindowAffordanceState {
    /// Neither hovered nor pressed.
    Idle,
    /// Pointer is over the affordance.
    Hovered,
    /// Pointer is pressing or dragging the affordance.
    Active,
    /// Operation is unavailable; currently used by styling helpers.
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Lifecycle phase emitted for an affordance interaction.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::chrome::WindowAffordanceDragPhase;
/// assert_ne!(WindowAffordanceDragPhase::Start, WindowAffordanceDragPhase::End);
/// ```
pub enum WindowAffordanceDragPhase {
    /// Initial left-button press on a movable/resizable region.
    Start,
    /// Pointer motion while a drag is captured.
    Drag,
    /// Left-button release after a drag.
    End,
    /// Press and release on the same non-drag chrome control.
    Click,
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// Provider-neutral chrome interaction emitted in screen logical pixels.
///
/// `delta` is relative to the previous drag event and `total_delta` is relative
/// to the drag start. Clicks use zero offsets.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Offset, Point};
/// use ailloli_ui_widgets::chrome::{WindowAffordanceDragPhase, WindowAffordanceEvent, WindowAffordanceKind};
/// let event = WindowAffordanceEvent {
///     kind: WindowAffordanceKind::Move,
///     phase: WindowAffordanceDragPhase::Drag,
///     position: Point::new(12.0, 8.0),
///     delta: Offset::new(2.0, 0.0),
///     total_delta: Offset::new(4.0, 1.0),
/// };
/// assert_eq!(event.delta.x, 2.0);
/// ```
pub struct WindowAffordanceEvent {
    /// Operation selected by hit testing.
    pub kind: WindowAffordanceKind,
    /// Press/drag/release/click lifecycle phase.
    pub phase: WindowAffordanceDragPhase,
    /// Current pointer position in screen logical pixels.
    pub position: Point,
    /// Logical-pixel displacement since the previous event.
    pub delta: Offset,
    /// Logical-pixel displacement since drag start.
    pub total_delta: Offset,
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// Geometry and colors for [`WindowAffordanceFrame`].
///
/// All dimensions are logical pixels. The default uses a 38-pixel title bar,
/// 12-pixel resize hit frame, three 24-pixel controls, and a 12-pixel radius.
/// Values are sanitized only where consumed, so custom styles should use
/// finite non-negative dimensions.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::chrome::WindowAffordanceStyle;
/// let style = WindowAffordanceStyle::default();
/// assert_eq!(style.titlebar_height, 38.0);
/// assert_eq!(style.resize_hit_thickness, 12.0);
/// ```
pub struct WindowAffordanceStyle {
    /// Title-bar height in logical pixels.
    pub titlebar_height: f32,
    /// Square chrome-control extent in logical pixels.
    pub control_size: f32,
    /// Horizontal gap between chrome controls in logical pixels.
    pub control_gap: f32,
    /// Distance from the right bound to the close control.
    pub control_margin: f32,
    /// Invisible resize-hit frame thickness in logical pixels.
    pub resize_hit_thickness: f32,
    /// Visible resize-handle length in logical pixels.
    pub resize_handle_length: f32,
    /// Visible resize-handle thickness in logical pixels.
    pub resize_handle_thickness: f32,
    /// Frame corner radius in logical pixels.
    pub radius: f32,
    /// Content surface color.
    pub background: Color,
    /// Title-bar fill color.
    pub titlebar_background: Color,
    /// One-logical-pixel frame border color.
    pub border: Color,
    /// Outer frame shadow; inset shadows are skipped.
    pub shadow: BoxShadow,
    /// Idle chrome-control fill.
    pub control_idle: Color,
    /// Hovered chrome-control fill.
    pub control_hover: Color,
    /// Pressed chrome-control fill.
    pub control_active: Color,
    /// Destructive close-control hover fill.
    pub close_hover: Color,
    /// Idle visible resize-handle fill.
    pub handle_idle: Color,
    /// Hovered resize-handle fill.
    pub handle_hover: Color,
    /// Active resize-handle fill.
    pub handle_active: Color,
}

/// Supplies the dark-theme defaults described by [`WindowAffordanceStyle`].
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

/// Shared retained callback invoked for each emitted affordance event.
type AffordanceHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, WindowAffordanceEvent)>;

/// Retained frame that paints client chrome and emits provider-neutral actions.
///
/// The default logical window id is `"window-affordance"`; movement, resizing,
/// and the three chrome controls are enabled. The optional content is placed
/// below the title bar.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::chrome::WindowAffordanceFrame;
/// let frame: WindowAffordanceFrame<()> = WindowAffordanceFrame::new("Inspector");
/// let _ = frame;
/// ```
pub struct WindowAffordanceFrame<A = ()> {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Parent-flex participation metadata.
    flex_item: FlexItemStyle,
    /// Logical host-window identity used by window commands.
    logical_window_id: String,
    /// User-visible title painted in the chrome row.
    title: String,
    /// Optional body view placed below the title bar.
    content: Option<View<A>>,
    /// Whether title-bar hit testing emits move gestures.
    movable: bool,
    /// Whether edge/corner hit testing emits resize gestures.
    resizable: bool,
    /// Whether close, minimize, and follow controls are painted and interactive.
    show_controls: bool,
    /// Chrome colors and logical-pixel geometry.
    style: WindowAffordanceStyle,
    /// Optional callback receiving semantic chrome events.
    on_affordance: Option<AffordanceHandler<A>>,
}

crate::impl_layout_builders!(WindowAffordanceFrame);

impl<A: 'static> WindowAffordanceFrame<A> {
    /// Creates a default movable and resizable frame with `title`.
    ///
    /// Empty titles are accepted and displayed as empty text.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::chrome::WindowAffordanceFrame;
    /// let frame: WindowAffordanceFrame<()> = WindowAffordanceFrame::new("Tools");
    /// let _ = frame;
    /// ```
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

    /// Sets the host logical id used by built-in minimize requests.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::chrome::WindowAffordanceFrame;
    /// let frame: WindowAffordanceFrame<()> = WindowAffordanceFrame::new("Tools").logical_window_id("tools");
    /// let _ = frame;
    /// ```
    pub fn logical_window_id(mut self, value: impl Into<String>) -> Self {
        self.logical_window_id = value.into();
        self
    }

    /// Installs the single child view below the title bar.
    ///
    /// Repeated calls replace the previous child.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{chrome::WindowAffordanceFrame, text::Text};
    /// let frame: WindowAffordanceFrame<()> = WindowAffordanceFrame::new("Tools").content(Text::new("Body"));
    /// let _ = frame;
    /// ```
    pub fn content(mut self, content: impl IntoView<A>) -> Self {
        self.content = Some(content.into_view());
        self
    }

    /// Enables or disables title-bar move hit testing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::chrome::WindowAffordanceFrame;
    /// let frame: WindowAffordanceFrame<()> = WindowAffordanceFrame::new("Fixed").movable(false);
    /// let _ = frame;
    /// ```
    pub fn movable(mut self, value: bool) -> Self {
        self.movable = value;
        self
    }

    /// Enables or disables edge and corner resize hit testing and handles.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::chrome::WindowAffordanceFrame;
    /// let frame: WindowAffordanceFrame<()> = WindowAffordanceFrame::new("Fixed").resizable(false);
    /// let _ = frame;
    /// ```
    pub fn resizable(mut self, value: bool) -> Self {
        self.resizable = value;
        self
    }

    /// Shows or hides close, minimize, and follow controls.
    ///
    /// Hiding controls removes both paint and hit targets.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::chrome::WindowAffordanceFrame;
    /// let frame: WindowAffordanceFrame<()> = WindowAffordanceFrame::new("Bare").show_controls(false);
    /// let _ = frame;
    /// ```
    pub fn show_controls(mut self, value: bool) -> Self {
        self.show_controls = value;
        self
    }

    /// Replaces every chrome geometry and color token.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::chrome::{WindowAffordanceFrame, WindowAffordanceStyle};
    /// let style = WindowAffordanceStyle { titlebar_height: 42.0, ..Default::default() };
    /// let frame: WindowAffordanceFrame<()> = WindowAffordanceFrame::new("Tools").window_affordance_style(style);
    /// let _ = frame;
    /// ```
    pub fn window_affordance_style(mut self, style: WindowAffordanceStyle) -> Self {
        self.style = style;
        self
    }

    /// Maps each emitted event into an application action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::chrome::{WindowAffordanceEvent, WindowAffordanceFrame};
    /// enum Action { Chrome(WindowAffordanceEvent) }
    /// let frame = WindowAffordanceFrame::new("Tools").on_affordance(Action::Chrome);
    /// let _: WindowAffordanceFrame<Action> = frame;
    /// ```
    pub fn on_affordance(mut self, f: impl Fn(WindowAffordanceEvent) -> A + 'static) -> Self {
        self.on_affordance = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    /// Handles emitted events with mutable runtime event context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::chrome::WindowAffordanceFrame;
    /// let frame: WindowAffordanceFrame<()> = WindowAffordanceFrame::new("Tools")
    ///     .on_affordance_ctx(|_ctx, _event| {});
    /// let _ = frame;
    /// ```
    pub fn on_affordance_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, WindowAffordanceEvent) + 'static,
    ) -> Self {
        self.on_affordance = Some(Rc::new(f));
        self
    }
}

/// Converts the builder into its retained frame component.
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
/// Captured drag origin and previous position for delta generation.
struct AffordanceDragState {
    /// Move, edge, or corner affordance captured by the press.
    kind: WindowAffordanceKind,
    /// Logical window-coordinate pointer position at drag start.
    start: Point,
    /// Pointer position used as the next incremental-delta origin.
    last: Point,
}

/// Retained builder snapshot rebuilt into the widget and its child views.
struct WindowAffordanceFrameComponent<A> {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Parent-flex participation metadata.
    flex_item: FlexItemStyle,
    /// Logical host-window identity used by window commands.
    logical_window_id: String,
    /// User-visible title painted in the chrome row.
    title: String,
    /// Optional body view placed below the title bar.
    content: Option<View<A>>,
    /// Whether title-bar hit testing emits move gestures.
    movable: bool,
    /// Whether edge/corner hit testing emits resize gestures.
    resizable: bool,
    /// Whether window controls are painted and interactive.
    show_controls: bool,
    /// Chrome colors and logical-pixel geometry.
    style: WindowAffordanceStyle,
    /// Optional retained semantic-event callback.
    on_affordance: Option<AffordanceHandler<A>>,
}

/// Builds the title/content children and persistent interaction signals.
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

/// Layout, paint, and pointer-event implementation for the chrome frame.
struct WindowAffordanceFrameWidget<A> {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Logical host-window identity used by built-in commands.
    logical_window_id: String,
    /// Whether title-bar hit testing emits move gestures.
    movable: bool,
    /// Whether edge/corner hit testing emits resize gestures.
    resizable: bool,
    /// Whether window controls are painted and interactive.
    show_controls: bool,
    /// Chrome colors and logical-pixel geometry.
    style: WindowAffordanceStyle,
    /// Optional retained semantic-event callback.
    on_affordance: Option<AffordanceHandler<A>>,
    /// Affordance currently under the pointer.
    hover: Signal<Option<WindowAffordanceKind>>,
    /// Affordance captured by the active press before dragging.
    press: Signal<Option<WindowAffordanceKind>>,
    /// Active move/resize gesture state.
    drag: Signal<Option<AffordanceDragState>>,
}

/// Implements retained chrome geometry, painting, capture, and cursor roles.
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

/// Callback dispatch and built-in host chrome actions.
impl<A: 'static> WindowAffordanceFrameWidget<A> {
    /// Forwards a semantic chrome event to the optional application callback.
    fn emit(&self, ctx: &mut EventCtx<A>, event: WindowAffordanceEvent) {
        if let Some(handler) = &self.on_affordance {
            handler(ctx, event);
        }
    }

    /// Executes close/minimize host commands when no custom behavior is needed.
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

/// Resolves a point to the highest-priority enabled chrome affordance.
///
/// Points outside `bounds` return `None`. Visible controls win over resize
/// regions, corners win over edges, and move is considered last. Dimensions
/// are interpreted as screen logical pixels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Point, Rect};
/// use ailloli_ui_widgets::chrome::{classify_window_affordance_hit, WindowAffordanceKind, WindowAffordanceStyle};
/// let hit = classify_window_affordance_hit(
///     Rect::new(0.0, 0.0, 320.0, 220.0),
///     &WindowAffordanceStyle::default(),
///     Point::new(80.0, 20.0),
///     true,
///     true,
///     true,
/// );
/// assert_eq!(hit, Some(WindowAffordanceKind::Move));
/// ```
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

/// Builds one event with incremental and start-relative pointer deltas.
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

/// Resolves pressed/dragged state before hover and otherwise returns idle.
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

/// Selects a chrome-control fill from kind and interaction state.
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

/// Clamps title-bar height to the non-negative frame height.
fn titlebar_rect(bounds: Rect, style: &WindowAffordanceStyle) -> Rect {
    Rect::new(
        bounds.x,
        bounds.y,
        bounds.w,
        style.titlebar_height.min(bounds.h).max(0.0),
    )
}

/// Places close, minimize, and follow controls from right to left.
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

/// Produces four edge and four corner indicator rectangles.
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
/// Hit-priority and enablement regression scenarios for chrome geometry.
mod tests {
    use super::*;

    /// Returns compact deterministic geometry shared by hit-test scenarios.
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
