//! Shared retained-popup authority, placement, scrolling, and paint helpers.
//!
//! This crate-private module keeps popup ownership in the runtime, chooses
//! deterministic headless fallbacks until host metadata arrives, delegates
//! geometry to runtime primitives, and centralizes select-style popup painting.

use super::select::SelectStyle;
use ailloli_ui_core::event::{Modifiers, WheelDelta};
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
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::Select;
/// let select: Select<i32> = Select::new().option(1, "One").default_open(true);
/// let _ = select; // Its component owns the internal portal bridge.
/// ```
pub(super) struct PopupPortalBridge<A> {
    /// Runtime authority used for registration, content, and visibility.
    runtime: RuntimeHandle<A>,
    /// Stable element that owns the popup across descendant events.
    owner_element: ElementId,
    /// Deterministic runtime popup ID, or `None` if allocation failed.
    popup_id: Option<PopupId>,
    /// Host-facing popup role and focus behavior.
    semantics: PopupSemantics,
    /// Procedural or retained overlay mounting policy.
    mount_policy: PopupMountPolicy,
    /// Clonable retained content factory.
    content: PopupContent<A>,
}

impl<A: 'static> PopupPortalBridge<A> {
    /// Registers content that is mounted and painted by the host retained
    /// popup overlay. The owner widget may still publish geometry from its
    /// paint pass, but must not draw a procedural copy of the popup.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().option(1, "One");
    /// let _ = select; // Building this control registers retained listbox content.
    /// ```
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

    /// Registers a bridge with an explicit mount policy and honors initial-open once.
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

    /// Refreshes owner metadata from an event and opens at explicit geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().option(1, "One");
    /// let _ = select; // Pointer/keyboard activation opens through this bridge path.
    /// ```
    pub(super) fn open(&self, ctx: &EventCtx<A>, anchor: Rect, bounds: Rect) {
        self.ensure_registered(Some(ctx));
        if let Some(popup_id) = self.popup_id {
            let _ = self.runtime.open_popup(popup_id, anchor, bounds);
        }
    }

    /// Opens at explicit geometry using current presentation or fallback ownership.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let combo: ComboBox<i32> = ComboBox::new().default_open(true);
    /// let _ = combo; // Its paint refresh uses the event-free bridge path.
    /// ```
    pub(super) fn open_without_event(&self, anchor: Rect, bounds: Rect) {
        self.ensure_registered(None);
        if let Some(popup_id) = self.popup_id {
            let _ = self.runtime.open_popup(popup_id, anchor, bounds);
        }
    }

    /// Refreshes event ownership and requests runtime-resolved placement.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{PopupPlacement, Select};
    /// let select: Select<i32> = Select::new().popup_placement(PopupPlacement::Top);
    /// let _ = select;
    /// ```
    pub(super) fn open_placed(&self, ctx: &EventCtx<A>, placement: PopupPlacementSpec) {
        self.ensure_registered(Some(ctx));
        if let Some(popup_id) = self.popup_id {
            let _ = self.runtime.open_popup_placed(popup_id, placement);
        }
    }

    /// Requests runtime-resolved placement without new event metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{PopupPlacement, Select};
    /// let select: Select<i32> = Select::new().popup_placement(PopupPlacement::Bottom);
    /// let _ = select;
    /// ```
    pub(super) fn open_placed_without_event(&self, placement: PopupPlacementSpec) {
        self.ensure_registered(None);
        if let Some(popup_id) = self.popup_id {
            let _ = self.runtime.open_popup_placed(popup_id, placement);
        }
    }

    /// Opens before geometry is available, optionally refreshing event ownership.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().default_open(true);
    /// let _ = select; // Initial-open registration is intentionally unpositioned.
    /// ```
    pub(super) fn open_unpositioned(&self, ctx: Option<&EventCtx<A>>) {
        self.ensure_registered(ctx);
        if let Some(popup_id) = self.popup_id {
            let _ = self.runtime.open_popup_unpositioned(popup_id);
        }
    }

    /// Dismisses a registered popup with the supplied host-visible reason.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().disabled(true);
    /// let _ = select; // Disabled retained controls close their popup programmatically.
    /// ```
    pub(super) fn close(&self, reason: PopupDismissReason) {
        if let Some(popup_id) = self.popup_id {
            self.runtime.close_popup(popup_id, reason);
        }
    }

    /// Semantic visibility is owned by the portal, not by a widget-local
    /// boolean. This is what lets Escape/outside press/stale-owner dismissal
    /// immediately control the procedural fallback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().default_open(false);
    /// let _ = select; // Visibility is queried from runtime authority when painted.
    /// ```
    pub(super) fn is_open(&self) -> bool {
        self.popup_id
            .is_some_and(|popup_id| self.runtime.popup_is_open(popup_id))
    }

    /// Reconciles popup ownership with the current event presentation metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new();
    /// let _ = select; // Widget events refresh the bridge owner before opening.
    /// ```
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

    /// Resolves owner priority: event, tree scope, existing owner, then headless.
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

