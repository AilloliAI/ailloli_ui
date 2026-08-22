//! Direct rounded-rectangle command construction.

use ailloli_ui_core::{Color, Rect};
use ailloli_ui_runtime::{DrawCmd, DrawRRect};

/// Creates a rounded rectangle command with a logical-pixel corner `radius`.
///
/// Rect and radius values are passed through without normalization or clamping.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, Rect};
/// use ailloli_ui_runtime::DrawCmd;
/// use ailloli_ui_widgets::primitives::draw_rounded_rect;
/// let DrawCmd::RRect(rect) = draw_rounded_rect(Rect::new(0.0, 0.0, 20.0, 10.0), 4.0, Color::WHITE) else { panic!() };
/// assert_eq!(rect.radius, 4.0);
/// ```
pub fn draw_rounded_rect(rect: Rect, radius: f32, color: Color) -> DrawCmd {
    DrawCmd::RRect(DrawRRect {
        rect,
        radius,
        color,
    })
}
