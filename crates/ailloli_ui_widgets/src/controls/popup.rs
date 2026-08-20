use super::select::SelectStyle;
use ailloli_ui_core::event::WheelDelta;
use ailloli_ui_core::geometry::{Rect, Size};
use ailloli_ui_core::scroll::{ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollState};
use ailloli_ui_core::style::{AlignItems, Border, JustifyContent};
use ailloli_ui_core::{Color, ElementId, IconId, LogicalWindowId, TextStyle};
use ailloli_ui_runtime::app::{PresentationGeneration, RuntimeHandle};
use ailloli_ui_runtime::component::Context;
use ailloli_ui_runtime::input::EventCtx;
use ailloli_ui_runtime::popup::{
    clamp_popup_to_viewport, position_popup, resolve_popup_placement, PopupBackendCapabilities,
    PopupContent, PopupDismissReason, PopupFocusPolicy, PopupId, PopupMountPolicy, PopupOwner,
    PopupPlacementInput, PopupPlacementSpec, PopupRequest, PopupRole, PopupSemantics,
    HEADLESS_POPUP_WINDOW_ID,
};
pub use ailloli_ui_runtime::popup::{PopupAlignment, PopupPlacement};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{
    DrawBorder, DrawBoxShadow, DrawCmd, DrawImage, DrawRRect, DrawRect, DrawText,
};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

/// Connects a widget popup to the runtime popup authority.
///
/// Events carrying host metadata replace the deterministic headless
/// window/generation fallback as soon as they reach the widget. The selected
/// mount policy decides whether the widget keeps its procedural renderer or
/// the host mounts [`PopupContent`] in its retained overlay. In both cases the
/// portal owns identity, semantic role, open/close state, geometry and
/// host-facing intents.
pub(super) struct PopupPortalBridge<A> {
    runtime: RuntimeHandle<A>,
    owner_element: ElementId,
    popup_id: Option<PopupId>,
    semantics: PopupSemantics,
    mount_policy: PopupMountPolicy,
    content: PopupContent<A>,
}

