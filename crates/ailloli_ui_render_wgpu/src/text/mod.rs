//! Glyph atlas and GPU upload for text rendering.

pub mod glyph_upload;
pub mod text_atlas;

pub use text_atlas::{Glyph, GlyphKey, TextAtlas, TextAtlasFrame, TextAtlasStats, MAX_ATLAS_PAGES};
