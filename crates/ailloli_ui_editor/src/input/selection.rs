//! Syntax-aware selection ranges and visual selection rectangles.

use std::ops::Range;

use ailloli_ui_core::Rect;

use crate::code::{EditorLanguage, SyntaxKind, SyntaxToken};
use crate::layout::{first_layout_baseline, EditorTextRun};
use crate::EditorStyle;

/// Unit chosen for a pointer-driven selection gesture.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::SelectionGranularity;
///
/// assert_ne!(SelectionGranularity::Character, SelectionGranularity::Word);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionGranularity {
    /// Extend to an exact UTF-8 caret position.
    Character,
    /// Select a syntax token or lexical word around the pointer.
    Word,
    /// Select one logical line without its newline delimiter.
    Line,
    /// Select a syntax token when one is available.
    Token,
}

/// Builds viewport-space rectangles for a run-local byte selection.
///
/// `lo_local..hi_local` is clamped to the run layout's UTF-8 byte length. One
/// rectangle is returned per intersected visual line, with rounded `x`/`y`, a
/// minimum width of one logical pixel, and at least `px_size + 2` logical pixels
/// of height. Empty or reversed selections return an empty vector.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_editor::{input::selection::selection_rects_for_run, layout::EditorTextRun, EditorStyle};
/// use ailloli_ui_text::{TextLayoutParams, TextSystem};
///
/// let mut text_system = TextSystem::new();
/// let layout = text_system.layout_cached(TextLayoutParams::new(
///     "abc",
///     TextStyle::new(FontId::Mono, 13, Color::WHITE),
/// ));
/// let run = EditorTextRun { index: 0, byte_range: 0..3, baseline_y: 10.0, layout };
/// let rects = selection_rects_for_run(4.0, 6.0, &run, 0, 2, EditorStyle::default());
/// assert_eq!(rects.len(), 1);
/// assert!(rects[0].x >= 4.0 && rects[0].w >= 1.0);
/// ```
pub fn selection_rects_for_run(
    content_x: f32,
    content_y: f32,
    run: &EditorTextRun,
    lo_local: usize,
    hi_local: usize,
    style: EditorStyle,
) -> Vec<Rect> {
    let text_len = run.layout.text().len();
    let lo_local = lo_local.min(text_len);
    let hi_local = hi_local.min(text_len);
    if hi_local <= lo_local {
        return Vec::new();
    }

    let first_baseline = first_layout_baseline(&run.layout);
    let text_origin_y = run.baseline_y - first_baseline;
    let mut out = Vec::new();
    for line in &run.layout.lines {
        let line_start = line.text_range.start.min(text_len);
        let line_end = line.text_range.end.min(text_len);
        let start = lo_local.max(line_start);
        let end = hi_local.min(line_end);
        if end <= start {
            continue;
        }

        let x0 = if start <= line_start {
            0.0
        } else {
            run.layout.caret_rect_at(start, 0.0).x
        };
        let x1 = if end >= line_end {
            line.width
        } else {
            run.layout.caret_rect_at(end, 0.0).x
        };
        let x_left = x0.min(x1);
        let width = (x1 - x0).abs().max(1.0);
        let caret = run.layout.caret_rect_at(line_start, 0.0);
        let fallback_h = style.px_size as f32 + 2.0;
        out.push(Rect::new(
            (content_x + x_left).round(),
            (content_y + text_origin_y + caret.y).round(),
            width,
            caret.h.max(fallback_h),
        ));
    }
    out
}

/// Selects a syntax token or ASCII lexical word around a UTF-8 byte offset.
///
/// A selectable token wins when supplied. The fallback recognizes ASCII Rust
/// raw identifiers and lifetimes, ASCII identifiers in every language, and a
/// fixed set of one-byte punctuation. At or beyond EOF, it examines the final
/// character; empty text and unsupported characters return `None`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{input::selection::select_word_at, EditorLanguage};
///
/// assert_eq!(select_word_at("let r#type = 1;", 7, None, EditorLanguage::Rust), Some(4..10));
/// assert_eq!(select_word_at("abc", usize::MAX, None, EditorLanguage::PlainText), Some(0..3));
/// assert_eq!(select_word_at(" ", 0, None, EditorLanguage::PlainText), None);
/// ```
pub fn select_word_at(
    text: &str,
    byte: usize,
    syntax_tokens: Option<&[SyntaxToken]>,
    language: EditorLanguage,
) -> Option<Range<usize>> {
    if let Some(range) = syntax_tokens.and_then(|tokens| select_token_at(byte, tokens)) {
        return Some(range);
    }
    lexical_word_range_at(text, byte, language)
}

/// Returns the first selectable syntax-token range containing `byte`.
///
/// Containment is half-open. Keyword, type, function, number, operator,
/// punctuation, and identifier tokens are selectable; string and comment
/// tokens are skipped. Token ranges are returned as supplied without clamping
/// or overlap resolution.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{input::selection::select_token_at, code::{SyntaxKind, SyntaxToken}};
///
/// let tokens = [
///     SyntaxToken { range: 0..3, kind: SyntaxKind::Comment },
///     SyntaxToken { range: 4..7, kind: SyntaxKind::Identifier },
/// ];
/// assert_eq!(select_token_at(5, &tokens), Some(4..7));
/// assert_eq!(select_token_at(3, &tokens), None);
/// ```
pub fn select_token_at(byte: usize, syntax_tokens: &[SyntaxToken]) -> Option<Range<usize>> {
    syntax_tokens
        .iter()
        .filter(|token| selectable_syntax_kind(token.kind))
        .find(|token| byte_in_range(byte, &token.range))
        .map(|token| token.range.clone())
}

