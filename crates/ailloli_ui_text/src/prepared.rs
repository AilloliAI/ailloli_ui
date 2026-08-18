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
#[derive(Debug)]
pub struct PreparedTextLayout {
    text: Arc<str>,
    style: TextStyle,
    layout: Arc<ParleyLayoutDebug>,
    pub lines: Vec<LaidOutLine>,
    pub metrics: TextMetrics,
    glyphs: Arc<[GlyphInstance]>,
}

/// Local wrapper to implement `Debug` without exposing Parley brush types.
pub enum ParleyLayoutDebug {
    Uniform(parley::Layout<()>),
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
    pub fn text(&self) -> &str {
        self.text.as_ref()
    }

    pub fn style(&self) -> TextStyle {
        self.style
    }

    pub fn width(&self) -> f32 {
        self.metrics.width
    }

    pub fn height(&self) -> f32 {
        self.metrics.height
    }

    pub fn glyphs(&self) -> &[GlyphInstance] {
        &self.glyphs
    }

    pub fn caret_x_at(&self, byte_idx: usize) -> f32 {
        self.caret_rect_at(byte_idx, 0.0).x
    }

    pub fn caret_rect_at(&self, byte_idx: usize, width: f32) -> Rect {
        match self.layout.as_ref() {
            ParleyLayoutDebug::Uniform(layout) => caret_rect_at_layout(layout, byte_idx, width),
            ParleyLayoutDebug::Styled(layout) => caret_rect_at_layout(layout, byte_idx, width),
        }
    }

    pub fn caret_index_at_point(&self, x: f32, y: f32) -> usize {
        match self.layout.as_ref() {
            ParleyLayoutDebug::Uniform(layout) => Cursor::from_point(layout, x, y).index(),
            ParleyLayoutDebug::Styled(layout) => Cursor::from_point(layout, x, y).index(),
        }
    }
}

/// Builds a Parley layout and glyphs; registers font blobs by `face_id`.
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
