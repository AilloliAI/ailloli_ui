//! Immediate-mode modal scrim and rounded popup-card helpers.

use ailloli_ui_core::{Color, Rect};
use ailloli_ui_runtime::{DrawCmd, DrawRRect, DrawRect};

#[derive(Debug, Clone, Copy)]
/// Fill style for [`draw_modal_overlay`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Color;
/// use ailloli_ui_widgets::overlay::popup::OverlayStyle;
/// let style = OverlayStyle { bg: Color::rgba(0, 0, 0, 0.5) };
/// assert_eq!(style.bg.a, 0.5);
/// ```
pub struct OverlayStyle {
    /// Full-overlay fill color, including alpha.
    pub bg: Color,
}

/// Returns exactly one solid rectangle command covering `full`.
///
/// Geometry is passed through unchanged in logical pixels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, Rect};
/// use ailloli_ui_widgets::overlay::popup::{draw_modal_overlay, OverlayStyle};
/// let commands = draw_modal_overlay(Rect::new(0.0, 0.0, 100.0, 80.0), OverlayStyle { bg: Color::BLACK });
/// assert_eq!(commands.len(), 1);
/// ```
pub fn draw_modal_overlay(full: Rect, style: OverlayStyle) -> Vec<DrawCmd> {
    vec![DrawCmd::Rect(DrawRect {
        rect: full,
        color: style.bg,
    })]
}

/// Draws a rounded popup card and optional top/bottom one-pixel rules.
///
/// `None` returns only the background command; `Some` returns three commands.
/// Side borders are intentionally absent. Radius and rect are logical pixels
/// and are not clamped or normalized.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, Rect};
/// use ailloli_ui_widgets::overlay::popup::draw_modal_card;
/// let commands = draw_modal_card(Rect::new(0.0, 0.0, 100.0, 60.0), Color::BLACK, Some(Color::WHITE), 8.0);
/// assert_eq!(commands.len(), 3);
/// assert_eq!(draw_modal_card(Rect::new(0.0, 0.0, 1.0, 1.0), Color::BLACK, None, 0.0).len(), 1);
/// ```
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