/// Returns non-focus-taking listbox semantics used by selection popups.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::Select;
/// let select: Select<i32> = Select::new().option(1, "One");
/// let _ = select;
/// ```
pub(super) const fn listbox_popup_semantics() -> PopupSemantics {
    PopupSemantics::new()
        .with_role(PopupRole::Listbox)
        .with_focus_policy(PopupFocusPolicy::None)
}

/// Returns menu semantics with optional focus trapping inside the popup.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::Dropdown;
/// let menu: Dropdown<()> = Dropdown::new("Menu");
/// let _ = menu;
/// ```
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
/// Test adapter positioning a popup four pixels below a zero-origin trigger.
///
/// Invalid geometry returns a zero rectangle rather than propagating an error.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Rect, Size};
/// use ailloli_ui_runtime::popup::{position_popup, PopupAlignment, PopupPlacement};
/// let rect = position_popup(Rect::new(0.0, 0.0, 80.0, 24.0), Size::new(120.0, 60.0),
///                           PopupPlacement::Bottom, PopupAlignment::Start, 4.0)?;
/// assert_eq!(rect, Rect::new(0.0, 28.0, 120.0, 60.0));
/// # Ok::<(), ailloli_ui_runtime::popup::PopupPlacementError>(())
/// ```
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
/// Test adapter positioning from a zero-origin trigger and explicit placement.
///
/// Invalid geometry returns a zero rectangle.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::PopupPlacement;
/// assert_eq!(PopupPlacement::Top, PopupPlacement::Top);
/// ```
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

/// Positions a start-aligned popup around an arbitrary trigger rectangle.
///
/// Invalid geometry returns a zero-size rectangle at the trigger origin.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{PopupPlacement, Select};
/// let select: Select<i32> = Select::new().popup_placement(PopupPlacement::Top);
/// let _ = select;
/// ```
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

/// Resolves placement against a viewport with optional side flipping.
///
/// The overlay-only backend clamps the resolved rectangle. Invalid input returns
/// a zero-size rectangle at the viewport origin.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{PopupAlignment, PopupPlacement};
/// let preference = (PopupPlacement::Bottom, PopupAlignment::Start, true);
/// assert!(preference.2);
/// ```
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
/// Test adapter resolving a zero-size pointer anchor within clamp bounds.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_runtime::popup::clamp_popup_to_viewport;
/// let rect = clamp_popup_to_viewport(
///     Rect::new(90.0, 90.0, 40.0, 30.0),
///     Rect::new(0.0, 0.0, 100.0, 100.0),
/// )?;
/// assert_eq!(rect, Rect::new(60.0, 70.0, 40.0, 30.0));
/// # Ok::<(), ailloli_ui_runtime::popup::PopupPlacementError>(())
/// ```
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

/// Clamps popup geometry to bounds with a safe zero-size fallback.
///
/// On invalid geometry, finite bound origins are retained and non-finite origins
/// become zero.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_runtime::popup::clamp_popup_to_viewport;
/// let rect = clamp_popup_to_viewport(
///     Rect::new(90.0, 90.0, 20.0, 20.0),
///     Rect::new(0.0, 0.0, 100.0, 100.0),
/// )?;
/// assert_eq!(rect, Rect::new(80.0, 80.0, 20.0, 20.0));
/// # Ok::<(), ailloli_ui_runtime::popup::PopupPlacementError>(())
/// ```
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
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::Select;
/// let select: Select<i32> = Select::new().default_open(true);
/// let _ = select; // Open procedural fallbacks resolve against the window root.
/// ```
pub(crate) fn window_viewport(ctx: &PaintCtx<'_>) -> Option<Rect> {
    ctx.current_clip()
        .entries()
        .iter()
        .find(|entry| entry.is_window_root)
        .map(|entry| entry.shape.bounding_rect())
}

