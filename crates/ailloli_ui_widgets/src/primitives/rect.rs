//! Direct axis-aligned rectangle command construction.

use ailloli_ui_core::{Color, Rect};
use ailloli_ui_runtime::{DrawCmd, DrawRect};

/// Wraps `rect` and `color` in an unmodified solid rectangle draw command.
///
/// Coordinates are logical pixels; negative or non-finite dimensions are not
/// normalized here and remain the renderer's responsibility.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, Rect};
/// use ailloli_ui_runtime::DrawCmd;
/// use ailloli_ui_widgets::primitives::draw_rect;
/// let DrawCmd::Rect(rect) = draw_rect(Rect::new(1.0, 2.0, 3.0, 4.0), Color::BLACK) else { panic!() };
/// assert_eq!(rect.rect.w, 3.0);
/// ```
pub fn draw_rect(rect: Rect, color: Color) -> DrawCmd {
    DrawCmd::Rect(DrawRect { rect, color })
}
