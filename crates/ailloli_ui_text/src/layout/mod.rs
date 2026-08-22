//! Cluster-aware shaping, line metrics, glyph extraction, and caret mapping.

use std::sync::Arc;

use ailloli_ui_core::{style::TextStyle, Rect};
use parley::{Affinity, Cursor};

use crate::engine_parley::ParleyEngine;

pub use crate::glyph::GlyphInstance;
pub use crate::params::{LaidOutLine, TextLayoutParams, TextMetrics, WrapMode};

/// Backend-agnostic layout result (Parley types not exposed).
///
/// The value owns a copy of the input text and shares an immutable Parley
/// layout through an [`Arc`]. Dimensions and positions are logical pixels.
/// Cloning is therefore cheap except for the small metadata vectors.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::{layout_text, ParleyEngine, TextLayoutParams};
/// #[allow(deprecated)]
/// let mut engine = ParleyEngine::new();
/// let laid = layout_text(
///     &mut engine,
///     TextLayoutParams::new("hello", TextStyle::new(FontId::Ui, 16, Color::WHITE)),
/// );
/// assert_eq!(laid.text(), "hello");
/// assert!(!laid.lines.is_empty());
/// ```
#[allow(dead_code)]
#[derive(Clone)]
pub struct LaidOutText {
    /// Owned source text whose byte offsets are referenced by line metrics.
    text: String,
    /// Base Ailloli style used to construct the unstyled Parley layout.
    style: TextStyle,
    /// Immutable shaped layout shared by clones.
    layout: Arc<parley::Layout<()>>,
    /// Public per-line summaries in source order.
    pub lines: Vec<LaidOutLine>,
    /// Overall logical-pixel layout extent.
    pub metrics: TextMetrics,
}

impl core::fmt::Debug for LaidOutText {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LaidOutText")
            .field("text_len", &self.text.len())
            .field("style", &self.style)
            .field("lines", &self.lines.len())
            .field("metrics", &self.metrics)
            .finish()
    }
}

impl LaidOutText {
    /// Returns the owned source text used to build this layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{layout_text, ParleyEngine, TextLayoutParams};
    /// #[allow(deprecated)] let mut engine = ParleyEngine::new();
    /// let laid = layout_text(&mut engine, TextLayoutParams::new("é", TextStyle::new(FontId::Ui, 14, Color::WHITE)));
    /// assert_eq!(laid.text().as_bytes().len(), 2);
    /// ```
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the copyable base style supplied to [`layout_text`].
    ///
    /// The style color is metadata for the painter; unstyled glyph instances
    /// returned by [`Self::glyph_instances`] have `color == None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{layout_text, ParleyEngine, TextLayoutParams};
    /// #[allow(deprecated)] let mut engine = ParleyEngine::new();
    /// let style = TextStyle::new(FontId::Mono, 13, Color::WHITE);
    /// let laid = layout_text(&mut engine, TextLayoutParams::new("x", style));
    /// assert_eq!(laid.style(), style);
    /// ```
    pub fn style(&self) -> TextStyle {
        self.style
    }

    /// Flattens all positioned glyph runs into renderer-facing instances.
    ///
    /// The returned vector is newly allocated on every call. Positions are
    /// logical pixels and keep fractional values. Font sizes are rounded to the
    /// nearest integer and clamped to at least one before conversion to `u16`;
    /// colors are `None` because this layout stores `()` brushes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{layout_text, ParleyEngine, TextLayoutParams};
    /// #[allow(deprecated)] let mut engine = ParleyEngine::new();
    /// let laid = layout_text(&mut engine, TextLayoutParams::new("Hi", TextStyle::new(FontId::Ui, 16, Color::WHITE)));
    /// let glyphs = laid.glyph_instances();
    /// assert!(!glyphs.is_empty());
    /// assert!(glyphs.iter().all(|glyph| glyph.px_size >= 1 && glyph.color.is_none()));
    /// ```
    pub fn glyph_instances(&self) -> Vec<GlyphInstance> {
        let layout = self.parley_layout();
        let mut out = Vec::new();
        for line in layout.lines() {
            for item in line.items() {
                use parley::PositionedLayoutItem;
                let PositionedLayoutItem::GlyphRun(run) = item else {
                    continue;
                };
                let font = run.run().font();
                let face_id = font.data.id();
                let font_index = font.index;
                let px_size = run.run().font_size().round().max(1.0) as u16;
                for g in run.positioned_glyphs() {
                    out.push(GlyphInstance {
                        face_id,
                        font_index,
                        glyph_id: g.id,
                        px_size,
                        x: g.x,
                        y: g.y,
                        color: None,
                    });
                }
            }
        }
        out
    }

