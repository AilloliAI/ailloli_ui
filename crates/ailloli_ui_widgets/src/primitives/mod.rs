//! Primitive shapes and icons (direct `DrawCmd` helpers and `Icon` widget).

pub mod icon;
pub mod icon_widget;
pub mod image;
pub mod rect;
pub mod rounded_rect;
pub mod spacer;

pub use icon::draw_icon;
pub use icon_widget::Icon;
pub use image::{draw_image, ImageRef};
pub use rect::draw_rect;
pub use rounded_rect::draw_rounded_rect;
