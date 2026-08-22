//! Paragraph layout, caching, metrics, and viewport virtualization.

/// Per-paragraph shaped layout cache.
pub mod layout_cache;
/// Cached paragraph dimensions and prefix-sum index.
pub mod paragraph_metrics;
/// Visible paragraph run geometry.
pub mod text_runs;
/// Visible-run construction and no-wrap virtualization.
pub mod visible_lines;

pub use layout_cache::LayoutCache;
pub use paragraph_metrics::{
    FenwickTree, ParagraphMetrics, ParagraphMetricsCache, ParagraphMetricsMetadata,
    ParagraphMetricsStats,
};
pub use text_runs::{
    first_layout_baseline, layout_visual_height, run_visual_bottom, run_visual_top, EditorTextRun,
};
pub use visible_lines::{
    build_visible_text_layout, build_visible_text_layout_filtered, VisibleTextLayout,
};
