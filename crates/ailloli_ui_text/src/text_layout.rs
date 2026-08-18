use ailloli_ui_core::FontId;

use crate::text_measure::TextMeasure;

/// Per-line byte offsets and cumulative advances (simple fontdue metrics).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineLayout {
    /// Cumulative advances in pixels after each UTF-8 boundary.
    ///
    /// Invariant: `byte_offsets.len() == advances.len()` and `byte_offsets[0] == 0`.
    pub byte_offsets: Vec<usize>,
    pub advances: Vec<f32>,
}

impl LineLayout {
    pub fn width(&self) -> f32 {
        self.advances.last().copied().unwrap_or(0.0)
    }
}

pub fn line_layout(text: &str, font: FontId, px: u16, m: &dyn TextMeasure) -> LineLayout {
    let mut byte_offsets = Vec::with_capacity(text.chars().count() + 1);
    let mut advances = Vec::with_capacity(text.chars().count() + 1);

    byte_offsets.push(0);
    advances.push(0.0);

    let mut acc = 0.0f32;
    for (byte_idx, ch) in text.char_indices() {
        let next = byte_idx + ch.len_utf8();
        acc += m.advance(ch, font, px);
        byte_offsets.push(next);
        advances.push(acc);
    }

    LineLayout {
        byte_offsets,
        advances,
    }
}

pub fn caret_x_at(layout: &LineLayout, byte_idx: usize) -> f32 {
    if layout.byte_offsets.is_empty() {
        return 0.0;
    }

    let mut best_i = 0usize;
    for (i, &off) in layout.byte_offsets.iter().enumerate() {
        if off <= byte_idx {
            best_i = i;
        } else {
            break;
        }
    }
    layout.advances.get(best_i).copied().unwrap_or(0.0)
}

pub fn caret_index_at_x(layout: &LineLayout, x: f32) -> usize {
    let x = x.max(0.0);
    if layout.advances.is_empty() {
        return 0;
    }

    for (i, &ax) in layout.advances.iter().enumerate() {
        if ax >= x {
            return i;
        }
    }
    layout.advances.len().saturating_sub(1)
}
