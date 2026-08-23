//! Central text subsystem: shared Parley engine, layout LRU cache, font blobs per face.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::Arc;

use ailloli_ui_core::style::TextStyle;
use lru::LruCache;

use crate::engine_parley::ParleyEngine;
use crate::params::{StyledTextLayoutParams, StyledTextSpan, TextLayoutParams, WrapMode};
use crate::prepared::{prepare_layout, prepare_styled_layout, PreparedTextLayout};

/// Hashes only metric-affecting uniform style fields.
///
/// Color and decoration are deliberately excluded because the uniform paint
/// path supplies both independently of shaping.
fn hash_style(style: TextStyle) -> u64 {
    let mut h = DefaultHasher::new();
    style.font.hash(&mut h);
    style.px_size.hash(&mut h);
    h.finish()
}

/// Hashes metric fields and color quantized through [`ailloli_ui_core::Color::as_rgba8`].
///
/// Paint-only decoration remains excluded.
fn hash_style_with_color(style: TextStyle) -> u64 {
    let mut h = DefaultHasher::new();
    style.font.hash(&mut h);
    style.px_size.hash(&mut h);
    style.color.as_rgba8().hash(&mut h);
    h.finish()
}

/// Hashes ordered span ranges and their size, family, and 8-bit color values.
fn hash_spans(spans: &[StyledTextSpan]) -> u64 {
    let mut h = DefaultHasher::new();
    for span in spans {
        span.range.start.hash(&mut h);
        span.range.end.hash(&mut h);
        hash_style_with_color(span.style).hash(&mut h);
    }
    h.finish()
}

/// Cache key for a text layout (content + style + quantized max width + wrap mode).
///
/// Text is represented by a [`DefaultHasher`] digest plus byte length. Uniform
/// style keys include font and size but intentionally omit color and decoration.
/// Styled keys add base/span colors after 8-bit quantization and still omit
/// decoration. Widths are rounded to thousandths of a logical pixel. Therefore
/// the key is optimized for cache reuse rather than being a lossless serialization
/// of every request field.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::{TextLayoutKey, TextLayoutParams, WrapMode};
/// let style = TextStyle::new(FontId::Ui, 14, Color::WHITE);
/// let a = TextLayoutKey::from_params(&TextLayoutParams {
///     text: "same", style, max_width: Some(80.0), wrap_mode: WrapMode::NoWrap,
/// });
/// let b = TextLayoutKey::from_params(&TextLayoutParams {
///     text: "same", style, max_width: Some(800.0), wrap_mode: WrapMode::NoWrap,
/// });
/// assert_eq!(a, b);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextLayoutKey {
    /// Hash of all source-text bytes.
    text_hash: u64,
    /// Source byte length, adding a cheap collision discriminator.
    text_len: usize,
    /// Uniform or color-aware style hash, depending on the constructor.
    style_hash: u64,
    /// Ordered styled-span hash, or zero for uniform text.
    span_hash: u64,
    /// `max_width` quantized to thousandths of a pixel (`None` => `u32::MAX`).
    max_width_q: u32,
    /// Stable local tag: zero for no-wrap, one for word, two for anywhere.
    wrap: u8,
}

impl TextLayoutKey {
    /// Builds a cache key from layout parameters.
    ///
    /// `NoWrap` always uses the same sentinel width. Otherwise `None` uses
    /// `u32::MAX`; finite widths are lower-bounded at zero, multiplied by 1000,
    /// rounded, and saturating-cast to `u32`. NaN behaves as zero, while positive
    /// infinity and sufficiently large widths collide with the `None` sentinel.
    /// Uniform color and decoration do not affect this key.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{TextLayoutKey, TextLayoutParams, WrapMode};
    /// let white = TextStyle::new(FontId::Ui, 14, Color::WHITE);
    /// let black = TextStyle::new(FontId::Ui, 14, Color::BLACK).underline();
    /// let a = TextLayoutKey::from_params(&TextLayoutParams { text: "x", style: white, max_width: Some(10.0001), wrap_mode: WrapMode::Word });
    /// let b = TextLayoutKey::from_params(&TextLayoutParams { text: "x", style: black, max_width: Some(10.0002), wrap_mode: WrapMode::Word });
    /// assert_eq!(a, b);
    /// ```
    pub fn from_params(params: &TextLayoutParams<'_>) -> Self {
        let mut h = DefaultHasher::new();
        params.text.hash(&mut h);
        let text_hash = h.finish();
        let wrap = match params.wrap_mode {
            WrapMode::NoWrap => 0u8,
            WrapMode::Word => 1u8,
            WrapMode::WordOrAnywhere => 2u8,
        };
        let max_width_q = match params.wrap_mode {
            WrapMode::NoWrap => u32::MAX,
            WrapMode::Word | WrapMode::WordOrAnywhere => params
                .max_width
                .map(|w: f32| (w.max(0.0) * 1000.0).round() as u32)
                .unwrap_or(u32::MAX),
        };
        Self {
            text_hash,
            text_len: params.text.len(),
            style_hash: hash_style(params.style),
            span_hash: 0,
            max_width_q,
            wrap,
        }
    }

