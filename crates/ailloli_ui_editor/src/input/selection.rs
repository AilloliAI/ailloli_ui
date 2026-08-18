use std::ops::Range;

use ailloli_ui_core::Rect;

use crate::code::{EditorLanguage, SyntaxKind, SyntaxToken};
use crate::layout::{first_layout_baseline, EditorTextRun};
use crate::EditorStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionGranularity {
    Character,
    Word,
    Line,
    Token,
}

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

pub fn select_token_at(byte: usize, syntax_tokens: &[SyntaxToken]) -> Option<Range<usize>> {
    syntax_tokens
        .iter()
        .filter(|token| selectable_syntax_kind(token.kind))
        .find(|token| byte_in_range(byte, &token.range))
        .map(|token| token.range.clone())
}

pub fn select_line_at(text: &str, byte: usize) -> Range<usize> {
    let byte = clamp_boundary(text, byte.min(text.len()));
    let start = text[..byte].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let end = text[byte..]
        .find('\n')
        .map(|idx| byte + idx)
        .unwrap_or(text.len());
    start..end
}

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

fn byte_in_range(byte: usize, range: &Range<usize>) -> bool {
    range.start < range.end && range.start <= byte && byte < range.end
}

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

fn is_ident_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn is_ident_byte(byte: u8) -> bool {
    is_ident_start(byte) || byte.is_ascii_digit()
}

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

fn clamp_boundary(text: &str, mut byte: usize) -> usize {
    byte = byte.min(text.len());
    while byte > 0 && !text.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

fn previous_boundary(text: &str, byte: usize) -> usize {
    let mut byte = clamp_boundary(text, byte);
    if byte == 0 {
        return 0;
    }
    byte -= 1;
    clamp_boundary(text, byte)
}
