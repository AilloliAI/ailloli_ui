//! Simple per-character layout helpers independent of Parley shaping.

use ailloli_ui_core::FontId;

use crate::text_measure::TextMeasure;

/// Per-line byte offsets and cumulative advances (simple fontdue metrics).
///
/// Entries represent Unicode scalar boundaries rather than grapheme or shaped
/// cluster boundaries. Consumers that need ligature-, bidi-, or grapheme-aware
/// carets should use [`crate::LaidOutText`] and [`crate::caret_rect_at`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::LineLayout;
/// let layout = LineLayout { byte_offsets: vec![0, 1], advances: vec![0.0, 7.0] };
/// assert_eq!(layout.width(), 7.0);
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineLayout {
    /// Cumulative advances in pixels after each UTF-8 boundary.
    ///
    /// Invariant: `byte_offsets.len() == advances.len()` and `byte_offsets[0] == 0`.
    pub byte_offsets: Vec<usize>,
    /// Cumulative logical-pixel advances corresponding one-for-one to `byte_offsets`.
    pub advances: Vec<f32>,
}

impl LineLayout {
    /// Returns the final cumulative advance in logical pixels, or zero if empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::LineLayout;
    /// assert_eq!(LineLayout::default().width(), 0.0);
    /// assert_eq!(LineLayout { byte_offsets: vec![0, 1], advances: vec![0.0, 8.5] }.width(), 8.5);
    /// ```
    pub fn width(&self) -> f32 {
        self.advances.last().copied().unwrap_or(0.0)
    }
}

/// Measures each Unicode scalar and records its next UTF-8 boundary.
///
/// `px` is a logical-pixel font size forwarded unchanged to `m`. The generated
/// vectors always start with zero and have `text.chars().count() + 1` entries.
/// No kerning or shaping across adjacent characters is performed.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::FontId;
/// use ailloli_ui_text::{line_layout, ApproxTextMeasure};
/// let layout = line_layout("éa", FontId::Ui, 10, &ApproxTextMeasure);
/// assert_eq!(layout.byte_offsets, [0, 2, 3]);
/// assert_eq!(layout.advances.len(), 3);
/// ```
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

/// Maps a UTF-8 byte position to the preceding recorded X advance.
///
/// Positions inside a multi-byte scalar snap backward to that scalar's leading
/// boundary. Values beyond the line snap to its width. Malformed layouts are
/// tolerated by returning zero if the matching advance is absent.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::{text_layout::caret_x_at, LineLayout};
/// let layout = LineLayout { byte_offsets: vec![0, 2, 3], advances: vec![0.0, 6.0, 12.0] };
/// assert_eq!(caret_x_at(&layout, 1), 0.0);
/// assert_eq!(caret_x_at(&layout, usize::MAX), 12.0);
/// ```
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

/// Returns the first cumulative-advance index at or beyond `x`.
///
/// The result indexes `layout.advances` and `layout.byte_offsets`; callers can
/// obtain the UTF-8 offset with `layout.byte_offsets[result]`. Negative and NaN
/// X values behave as zero. Values beyond the width return the last advance
/// index. An empty layout returns zero.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::{caret_index_at_x, LineLayout};
/// let layout = LineLayout { byte_offsets: vec![0, 2, 3], advances: vec![0.0, 6.0, 12.0] };
/// let position = caret_index_at_x(&layout, 7.0);
/// assert_eq!(position, 2);
/// assert_eq!(layout.byte_offsets[position], 3);
/// ```
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
