//! Icon rasterization (Lucide, Devicon, custom SVG) and GPU texture cache.

pub mod devicons;
pub mod icon_cache;
pub mod lucide;
pub mod raster;
pub mod svg;

pub use icon_cache::{IconCache, IconGpu, IconKey};
pub use svg::rasterize_svg;
