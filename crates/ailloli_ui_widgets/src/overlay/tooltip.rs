use ailloli_ui_core::{Color, FontId, Rect, TextStyle};
use ailloli_ui_runtime::{DrawCmd, DrawRRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TooltipStyle {
    pub bg: Color,
    pub fg: Color,
    pub radius: f32,
    pub pad_x: f32,
    pub pad_y: f32,
    pub font_px: u16,
    pub max_width: f32,
}

impl Default for TooltipStyle {
    fn default() -> Self {
        Self {
            bg: Color::rgba(17, 24, 40, 0.95),
            fg: Color::rgba(243, 246, 251, 1.0),
            radius: 6.0,
            pad_x: 10.0,
            pad_y: 6.0,
            font_px: 12,
            max_width: 280.0,
        }
    }
}

/// Tooltip bubble (place in an **overlay** layer for correct z-order).
pub fn draw_tooltip(
    card: Rect,
    text: &str,
    style: TooltipStyle,
    text_system: &mut TextSystem,
) -> Vec<DrawCmd> {
    vec![
        DrawCmd::RRect(DrawRRect {
            rect: card,
            radius: style.radius,
            color: style.bg,
        }),
        DrawCmd::Text(DrawText {
            pos: [
                card.x + style.pad_x,
                card.y + style.pad_y + style.font_px as f32,
            ],
            color: style.fg,
            decoration: ailloli_ui_core::TextDecoration::None,
            layout: text_system.layout_cached(TextLayoutParams {
                text,
                style: TextStyle::new(FontId::Ui, style.font_px, style.fg),
                max_width: Some(style.max_width),
                wrap_mode: WrapMode::Word,
            }),
        }),
    ]
}
