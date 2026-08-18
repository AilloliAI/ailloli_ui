/// Glyph instance in layout space (font bytes live in `TextSystem::face_blobs`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphInstance {
    pub face_id: u64,
    pub font_index: u32,
    pub glyph_id: u32,
    pub px_size: u16,
    pub x: f32,
    pub y: f32,
    pub color: Option<ailloli_ui_core::Color>,
}