/// Applies wheel delta using caller-selected axes and line-pixel conversion.
///
/// The returned outcome reports both the resulting state and whether it changed;
/// the input state is not mutated.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::scroll::{ScrollAxes, ScrollState};
/// let state = ScrollState::new();
/// assert_eq!(state.offset.x, 0.0);
/// assert_eq!(ScrollAxes::VERTICAL, ScrollAxes::VERTICAL);
/// ```
pub(crate) fn scroll_popup(
    state: &ScrollState,
    delta: WheelDelta,
    modifiers: Modifiers,
    viewport: Size,
    content: Size,
    line_px: f32,
    axes: ScrollAxes,
) -> ailloli_ui_core::scroll::ScrollOutcome {
    let metrics = ScrollMetrics::new(viewport, content);
    let behavior = ScrollBehavior::new(axes).with_line_px(line_px);
    state.scroll_by(
        behavior.wheel_delta_with_modifiers(delta, modifiers),
        metrics,
        axes,
    )
}

/// Paints visible non-inset shadows followed by the popup surface.
///
/// Inset, fully transparent, and border data are ignored here; the border is a
/// separate final pass so rows cannot cover it.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{Select, SelectStyle};
/// let select: Select<i32> = Select::new().select_style(SelectStyle::default());
/// let _ = select;
/// ```
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

/// Paints the popup border only when at least one side is visible.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::SelectStyle;
/// let style = SelectStyle::default();
/// assert!(style.popup_border.is_visible());
/// ```
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
/// Per-row disabled, selected, and keyboard/pointer-active state.
///
/// Active fill takes precedence over selected fill; disabled opacity applies to
/// the chosen fill, icon, and text.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::SelectOption;
/// let option = SelectOption::new(1, "One").disabled(true);
/// let _ = option;
/// ```
pub(crate) struct PopupRowState {
    /// Whether the row cannot be activated and uses disabled styling.
    pub(crate) disabled: bool,
    /// Whether its value is the controlled selection.
    pub(crate) selected: bool,
    /// Whether pointer or keyboard navigation currently targets it.
    pub(crate) active: bool,
}

/// Paints optional leading icon and one-line label for a popup row.
///
/// Text width reserves trailing space for a selection mark even when this helper
/// does not paint that mark itself.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::IconId;
/// use ailloli_ui_widgets::controls::SelectOption;
/// let option = SelectOption::new(1, "History").leading_icon(IconId::History);
/// let _ = option;
/// ```
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

/// Paints centered, unwrapped regular-layer text within a rectangle.
///
/// Does nothing when the paint context has no text system. Horizontal overflow
/// is delegated to the prepared layout; `opacity` multiplies and clamps alpha.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// let style = TextStyle::new(FontId::Ui, 13, Color::WHITE);
/// assert_eq!(style.px_size, 13);
/// ```
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

/// Paints start-aligned, vertically centered, unwrapped overlay text.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::Select;
/// let select: Select<i32> = Select::new().option(1, "One");
/// let _ = select; // Popup row labels use overlay text painting.
/// ```
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

/// Overlay text opacity, wrapping, and two-axis alignment.
///
/// Negative free space is treated as zero, so oversized layouts start at the
/// rectangle origin for all alignments.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::{AlignItems, JustifyContent};
/// let alignment = (JustifyContent::Center, AlignItems::Center);
/// assert_eq!(alignment.0, JustifyContent::Center);
/// ```
pub(crate) struct OverlayTextOptions {
    /// Alpha multiplier; final alpha is clamped to `0.0..=1.0`.
    pub opacity: f32,
    /// Text engine wrapping policy.
    pub wrap_mode: WrapMode,
    /// Horizontal positioning policy.
    pub justify_content: JustifyContent,
    /// Vertical positioning policy.
    pub align_items: AlignItems,
}

/// Paints overlay text with configurable wrapping and alignment.
///
/// Maximum layout width and alignment dimensions are floored at zero. No command
/// is emitted without a text system.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::{AlignItems, JustifyContent};
/// use ailloli_ui_text::WrapMode;
/// let config = (WrapMode::WordOrAnywhere, JustifyContent::End, AlignItems::Start);
/// assert_eq!(config.1, JustifyContent::End);
/// ```
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

/// Returns start, midpoint, or end offset from nonnegative horizontal free space.
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

/// Returns start, midpoint, or end offset from nonnegative vertical free space.
fn overlay_cross_axis_offset(align_items: AlignItems, available: f32, child: f32) -> f32 {
    let free = (available - child).max(0.0);
    match align_items {
        AlignItems::Start | AlignItems::Stretch => 0.0,
        AlignItems::Center => free * 0.5,
        AlignItems::End => free,
    }
}

