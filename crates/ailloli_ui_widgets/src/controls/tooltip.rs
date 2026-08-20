use std::cell::Cell;
use std::time::{Duration, Instant};

use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_core::{Constraints, EdgeInsets, FontId, Offset, Rect, Size, TextStyle};
use ailloli_ui_runtime::app::RuntimeHandle;
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Memo, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::popup::{position_popup, PopupContent, PopupDismissReason, PopupSemantics};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_text::{TextLayoutParams, WrapMode};

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized, LayoutExt};
use crate::layout::Container;
use crate::text::Text;

use super::popup::{
    resolve_popup_rect, window_viewport, PopupAlignment, PopupPlacement, PopupPortalBridge,
};

pub use crate::overlay::tooltip::TooltipStyle;

const DEFAULT_OPEN_DELAY: Duration = Duration::from_millis(500);
const DEFAULT_CLOSE_DELAY: Duration = Duration::from_millis(100);
const DEFAULT_GAP: f32 = 6.0;

/// Text displayed by a [`Tooltip`].
///
/// This enum deliberately isolates content ownership from the trigger. The
/// retained popup renderer currently supports text content. A future public
/// arbitrary-view content API can extend this representation without changing
/// the tooltip interaction state machine.
#[derive(Clone)]
enum TooltipContent {
    Empty,
    Label(Binding<String>),
}

impl TooltipContent {
    fn text(&self) -> Option<String> {
        match self {
            Self::Empty => None,
            Self::Label(label) => Some(label.read()),
        }
    }

    fn has_text(&self) -> bool {
        self.text().is_some_and(|text| !text.is_empty())
    }

    fn retained<A: 'static>(&self, style: TooltipStyle) -> PopupContent<A> {
        match self {
            Self::Empty => PopupContent::new(View::empty),
            Self::Label(label) => {
                let label = label.clone();
                PopupContent::new(move || {
                    let text_style = TextStyle::new(FontId::Ui, style.font_px, style.fg);
                    let mut bubble = Container::<A>::new()
                        .fill()
                        .background(style.bg)
                        .radius(style.radius)
                        .child(
                            Text::new(label.clone())
                                .style(text_style)
                                .max_width(style.max_width.max(0.0))
                                .wrap_words(),
                        );
                    bubble.layout_mut().padding =
                        EdgeInsets::new(style.pad_x, style.pad_y, style.pad_x, style.pad_y);
                    bubble.into_view()
                })
            }
        }
    }
}

/// A non-interactive text tooltip attached to one trigger view.
///
/// Text is mounted in the host's retained popup overlay through
/// [`PopupContent`]. The trigger remains fully composable through
/// [`Tooltip::child`]. Tooltip content is hit-tested by the popup host for
/// correct z-order, but its non-interactive semantics never consume pointer
/// input or enter focus traversal.
///
/// Hover opens after 500 ms and closes after a 100 ms grace period. Keyboard
/// focus opens immediately; blur and `Escape` close immediately. Delays can be
/// customized with [`Tooltip::open_delay`] and [`Tooltip::close_delay`].
pub struct Tooltip<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    content: TooltipContent,
    child: Option<View<A>>,
    placement: PopupPlacement,
    alignment: PopupAlignment,
    disabled: Binding<bool>,
    open_delay: Duration,
    close_delay: Duration,
    gap: f32,
    style: TooltipStyle,
}

crate::impl_layout_builders!(Tooltip);

impl<A: 'static> Default for Tooltip<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Tooltip<A> {
    /// Creates an empty tooltip. Add text with [`Self::content`] and a trigger
    /// with [`Self::child`].
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            content: TooltipContent::Empty,
            child: None,
            placement: PopupPlacement::Top,
            alignment: PopupAlignment::Center,
            disabled: Binding::Static(false),
            open_delay: DEFAULT_OPEN_DELAY,
            close_delay: DEFAULT_CLOSE_DELAY,
            gap: DEFAULT_GAP,
            style: TooltipStyle::default(),
        }
    }

    /// Creates a tooltip with retained text content.
    pub fn with_label(content: impl Into<Binding<String>>) -> Self {
        Self::new().content(content)
    }

    /// Sets the text rendered in the retained popup bubble.
    ///
    /// Static strings, `Signal<String>`, `State<String>`, and `Memo<String>` are
    /// accepted through [`Binding`]. Arbitrary public view content remains
    /// intentionally deferred while the shared portal already mounts this
    /// specialized text subtree in the top-level overlay.
    pub fn content(mut self, content: impl Into<Binding<String>>) -> Self {
        self.content = TooltipContent::Label(content.into());
        self
    }

    /// Sets or replaces the trigger view.
    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.child = Some(child.into_view());
        self
    }

    pub fn placement(mut self, placement: PopupPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub fn alignment(mut self, alignment: PopupAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    pub fn open_delay(mut self, delay: Duration) -> Self {
        self.open_delay = delay;
        self
    }

    pub fn close_delay(mut self, delay: Duration) -> Self {
        self.close_delay = delay;
        self
    }

    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }

    pub fn tooltip_style(mut self, style: TooltipStyle) -> Self {
        self.style = style;
        self
    }
}

