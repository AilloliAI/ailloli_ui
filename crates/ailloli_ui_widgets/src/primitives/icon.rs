//! Direct icon draw-command construction without a retained widget.

use ailloli_ui_core::{Color, IconId, Rect};
use ailloli_ui_runtime::{DrawCmd, DrawImage};

/// Creates an unrotated image command for `icon` in `rect` with `tint`.
///
/// Coordinates are logical pixels and are passed through unchanged. The helper
/// performs no SVG/font validation and always sets rotation to zero radians.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, IconId, Rect};
/// use ailloli_ui_runtime::DrawCmd;
/// use ailloli_ui_widgets::primitives::draw_icon;
/// let command = draw_icon(Rect::new(1.0, 2.0, 16.0, 16.0), IconId::Close, Color::WHITE);
/// let DrawCmd::Image(image) = command else { panic!("expected image command") };
/// assert_eq!(image.rotation_rad, 0.0);
/// ```
pub fn draw_icon(rect: Rect, icon: IconId, tint: Color) -> DrawCmd {
    DrawCmd::Image(DrawImage {
        rect,
        icon,
        tint,
        rotation_rad: 0.0,
    })
}