impl<A: 'static> PopupPortalBridge<A> {
    /// Registers content that is mounted and painted by the host retained
    /// popup overlay. The owner widget may still publish geometry from its
    /// paint pass, but must not draw a procedural copy of the popup.
    pub(super) fn new_retained_with_content(
        context: &Context<A>,
        semantics: PopupSemantics,
        initially_open: bool,
        content: PopupContent<A>,
    ) -> Self {
        Self::new_with_content_and_policy(
            context,
            semantics,
            initially_open,
            PopupMountPolicy::RetainedOverlay,
            content,
        )
    }

    fn new_with_content_and_policy(
        context: &Context<A>,
        semantics: PopupSemantics,
        initially_open: bool,
        mount_policy: PopupMountPolicy,
        content: PopupContent<A>,
    ) -> Self {
        let runtime = context.runtime();
        let owner_element = context.element_id();
        let popup_id = runtime.popup_id_for_element(owner_element).ok();
        let bridge = Self {
            runtime,
            owner_element,
            popup_id,
            semantics,
            mount_policy,
            content,
        };
        let first_registration = bridge.ensure_registered(None);
        if initially_open && first_registration {
            bridge.open_unpositioned(None);
        }
        bridge
    }

    pub(super) fn open(&self, ctx: &EventCtx<A>, anchor: Rect, bounds: Rect) {
        self.ensure_registered(Some(ctx));
        if let Some(popup_id) = self.popup_id {
            let _ = self.runtime.open_popup(popup_id, anchor, bounds);
        }
    }

    pub(super) fn open_without_event(&self, anchor: Rect, bounds: Rect) {
        self.ensure_registered(None);
        if let Some(popup_id) = self.popup_id {
            let _ = self.runtime.open_popup(popup_id, anchor, bounds);
        }
    }

    pub(super) fn open_placed(&self, ctx: &EventCtx<A>, placement: PopupPlacementSpec) {
        self.ensure_registered(Some(ctx));
        if let Some(popup_id) = self.popup_id {
            let _ = self.runtime.open_popup_placed(popup_id, placement);
        }
    }

    pub(super) fn open_placed_without_event(&self, placement: PopupPlacementSpec) {
        self.ensure_registered(None);
        if let Some(popup_id) = self.popup_id {
            let _ = self.runtime.open_popup_placed(popup_id, placement);
        }
    }

    pub(super) fn open_unpositioned(&self, ctx: Option<&EventCtx<A>>) {
        self.ensure_registered(ctx);
        if let Some(popup_id) = self.popup_id {
            let _ = self.runtime.open_popup_unpositioned(popup_id);
        }
    }

    pub(super) fn close(&self, reason: PopupDismissReason) {
        if let Some(popup_id) = self.popup_id {
            self.runtime.close_popup(popup_id, reason);
        }
    }

    /// Semantic visibility is owned by the portal, not by a widget-local
    /// boolean. This is what lets Escape/outside press/stale-owner dismissal
    /// immediately control the procedural fallback.
    pub(super) fn is_open(&self) -> bool {
        self.popup_id
            .is_some_and(|popup_id| self.runtime.popup_is_open(popup_id))
    }

    pub(super) fn refresh_owner(&self, ctx: &EventCtx<A>) {
        self.ensure_registered(Some(ctx));
    }

    /// Ensures registration and reports whether this popup had no prior
    /// registration. A component rebuild must not reinterpret `default_open`
    /// as a fresh open request after the portal dismissed an existing popup.
    fn ensure_registered(&self, ctx: Option<&EventCtx<A>>) -> bool {
        let Some(popup_id) = self.popup_id else {
            return false;
        };
        let owner = self.owner(ctx, popup_id);
        let portal = self.runtime.popup_portal();
        let current_owner = portal
            .borrow()
            .request(popup_id)
            .map(|request| request.owner().clone());

        if current_owner.as_ref() == Some(&owner) {
            let _ = self
                .runtime
                .set_popup_content(popup_id, self.content.clone());
            return false;
        }

        let was_open = portal.borrow().is_open(popup_id);
        drop(portal);
        if current_owner.is_some() {
            if was_open {
                self.runtime
                    .close_popup(popup_id, PopupDismissReason::PresentationStale);
            }
            self.runtime.unregister_popup(popup_id);
        }

        let request = PopupRequest::new(popup_id, owner, self.content.clone())
            .with_semantics(self.semantics.clone())
            .with_mount_policy(self.mount_policy);
        if self.runtime.register_popup(request).is_ok() {
            if was_open {
                let _ = self.runtime.open_popup_unpositioned(popup_id);
            }
            return current_owner.is_none();
        }
        false
    }

    fn owner(&self, ctx: Option<&EventCtx<A>>, popup_id: PopupId) -> PopupOwner {
        if let Some(meta) = ctx.and_then(EventCtx::event_meta) {
            return PopupOwner::new(
                meta.logical_window_id().clone(),
                meta.presentation_generation(),
                self.runtime.element_tree_id(),
                self.owner_element,
            );
        }

        // Focus/blur events synthesized by the runtime carry no EventMeta and
        // may bubble from a descendant trigger. Popup ownership must remain
        // attached to the component that registered the bridge; using the
        // transient event target here would silently rewrite a native owner
        // into a headless descendant and disable host-level dismissal.
        if let Some((logical_window_id, presentation_generation)) =
            self.runtime.presentation_scope()
        {
            return PopupOwner::new(
                logical_window_id,
                presentation_generation,
                self.runtime.element_tree_id(),
                self.owner_element,
            );
        }

        if let Some(owner) = self
            .runtime
            .popup_portal()
            .borrow()
            .request(popup_id)
            .map(|request| request.owner().clone())
        {
            return owner;
        }

        PopupOwner::new(
            LogicalWindowId::new(HEADLESS_POPUP_WINDOW_ID),
            PresentationGeneration::INITIAL,
            self.runtime.element_tree_id(),
            self.owner_element,
        )
    }
}

pub(super) const fn listbox_popup_semantics() -> PopupSemantics {
    PopupSemantics::new()
        .with_role(PopupRole::Listbox)
        .with_focus_policy(PopupFocusPolicy::None)
}

pub(super) const fn menu_popup_semantics(trap_focus: bool) -> PopupSemantics {
    PopupSemantics::new()
        .with_role(PopupRole::Menu)
        .with_focus_policy(if trap_focus {
            PopupFocusPolicy::TrapWithinPopup
        } else {
            PopupFocusPolicy::None
        })
}

#[cfg(test)]
pub(crate) fn popup_rect_for_size(trigger: Size, popup_width: f32, popup_height: f32) -> Rect {
    popup_rect_for_size_with_placement(
        trigger,
        popup_width,
        popup_height,
        4.0,
        PopupPlacement::Bottom,
    )
}

