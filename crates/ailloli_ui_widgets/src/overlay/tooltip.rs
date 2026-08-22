//! Immediate-mode tooltip bubble drawing for an overlay layer.

use ailloli_ui_core::{Color, FontId, Rect, TextStyle};
use ailloli_ui_runtime::{DrawCmd, DrawRRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

#[derive(Debug, Clone, Copy, PartialEq)]
/// Tooltip palette, padding, typography, and wrap bound.
///
/// All floating dimensions are logical pixels. Values pass through without
/// clamping; callers should provide finite non-negative padding, radius, and
/// maximum width. `font_px` is an integer logical-pixel size.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::overlay::TooltipStyle;
/// let style = TooltipStyle::default();
/// assert_eq!(style.font_px, 12);
/// assert_eq!(style.max_width, 280.0);
/// ```
pub struct TooltipStyle {
    /// Bubble fill.
    pub bg: Color,
    /// Glyph fill.
    pub fg: Color,
    /// Bubble corner radius in logical pixels.
    pub radius: f32,
    /// Horizontal text inset in logical pixels.
    pub pad_x: f32,
    /// Vertical text inset in logical pixels.
    pub pad_y: f32,
    /// Font size in integer logical pixels.
    pub font_px: u16,
    /// Maximum shaped line width in logical pixels.
    pub max_width: f32,
}

/// Supplies the dark tooltip defaults.
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
///
/// `card` is caller-resolved geometry; the helper does not measure or resize it.
/// It returns exactly two commands: background, then word-wrapped text. Text y
/// is baseline-positioned at `card.y + pad_y + font_px`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_text::TextSystem;
/// use ailloli_ui_widgets::overlay::{draw_tooltip, TooltipStyle};
/// let mut text_system = TextSystem::new();
/// let commands = draw_tooltip(Rect::new(0.0, 0.0, 120.0, 32.0), "Details", TooltipStyle::default(), &mut text_system);
/// assert_eq!(commands.len(), 2);
/// ```
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