/// Returns the logical line containing a UTF-8 byte offset, excluding `\n`.
///
/// Out-of-range offsets clamp to EOF and offsets inside a multi-byte character
/// clamp backward to its start. For a trailing newline, EOF selects the empty
/// final line.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::input::selection::select_line_at;
///
/// assert_eq!(select_line_at("first\nsecond\n", 8), 6..12);
/// assert_eq!(select_line_at("first\nsecond\n", usize::MAX), 13..13);
/// ```
pub fn select_line_at(text: &str, byte: usize) -> Range<usize> {
    let byte = clamp_boundary(text, byte.min(text.len()));
    let start = text[..byte].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let end = text[byte..]
        .find('\n')
        .map(|idx| byte + idx)
        .unwrap_or(text.len());
    start..end
}

/// Reports whether syntax selection accepts a token category.
fn selectable_syntax_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Keyword
            | SyntaxKind::Type
            | SyntaxKind::Function
            | SyntaxKind::Number
            | SyntaxKind::Operator
            | SyntaxKind::Punctuation
            | SyntaxKind::Identifier
    )
}

/// Tests non-empty half-open byte-range containment.
fn byte_in_range(byte: usize, range: &Range<usize>) -> bool {
    range.start < range.end && range.start <= byte && byte < range.end
}

/// Finds the language-aware ASCII lexical unit around a byte position.
fn lexical_word_range_at(
    text: &str,
    byte: usize,
    language: EditorLanguage,
) -> Option<Range<usize>> {
    if text.is_empty() {
        return None;
    }
    let mut at = clamp_boundary(text, byte.min(text.len()));
    if at == text.len() {
        at = previous_boundary(text, at);
    }
    if at >= text.len() {
        return None;
    }
    let bytes = text.as_bytes();
    if matches!(language, EditorLanguage::Rust) {
        if let Some(range) = rust_lifetime_range_at(text, at) {
            return Some(range);
        }
        if bytes.get(at) == Some(&b'#') && at > 0 && bytes.get(at - 1) == Some(&b'r') {
            at -= 1;
        }
        if let Some(range) = rust_raw_identifier_range_at(text, at) {
            return Some(range);
        }
    }
    if is_ident_byte(bytes[at]) {
        return Some(identifier_range_at(text, at));
    }
    if is_selectable_punctuation(bytes[at]) {
        return Some(at..at + 1);
    }
    None
}

/// Finds a Rust `r#identifier` containing a byte position.
fn rust_raw_identifier_range_at(text: &str, at: usize) -> Option<Range<usize>> {
    let bytes = text.as_bytes();
    let mut start = at;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    if start >= 2 && &bytes[start - 2..start] == b"r#" {
        start -= 2;
    }
    if &bytes.get(start..start + 2)? != b"r#" {
        return None;
    }
    let mut end = start + 2;
    if !bytes.get(end).is_some_and(|byte| is_ident_start(*byte)) {
        return None;
    }
    end += 1;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }
    (start <= at && at < end).then_some(start..end)
}

/// Finds a Rust lifetime containing a byte position, excluding char literals.
fn rust_lifetime_range_at(text: &str, at: usize) -> Option<Range<usize>> {
    let bytes = text.as_bytes();
    let start = if bytes.get(at) == Some(&b'\'') {
        at
    } else if at > 0 && is_ident_byte(bytes[at]) && bytes.get(at - 1) == Some(&b'\'') {
        at - 1
    } else {
        return None;
    };
    let first = *bytes.get(start + 1)?;
    if !is_ident_start(first) {
        return None;
    }
    let mut end = start + 2;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }
    if bytes.get(end) == Some(&b'\'') {
        return None;
    }
    Some(start..end)
}

/// Expands an ASCII identifier byte to its complete range.
fn identifier_range_at(text: &str, at: usize) -> Range<usize> {
    let bytes = text.as_bytes();
    let mut start = at;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = at + 1;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }
    start..end
}

/// Reports whether a byte starts an ASCII identifier.
fn is_ident_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

/// Reports whether a byte continues an ASCII identifier.
fn is_ident_byte(byte: u8) -> bool {
    is_ident_start(byte) || byte.is_ascii_digit()
}

/// Reports whether a byte is one of the selectable punctuation characters.
fn is_selectable_punctuation(byte: u8) -> bool {
    matches!(
        byte,
        b'!' | b'%'
            | b'&'
            | b'*'
            | b'+'
            | b'-'
            | b'.'
            | b'/'
            | b':'
            | b';'
            | b'<'
            | b'='
            | b'>'
            | b'?'
            | b'|'
            | b'('
            | b')'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b','
            | b'#'
    )
}

/// Clamps an offset to EOF and then backward to a UTF-8 boundary.
fn clamp_boundary(text: &str, mut byte: usize) -> usize {
    byte = byte.min(text.len());
    while byte > 0 && !text.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

/// Returns the UTF-8 boundary immediately before an offset, or zero.
fn previous_boundary(text: &str, byte: usize) -> usize {
    let mut byte = clamp_boundary(text, byte);
    if byte == 0 {
        return 0;
    }
    byte -= 1;
    clamp_boundary(text, byte)
}
