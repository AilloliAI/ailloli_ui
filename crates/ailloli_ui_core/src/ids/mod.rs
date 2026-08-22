//! Stable identifiers for layout nodes, widgets, fonts, images, and icons.

/// Reconciled element-tree identifiers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ids::element_id::ElementId;
/// assert_eq!(ElementId(1).0, 1);
/// ```
pub mod element_id;
/// Built-in font-family slots.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ids::font_id::FontId;
/// assert_ne!(FontId::Ui, FontId::Mono);
/// ```
pub mod font_id;
/// Built-in and dynamic icon identifiers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ids::icon_id::IconId;
/// assert_eq!(IconId::Check, IconId::Check);
/// ```
pub mod icon_id;
/// Cached image-resource handles.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ids::image_id::ImageId;
/// assert_eq!(ImageId(2).0, 2);
/// ```
pub mod image_id;
/// Stable application-window identifiers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ids::logical_window_id::LogicalWindowId;
/// assert_eq!(LogicalWindowId::new("main").as_str(), "main");
/// ```
pub mod logical_window_id;
/// Borrowed or shared SVG byte sources.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ids::svg_source::SvgSource;
/// assert_eq!(SvgSource::Static(b"svg").as_bytes(), b"svg");
/// ```
pub mod svg_source;
/// Widget-instance identifiers used for input and paint.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ids::widget_id::WidgetId;
/// assert_eq!(WidgetId(3).0, 3);
/// ```
pub mod widget_id;

pub use element_id::ElementId;
pub use font_id::FontId;
pub use icon_id::IconId;
pub use image_id::ImageId;
pub use logical_window_id::LogicalWindowId;
pub use svg_source::SvgSource;
pub use widget_id::WidgetId;