struct TooltipComponent<A> {
    layout: LayoutStyle,
    content: TooltipContent,
    child: Option<View<A>>,
    placement: PopupPlacement,
    alignment: PopupAlignment,
    disabled: Binding<bool>,
    open_delay: Duration,
    close_delay: Duration,
    gap: f32,
    style: TooltipStyle,
}

impl<A: 'static> ComponentNode<A> for TooltipComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let state = context.signal(TooltipState::default());
        let children = self.child.clone().into_iter().collect();
        let popup_content = self.content.retained(self.style);
        View::node(
            TooltipWidget {
                layout: self.layout,
                content: self.content.clone(),
                placement: self.placement,
                alignment: self.alignment,
                disabled: self.disabled.clone(),
                open_delay: self.open_delay,
                close_delay: self.close_delay,
                gap: self.gap,
                style: self.style,
                state,
                owner: context.element_id(),
                runtime: context.runtime(),
                has_trigger: Cell::new(false),
                portal_presented: context.signal(false),
                popup: PopupPortalBridge::new_retained_with_content(
                    context,
                    PopupSemantics::tooltip(),
                    false,
                    popup_content,
                ),
            },
            children,
        )
    }
}

impl<A: 'static> IntoView<A> for Tooltip<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(TooltipComponent {
                layout: self.layout,
                content: self.content,
                child: self.child,
                placement: self.placement,
                alignment: self.alignment,
                disabled: self.disabled,
                open_delay: self.open_delay,
                close_delay: self.close_delay,
                gap: self.gap,
                style: self.style,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum TooltipPhase {
    #[default]
    Hidden,
    Opening(Instant),
    Open,
    Closing(Instant),
}

impl TooltipPhase {
    fn is_painted(self) -> bool {
        matches!(self, Self::Open | Self::Closing(_))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TooltipState {
    phase: TooltipPhase,
    hovered: bool,
    focused: bool,
    dismissed_until_hover_exit: bool,
    dismissed_until_focus_exit: bool,
}

struct TooltipWidget<A> {
    layout: LayoutStyle,
    content: TooltipContent,
    placement: PopupPlacement,
    alignment: PopupAlignment,
    disabled: Binding<bool>,
    open_delay: Duration,
    close_delay: Duration,
    gap: f32,
    style: TooltipStyle,
    state: Signal<TooltipState>,
    owner: ailloli_ui_core::ElementId,
    runtime: RuntimeHandle<A>,
    has_trigger: Cell<bool>,
    portal_presented: Signal<bool>,
    popup: PopupPortalBridge<A>,
}

impl<A: 'static> Widget<A> for TooltipWidget<A> {
    fn debug_name(&self) -> &'static str {
        "Tooltip"
    }

