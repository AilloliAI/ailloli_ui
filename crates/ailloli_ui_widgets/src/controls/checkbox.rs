use ailloli_ui_core::style::{Border, Radius};
use ailloli_ui_core::{Color, FontId, IconId, Rect, TextStyle, Theme};
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawRRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

use crate::primitives::draw_icon;

#[derive(Debug, Clone, Copy)]
pub struct CheckboxStyle {
    pub box_bg: Color,
    pub checked_bg: Color,
    pub box_border: Color,
    pub checked_border: Color,
    pub box_radius: f32,
    pub box_size: f32,
    pub gap: f32,
    pub label_color: Color,
    pub label_px: u16,
    pub icon_tint: Color,
}

impl Default for CheckboxStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl CheckboxStyle {
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        Self {
            box_bg: palette.surface,
            checked_bg: palette.accent,
            box_border: palette.border,
            checked_border: palette.accent,
            box_radius: 4.0,
            box_size: 18.0,
            gap: 8.0,
            label_color: palette.text,
            label_px: 13,
            icon_tint: palette.text,
        }
    }
}

pub fn draw_checkbox(
    row_rect: Rect,
    checked: bool,
    label: Option<&str>,
    disabled: bool,
    style: CheckboxStyle,
    text_system: &mut TextSystem,
) -> Vec<DrawCmd> {
    let mut out = Vec::new();
    let alpha = if disabled { 0.45 } else { 1.0 };
    let bg_src = if checked {
        style.checked_bg
    } else {
        style.box_bg
    };
    let border_src = if checked {
        style.checked_border
    } else {
        style.box_border
    };
    let box_bg = Color::new(bg_src.r, bg_src.g, bg_src.b, bg_src.a * alpha);
    let border = Color::new(
        border_src.r,
        border_src.g,
        border_src.b,
        border_src.a * alpha,
    );
    let box_r = Rect::new(
        row_rect.x,
        row_rect.y + (row_rect.h - style.box_size) / 2.0,
        style.box_size,
        style.box_size,
    );
    out.push(DrawCmd::RRect(DrawRRect {
        rect: box_r,
        radius: style.box_radius,
        color: box_bg,
    }));
    out.push(DrawCmd::Border(DrawBorder {
        rect: box_r,
        radius: Radius::uniform(style.box_radius),
        border: Border::new(1.0, border),
    }));

    if checked && !disabled {
        let inset = 3.0;
        out.push(draw_icon(
            Rect::new(
                box_r.x + inset,
                box_r.y + inset,
                box_r.w - inset * 2.0,
                box_r.h - inset * 2.0,
            ),
            IconId::Check,
            style.icon_tint,
        ));
    }

    if let Some(lbl) = label {
        let lx = box_r.x + box_r.w + style.gap;
        let baseline_y = row_rect.y + row_rect.h / 2.0 + 4.0;
        out.push(DrawCmd::Text(DrawText {
            pos: [lx, baseline_y],
            color: style.label_color,
            layout: text_system.layout_cached(TextLayoutParams {
                text: lbl,
                style: TextStyle::new(FontId::Ui, style.label_px, style.label_color),
                max_width: None,
                wrap_mode: WrapMode::NoWrap,
            }),
        }));
    }

    out
}