    /// Builds a cache key from styled layout parameters.
    ///
    /// The key includes ordered span ranges and base/span colors quantized to
    /// eight bits per channel. It omits all decorations. Unlike
    /// [`Self::from_params`], a base color change normally changes the key;
    /// colors that quantize to identical RGBA8 values remain equivalent.
    /// Width normalization and sentinels are the same as for uniform text.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{StyledTextLayoutParams, TextLayoutKey, WrapMode};
    /// let white = TextStyle::new(FontId::Mono, 13, Color::WHITE);
    /// let black = TextStyle::new(FontId::Mono, 13, Color::BLACK);
    /// let a = TextLayoutKey::from_styled_params(&StyledTextLayoutParams { text: "x", base_style: white, spans: &[], max_width: None, wrap_mode: WrapMode::NoWrap });
    /// let b = TextLayoutKey::from_styled_params(&StyledTextLayoutParams { text: "x", base_style: black, spans: &[], max_width: None, wrap_mode: WrapMode::NoWrap });
    /// assert_ne!(a, b);
    /// ```
    pub fn from_styled_params(params: &StyledTextLayoutParams<'_>) -> Self {
        let mut h = DefaultHasher::new();
        params.text.hash(&mut h);
        let text_hash = h.finish();
        let wrap = match params.wrap_mode {
            WrapMode::NoWrap => 0u8,
            WrapMode::Word => 1u8,
            WrapMode::WordOrAnywhere => 2u8,
        };
        let max_width_q = match params.wrap_mode {
            WrapMode::NoWrap => u32::MAX,
            WrapMode::Word | WrapMode::WordOrAnywhere => params
                .max_width
                .map(|w: f32| (w.max(0.0) * 1000.0).round() as u32)
                .unwrap_or(u32::MAX),
        };
        Self {
            text_hash,
            text_len: params.text.len(),
            style_hash: hash_style_with_color(params.base_style),
            span_hash: hash_spans(params.spans),
            max_width_q,
            wrap,
        }
    }

