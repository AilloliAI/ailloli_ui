//! Two-dimensional geometry in **logical pixel** space.
//!
//! Types here are used by layout, hit-testing, and clip propagation. GPU clip
//! strategy (scissor vs stencil) is chosen in `ailloli_ui_render_wgpu`, not in this module.

/// Axis-aligned and rounded local clip shapes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::geometry::clip_shape::ClipShape;
/// use ailloli_ui_core::Rect;
/// assert!(matches!(ClipShape::rect(Rect::new(0.0, 0.0, 1.0, 1.0)), ClipShape::Rect(_)));
/// ```
pub mod clip_shape;
/// Parent-to-child minimum and maximum layout sizes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::geometry::constraints::Constraints;
/// assert_eq!(Constraints::tight(10.0, 20.0).max_w, 10.0);
/// ```
pub mod constraints;
/// Four-sided padding and margin values.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::geometry::edge_insets::EdgeInsets;
/// assert_eq!(EdgeInsets::all(2.0).horizontal(), 4.0);
/// ```
pub mod edge_insets;
/// Two-dimensional translation vectors.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::geometry::offset::Offset;
/// assert_eq!(Offset::new(1.0, 2.0).x, 1.0);
/// ```
pub mod offset;
/// Positions in logical coordinate spaces.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::geometry::point::Point;
/// assert_eq!(Point::new(1.0, 2.0).y, 2.0);
/// ```
pub mod point;
/// Axis-aligned rectangles and intersection helpers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::geometry::rect::Rect;
/// assert_eq!(Rect::new(0.0, 0.0, 2.0, 3.0).right(), 2.0);
/// ```
pub mod rect;
/// Logical width and height pairs.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::geometry::size::Size;
/// assert!(!Size::new(2.0, 3.0).is_empty());
/// ```
pub mod size;

pub use clip_shape::ClipShape;
pub use constraints::Constraints;
pub use edge_insets::EdgeInsets;
pub use offset::Offset;
pub use point::Point;
pub use rect::Rect;
pub use size::Size;
