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

fn hash_style(style: TextStyle) -> u64 {
    let mut h = DefaultHasher::new();
    style.font.hash(&mut h);
    style.px_size.hash(&mut h);
    h.finish()
}

fn hash_style_with_color(style: TextStyle) -> u64 {
    let mut h = DefaultHasher::new();
    style.font.hash(&mut h);
    style.px_size.hash(&mut h);
    style.color.as_rgba8().hash(&mut h);
    h.finish()
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextLayoutKey {
    text_hash: u64,
    text_len: usize,
    style_hash: u64,
    span_hash: u64,
    /// `max_width` quantized to thousandths of a pixel (`None` => `u32::MAX`).
    max_width_q: u32,
    wrap: u8,
}

impl TextLayoutKey {
    /// Builds a cache key from layout parameters.
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
pub type TextLayoutHandle = Arc<PreparedTextLayout>;

/// Retained text system: one instance per window (or shared app-wide).
pub struct TextSystem {
    engine: ParleyEngine,
    face_blobs: HashMap<u64, Arc<[u8]>>,
    cache: LruCache<TextLayoutKey, TextLayoutHandle>,
}

impl Default for TextSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl TextSystem {
    /// Creates a new system with an empty LRU cache (capacity 2048 entries).
    pub fn new() -> Self {
        let cap = NonZeroUsize::new(2048).expect("2048 > 0");
        Self {
            #[allow(deprecated)]
            engine: ParleyEngine::new(),
            face_blobs: HashMap::new(),
            cache: LruCache::new(cap),
        }
    }

    /// Lays out text with LRU caching; skips relayout when the key and text match.
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
    pub fn parley_engine_mut(&mut self) -> &mut ParleyEngine {
        &mut self.engine
    }

    /// Font bytes for a Parley `face_id`, if registered.
    pub fn face_blob(&self, face_id: u64) -> Option<&[u8]> {
        self.face_blobs.get(&face_id).map(|b| b.as_ref())
    }

    /// All registered face blobs (for renderer lookup).
    pub fn face_blobs(&self) -> &HashMap<u64, Arc<[u8]>> {
        &self.face_blobs
    }

    /// Cheap snapshot for the GPU renderer (`Arc` values inside).
    pub fn face_blobs_snapshot(&self) -> Arc<HashMap<u64, Arc<[u8]>>> {
        Arc::new(self.face_blobs.clone())
    }

    pub fn cached_layout_count(&self) -> usize {
        self.cache.len()
    }
}

pub(crate) fn normalize_styled_spans(
    text: &str,
    base_style: TextStyle,
    spans: &[StyledTextSpan],
) -> Vec<StyledTextSpan> {
    #[derive(Clone, Copy)]
    struct Candidate {
        start: usize,
        end: usize,
        index: usize,
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