    /// Builds a styled key from already-normalized span parts.
    fn from_styled_parts(
        text: &str,
        base_style: TextStyle,
        spans: &[StyledTextSpan],
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> Self {
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        let text_hash = h.finish();
        let wrap = match wrap_mode {
            WrapMode::NoWrap => 0u8,
            WrapMode::Word => 1u8,
            WrapMode::WordOrAnywhere => 2u8,
        };
        let max_width_q = match wrap_mode {
            WrapMode::NoWrap => u32::MAX,
            WrapMode::Word | WrapMode::WordOrAnywhere => max_width
                .map(|w: f32| (w.max(0.0) * 1000.0).round() as u32)
                .unwrap_or(u32::MAX),
        };
        Self {
            text_hash,
            text_len: text.len(),
            style_hash: hash_style_with_color(base_style),
            span_hash: hash_spans(spans),
            max_width_q,
            wrap,
        }
    }
}

/// Cheap handle to a prepared layout (`Arc` clone).
///
/// Cloning the handle increments a reference count and does not repeat shaping
/// or copy glyph/font data.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::{TextLayoutHandle, TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let handle: TextLayoutHandle = system.layout_cached(TextLayoutParams::new(
///     "shared", TextStyle::new(FontId::Ui, 14, Color::WHITE),
/// ));
/// assert!(Arc::ptr_eq(&handle, &handle.clone()));
/// ```
pub type TextLayoutHandle = Arc<PreparedTextLayout>;

/// Retained text system: one instance per window (or shared app-wide).
///
/// One system owns mutable Parley contexts, a 2048-entry LRU layout cache, and
/// an unbounded registry of font-face byte blobs encountered by prepared
/// layouts. Methods require `&mut self` while shaping or updating LRU recency;
/// external synchronization is required for shared concurrent use.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::TextSystem;
/// let system = TextSystem::new();
/// assert_eq!(system.cached_layout_count(), 0);
/// assert_eq!(system.metrics_revision(), 1);
/// ```
pub struct TextSystem {
    /// Stateful font and layout contexts.
    engine: ParleyEngine,
    /// Font bytes retained by face ID without eviction.
    face_blobs: HashMap<u64, Arc<[u8]>>,
    /// Prepared-layout LRU with a fixed entry capacity.
    cache: LruCache<TextLayoutKey, TextLayoutHandle>,
    /// Nonzero wrapping generation for metric-dependent consumers.
    metrics_revision: u64,
}

impl Default for TextSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl TextSystem {
    /// Creates a new system with an empty LRU cache (capacity 2048 entries).
    ///
    /// The bundled monospace font is registered in the internal engine, while
    /// the renderer-facing face registry remains empty until the first layout
    /// produces glyphs. The metrics revision starts at the nonzero sentinel 1.
    /// Construction may scan the relative `assets/fonts` directory through the
    /// legacy Parley constructor.
    ///
    /// # Panics
    ///
    /// Panics only if the fixed cache-capacity constant is changed from 2048 to
    /// zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextSystem;
    /// let system = TextSystem::new();
    /// assert_eq!(system.cached_layout_count(), 0);
    /// assert!(system.face_blobs().is_empty());
    /// assert_eq!(system.metrics_revision(), 1);
    /// ```
    pub fn new() -> Self {
        let cap = NonZeroUsize::new(2048).expect("2048 > 0");
        Self {
            #[allow(deprecated)]
            engine: ParleyEngine::new(),
            face_blobs: HashMap::new(),
            cache: LruCache::new(cap),
            metrics_revision: 1,
        }
    }

    /// Lays out text with LRU caching; skips relayout when the key and text match.
    ///
    /// A hit clones the stored [`Arc`] and updates LRU recency. Full source-text
    /// equality guards text-hash collisions, but other lossy key components are
    /// not secondarily compared. In particular, uniform color and decoration
    /// are paint-only and reuse the same entry, so [`PreparedTextLayout::style`]
    /// on a hit can be the style from the request that populated the cache.
    /// Widths differing by less than key quantization may likewise share a layout.
    /// The cache evicts least-recently-used layouts after 2048 distinct keys;
    /// registered font blobs are not evicted.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{TextLayoutParams, TextSystem};
    /// let mut system = TextSystem::new();
    /// let params = TextLayoutParams::new("cached", TextStyle::new(FontId::Ui, 14, Color::WHITE));
    /// let first = system.layout_cached(params);
    /// let second = system.layout_cached(params);
    /// assert!(Arc::ptr_eq(&first, &second));
    /// assert_eq!(system.cached_layout_count(), 1);
    /// ```
    pub fn layout_cached(&mut self, params: TextLayoutParams<'_>) -> TextLayoutHandle {
        let key = TextLayoutKey::from_params(&params);
        if let Some(hit) = self.cache.get(&key) {
            if hit.text() == params.text {
                return hit.clone();
            }
        }
        let prepared = prepare_layout(&mut self.engine, params, &mut self.face_blobs);
        let arc = Arc::new(prepared);
        self.cache.put(key, arc.clone());
        arc
    }

