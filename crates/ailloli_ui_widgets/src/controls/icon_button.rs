//! Legacy draw-command helper for a square icon button.
//!
//! New code should compose [`crate::controls::Button`] and
//! [`crate::primitives::Icon`] instead.

#![allow(deprecated)]

use ailloli_ui_core::{Color, IconId, Rect};
use ailloli_ui_runtime::{DrawCmd, DrawRRect};

use crate::primitives::draw_icon;

#[deprecated(note = "use Button::new().child(Icon::new(...)) instead")]
#[derive(Debug, Clone, Copy)]
/// Visual metrics and colors for [`draw_icon_button`].
///
/// All dimensions are logical pixels. The default geometry expects a
/// `28 × 28` logical-pixel rectangle (`6 + 16 + 6`). Values are used as-is;
/// this legacy helper does not clamp negative sizes or padding.
///
/// # Examples
///
/// ```
/// #![allow(deprecated)]
/// use ailloli_ui_widgets::controls::IconButtonStyle;
/// let style = IconButtonStyle::default();
/// assert_eq!(style.icon_size, 16.0);
/// assert_eq!(style.padding, 6.0);
/// ```
pub struct IconButtonStyle {
    /// Background color of the rounded rectangle.
    pub bg: Color,
    /// Corner radius in logical pixels.
    pub radius: f32,
    /// Width and height of the square icon in logical pixels.
    pub icon_size: f32,
    /// Offset from the rectangle's top-left corner in logical pixels.
    pub padding: f32,
}

impl Default for IconButtonStyle {
    fn default() -> Self {
        Self {
            bg: Color::rgba(31, 38, 55, 1.0),
            radius: 8.0,
            icon_size: 16.0,
            padding: 6.0,
        }
    }
}

#[deprecated(note = "use Button::new().child(Icon::new(...)) instead")]
/// Produces the background and icon commands for a legacy icon button.
///
/// The first command fills `rect`; the second draws a square icon at
/// `(rect.x + padding, rect.y + padding)`. This function only paints the
/// control: it provides no hit testing, focus handling, or click action.
///
/// # Examples
///
/// ```
/// #![allow(deprecated)]
/// use ailloli_ui_core::{Color, IconId, Rect};
/// use ailloli_ui_widgets::controls::{draw_icon_button, IconButtonStyle};
/// let commands = draw_icon_button(
///     Rect::new(0.0, 0.0, 28.0, 28.0),
///     IconId::Check,
///     Color::WHITE,
///     IconButtonStyle::default(),
/// );
/// assert_eq!(commands.len(), 2);
/// ```
pub fn draw_icon_button(
    rect: Rect,
    icon: IconId,
    tint: Color,
    style: IconButtonStyle,
) -> Vec<DrawCmd> {
    vec![
        DrawCmd::RRect(DrawRRect {
            rect,
            radius: style.radius,
            color: style.bg,
        }),
        draw_icon(
            Rect::new(
                rect.x + style.padding,
                rect.y + style.padding,
                style.icon_size,
                style.icon_size,
            ),
            icon,
            tint,
        ),
    ]
}
