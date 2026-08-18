//! Text measurement, layout, wrapping, and editing for Ailloli UI.
//!
//! Decoupled from the GPU backend: this crate produces [`PreparedTextLayout`] and
//! [`GlyphInstance`] data; `ailloli_ui_render_wgpu` rasterizes glyphs into an atlas.
//!
//! # Main types
//!
//! | Type | Role |
//! |------|------|
//! | [`TextSystem`] | Per-window Parley engine + LRU layout cache + font blobs |
//! | [`TextBuffer`] | Rope-backed document with per-paragraph revisions |
//! | [`PreparedTextLayout`] | Cached layout + glyphs for `DrawCmd::Text` |
//! | [`TextEditState`] | Caret, selection, IME, undo/redo for text widgets |

/// Rope buffer with paragraph indexing and targeted invalidation.
pub mod buffer;
/// Keyboard/IME editing engine shared by `TextInput` and `Editor`.
pub mod edit;
/// Low-level Parley layout engine wrapper.
pub mod engine_parley;
/// Font discovery via `fontique` (system + bundled assets).
pub mod font_db;
/// Glyph positions for GPU atlas lookup.
pub mod glyph;
/// Parley-based layout helpers and caret mapping.
pub mod layout;
/// Layout request parameters and line metrics.
pub mod params;
/// Prepared layouts ready for paint.
pub mod prepared;
/// Central text subsystem (cache + fonts).
pub mod system;
/// Simple per-line metrics (fontdue-backed).
pub mod text_layout;
/// Width measurement trait and implementations.
pub mod text_measure;
/// Word-wrap without hyphenation.
pub mod wrap;

pub use buffer::{ParagraphMeta, TextBuffer};
pub use edit::{
    PlatformKeymap, TextEditAction, TextEditOutcome, TextEditState, TextInputMode, TextKeymap,
    TextMovement, TextSelection,
};
pub use engine_parley::ParleyEngine;
pub use font_db::FontDb;
pub use glyph::GlyphInstance;
pub use layout::{
    caret_index_at_point, caret_rect_at, caret_x_at, layout_text, LaidOutLine, LaidOutText,
    TextLayoutParams, TextMetrics, WrapMode,
};
pub use params::{StyledTextLayoutParams, StyledTextSpan};
pub use prepared::{prepare_layout, PreparedTextLayout};
pub use system::{ParagraphStore, TextLayoutHandle, TextLayoutKey, TextSystem};
pub use text_layout::{caret_index_at_x, line_layout, LineLayout};
pub use text_measure::{ApproxTextMeasure, FontMetrics, TextMeasure};
pub use wrap::wrap_lines;
