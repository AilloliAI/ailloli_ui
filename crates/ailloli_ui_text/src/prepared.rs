//! One-shot prepared text layout: glyphs without per-instance font byte copies.

use std::collections::HashMap;
use std::sync::Arc;

use ailloli_ui_core::{style::TextStyle, Color, Rect};
use parley::PositionedLayoutItem;
use parley::{Affinity, Cursor};

use crate::engine_parley::ParleyEngine;
use crate::glyph::GlyphInstance;
use crate::params::{LaidOutLine, StyledTextLayoutParams, TextLayoutParams, TextMetrics, WrapMode};

/// Laid-out text plus glyph instances ready for GPU atlas rasterization.
///
/// The source text, internal Parley layout, and glyph slice are owned through
/// reference-counted allocations. Glyph instances identify font data stored in
/// the `face_blobs` map supplied to [`prepare_layout`] or
/// [`prepare_styled_layout`]; they do not copy font bytes individually.
/// Dimensions and glyph positions are logical pixels.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::{prepare_layout, ParleyEngine, TextLayoutParams};
/// #[allow(deprecated)] let mut engine = ParleyEngine::new();
/// let mut faces = HashMap::new();
/// let prepared = prepare_layout(
///     &mut engine,
///     TextLayoutParams::new("hello", TextStyle::new(FontId::Ui, 16, Color::WHITE)),
///     &mut faces,
/// );
/// assert_eq!(prepared.text(), "hello");
/// assert!(!prepared.lines.is_empty());
/// ```
#[derive(Debug)]
pub struct PreparedTextLayout {
    /// Owned source text referenced by byte ranges and caret methods.
    text: Arc<str>,
    /// Base style used for the layout.
    style: TextStyle,
    /// Uniform or styled Parley layout retained for caret hit testing.
    layout: Arc<ParleyLayoutDebug>,
    /// Per-line summaries in source order.
    pub lines: Vec<LaidOutLine>,
    /// Overall layout extent in logical pixels.
    pub metrics: TextMetrics,
    /// Flattened renderer-facing glyphs.
    glyphs: Arc<[GlyphInstance]>,
}

/// Local wrapper to implement `Debug` without exposing Parley brush types.
///
/// The enum remains public because it appears in generated private-item
/// documentation, but [`PreparedTextLayout`] keeps its instance private. Its
/// `Debug` implementation intentionally prints only the layout brush kind and
/// does not dump shaped text or font data.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::{ParleyEngine, prepared::ParleyLayoutDebug};
/// #[allow(deprecated)] let mut engine = ParleyEngine::new();
/// let layout = engine.layout_text("x", TextStyle::new(FontId::Ui, 12, Color::WHITE), None);
/// let wrapped = ParleyLayoutDebug::Uniform(layout);
/// assert!(format!("{wrapped:?}").contains("Layout"));
/// ```
pub enum ParleyLayoutDebug {
    /// Layout whose glyph runs carry unit brushes and use a uniform paint color.
    Uniform(parley::Layout<()>),
    /// Layout whose glyph runs carry per-span linear-RGBA brushes.
    Styled(parley::Layout<[f32; 4]>),
}

impl std::fmt::Debug for ParleyLayoutDebug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Uniform(_) => f.debug_struct("parley::Layout<()>").finish(),
            Self::Styled(_) => f.debug_struct("parley::Layout<[f32;4]>").finish(),
        }
    }
}

impl PreparedTextLayout {
    /// Returns the source text owned by this prepared layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::collections::HashMap;
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{prepare_layout, ParleyEngine, TextLayoutParams};
    /// #[allow(deprecated)] let mut engine = ParleyEngine::new();
    /// let mut faces = HashMap::new();
    /// let prepared = prepare_layout(&mut engine, TextLayoutParams::new("é", TextStyle::new(FontId::Ui, 14, Color::WHITE)), &mut faces);
    /// assert_eq!(prepared.text().len(), 2);
    /// ```
    pub fn text(&self) -> &str {
        self.text.as_ref()
    }