#[cfg(test)]
pub(crate) fn popup_rect_for_size_with_placement(
    trigger: Size,
    popup_width: f32,
    popup_height: f32,
    gap: f32,
    placement: PopupPlacement,
) -> Rect {
    position_popup(
        Rect::new(0.0, 0.0, trigger.w, trigger.h),
        Size::new(popup_width, popup_height),
        placement,
        PopupAlignment::Start,
        gap,
    )
    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))
}

pub(crate) fn popup_rect_for_bounds(
    trigger: Rect,
    popup_width: f32,
    popup_height: f32,
    gap: f32,
    placement: PopupPlacement,
) -> Rect {
    position_popup(
        trigger,
        Size::new(popup_width, popup_height),
        placement,
        PopupAlignment::Start,
        gap,
    )
    .unwrap_or(Rect::new(trigger.x, trigger.y, 0.0, 0.0))
}

pub(crate) fn resolve_popup_rect(
    anchor: Rect,
    desired_size: Size,
    viewport: Rect,
    gap: f32,
    placement: PopupPlacement,
    alignment: PopupAlignment,
    allow_flip: bool,
) -> Rect {
    resolve_popup_placement(
        PopupPlacementInput::new(anchor, desired_size, viewport)
            .with_gap(gap)
            .with_placement(placement)
            .with_alignment(alignment)
            .with_flip(allow_flip),
        PopupBackendCapabilities::overlay_only(),
    )
    .map(|resolved| resolved.bounds())
    .unwrap_or(Rect::new(viewport.x, viewport.y, 0.0, 0.0))
}

#[cfg(test)]
pub(crate) fn popup_rect_at_pointer(
    pointer_x: f32,
    pointer_y: f32,
    width: f32,
    height: f32,
    clamp_bounds: Rect,
) -> Rect {
    resolve_popup_rect(
        Rect::new(pointer_x, pointer_y, 0.0, 0.0),
        Size::new(width, height),
        clamp_bounds,
        0.0,
        PopupPlacement::Bottom,
        PopupAlignment::Start,
        true,
    )
}

pub(crate) fn clamp_rect_to_bounds(rect: Rect, bounds: Rect) -> Rect {
    clamp_popup_to_viewport(rect, bounds).unwrap_or_else(|_| {
        Rect::new(
            if bounds.x.is_finite() { bounds.x } else { 0.0 },
            if bounds.y.is_finite() { bounds.y } else { 0.0 },
            0.0,
            0.0,
        )
    })
}

/// Returns the top-level window viewport rather than a nested widget clip.
///
/// Procedural overlay fallbacks use this boundary for placement so a popup is
/// not accidentally reduced to its trigger or a scroll viewport.
pub(crate) fn window_viewport(ctx: &PaintCtx<'_>) -> Option<Rect> {
    ctx.current_clip()
        .entries()
        .iter()
        .find(|entry| entry.is_window_root)
        .map(|entry| entry.shape.bounding_rect())
}

#[allow(dead_code)]
pub(crate) fn scroll_popup(
    state: &ScrollState,
    delta: WheelDelta,
    viewport: Size,
    content: Size,
    line_px: f32,
    axes: ScrollAxes,
) -> ailloli_ui_core::scroll::ScrollOutcome {
    let metrics = ScrollMetrics::new(viewport, content);
    let behavior = ScrollBehavior::new(axes).with_line_px(line_px);
    state.scroll_by(behavior.wheel_delta(delta), metrics, axes)
}

pub(crate) fn paint_popup_shell(ctx: &mut PaintCtx<'_>, popup: Rect, style: &SelectStyle) {
    for shadow in style.shadows.iter().copied() {
        if !shadow.inset && shadow.color.a > 0.0 {
            ctx.push_overlay(DrawCmd::BoxShadow(DrawBoxShadow {
                rect: popup,
                radius: style.radius,
                shadow,
            }));
        }
    }
    ctx.push_overlay(DrawCmd::RRect(DrawRRect {
        rect: popup,
        radius: style.radius.tl,
        color: style.popup_background,
    }));
}

