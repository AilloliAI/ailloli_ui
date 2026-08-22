//! Backend-neutral glyph placement produced by the text layout pipeline.

/// One positioned glyph in logical layout space.
///
/// The matching font bytes remain owned by [`crate::TextSystem`]. Coordinates
/// are logical pixels relative to the layout origin; they are not rounded to
/// device pixels. `color` is absent for layouts that use one caller-supplied
/// paint color and present for styled runs that carry their own color.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::GlyphInstance;
///
/// let glyph = GlyphInstance {
///     face_id: 7,
///     font_index: 0,
///     glyph_id: 42,
///     px_size: 16,
///     x: 12.5,
///     y: 20.0,
///     color: None,
/// };
/// assert_eq!(glyph.glyph_id, 42);
/// assert_eq!(glyph.color, None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphInstance {
    /// Stable identifier of the selected font face within the Parley database.
    pub face_id: u64,
    /// Face index inside a font collection; zero for ordinary single-face fonts.
    pub font_index: u32,
    /// Font-specific glyph identifier used for rasterization.
    pub glyph_id: u32,
    /// Requested font size in logical pixels, rounded and clamped to at least one.
    pub px_size: u16,
    /// Horizontal glyph origin in logical pixels.
    pub x: f32,
    /// Vertical glyph origin in logical pixels.
    pub y: f32,
    /// Per-run linear-RGBA color, or `None` when paint supplies a uniform color.
    pub color: Option<ailloli_ui_core::Color>,
}
