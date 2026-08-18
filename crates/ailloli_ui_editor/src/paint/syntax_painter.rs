use ailloli_ui_core::TextStyle;
use ailloli_ui_text::{StyledTextLayoutParams, StyledTextSpan, TextSystem, WrapMode};

use crate::code::{CodeTheme, SyntaxKind, SyntaxToken};
use crate::layout::EditorTextRun;
use crate::paint::EditorPaintItem;
use crate::EditorStyle;

pub fn syntax_text_items_for_run(
    content_x: f32,
    content_y: f32,
    run: &EditorTextRun,
    tokens: &[SyntaxToken],
    style: EditorStyle,
    theme: CodeTheme,
    text_system: &mut TextSystem,
) -> Vec<EditorPaintItem> {
    let spans = styled_spans_for_run(run, tokens, style, theme);
    if spans.is_empty() {
        return vec![EditorPaintItem::Text {
            pos: [content_x, content_y + run.baseline_y],
            color: style.fg,
            layout: run.layout.clone(),
        }];
    }
    let base_style = TextStyle::new(style.font, style.px_size, style.fg);
    let layout = text_system.layout_styled_cached(StyledTextLayoutParams {
        text: run.layout.text(),
        base_style,
        spans: &spans,
        max_width: None,
        wrap_mode: WrapMode::NoWrap,
    });
    vec![EditorPaintItem::Text {
        pos: [content_x, content_y + run.baseline_y],
        color: style.fg,
        layout,
    }]
}

pub fn styled_spans_for_run(
    run: &EditorTextRun,
    tokens: &[SyntaxToken],
    style: EditorStyle,
    theme: CodeTheme,
) -> Vec<StyledTextSpan> {
    let text_len = run.layout.text().len();
    let base_color = style.fg;
    let mut spans = Vec::new();
    for token in tokens {
        let start = token.range.start.max(run.byte_range.start);
        let end = token.range.end.min(run.byte_range.end);
        if start >= end {
            continue;
        }
        let local_start = start - run.byte_range.start;
        let local_end = end - run.byte_range.start;
        if local_start >= text_len
            || local_end > text_len
            || !run.layout.text().is_char_boundary(local_start)
            || !run.layout.text().is_char_boundary(local_end)
        {
            continue;
        }
        let color = syntax_color(token.kind, theme);
        if color == base_color {
            continue;
        }
        spans.push(StyledTextSpan {
            range: local_start..local_end,
            style: TextStyle::new(style.font, style.px_size, color),
        });
    }
    spans
}

fn syntax_color(kind: SyntaxKind, theme: CodeTheme) -> ailloli_ui_core::Color {
    match kind {
        SyntaxKind::Keyword => theme.syntax_keyword,
        SyntaxKind::Type => theme.syntax_type,
        SyntaxKind::Function => theme.syntax_function,
        SyntaxKind::String => theme.syntax_string,
        SyntaxKind::Number => theme.syntax_number,
        SyntaxKind::Comment => theme.syntax_comment,
        SyntaxKind::Operator => theme.syntax_operator,
        SyntaxKind::Punctuation => theme.syntax_punctuation,
        SyntaxKind::Identifier => theme.syntax_identifier,
    }
}