/// Measures unwrapped text or returns a deterministic estimate without a text system.
///
/// The fallback width is `character_count * font_px * 0.58`; fallback height is
/// `font_px * 1.2`, both in logical pixels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// let style = TextStyle::new(FontId::Ui, 10, Color::WHITE);
/// let fallback = ("abc".chars().count() as f32 * 10.0 * 0.58, 10.0 * 1.2);
/// assert_eq!(fallback, (17.4, 12.0));
/// let _ = style;
/// ```
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

/// Estimates unwrapped width as `Unicode scalar count * font_px * 0.58`.
///
/// # Examples
///
/// ```
/// let width = "é".chars().count() as f32 * 10.0 * 0.58;
/// assert!((width - 5.8).abs() < 0.0001);
/// ```
pub(crate) fn estimate_text_width(text: &str, style: TextStyle) -> f32 {
    text.chars().count() as f32 * style.px_size as f32 * 0.58
}

/// Multiplies only alpha by `opacity` and clamps the result to `0.0..=1.0`.
///
/// NaN remains NaN under `f32::clamp`; RGB channels are unchanged.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Color;
/// let source = Color::rgba(255, 0, 0, 0.8);
/// let expected_alpha = (source.a * 0.5).clamp(0.0, 1.0);
/// assert_eq!(expected_alpha, 0.4);
/// ```
pub(crate) fn apply_opacity(mut color: Color, opacity: f32) -> Color {
    color.a = (color.a * opacity).clamp(0.0, 1.0);
    color
}

/// Applies the same alpha multiplier independently to all four border colors.
///
/// Widths and radii are not changed.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, style::Border};
/// let border = Border::new(2.0, Color::WHITE);
/// assert_eq!(border.widths.top, 2.0);
/// ```
pub(crate) fn apply_border_opacity(mut border: Border, opacity: f32) -> Border {
    border.colors.left = apply_opacity(border.colors.left, opacity);
    border.colors.top = apply_opacity(border.colors.top, opacity);
    border.colors.right = apply_opacity(border.colors.right, opacity);
    border.colors.bottom = apply_opacity(border.colors.bottom, opacity);
    border
}

/// Returns the largest of the left, top, right, and bottom border widths.
///
/// `f32::max` means a NaN encountered before a finite later operand is replaced
/// by that operand rather than necessarily propagating.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, style::Border};
/// let border = Border::new(3.0, Color::WHITE);
/// assert_eq!(border.widths.left.max(border.widths.top).max(border.widths.right).max(border.widths.bottom), 3.0);
/// ```
pub(crate) fn max_border_width(border: Border) -> f32 {
    border
        .widths
        .left
        .max(border.widths.top)
        .max(border.widths.right)
        .max(border.widths.bottom)
}

/// Returns the axis-aligned union of two rectangles without validation.
///
/// Negative dimensions or NaN coordinates follow [`Rect::right`], [`Rect::bottom`],
/// and `f32::min`/`max` semantics directly.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// let a = Rect::new(0.0, 0.0, 10.0, 10.0);
/// let b = Rect::new(5.0, -2.0, 10.0, 4.0);
/// let x0 = a.x.min(b.x);
/// let y0 = a.y.min(b.y);
/// let x1 = a.right().max(b.right());
/// let y1 = a.bottom().max(b.bottom());
/// assert_eq!(Rect::new(x0, y0, x1 - x0, y1 - y0), Rect::new(0.0, -2.0, 15.0, 12.0));
/// ```
pub(crate) fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = a.right().max(b.right());
    let y1 = a.bottom().max(b.bottom());
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

#[cfg(test)]
mod tests {
    //! Covers runtime geometry delegation and presentation-owner refresh rules.

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
    fn popup_scroll_reports_no_change_beyond_a_reached_limit() {
        let viewport = Size::new(200.0, 64.0);
        let content = Size::new(200.0, 144.0);
        let state = ScrollState::with_offset(ailloli_ui_core::Offset::new(0.0, 80.0));
        let outcome = scroll_popup(
            &state,
            WheelDelta::PixelDelta { x: 0.0, y: -1.0 },
            Modifiers::default(),
            viewport,
            content,
            36.0,
            ScrollAxes::VERTICAL,
        );

        assert!(!outcome.changed);
        assert_eq!(outcome.after.y, 80.0);
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
