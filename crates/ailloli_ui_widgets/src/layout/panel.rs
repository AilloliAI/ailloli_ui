use ailloli_ui_core::{Color, Rect};
use ailloli_ui_runtime::{DrawCmd, DrawRRect, DrawRect};

#[derive(Debug, Clone, Copy)]
pub struct PanelStyle {
    pub bg: Color,
    pub border: Option<Color>,
    pub radius: f32,
}

impl PanelStyle {
    pub fn simple(bg: Color) -> Self {
        Self {
            bg,
            border: None,
            radius: 0.0,
        }
    }
}

pub fn draw_panel(rect: Rect, style: PanelStyle) -> Vec<DrawCmd> {
    let mut out = Vec::new();
    if style.radius > 0.0 {
        out.push(DrawCmd::RRect(DrawRRect {
            rect,
            radius: style.radius,
            color: style.bg,
        }));
    } else {
        out.push(DrawCmd::Rect(DrawRect {
            rect,
            color: style.bg,
        }));
    }

    if let Some(border) = style.border {
        out.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.x, rect.y, rect.w, 1.0),
            color: border,
        }));
        out.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.x, rect.y + rect.h - 1.0, rect.w, 1.0),
            color: border,
        }));
    }

    out
}
