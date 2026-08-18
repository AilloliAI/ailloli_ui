use std::sync::Arc;

use ailloli_ui_core::{style::TextStyle, Rect};
use parley::{Affinity, Cursor};

use crate::engine_parley::ParleyEngine;

pub use crate::glyph::GlyphInstance;
pub use crate::params::{LaidOutLine, TextLayoutParams, TextMetrics, WrapMode};

/// Backend-agnostic layout result (Parley types not exposed).
#[allow(dead_code)]
#[derive(Clone)]
pub struct LaidOutText {
    text: String,
    style: TextStyle,
    layout: Arc<parley::Layout<()>>,
    pub lines: Vec<LaidOutLine>,
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
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn style(&self) -> TextStyle {
        self.style
    }

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

    pub fn width(&self) -> f32 {
        self.metrics.width
    }

    pub fn height(&self) -> f32 {
        self.metrics.height
    }

    #[allow(dead_code)]
    pub(crate) fn parley_layout(&self) -> &parley::Layout<()> {
        self.layout.as_ref()
    }
}

/// Maps a UTF-8 byte index to X in layout space (cluster-aware).
pub fn caret_x_at(laid: &LaidOutText, byte_idx: usize) -> f32 {
    caret_rect_at(laid, byte_idx, 0.0).x
}

/// Maps a UTF-8 byte index to a caret rectangle in layout space.
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
pub fn caret_index_at_point(laid: &LaidOutText, x: f32, y: f32) -> usize {
    let layout = laid.parley_layout();
    Cursor::from_point(layout, x, y).index()
}

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