    /// Returns the copyable base style used to build the layout.
    ///
    /// For styled layouts this is the style outside overrides; individual glyph
    /// colors are available through [`Self::glyphs`].
    ///
    /// # Examples
    ///
    /// ```
    /// use std::collections::HashMap;
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{prepare_layout, ParleyEngine, TextLayoutParams};
    /// #[allow(deprecated)] let mut engine = ParleyEngine::new();
    /// let style = TextStyle::new(FontId::Mono, 13, Color::WHITE);
    /// let mut faces = HashMap::new();
    /// let prepared = prepare_layout(&mut engine, TextLayoutParams::new("x", style), &mut faces);
    /// assert_eq!(prepared.style(), style);
    /// ```
    pub fn style(&self) -> TextStyle {
        self.style
    }

    /// Returns the overall layout width in logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::collections::HashMap;
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{prepare_layout, ParleyEngine, TextLayoutParams};
    /// #[allow(deprecated)] let mut engine = ParleyEngine::new();
    /// let mut faces = HashMap::new();
    /// let prepared = prepare_layout(&mut engine, TextLayoutParams::new("width", TextStyle::new(FontId::Ui, 16, Color::WHITE)), &mut faces);
    /// assert_eq!(prepared.width(), prepared.metrics.width);
    /// ```
    pub fn width(&self) -> f32 {
        self.metrics.width
    }

    /// Returns the overall layout height in logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::collections::HashMap;
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{prepare_layout, ParleyEngine, TextLayoutParams};
    /// #[allow(deprecated)] let mut engine = ParleyEngine::new();
    /// let mut faces = HashMap::new();
    /// let prepared = prepare_layout(&mut engine, TextLayoutParams::new("height", TextStyle::new(FontId::Ui, 16, Color::WHITE)), &mut faces);
    /// assert_eq!(prepared.height(), prepared.metrics.height);
    /// assert!(prepared.height() > 0.0);
    /// ```
    pub fn height(&self) -> f32 {
        self.metrics.height
    }

    /// Borrows flattened glyph instances without allocating.
    ///
    /// Uniform layouts use `color == None`; styled layouts attach the effective
    /// linear-RGBA run color. An empty input can produce an empty slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::collections::HashMap;
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{prepare_layout, ParleyEngine, TextLayoutParams};
    /// #[allow(deprecated)] let mut engine = ParleyEngine::new();
    /// let mut faces = HashMap::new();
    /// let prepared = prepare_layout(&mut engine, TextLayoutParams::new("Hi", TextStyle::new(FontId::Ui, 16, Color::WHITE)), &mut faces);
    /// assert!(!prepared.glyphs().is_empty());
    /// assert!(prepared.glyphs().iter().all(|glyph| glyph.color.is_none()));
    /// ```
    pub fn glyphs(&self) -> &[GlyphInstance] {
        &self.glyphs
    }

    /// Maps a UTF-8 byte index to the downstream caret's logical-pixel X.
    ///
    /// Shaped clusters, combining sequences, and ligatures may make multiple
    /// byte indices share an X coordinate.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::collections::HashMap;
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{prepare_layout, ParleyEngine, TextLayoutParams};
    /// #[allow(deprecated)] let mut engine = ParleyEngine::new();
    /// let mut faces = HashMap::new();
    /// let prepared = prepare_layout(&mut engine, TextLayoutParams::new("abc", TextStyle::new(FontId::Ui, 16, Color::WHITE)), &mut faces);
    /// assert!(prepared.caret_x_at(3) >= prepared.caret_x_at(0));
    /// ```
    pub fn caret_x_at(&self, byte_idx: usize) -> f32 {
        self.caret_rect_at(byte_idx, 0.0).x
    }