    /// Lays out styled text with LRU caching.
    ///
    /// Spans are first clamped, validated, overlap-resolved with later input
    /// winning, stripped when equal to the base style, and merged when adjacent.
    /// If none remain, this delegates to the uniform cache. Styled keys include
    /// RGBA8 colors but omit decoration, so sub-byte color differences and
    /// decorations can reuse the first prepared entry. A cache hit also verifies
    /// exact source text. Output glyphs carry their effective run colors.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{StyledTextLayoutParams, StyledTextSpan, TextSystem, WrapMode};
    /// let mut system = TextSystem::new();
    /// let base = TextStyle::new(FontId::Mono, 13, Color::WHITE);
    /// let accent = TextStyle::new(FontId::Mono, 13, Color::BLACK);
    /// let spans = [StyledTextSpan { range: 0..2, style: accent }];
    /// let params = StyledTextLayoutParams { text: "fn main", base_style: base, spans: &spans, max_width: None, wrap_mode: WrapMode::NoWrap };
    /// let first = system.layout_styled_cached(params);
    /// let second = system.layout_styled_cached(params);
    /// assert!(Arc::ptr_eq(&first, &second));
    /// assert!(first.glyphs().iter().any(|glyph| glyph.color == Some(accent.color)));
    /// ```
    pub fn layout_styled_cached(&mut self, params: StyledTextLayoutParams<'_>) -> TextLayoutHandle {
        let spans = normalize_styled_spans(params.text, params.base_style, params.spans);
        if spans.is_empty() {
            return self.layout_cached(TextLayoutParams {
                text: params.text,
                style: params.base_style,
                max_width: params.max_width,
                wrap_mode: params.wrap_mode,
            });
        }
        let key = TextLayoutKey::from_styled_parts(
            params.text,
            params.base_style,
            &spans,
            params.max_width,
            params.wrap_mode,
        );
        if let Some(hit) = self.cache.get(&key) {
            if hit.text() == params.text {
                return hit.clone();
            }
        }
        let prepared = prepare_styled_layout(
            &mut self.engine,
            StyledTextLayoutParams {
                text: params.text,
                base_style: params.base_style,
                spans: &spans,
                max_width: params.max_width,
                wrap_mode: params.wrap_mode,
            },
            &mut self.face_blobs,
        );
        let arc = Arc::new(prepared);
        self.cache.put(key, arc.clone());
        arc
    }

    /// Direct access to the Parley engine (migration / diagnostics).
    ///
    /// Layouts created directly through this reference bypass this system's LRU
    /// and face-blob registration. Mutating fonts or shaping state can make
    /// cached metrics stale; call [`Self::invalidate_metrics`] afterward when
    /// the change can affect existing results.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::{ParleyEngine, TextSystem};
    /// let mut system = TextSystem::new();
    /// let _: &mut ParleyEngine = system.parley_engine_mut();
    /// system.invalidate_metrics();
    /// assert_eq!(system.cached_layout_count(), 0);
    /// ```
    pub fn parley_engine_mut(&mut self) -> &mut ParleyEngine {
        &mut self.engine
    }

    /// Font bytes for a Parley `face_id`, if registered.
    ///
    /// A face becomes registered only after a cached prepared layout emits at
    /// least one glyph using it. `None` means the system has not encountered the
    /// ID. The returned slice is borrowed from retained reference-counted data.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{TextLayoutParams, TextSystem};
    /// let mut system = TextSystem::new();
    /// let layout = system.layout_cached(TextLayoutParams::new("x", TextStyle::new(FontId::Ui, 14, Color::WHITE)));
    /// let face_id = layout.glyphs()[0].face_id;
    /// assert!(!system.face_blob(face_id).unwrap().is_empty());
    /// assert_eq!(system.face_blob(u64::MAX), None);
    /// ```
    pub fn face_blob(&self, face_id: u64) -> Option<&[u8]> {
        self.face_blobs.get(&face_id).map(|b| b.as_ref())
    }

    /// All registered face blobs (for renderer lookup).
    ///
    /// The borrowed map grows as new faces are shaped and is not pruned by LRU
    /// eviction or metric invalidation. Keys are Parley face IDs; values hold
    /// complete font bytes shared with snapshots.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{TextLayoutParams, TextSystem};
    /// let mut system = TextSystem::new();
    /// assert!(system.face_blobs().is_empty());
    /// let _ = system.layout_cached(TextLayoutParams::new("x", TextStyle::new(FontId::Mono, 14, Color::WHITE)));
    /// assert!(!system.face_blobs().is_empty());
    /// ```
    pub fn face_blobs(&self) -> &HashMap<u64, Arc<[u8]>> {
        &self.face_blobs
    }