    fn layout(
        &self,
        engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let mut child_layouts = Vec::new();
        let mut intrinsic = Size::default();
        if let Some(child) = children.first_mut() {
            let result = child.layout(engine, ctx, constraints.loosen());
            intrinsic = result.size;
            child_layouts.push(ChildLayout {
                offset: Offset::default(),
                size: result.size,
                paint_bounds: Rect::new(0.0, 0.0, result.size.w, result.size.h),
                visual_bounds: result.visual_bounds,
            });
        }
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        self.has_trigger
            .set(!children.is_empty() && size.w > 0.0 && size.h > 0.0);
        if !self.is_available() {
            self.popup.close(PopupDismissReason::Programmatic);
            self.portal_presented.set(false);
        }
        let bounds = Rect::new(0.0, 0.0, size.w, size.h);
        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds: bounds,
            visual_bounds: bounds,
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        self.sync_focus_within(ctx.has_focus_within());
        self.sync_hover(ctx.interaction().hovered, Instant::now());
    }

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let now = Instant::now();
        self.sync_focus_within(ctx.has_focus_within());
        self.sync_hover(ctx.interaction().hovered, now);
        if self.consume_portal_dismissal() {
            return;
        }
        if !self.resolve_phase(now).is_painted() || !self.is_available() {
            self.popup.close(PopupDismissReason::Programmatic);
            self.portal_presented.set(false);
            return;
        }
        let Some(label) = self.content.text().filter(|label| !label.is_empty()) else {
            self.popup.close(PopupDismissReason::Programmatic);
            self.portal_presented.set(false);
            return;
        };
        self.publish_label_geometry(ctx, bounds, &label);
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, _bounds: Rect, _layout: &LayoutResult) {
        self.popup.refresh_owner(ctx);
        if !self.is_available() {
            self.hide(false, PopupDismissReason::Programmatic);
            return;
        }
        match event {
            Event::Focus(focus) if focus.focused => {
                self.sync_focus_within(true);
                self.popup.open_unpositioned(Some(ctx));
                self.portal_presented.set(true);
            }
            Event::Focus(_) => {
                self.sync_focus_within(false);
                self.popup.close(PopupDismissReason::OutsidePress);
                self.portal_presented.set(false);
            }
            Event::Keyboard(key)
                if key.state == KeyState::Pressed
                    && matches!(&key.key, Key::Named(NamedKey::Escape))
                    && self.state.read().phase.is_painted() =>
            {
                self.hide(true, PopupDismissReason::Escape);
                ctx.stop_propagation();
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        if !self.is_available() {
            FocusPolicy::NotFocusable
        } else {
            // Acts as a fallback for non-focusable triggers. A focusable child is
            // selected first by the runtime's nearest-focusable routing.
            FocusPolicy::Focusable
        }
    }
}

impl<A: 'static> TooltipWidget<A> {
    fn is_available(&self) -> bool {
        !self.disabled.read() && self.has_trigger.get() && self.content.has_text()
    }

    fn consume_portal_dismissal(&self) -> bool {
        if !self.portal_presented.read() || self.popup.is_open() {
            return false;
        }
        let mut state = self.state.read();
        state.dismissed_until_focus_exit = state.focused;
        state.dismissed_until_hover_exit = state.hovered;
        state.phase = TooltipPhase::Hidden;
        self.update_state(state);
        self.portal_presented.set(false);
        true
    }

    fn sync_focus_within(&self, focused: bool) {
        let focused = focused && self.is_available();
        let mut state = self.state.read();
        if state.focused == focused {
            return;
        }
        state.focused = focused;
        if focused {
            if state.dismissed_until_focus_exit {
                self.update_state(state);
                return;
            }
            state.dismissed_until_hover_exit = false;
            state.phase = TooltipPhase::Open;
        } else {
            state.dismissed_until_focus_exit = false;
            state.dismissed_until_hover_exit = state.hovered;
            state.phase = TooltipPhase::Hidden;
        }
        self.update_state(state);
    }

    fn sync_hover(&self, hovered: bool, now: Instant) {
        let mut state = self.state.read();
        let before = state;

        if !self.is_available() {
            state = TooltipState::default();
        } else {
            if state.hovered != hovered {
                state.hovered = hovered;
                if hovered {
                    if !state.focused && !state.dismissed_until_hover_exit {
                        state.phase = delayed_phase(now, self.open_delay, true);
                    }
                } else {
                    state.dismissed_until_hover_exit = false;
                    if !state.focused {
                        state.phase = match state.phase {
                            TooltipPhase::Open | TooltipPhase::Closing(_) => {
                                delayed_phase(now, self.close_delay, false)
                            }
                            TooltipPhase::Opening(_) | TooltipPhase::Hidden => TooltipPhase::Hidden,
                        };
                    }
                }
            }

            state.phase = resolve_deadline(state.phase, now);
        }

        if state != before {
            self.update_state(state);
        }
        self.schedule_phase(state.phase, now);
    }

    fn resolve_phase(&self, now: Instant) -> TooltipPhase {
        let mut state = self.state.read();
        let phase = resolve_deadline(state.phase, now);
        if phase != state.phase {
            state.phase = phase;
            self.update_state(state);
        }
        self.schedule_phase(phase, now);
        phase
    }

    fn schedule_phase(&self, phase: TooltipPhase, now: Instant) {
        let due = match phase {
            TooltipPhase::Opening(due) | TooltipPhase::Closing(due) => due,
            TooltipPhase::Hidden | TooltipPhase::Open => return,
        };
        self.runtime
            .request_repaint_after(self.owner, due.saturating_duration_since(now));
    }

    fn update_state(&self, state: TooltipState) {
        if self.state.read() != state {
            self.state.set(state);
        }
    }

    fn hide(&self, dismiss_until_hover_exit: bool, reason: PopupDismissReason) {
        let mut state = self.state.read();
        state.dismissed_until_focus_exit = dismiss_until_hover_exit && state.focused;
        if !state.dismissed_until_focus_exit {
            state.focused = false;
        }
        state.dismissed_until_hover_exit = dismiss_until_hover_exit && state.hovered;
        state.phase = TooltipPhase::Hidden;
        self.update_state(state);
        self.popup.close(reason);
        self.portal_presented.set(false);
    }

    /// Measures the retained content and publishes its resolved host geometry.
    ///
    /// Drawing is deliberately absent here: [`PopupOverlayMounts`] owns the
    /// one retained paint of the bubble and its text.
    fn publish_label_geometry(&self, ctx: &mut PaintCtx<'_>, trigger: Rect, label: &str) {
        let text_style = TextStyle::new(FontId::Ui, self.style.font_px, self.style.fg);
        let Some(text_system) = ctx.text_system.as_deref_mut() else {
            return;
        };
        let text = text_system.layout_cached(TextLayoutParams {
            text: label,
            style: text_style,
            max_width: Some(self.style.max_width.max(0.0)),
            wrap_mode: WrapMode::Word,
        });
        let width = text.metrics.width + self.style.pad_x * 2.0;
        let height = text.metrics.height + self.style.pad_y * 2.0;
        let desired_size = Size::new(width, height);
        let card = if let Some(viewport) = window_viewport(ctx) {
            resolve_popup_rect(
                trigger,
                desired_size,
                viewport,
                self.gap,
                self.placement,
                self.alignment,
                true,
            )
        } else {
            position_popup(
                trigger,
                desired_size,
                self.placement,
                self.alignment,
                self.gap,
            )
            .unwrap_or(Rect::new(trigger.x, trigger.y, 0.0, 0.0))
        };
        self.popup.open_without_event(trigger, card);
        self.portal_presented.set(true);
    }
}

