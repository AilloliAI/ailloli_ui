use ailloli_ui_core::{Color, Rect};
use ailloli_ui_runtime::{DrawCmd, DrawRRect, DrawRect};

#[derive(Debug, Clone, Copy)]
pub struct OverlayStyle {
    pub bg: Color,
}

pub fn draw_modal_overlay(full: Rect, style: OverlayStyle) -> Vec<DrawCmd> {
    vec![DrawCmd::Rect(DrawRect {
        rect: full,
        color: style.bg,
    })]
}

pub fn draw_modal_card(rect: Rect, bg: Color, border: Option<Color>, radius: f32) -> Vec<DrawCmd> {
    let mut out = Vec::new();
    out.push(DrawCmd::RRect(DrawRRect {
        rect,
        radius,
        color: bg,
    }));
    if let Some(b) = border {
        out.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.x, rect.y, rect.w, 1.0),
            color: b,
        }));
        out.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.x, rect.y + rect.h - 1.0, rect.w, 1.0),
            color: b,
        }));
    }
    out
}