    /// Cheap snapshot for the GPU renderer (`Arc` values inside).
    ///
    /// This allocates and clones the hash-map structure in O(number of faces),
    /// but only increments reference counts for the potentially large font byte
    /// buffers. Later registrations do not appear in an existing snapshot.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use ailloli_ui_text::TextSystem;
    /// let system = TextSystem::new();
    /// let snapshot = system.face_blobs_snapshot();
    /// assert!(snapshot.is_empty());
    /// assert_eq!(Arc::strong_count(&snapshot), 1);
    /// ```
    pub fn face_blobs_snapshot(&self) -> Arc<HashMap<u64, Arc<[u8]>>> {
        Arc::new(self.face_blobs.clone())
    }

    /// Returns the number of resident LRU entries, from zero through 2048.
    ///
    /// Styled requests that normalize to no spans share the uniform cache and
    /// do not necessarily add a distinct entry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{TextLayoutParams, TextSystem};
    /// let mut system = TextSystem::new();
    /// assert_eq!(system.cached_layout_count(), 0);
    /// let _ = system.layout_cached(TextLayoutParams::new("x", TextStyle::new(FontId::Ui, 14, Color::WHITE)));
    /// assert_eq!(system.cached_layout_count(), 1);
    /// ```
    pub fn cached_layout_count(&self) -> usize {
        self.cache.len()
    }

    /// Nonzero revision of inputs that can alter text metrics.
    ///
    /// The value starts at one and never returns zero. It increases on
    /// [`Self::invalidate_metrics`] until `u64` overflow, at which point it wraps
    /// back to one; consumers should compare for inequality rather than ordering.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextSystem;
    /// let mut system = TextSystem::new();
    /// let before = system.metrics_revision();
    /// system.invalidate_metrics();
    /// assert_ne!(system.metrics_revision(), before);
    /// ```
    pub const fn metrics_revision(&self) -> u64 {
        self.metrics_revision
    }

    /// Invalidates metric-dependent caches after a font or shaping change.
    ///
    /// This clears every prepared layout and advances the nonzero wrapping
    /// revision. It deliberately retains registered face blobs and all internal
    /// Parley/font caches; memory associated with those resources is not freed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{TextLayoutParams, TextSystem};
    /// let mut system = TextSystem::new();
    /// let _ = system.layout_cached(TextLayoutParams::new("x", TextStyle::new(FontId::Ui, 14, Color::WHITE)));
    /// let faces = system.face_blobs().len();
    /// system.invalidate_metrics();
    /// assert_eq!(system.cached_layout_count(), 0);
    /// assert_eq!(system.face_blobs().len(), faces);
    /// ```
    pub fn invalidate_metrics(&mut self) {
        self.metrics_revision = self.metrics_revision.wrapping_add(1).max(1);
        self.cache.clear();
    }
}

/// Canonicalizes externally supplied style ranges before shaping and caching.
///
/// Endpoints are clamped to the text length. Empty, reversed, or non-UTF-8-boundary
/// ranges are dropped. Where valid ranges overlap, the later input slice entry
/// wins. Segments equal to `base_style` are removed, and adjacent segments with
/// identical styles are merged. The result is sorted, disjoint, and contains no
/// empty ranges.
///
/// # Examples
///
/// The canonicalization is observable through public styled-cache reuse:
///
/// ```
/// use std::sync::Arc;
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::{StyledTextLayoutParams, StyledTextSpan, TextSystem, WrapMode};
/// let mut system = TextSystem::new();
/// let base = TextStyle::new(FontId::Mono, 13, Color::WHITE);
/// let accent = TextStyle::new(FontId::Mono, 13, Color::BLACK);
/// let split = [
///     StyledTextSpan { range: 0..1, style: accent },
///     StyledTextSpan { range: 1..2, style: accent },
/// ];
/// let merged = [StyledTextSpan { range: 0..2, style: accent }];
/// let a = system.layout_styled_cached(StyledTextLayoutParams { text: "fn", base_style: base, spans: &split, max_width: None, wrap_mode: WrapMode::NoWrap });
/// let b = system.layout_styled_cached(StyledTextLayoutParams { text: "fn", base_style: base, spans: &merged, max_width: None, wrap_mode: WrapMode::NoWrap });
/// assert!(Arc::ptr_eq(&a, &b));
/// ```
pub(crate) fn normalize_styled_spans(
    text: &str,
    base_style: TextStyle,
    spans: &[StyledTextSpan],
) -> Vec<StyledTextSpan> {
    #[derive(Clone, Copy)]
    /// Validated input span with its precedence position.
    struct Candidate {
        /// Inclusive start byte.
        start: usize,
        /// Exclusive end byte.
        end: usize,
        /// Input order used for last-wins resolution.
        index: usize,
        /// Complete style carried by this candidate.
        style: TextStyle,
    }

    let mut candidates = Vec::new();
    let text_len = text.len();
    for (index, span) in spans.iter().enumerate() {
        let start = span.range.start.min(text_len);
        let end = span.range.end.min(text_len);
        if start >= end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            continue;
        }
        candidates.push(Candidate {
            start,
            end,
            index,
            style: span.style,
        });
    }
    if candidates.is_empty() {
        return Vec::new();
    }

