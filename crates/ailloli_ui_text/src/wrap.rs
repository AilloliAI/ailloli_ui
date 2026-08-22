//! Lightweight whitespace-normalizing word wrapping.

use ailloli_ui_core::FontId;

use crate::text_measure::TextMeasure;

/// Word-wraps text without hyphenation.
///
/// `max_w_px` is a non-negative logical-pixel target; negative and NaN values
/// behave as zero. Runs of Unicode whitespace, including explicit newlines,
/// are normalized to one ASCII space within an output line. A word wider than
/// the target is emitted intact, so this is not a hard width bound. The result
/// always contains at least one line, with an empty string for whitespace-only
/// input.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::FontId;
/// use ailloli_ui_text::{wrap_lines, ApproxTextMeasure};
///
/// let lines = wrap_lines("one   two three", 41.0, FontId::Ui, 10, &ApproxTextMeasure);
/// assert_eq!(lines, ["one two", "three"]);
/// assert_eq!(wrap_lines(" \n ", 20.0, FontId::Ui, 10, &ApproxTextMeasure), [""]);
/// ```
pub fn wrap_lines(
    text: &str,
    max_w_px: f32,
    font: FontId,
    px_size: u16,
    m: &dyn TextMeasure,
) -> Vec<String> {
    let max_w_px = max_w_px.max(0.0);

    let mut out = Vec::<String>::new();
    let mut cur = String::new();
    let mut cur_w = 0.0f32;

    for word in text.split_whitespace() {
        let word_w = m.measure(word, font, px_size);
        let space_w = if cur.is_empty() {
            0.0
        } else {
            m.advance(' ', font, px_size)
        };

        let next_w = cur_w + space_w + word_w;

        if !cur.is_empty() && next_w > max_w_px {
            out.push(std::mem::take(&mut cur));
            cur_w = 0.0;
        }

        if !cur.is_empty() {
            cur.push(' ');
            cur_w += space_w;
        }
        cur.push_str(word);
        cur_w += word_w;
    }

    if !cur.is_empty() {
        out.push(cur);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}