    /// Returns the overall Parley layout width in logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{layout_text, ParleyEngine, TextLayoutParams};
    /// #[allow(deprecated)] let mut engine = ParleyEngine::new();
    /// let laid = layout_text(&mut engine, TextLayoutParams::new("width", TextStyle::new(FontId::Ui, 16, Color::WHITE)));
    /// assert_eq!(laid.width(), laid.metrics.width);
    /// ```
    pub fn width(&self) -> f32 {
        self.metrics.width
    }

    /// Returns the overall Parley layout height in logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{layout_text, ParleyEngine, TextLayoutParams};
    /// #[allow(deprecated)] let mut engine = ParleyEngine::new();
    /// let laid = layout_text(&mut engine, TextLayoutParams::new("height", TextStyle::new(FontId::Ui, 16, Color::WHITE)));
    /// assert_eq!(laid.height(), laid.metrics.height);
    /// assert!(laid.height() > 0.0);
    /// ```
    pub fn height(&self) -> f32 {
        self.metrics.height
    }

    #[allow(dead_code)]
    /// Borrows the internal Parley layout for crate-internal preparation.
    ///
    /// # Examples
    ///
    /// Public callers use stable accessors such as [`Self::width`] instead:
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{layout_text, ParleyEngine, TextLayoutParams};
    /// #[allow(deprecated)] let mut engine = ParleyEngine::new();
    /// let laid = layout_text(&mut engine, TextLayoutParams::new("x", TextStyle::new(FontId::Ui, 12, Color::WHITE)));
    /// assert_eq!(laid.width(), laid.metrics.width);
    /// ```
    pub(crate) fn parley_layout(&self) -> &parley::Layout<()> {
        self.layout.as_ref()
    }
}

/// Maps a UTF-8 byte index to X in layout space (cluster-aware).
///
/// The returned logical-pixel X is the left edge of a zero-width downstream
/// caret. Byte positions inside one shaped cluster map to that cluster's
/// caret geometry.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::{caret_x_at, layout_text, ParleyEngine, TextLayoutParams};
/// #[allow(deprecated)] let mut engine = ParleyEngine::new();
/// let laid = layout_text(&mut engine, TextLayoutParams::new("abc", TextStyle::new(FontId::Ui, 16, Color::WHITE)));
/// assert!(caret_x_at(&laid, laid.text().len()) >= caret_x_at(&laid, 0));
/// ```
pub fn caret_x_at(laid: &LaidOutText, byte_idx: usize) -> f32 {
    caret_rect_at(laid, byte_idx, 0.0).x
}

/// Maps a UTF-8 byte index to a caret rectangle in layout space.
///
/// `width` is the requested logical-pixel caret thickness. Negative and NaN
/// values behave as zero. The rectangle height follows the shaped line; its
/// width is at least the normalized requested width. A downstream affinity is
/// used at ambiguous boundaries.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::{caret_rect_at, layout_text, ParleyEngine, TextLayoutParams};
/// #[allow(deprecated)] let mut engine = ParleyEngine::new();
/// let laid = layout_text(&mut engine, TextLayoutParams::new("abc", TextStyle::new(FontId::Ui, 16, Color::WHITE)));
/// let caret = caret_rect_at(&laid, 0, 2.0);
/// assert!(caret.w >= 2.0);
/// assert!(caret.h >= 0.0);
/// ```
pub fn caret_rect_at(laid: &LaidOutText, byte_idx: usize, width: f32) -> Rect {
    let layout = laid.parley_layout();
    let cur = Cursor::from_byte_index(layout, byte_idx, Affinity::Downstream);
    let rect = cur.geometry(layout, width.max(0.0));
    Rect::new(
        rect.x0 as f32,
        rect.y0 as f32,
        (rect.x1 - rect.x0).max(width.max(0.0) as f64) as f32,
        (rect.y1 - rect.y0).max(0.0) as f32,
    )
}