    let mut points = Vec::with_capacity(candidates.len() * 2);
    for span in &candidates {
        points.push(span.start);
        points.push(span.end);
    }
    points.sort_unstable();
    points.dedup();

    let mut normalized: Vec<StyledTextSpan> = Vec::new();
    for window in points.windows(2) {
        let start = window[0];
        let end = window[1];
        if start >= end {
            continue;
        }
        let Some(active) = candidates
            .iter()
            .filter(|span| span.start <= start && end <= span.end)
            .max_by_key(|span| span.index)
        else {
            continue;
        };
        if active.style == base_style {
            continue;
        }
        if let Some(previous) = normalized.last_mut() {
            if previous.range.end == start && previous.style == active.style {
                previous.range.end = end;
                continue;
            }
        }
        normalized.push(StyledTextSpan {
            range: start..end,
            style: active.style,
        });
    }
    normalized
}

/// Placeholder for future multi-paragraph document storage.
///
/// This is currently a zero-sized marker with no storage or behavior. Use
/// [`crate::TextBuffer`] for implemented rope-backed paragraph operations.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::ParagraphStore;
/// let store = ParagraphStore;
/// assert_eq!(core::mem::size_of_val(&store), 0);
/// ```
#[derive(Debug, Default)]
pub struct ParagraphStore;

#[cfg(test)]
mod tests {
    use super::*;
    use ailloli_ui_core::style::Color;
    use ailloli_ui_core::FontId;

