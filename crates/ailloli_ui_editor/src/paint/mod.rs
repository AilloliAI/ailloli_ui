//! Neutral paint-item construction for editor UI adapters.

/// Active caret-line fill and ring.
pub mod active_line_painter;
/// Caret item and blink timing.
pub mod caret_painter;
/// Search and diagnostic text decorations.
pub mod code_decorations_painter;
/// Collapsed-fold placeholder labels.
pub mod folding_painter;
/// Gutter backgrounds, numbers, folds, and diagnostics.
pub mod gutter_painter;
/// Framework-neutral paint item enum.
pub mod paint_model;
/// Selection fill items.
pub mod selection_painter;
/// Syntax-styled text items.
pub mod syntax_painter;
/// Uniform text items.
pub mod text_painter;

pub use paint_model::EditorPaintItem;
