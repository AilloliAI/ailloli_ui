use ailloli_ui_core::{Color, IconId, ImageId, Rect};
use ailloli_ui_runtime::{DrawCmd, DrawImage};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageRef {
    Icon(IconId),
    Image(ImageId),
}

/// MVP: the runtime IR currently supports Lucide icons via `IconId` only.
///
/// `Image(ImageId)` is reserved for future use and does not emit `DrawCmd` yet.
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