    /// Maps a UTF-8 byte index to a logical-pixel caret rectangle.
    ///
    /// `width` is normalized with a zero lower bound; negative and NaN values
    /// behave as zero. A downstream affinity resolves ambiguous boundaries.
    /// The result height is never negative.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::collections::HashMap;
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{prepare_layout, ParleyEngine, TextLayoutParams};
    /// #[allow(deprecated)] let mut engine = ParleyEngine::new();
    /// let mut faces = HashMap::new();
    /// let prepared = prepare_layout(&mut engine, TextLayoutParams::new("abc", TextStyle::new(FontId::Ui, 16, Color::WHITE)), &mut faces);
    /// let rect = prepared.caret_rect_at(0, 1.5);
    /// assert!(rect.w >= 1.5 && rect.h >= 0.0);
    /// ```
    pub fn caret_rect_at(&self, byte_idx: usize, width: f32) -> Rect {
        match self.layout.as_ref() {
            ParleyLayoutDebug::Uniform(layout) => caret_rect_at_layout(layout, byte_idx, width),
            ParleyLayoutDebug::Styled(layout) => caret_rect_at_layout(layout, byte_idx, width),
        }
    }

    /// Maps a logical layout-space point to a cluster-aware UTF-8 byte index.
    ///
    /// Parley resolves points outside glyph bounds to a valid cursor in the
    /// layout. No explicit coordinate clamping is performed here.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::collections::HashMap;
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{prepare_layout, ParleyEngine, TextLayoutParams};
    /// #[allow(deprecated)] let mut engine = ParleyEngine::new();
    /// let mut faces = HashMap::new();
    /// let prepared = prepare_layout(&mut engine, TextLayoutParams::new("abc", TextStyle::new(FontId::Ui, 16, Color::WHITE)), &mut faces);
    /// assert_eq!(prepared.caret_index_at_point(-100.0, 0.0), 0);
    /// ```
    pub fn caret_index_at_point(&self, x: f32, y: f32) -> usize {
        match self.layout.as_ref() {
            ParleyLayoutDebug::Uniform(layout) => Cursor::from_point(layout, x, y).index(),
            ParleyLayoutDebug::Styled(layout) => Cursor::from_point(layout, x, y).index(),
        }
    }
}

/// Builds a Parley layout and glyphs; registers font blobs by `face_id`.
///
/// `face_blobs` is an append-only caller-owned registry during this call:
/// existing IDs are not replaced, and each newly encountered face receives one
/// reference-counted copy of its complete font bytes. The map has no eviction
/// policy. [`WrapMode::NoWrap`] ignores `params.max_width`; other modes treat it
/// as an optional logical-pixel constraint. Uniform glyph colors are `None`
/// because painting supplies `params.style.color` separately.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::{prepare_layout, ParleyEngine, TextLayoutParams};
/// #[allow(deprecated)] let mut engine = ParleyEngine::new();
/// let mut faces = HashMap::new();
/// let prepared = prepare_layout(
///     &mut engine,
///     TextLayoutParams::new("render", TextStyle::new(FontId::Mono, 14, Color::WHITE)),
///     &mut faces,
/// );
/// assert!(!prepared.glyphs().is_empty());
/// assert!(!faces.is_empty());
/// ```
pub fn prepare_layout(
    engine: &mut ParleyEngine,
    params: TextLayoutParams<'_>,
    face_blobs: &mut HashMap<u64, Arc<[u8]>>,
) -> PreparedTextLayout {
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
    let lines = laid_out_lines(&layout);
    let glyphs = glyphs_from_layout(&layout, face_blobs, None);

    PreparedTextLayout {
        text: Arc::from(params.text),
        style: params.style,
        layout: Arc::new(ParleyLayoutDebug::Uniform(layout)),
        lines,
        metrics,
        glyphs: glyphs.into(),
    }
}

