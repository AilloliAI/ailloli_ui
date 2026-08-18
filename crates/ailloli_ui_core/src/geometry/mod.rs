//! Two-dimensional geometry in **logical pixel** space.
//!
//! Types here are used by layout, hit-testing, and clip propagation. GPU clip
//! strategy (scissor vs stencil) is chosen in `ailloli_ui_render_wgpu`, not in this module.

pub mod clip_shape;
pub mod constraints;
pub mod edge_insets;
pub mod offset;
pub mod point;
pub mod rect;
pub mod size;

pub use clip_shape::ClipShape;
pub use constraints::Constraints;
pub use edge_insets::EdgeInsets;
pub use offset::Offset;
pub use point::Point;
pub use rect::Rect;
pub use size::Size;
