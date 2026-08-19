use ailloli_ui_core::event::WheelDelta;
use ailloli_ui_core::geometry::{Rect, Size};
use ailloli_ui_core::scroll::{ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollState};
use ailloli_ui_core::style::{AlignItems, Border, JustifyContent};
use ailloli_ui_core::{Color, IconId, TextStyle};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{
    DrawBorder, DrawBoxShadow, DrawCmd, DrawImage, DrawRRect, DrawRect, DrawText,
};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

use super::select::SelectStyle;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PopupPlacement {
    Top,
    #[default]
    Bottom,
}

pub(crate) fn popup_rect_for_size(trigger: Size, popup_width: f32, popup_height: f32) -> Rect {
    Rect::new(0.0, trigger.h + 4.0, popup_width, popup_height)
}

pub(crate) fn popup_rect_for_size_with_placement(
    trigger: Size,
    popup_width: f32,
    popup_height: f32,
    gap: f32,
    placement: PopupPlacement,
) -> Rect {
    let y = match placement {
        PopupPlacement::Top => -gap - popup_height,
        PopupPlacement::Bottom => trigger.h + gap,
    };
    Rect::new(0.0, y, popup_width, popup_height)
}

pub(crate) fn popup_rect_for_bounds(
    trigger: Rect,
    popup_width: f32,
    popup_height: f32,
    gap: f32,
    placement: PopupPlacement,
) -> Rect {
    let y = match placement {
        PopupPlacement::Top => trigger.y - gap - popup_height,
        PopupPlacement::Bottom => trigger.bottom() + gap,
    };
    Rect::new(trigger.x, y, popup_width, popup_height)
}

pub(crate) fn popup_rect_at_pointer(
    pointer_x: f32,
    pointer_y: f32,
    width: f32,
    height: f32,
    clamp_bounds: Rect,
) -> Rect {
    clamp_rect_to_bounds(Rect::new(pointer_x, pointer_y, width, height), clamp_bounds)
}

pub(crate) fn clamp_rect_to_bounds(rect: Rect, bounds: Rect) -> Rect {
    let width = rect.w.min(bounds.w.max(0.0));
    let height = rect.h.min(bounds.h.max(0.0));
    let min_x = bounds.x;
    let min_y = bounds.y;
    let max_x = (bounds.right() - width).max(min_x);
    let max_y = (bounds.bottom() - height).max(min_y);
    Rect::new(
        rect.x.clamp(min_x, max_x),
        rect.y.clamp(min_y, max_y),
        width,
        height,
    )
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