    #[test]
    fn layout_cached_returns_same_arc_for_identical_key() {
        let mut ts = TextSystem::new();
        let style = TextStyle::new(FontId::Ui, 14, Color::new(1.0, 1.0, 1.0, 1.0));
        let params = TextLayoutParams {
            text: "hello world",
            style,
            max_width: Some(200.0),
            wrap_mode: WrapMode::NoWrap,
        };
        let a = ts.layout_cached(params);
        let b = ts.layout_cached(params);
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn paint_only_decoration_reuses_cached_layout() {
        let mut ts = TextSystem::new();
        let plain = TextStyle::new(FontId::Ui, 14, Color::WHITE);
        let underlined = plain.underline();
        let a = ts.layout_cached(TextLayoutParams {
            text: "Documentation",
            style: plain,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        });
        let b = ts.layout_cached(TextLayoutParams {
            text: "Documentation",
            style: underlined,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        });

        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn no_wrap_key_ignores_max_width() {
        let style = TextStyle::new(FontId::Ui, 14, Color::new(1.0, 1.0, 1.0, 1.0));
        let a = TextLayoutKey::from_params(&TextLayoutParams {
            text: "stable width",
            style,
            max_width: Some(80.0),
            wrap_mode: WrapMode::NoWrap,
        });
        let b = TextLayoutKey::from_params(&TextLayoutParams {
            text: "stable width",
            style,
            max_width: Some(240.0),
            wrap_mode: WrapMode::NoWrap,
        });
        assert_eq!(a, b);
    }

    #[test]
    fn no_wrap_cache_reuses_layout_across_widths() {
        let mut ts = TextSystem::new();
        let style = TextStyle::new(FontId::Ui, 14, Color::new(1.0, 1.0, 1.0, 1.0));
        let a = ts.layout_cached(TextLayoutParams {
            text: "paint reuses layout",
            style,
            max_width: Some(80.0),
            wrap_mode: WrapMode::NoWrap,
        });
        let b = ts.layout_cached(TextLayoutParams {
            text: "paint reuses layout",
            style,
            max_width: Some(240.0),
            wrap_mode: WrapMode::NoWrap,
        });
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn styled_layout_cache_includes_span_styles() {
        let mut ts = TextSystem::new();
        let base = TextStyle::new(FontId::Mono, 13, Color::WHITE);
        let keyword = TextStyle::new(FontId::Mono, 13, Color::hex_rgb(0xFF7A18));
        let string = TextStyle::new(FontId::Mono, 13, Color::hex_rgb(0x22C55E));
        let a_spans = [StyledTextSpan {
            range: 0..2,
            style: keyword,
        }];
        let b_spans = [StyledTextSpan {
            range: 0..2,
            style: string,
        }];

        let a = ts.layout_styled_cached(StyledTextLayoutParams {
            text: "fn main",
            base_style: base,
            spans: &a_spans,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        });
        let a_again = ts.layout_styled_cached(StyledTextLayoutParams {
            text: "fn main",
            base_style: base,
            spans: &a_spans,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        });
        let b = ts.layout_styled_cached(StyledTextLayoutParams {
            text: "fn main",
            base_style: base,
            spans: &b_spans,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        });

        assert!(Arc::ptr_eq(&a, &a_again));
        assert!(!Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn normalize_styled_spans_sorts_clamps_overlaps_and_merges() {
        let base = TextStyle::new(FontId::Mono, 13, Color::WHITE);
        let keyword = TextStyle::new(FontId::Mono, 13, Color::hex_rgb(0xFF7A18));
        let string = TextStyle::new(FontId::Mono, 13, Color::hex_rgb(0x22C55E));
        let text = "fn café value";
        let spans = [
            StyledTextSpan {
                range: 8..99,
                style: keyword,
            },
            StyledTextSpan {
                range: 0..2,
                style: keyword,
            },
            StyledTextSpan {
                range: 1..10,
                style: string,
            },
            StyledTextSpan {
                range: 4..5,
                style: keyword,
            },
        ];

        let normalized = normalize_styled_spans(text, base, &spans);

        assert_eq!(
            normalized,
            vec![
                StyledTextSpan {
                    range: 0..1,
                    style: keyword,
                },
                StyledTextSpan {
                    range: 1..4,
                    style: string,
                },
                StyledTextSpan {
                    range: 4..5,
                    style: keyword,
                },
                StyledTextSpan {
                    range: 5..10,
                    style: string,
                },
                StyledTextSpan {
                    range: 10..14,
                    style: keyword,
                },
            ]
        );
    }

    #[test]
    fn normalize_styled_spans_drops_empty_base_and_invalid_utf8_ranges() {
        let base = TextStyle::new(FontId::Mono, 13, Color::WHITE);
        let keyword = TextStyle::new(FontId::Mono, 13, Color::hex_rgb(0xFF7A18));
        let text = "éclair";
        let spans = [
            StyledTextSpan {
                range: 0..0,
                style: keyword,
            },
            StyledTextSpan {
                range: 1..3,
                style: keyword,
            },
            StyledTextSpan {
                range: 2..6,
                style: base,
            },
        ];

        assert!(normalize_styled_spans(text, base, &spans).is_empty());
    }

    #[test]
    fn styled_layout_cache_uses_normalized_spans_and_fallback_uniform() {
        let mut ts = TextSystem::new();
        let base = TextStyle::new(FontId::Mono, 13, Color::WHITE);
        let keyword = TextStyle::new(FontId::Mono, 13, Color::hex_rgb(0xFF7A18));
        let sorted = [
            StyledTextSpan {
                range: 0..1,
                style: keyword,
            },
            StyledTextSpan {
                range: 1..2,
                style: keyword,
            },
        ];
        let merged = [StyledTextSpan {
            range: 0..2,
            style: keyword,
        }];

        let a = ts.layout_styled_cached(StyledTextLayoutParams {
            text: "fn main",
            base_style: base,
            spans: &sorted,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        });
        let b = ts.layout_styled_cached(StyledTextLayoutParams {
            text: "fn main",
            base_style: base,
            spans: &merged,
            max_width: Some(500.0),
            wrap_mode: WrapMode::NoWrap,
        });
        let uniform = ts.layout_cached(TextLayoutParams {
            text: "fn main",
            style: base,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        });
        let styled_without_spans = ts.layout_styled_cached(StyledTextLayoutParams {
            text: "fn main",
            base_style: base,
            spans: &[],
            max_width: Some(100.0),
            wrap_mode: WrapMode::NoWrap,
        });

        assert!(Arc::ptr_eq(&a, &b));
        assert!(Arc::ptr_eq(&uniform, &styled_without_spans));
        assert!(a
            .glyphs()
            .iter()
            .any(|glyph| glyph.color == Some(keyword.color)));
    }

    #[test]
    fn style_key_ignores_color() {
        let white = TextStyle::new(FontId::Ui, 14, Color::new(1.0, 1.0, 1.0, 1.0));
        let red = TextStyle::new(FontId::Ui, 14, Color::new(1.0, 0.0, 0.0, 1.0));
        let a = TextLayoutKey::from_params(&TextLayoutParams {
            text: "same layout",
            style: white,
            max_width: Some(200.0),
            wrap_mode: WrapMode::Word,
        });
        let b = TextLayoutKey::from_params(&TextLayoutParams {
            text: "same layout",
            style: red,
            max_width: Some(200.0),
            wrap_mode: WrapMode::Word,
        });
        assert_eq!(a, b);
    }

    #[test]
    fn wrap_mode_is_part_of_cache_key() {
        let style = TextStyle::new(FontId::Ui, 14, Color::new(1.0, 1.0, 1.0, 1.0));
        let word = TextLayoutKey::from_params(&TextLayoutParams {
            text: "same layout",
            style,
            max_width: Some(80.0),
            wrap_mode: WrapMode::Word,
        });
        let anywhere = TextLayoutKey::from_params(&TextLayoutParams {
            text: "same layout",
            style,
            max_width: Some(80.0),
            wrap_mode: WrapMode::WordOrAnywhere,
        });
        assert_ne!(word, anywhere);
    }

    #[test]
    fn word_wrap_keeps_long_unspaced_text_on_one_line() {
        let mut ts = TextSystem::new();
        let style = TextStyle::new(FontId::Mono, 13, Color::new(1.0, 1.0, 1.0, 1.0));
        let layout = ts.layout_cached(TextLayoutParams {
            text: "cccccccccccccccccccccccccccccccccccccccccccccccc",
            style,
            max_width: Some(80.0),
            wrap_mode: WrapMode::Word,
        });
        assert_eq!(layout.lines.len(), 1);
    }

    #[test]
    fn word_or_anywhere_wraps_long_unspaced_text() {
        let mut ts = TextSystem::new();
        let style = TextStyle::new(FontId::Mono, 13, Color::new(1.0, 1.0, 1.0, 1.0));
        let layout = ts.layout_cached(TextLayoutParams {
            text: "cccccccccccccccccccccccccccccccccccccccccccccccc",
            style,
            max_width: Some(80.0),
            wrap_mode: WrapMode::WordOrAnywhere,
        });
        assert!(layout.lines.len() > 1);
        assert!(layout.width() <= 82.0);
    }

    #[test]
    fn caret_rect_reports_visual_line_y() {
        let mut ts = TextSystem::new();
        let style = TextStyle::new(FontId::Mono, 13, Color::new(1.0, 1.0, 1.0, 1.0));
        let text = "aaaaaaaaaaaa bbbbbbbbbbbb";
        let layout = ts.layout_cached(TextLayoutParams {
            text,
            style,
            max_width: Some(90.0),
            wrap_mode: WrapMode::WordOrAnywhere,
        });
        assert!(layout.lines.len() > 1);
        let start = layout.caret_rect_at(0, 1.0);
        let end = layout.caret_rect_at(text.len(), 1.0);
        assert!(end.y > start.y);
    }

    #[test]
    fn layout_cached_repopulates_face_blobs() {
        let mut ts = TextSystem::new();
        let style = TextStyle::new(FontId::Ui, 14, Color::new(1.0, 1.0, 1.0, 1.0));
        let _ = ts.layout_cached(TextLayoutParams {
            text: "blob registration",
            style,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        });
        assert!(!ts.face_blobs().is_empty());
    }
}