pub(crate) fn paint_popup_border(ctx: &mut PaintCtx<'_>, popup: Rect, style: &SelectStyle) {
    if style.popup_border.is_visible() {
        ctx.push_overlay(DrawCmd::Border(DrawBorder {
            rect: popup,
            radius: style.radius,
            border: style.popup_border,
        }));
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PopupRowState {
    pub(crate) disabled: bool,
    pub(crate) selected: bool,
    pub(crate) active: bool,
}

pub(crate) fn paint_popup_row(
    ctx: &mut PaintCtx<'_>,
    row: Rect,
    label: &str,
    icon: Option<&IconId>,
    state: PopupRowState,
    style: &SelectStyle,
) {
    let opacity = if state.disabled {
        style.disabled_opacity
    } else {
        1.0
    };
    if state.selected || state.active {
        let color = if state.active {
            style.option_active
        } else {
            style.option_selected
        };
        ctx.push_overlay(DrawCmd::Rect(DrawRect {
            rect: row,
            color: apply_opacity(color, opacity),
        }));
    }

    let mut x = row.x + style.padding_x;
    if let Some(icon) = icon {
        ctx.push_overlay(DrawCmd::Image(DrawImage {
            rect: Rect::new(
                x,
                row.y + (row.h - style.icon_size) * 0.5,
                style.icon_size,
                style.icon_size,
            ),
            icon: icon.clone(),
            tint: apply_opacity(
                if state.disabled {
                    style.disabled_icon_tint
                } else {
                    style.icon_tint
                },
                opacity,
            ),
            rotation_rad: 0.0,
        }));
        x += style.icon_size + style.icon_gap;
    }
    let text_right_inset = style.padding_x + style.icon_size + style.icon_gap;
    let text_rect = Rect::new(
        x,
        row.y,
        (row.right() - x - text_right_inset).max(0.0),
        row.h,
    );
    let text_style = if state.disabled {
        style.disabled_text
    } else {
        style.text
    };
    paint_overlay_text_in_rect(ctx, label, text_style, text_rect, opacity);
}

pub(crate) fn paint_text_in_rect(
    ctx: &mut PaintCtx<'_>,
    label: &str,
    style: TextStyle,
    rect: Rect,
    opacity: f32,
) {
    let Some(text_system) = ctx.text_system.as_deref_mut() else {
        return;
    };
    let layout = text_system.layout_cached(TextLayoutParams {
        text: label,
        style,
        max_width: Some(rect.w),
        wrap_mode: WrapMode::NoWrap,
    });
    let baseline = layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0);
    let y = rect.y + (rect.h - layout.metrics.height) * 0.5 + baseline;
    ctx.push(DrawCmd::Text(DrawText {
        pos: [rect.x, y],
        color: apply_opacity(style.color, opacity),
        decoration: ailloli_ui_core::TextDecoration::None,
        layout: layout.clone(),
    }));
}

pub(crate) fn paint_overlay_text_in_rect(
    ctx: &mut PaintCtx<'_>,
    label: &str,
    style: TextStyle,
    rect: Rect,
    opacity: f32,
) {
    paint_overlay_text_in_rect_aligned(
        ctx,
        label,
        style,
        rect,
        OverlayTextOptions {
            opacity,
            wrap_mode: WrapMode::NoWrap,
            justify_content: JustifyContent::Start,
            align_items: AlignItems::Center,
        },
    );
}

pub(crate) struct OverlayTextOptions {
    pub opacity: f32,
    pub wrap_mode: WrapMode,
    pub justify_content: JustifyContent,
    pub align_items: AlignItems,
}

pub(crate) fn paint_overlay_text_in_rect_aligned(
    ctx: &mut PaintCtx<'_>,
    label: &str,
    style: TextStyle,
    rect: Rect,
    options: OverlayTextOptions,
) {
    let Some(text_system) = ctx.text_system.as_deref_mut() else {
        return;
    };
    let layout = text_system.layout_cached(TextLayoutParams {
        text: label,
        style,
        max_width: Some(rect.w.max(0.0)),
        wrap_mode: options.wrap_mode,
    });
    let baseline = layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0);
    let x = rect.x
        + overlay_main_axis_offset(
            options.justify_content,
            rect.w.max(0.0),
            layout.metrics.width,
        );
    let y = rect.y
        + overlay_cross_axis_offset(options.align_items, rect.h.max(0.0), layout.metrics.height)
        + baseline;
    ctx.push_overlay(DrawCmd::Text(DrawText {
        pos: [x, y],
        color: apply_opacity(style.color, options.opacity),
        decoration: ailloli_ui_core::TextDecoration::None,
        layout: layout.clone(),
    }));
}