/// Maps layout-space `(x, y)` to a UTF-8 byte index (cluster-aware).
///
/// Coordinates are logical pixels relative to the layout origin. Parley
/// resolves points outside glyph bounds to its nearest valid cursor; the
/// returned index is therefore suitable for slicing only after ordinary UTF-8
/// boundary checks required by the caller's text handling.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::{caret_index_at_point, layout_text, ParleyEngine, TextLayoutParams};
/// #[allow(deprecated)] let mut engine = ParleyEngine::new();
/// let laid = layout_text(&mut engine, TextLayoutParams::new("abc", TextStyle::new(FontId::Ui, 16, Color::WHITE)));
/// assert_eq!(caret_index_at_point(&laid, -100.0, 0.0), 0);
/// ```
pub fn caret_index_at_point(laid: &LaidOutText, x: f32, y: f32) -> usize {
    let layout = laid.parley_layout();
    Cursor::from_point(layout, x, y).index()
}

/// Shapes text and returns backend-neutral lines, metrics, and glyph access.
///
/// For [`WrapMode::NoWrap`], `params.max_width` is deliberately ignored.
/// Other modes pass the optional logical-pixel width to Parley. The source text
/// is copied into the result; the engine may be reused immediately afterward.
/// Line widths exclude trailing whitespace, while overall metrics retain
/// Parley's layout extent.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::{layout_text, ParleyEngine, TextLayoutParams, WrapMode};
/// #[allow(deprecated)] let mut engine = ParleyEngine::new();
/// let laid = layout_text(&mut engine, TextLayoutParams {
///     text: "hello world",
///     style: TextStyle::new(FontId::Ui, 16, Color::WHITE),
///     max_width: Some(60.0),
///     wrap_mode: WrapMode::Word,
/// });
/// assert_eq!(laid.text(), "hello world");
/// assert!(!laid.lines.is_empty());
/// ```
pub fn layout_text(engine: &mut ParleyEngine, params: TextLayoutParams<'_>) -> LaidOutText {
    let max_width = match params.wrap_mode {
        WrapMode::NoWrap => None,
        WrapMode::Word | WrapMode::WordOrAnywhere => params.max_width,
    };

    let layout =
        engine.layout_text_with_wrap(params.text, params.style, max_width, params.wrap_mode);
    let metrics = TextMetrics {
        width: layout.width(),
        height: layout.height(),
    };

    let mut lines = Vec::new();
    for line in layout.lines() {
        let m = *line.metrics();
        let width = (m.advance - m.trailing_whitespace).max(0.0);
        lines.push(LaidOutLine {
            text_range: line.text_range(),
            width,
            ascent: m.ascent,
            descent: m.descent,
            baseline_y: m.baseline,
        });
    }

    LaidOutText {
        text: params.text.to_owned(),
        style: params.style,
        layout: Arc::new(layout),
        lines,
        metrics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailloli_ui_core::{style::Color, FontId};

    #[test]
    fn laid_out_text_has_lines_and_metrics() {
        #[allow(deprecated)]
        let mut eng = ParleyEngine::new();
        let style = TextStyle::new(FontId::Ui, 16, Color::new(1.0, 1.0, 1.0, 1.0));
        let params = TextLayoutParams {
            text: "hello\nworld",
            style,
            max_width: Some(80.0),
            wrap_mode: WrapMode::Word,
        };
        let laid = layout_text(&mut eng, params);
        assert!(!laid.lines.is_empty());
        assert!(laid.height() > 0.0);
    }

    #[test]
    fn caret_is_cluster_aware_for_combining_mark() {
        #[allow(deprecated)]
        let mut eng = ParleyEngine::new();
        let style = TextStyle::new(FontId::Ui, 16, Color::new(1.0, 1.0, 1.0, 1.0));
        let s = "e\u{301}"; // "e" + combining acute
        let laid = layout_text(
            &mut eng,
            TextLayoutParams {
                text: s,
                style,
                max_width: None,
                wrap_mode: WrapMode::NoWrap,
            },
        );
        assert_eq!(caret_x_at(&laid, 1), caret_x_at(&laid, 2));
        assert!(caret_x_at(&laid, s.len()) >= caret_x_at(&laid, 0));
    }

    #[test]
    fn caret_is_cluster_aware_for_emoji_sequence() {
        #[allow(deprecated)]
        let mut eng = ParleyEngine::new();
        let style = TextStyle::new(FontId::Ui, 16, Color::new(1.0, 1.0, 1.0, 1.0));
        let s = "👨‍👩‍👧‍👦";
        let laid = layout_text(
            &mut eng,
            TextLayoutParams {
                text: s,
                style,
                max_width: None,
                wrap_mode: WrapMode::NoWrap,
            },
        );
        assert_eq!(caret_x_at(&laid, 0), caret_x_at(&laid, 1));
        assert!(caret_x_at(&laid, s.len()) >= caret_x_at(&laid, 0));
    }
}