fn delayed_phase(now: Instant, delay: Duration, opening: bool) -> TooltipPhase {
    if delay.is_zero() {
        if opening {
            TooltipPhase::Open
        } else {
            TooltipPhase::Hidden
        }
    } else if opening {
        TooltipPhase::Opening(now + delay)
    } else {
        TooltipPhase::Closing(now + delay)
    }
}

fn resolve_deadline(phase: TooltipPhase, now: Instant) -> TooltipPhase {
    match phase {
        TooltipPhase::Opening(due) if due <= now => TooltipPhase::Open,
        TooltipPhase::Closing(due) if due <= now => TooltipPhase::Hidden,
        phase => phase,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailloli_ui_core::ClipShape;

    #[test]
    fn defaults_match_public_contract() {
        let tooltip = Tooltip::<()>::new();
        assert_eq!(tooltip.open_delay, Duration::from_millis(500));
        assert_eq!(tooltip.close_delay, Duration::from_millis(100));
        assert_eq!(tooltip.placement, PopupPlacement::Top);
        assert_eq!(tooltip.alignment, PopupAlignment::Center);
    }

    #[test]
    fn zero_delays_resolve_without_a_timer_tick() {
        let now = Instant::now();
        assert_eq!(delayed_phase(now, Duration::ZERO, true), TooltipPhase::Open);
        assert_eq!(
            delayed_phase(now, Duration::ZERO, false),
            TooltipPhase::Hidden
        );
    }

    #[test]
    fn closing_grace_stays_painted_until_its_deadline() {
        let now = Instant::now();
        let phase = delayed_phase(now, Duration::from_millis(100), false);
        assert!(phase.is_painted());
        assert_eq!(resolve_deadline(phase, now), phase);
        assert_eq!(
            resolve_deadline(phase, now + Duration::from_millis(100)),
            TooltipPhase::Hidden
        );
    }

    #[test]
    fn popup_clamps_to_window_root_instead_of_a_nested_scroll_clip() {
        let root = Rect::new(0.0, 0.0, 640.0, 480.0);
        let nested = Rect::new(20.0, 20.0, 100.0, 80.0);
        let mut ctx = PaintCtx::new();
        ctx.with_clip_shape(ClipShape::Rect(root), true, |ctx| {
            ctx.with_clip_shape(ClipShape::Rect(nested), false, |ctx| {
                assert_eq!(window_viewport(ctx), Some(root));
            });
        });
    }
}
