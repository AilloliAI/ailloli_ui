//! Generic row list (virtualization via `ListItemProvider`).

use ailloli_ui_core::{Color, FontId, Rect, TextStyle};
use ailloli_ui_runtime::{DrawCmd, DrawRRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

#[derive(Debug, Clone)]
pub struct ListRow {
    pub title: String,
    pub subtitle: String,
    pub right_icon_bg: Option<Color>,
}

#[derive(Debug, Clone, Copy)]
pub struct ListStyle {
    pub row_h: f32,
    pub radius: f32,
    pub title_fg: Color,
    pub subtitle_fg: Color,
    pub font: FontId,
    pub title_px: u16,
    pub subtitle_px: u16,
}

pub fn draw_list_rows(
    rect: Rect,
    rows: &[ListRow],
    style: ListStyle,
    text: &mut TextSystem,
) -> Vec<DrawCmd> {
    let mut out = Vec::new();
    let mut y = rect.y;
    for r in rows {
        if y + style.row_h > rect.y + rect.h {
            break;
        }
        let row_rect = Rect::new(rect.x, y, rect.w, style.row_h);
        out.push(DrawCmd::RRect(DrawRRect {
            rect: row_rect,
            radius: style.radius,
            color: Color::new(0.0, 0.0, 0.0, 0.0),
        }));
        out.push(DrawCmd::Text(DrawText {
            pos: [row_rect.x + 14.0, row_rect.y + 18.0],
            color: style.title_fg,
            decoration: ailloli_ui_core::TextDecoration::None,
            layout: text.layout_cached(TextLayoutParams {
                text: &r.title,
                style: TextStyle::new(style.font, style.title_px, style.title_fg),
                max_width: None,
                wrap_mode: WrapMode::NoWrap,
            }),
        }));
        out.push(DrawCmd::Text(DrawText {
            pos: [row_rect.x + 14.0, row_rect.y + 34.0],
            color: style.subtitle_fg,
            decoration: ailloli_ui_core::TextDecoration::None,
            layout: text.layout_cached(TextLayoutParams {
                text: &r.subtitle,
                style: TextStyle::new(style.font, style.subtitle_px, style.subtitle_fg),
                max_width: None,
                wrap_mode: WrapMode::NoWrap,
            }),
        }));

        if let Some(bg) = r.right_icon_bg {
            let icon_rect = Rect::new(row_rect.x + row_rect.w - 34.0, row_rect.y + 8.0, 28.0, 28.0);
            out.push(DrawCmd::RRect(DrawRRect {
                rect: icon_rect,
                radius: 8.0,
                color: bg,
            }));
        }

        y += style.row_h;
    }
    out
}