fn overlay_main_axis_offset(justify_content: JustifyContent, available: f32, child: f32) -> f32 {
    let free = (available - child).max(0.0);
    match justify_content {
        JustifyContent::Start | JustifyContent::SpaceBetween => 0.0,
        JustifyContent::Center | JustifyContent::SpaceAround | JustifyContent::SpaceEvenly => {
            free * 0.5
        }
        JustifyContent::End => free,
    }
}

fn overlay_cross_axis_offset(align_items: AlignItems, available: f32, child: f32) -> f32 {
    let free = (available - child).max(0.0);
    match align_items {
        AlignItems::Start | AlignItems::Stretch => 0.0,
        AlignItems::Center => free * 0.5,
        AlignItems::End => free,
    }
}

pub(crate) fn measure_text(
    text_system: Option<&mut TextSystem>,
    text: &str,
    style: TextStyle,
) -> Size {
    if let Some(text_system) = text_system {
        let layout = text_system.layout_cached(TextLayoutParams {
            text,
            style,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        });
        Size::new(layout.metrics.width, layout.metrics.height)
    } else {
        Size::new(estimate_text_width(text, style), style.px_size as f32 * 1.2)
    }
}

pub(crate) fn estimate_text_width(text: &str, style: TextStyle) -> f32 {
    text.chars().count() as f32 * style.px_size as f32 * 0.58
}

pub(crate) fn apply_opacity(mut color: Color, opacity: f32) -> Color {
    color.a = (color.a * opacity).clamp(0.0, 1.0);
    color
}

pub(crate) fn apply_border_opacity(mut border: Border, opacity: f32) -> Border {
    border.colors.left = apply_opacity(border.colors.left, opacity);
    border.colors.top = apply_opacity(border.colors.top, opacity);
    border.colors.right = apply_opacity(border.colors.right, opacity);
    border.colors.bottom = apply_opacity(border.colors.bottom, opacity);
    border
}

pub(crate) fn max_border_width(border: Border) -> f32 {
    border
        .widths
        .left
        .max(border.widths.top)
        .max(border.widths.right)
        .max(border.widths.bottom)
}

pub(crate) fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = a.right().max(b.right());
    let y1 = a.bottom().max(b.bottom());
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailloli_ui_runtime::component::IntoView;

    #[test]
    fn procedural_helpers_delegate_to_runtime_geometry() {
        let trigger = Size::new(80.0, 24.0);
        assert_eq!(
            popup_rect_for_size(trigger, 120.0, 60.0),
            Rect::new(0.0, 28.0, 120.0, 60.0)
        );
        assert_eq!(
            popup_rect_for_size_with_placement(trigger, 120.0, 60.0, 6.0, PopupPlacement::Top,),
            Rect::new(0.0, -66.0, 120.0, 60.0)
        );

        let pointer_popup =
            popup_rect_at_pointer(90.0, 90.0, 40.0, 30.0, Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(pointer_popup, Rect::new(60.0, 60.0, 40.0, 30.0));
    }

    #[test]
    fn procedural_clamp_handles_an_empty_viewport_without_panicking() {
        assert_eq!(
            clamp_rect_to_bounds(
                Rect::new(10.0, 10.0, 20.0, 20.0),
                Rect::new(5.0, 7.0, 0.0, 0.0),
            ),
            Rect::new(5.0, 7.0, 0.0, 0.0)
        );
    }

    #[test]
    fn bridge_prefers_the_current_tree_presentation_scope_without_an_event() {
        let runtime = RuntimeHandle::<()>::new();
        let element = ElementId(41);
        let context = Context::new(element, runtime.clone());
        runtime.set_presentation_scope("native-main", PresentationGeneration::new(4));

        let bridge = PopupPortalBridge::new_retained_with_content(
            &context,
            PopupSemantics::default(),
            false,
            PopupContent::new(|| crate::text::Text::new("popup").into_view()),
        );
        let popup_id = runtime.popup_id_for_element(element).unwrap();
        let owner = runtime
            .popup_portal()
            .borrow()
            .request(popup_id)
            .unwrap()
            .owner()
            .clone();
        assert_eq!(owner.logical_window_id().as_str(), "native-main");
        assert_eq!(
            owner.presentation_generation(),
            PresentationGeneration::new(4)
        );

        runtime.set_presentation_scope("native-main", PresentationGeneration::new(5));
        bridge.ensure_registered(None);
        assert_eq!(
            runtime
                .popup_portal()
                .borrow()
                .request(popup_id)
                .unwrap()
                .owner()
                .presentation_generation(),
            PresentationGeneration::new(5)
        );
    }
}
