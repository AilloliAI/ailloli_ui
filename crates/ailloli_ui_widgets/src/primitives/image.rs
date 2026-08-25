//! Provider-neutral image references and their current draw-command mapping.

use ailloli_ui_core::{Color, IconId, ImageId, Rect};
use ailloli_ui_runtime::{DrawCmd, DrawImage};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// Image source accepted by [`draw_image`].
///
/// The `Image` variant is a reserved sentinel until runtime image resources are
/// implemented; only `Icon` currently produces a command.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{IconId, ImageId};
/// use ailloli_ui_widgets::primitives::ImageRef;
/// assert_ne!(ImageRef::Icon(IconId::Close), ImageRef::Image(ImageId(1)));
/// ```
pub enum ImageRef {
    /// Font glyph or SVG source renderable by the current runtime.
    Icon(IconId),
    /// Future runtime image resource; currently produces no draw command.
    Image(ImageId),
}

/// The runtime IR currently supports Lucide icons via `IconId` only.
///
/// `Image(ImageId)` is reserved for future use and does not emit `DrawCmd` yet.
/// Coordinates, tint, and icon source are otherwise passed through unchanged,
/// with rotation fixed at zero radians.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, IconId, ImageId, Rect};
/// use ailloli_ui_widgets::primitives::{draw_image, ImageRef};
/// let rect = Rect::new(0.0, 0.0, 16.0, 16.0);
/// assert!(draw_image(rect, ImageRef::Icon(IconId::Close), Color::WHITE).is_some());
/// assert!(draw_image(rect, ImageRef::Image(ImageId(7)), Color::WHITE).is_none());
/// ```
pub fn draw_image(rect: Rect, image: ImageRef, tint: Color) -> Option<DrawCmd> {
    match image {
        ImageRef::Icon(icon) => Some(DrawCmd::Image(DrawImage {
            rect,
            icon,
            tint,
            rotation_rad: 0.0,
        })),
        ImageRef::Image(_image_id) => None,
    }
}