/// Builds a colored, ranged-style layout and registers its font blobs.
///
/// Each output glyph carries the effective run color. Span ranges must be
/// ordered, in bounds, and on UTF-8 boundaries; they replace font family, size,
/// and color, while text decoration remains paint-only. `face_blobs` has the
/// same append-only, no-eviction behavior as in [`prepare_layout`].
///
/// # Panics
///
/// Parley may panic if a span has an invalid UTF-8 byte range.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::{ParleyEngine, StyledTextLayoutParams, StyledTextSpan, WrapMode};
/// use ailloli_ui_text::prepared::prepare_styled_layout;
/// #[allow(deprecated)] let mut engine = ParleyEngine::new();
/// let base = TextStyle::new(FontId::Mono, 14, Color::WHITE);
/// let spans = [StyledTextSpan { range: 0..2, style: TextStyle::new(FontId::Mono, 14, Color::BLACK) }];
/// let mut faces = HashMap::new();
/// let prepared = prepare_styled_layout(&mut engine, StyledTextLayoutParams {
///     text: "fn main", base_style: base, spans: &spans,
///     max_width: None, wrap_mode: WrapMode::NoWrap,
/// }, &mut faces);
/// assert!(prepared.glyphs().iter().all(|glyph| glyph.color.is_some()));
/// ```
pub fn prepare_styled_layout(
    engine: &mut ParleyEngine,
    params: StyledTextLayoutParams<'_>,
    face_blobs: &mut HashMap<u64, Arc<[u8]>>,
) -> PreparedTextLayout {
    let max_width = match params.wrap_mode {
        WrapMode::NoWrap => None,
        WrapMode::Word | WrapMode::WordOrAnywhere => params.max_width,
    };

    let layout = engine.layout_styled_text_with_wrap(
        params.text,
        params.base_style,
        params.spans,
        max_width,
        params.wrap_mode,
    );
    let metrics = TextMetrics {
        width: layout.width(),
        height: layout.height(),
    };
    let lines = laid_out_lines(&layout);
    let glyphs = styled_glyphs_from_layout(&layout, face_blobs);

    PreparedTextLayout {
        text: Arc::from(params.text),
        style: params.base_style,
        layout: Arc::new(ParleyLayoutDebug::Styled(layout)),
        lines,
        metrics,
        glyphs: glyphs.into(),
    }
}

/// Extracts source ranges and logical-pixel metrics from all Parley lines.
fn laid_out_lines<B: parley::style::Brush>(layout: &parley::Layout<B>) -> Vec<LaidOutLine> {
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
    lines
}

/// Flattens uniform-brush glyph runs and records their font blobs.
fn glyphs_from_layout(
    layout: &parley::Layout<()>,
    face_blobs: &mut HashMap<u64, Arc<[u8]>>,
    color: Option<Color>,
) -> Vec<GlyphInstance> {
    let mut out = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            push_glyph_run(&mut out, face_blobs, &run, color);
        }
    }
    out
}

/// Flattens colored glyph runs and converts brushes to Ailloli colors.
fn styled_glyphs_from_layout(
    layout: &parley::Layout<[f32; 4]>,
    face_blobs: &mut HashMap<u64, Arc<[u8]>>,
) -> Vec<GlyphInstance> {
    let mut out = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let [r, g, b, a] = run.style().brush;
            push_glyph_run(
                &mut out,
                face_blobs,
                &run,
                Some(Color::from_f32_const(r, g, b, a)),
            );
        }
    }
    out
}

/// Registers one run's font once and appends all of its positioned glyphs.
fn push_glyph_run<B: parley::style::Brush>(
    out: &mut Vec<GlyphInstance>,
    face_blobs: &mut HashMap<u64, Arc<[u8]>>,
    run: &parley::layout::GlyphRun<'_, B>,
    color: Option<Color>,
) {
    let font = run.run().font();
    let face_id = font.data.id();
    let font_index = font.index;
    face_blobs
        .entry(face_id)
        .or_insert_with(|| Arc::from(font.data.data()));

    let px_size = run.run().font_size().round().max(1.0) as u16;
    for g in run.positioned_glyphs() {
        out.push(GlyphInstance {
            face_id,
            font_index,
            glyph_id: g.id,
            px_size,
            x: g.x,
            y: g.y,
            color,
        });
    }
}

/// Computes downstream caret geometry for either Parley brush type.
fn caret_rect_at_layout<B: parley::style::Brush>(
    layout: &parley::Layout<B>,
    byte_idx: usize,
    width: f32,
) -> Rect {
    let cur = Cursor::from_byte_index(layout, byte_idx, Affinity::Downstream);
    let rect = cur.geometry(layout, width.max(0.0));
    Rect::new(
        rect.x0 as f32,
        rect.y0 as f32,
        (rect.x1 - rect.x0).max(width.max(0.0) as f64) as f32,
        (rect.y1 - rect.y0).max(0.0) as f32,
    )
}
