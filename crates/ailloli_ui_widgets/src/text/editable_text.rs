use ailloli_ui_core::event::ImePreedit;
use ailloli_ui_core::{Color, Rect, TextStyle};
use ailloli_ui_runtime::input::Selection;
use ailloli_ui_runtime::{DrawCmd, DrawRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

#[derive(Debug, Clone, Copy)]
pub struct EditableTextStyle {
    pub text: TextStyle,
    pub caret: Color,
    pub caret_w: f32,
    pub caret_blink_ms: i64,
    pub selection_bg: Option<Color>,
}

impl Default for EditableTextStyle {
    fn default() -> Self {
        Self {
            text: TextStyle::new(
                ailloli_ui_core::FontId::Mono,
                15,
                Color::rgba(243, 246, 251, 1.0),
            ),
            caret: Color::rgba(243, 246, 251, 1.0),
            caret_w: 1.0,
            caret_blink_ms: 500,
            selection_bg: Some(Color::rgba(37, 99, 235, 0.35)),
        }
    }
}

fn layout_string_for_edit(
    text: &str,
    caret_byte: usize,
    preedit: Option<&ImePreedit>,
) -> (String, usize) {
    match preedit {
        None => (text.to_string(), caret_byte.min(text.len())),
        Some(p) => {
            let mut s = text.to_string();
            let at = caret_byte.min(s.len());
            if !p.text.is_empty() {
                s.insert_str(at, &p.text);
            }
            let caret_in_display = if let Some((a, b)) = p.selection {
                at + a.max(b).min(p.text.len())
            } else {
                at + p.text.len()
            };
            let len = s.len();
            (s, caret_in_display.min(len))
        }
    }
}

/// Single-line editable text; baseline Y = `baseline_y` (see `DrawText.pos[1]`).
///
/// Paint order: selection background (optional) → glyphs → caret.
#[allow(clippy::too_many_arguments)]
pub fn draw_editable_mono_line(
    baseline_x: f32,
    baseline_y: f32,
    text: &str,
    caret_byte: usize,
    selection: Option<Selection>,
    preedit: Option<&ImePreedit>,
    focused: bool,
    now_ms: i64,
    style: EditableTextStyle,
    text_system: &mut TextSystem,
) -> Vec<DrawCmd> {
    let mut out = Vec::new();
    let (display, caret_in_display) = layout_string_for_edit(text, caret_byte, preedit);

    let laid = text_system.layout_cached(TextLayoutParams {
        text: display.as_str(),
        style: style.text,
        max_width: None,
        wrap_mode: WrapMode::NoWrap,
    });

    let px = style.text.px_size as f32;
    let y_top = (baseline_y - px).round();

    let caret_x = (baseline_x + laid.caret_x_at(caret_in_display)).round();

    if preedit.is_none() {
        if let Some(sel) = selection {
            if !sel.is_collapsed() {
                if let Some(bg) = style.selection_bg {
                    let (lo, hi) = sel.normalized();
                    let lo = lo.min(display.len());
                    let hi = hi.min(display.len());
                    let x0 = (baseline_x + laid.caret_x_at(lo)).round();
                    let x1 = (baseline_x + laid.caret_x_at(hi)).round();
                    let w = (x1 - x0).max(1.0);
                    out.push(DrawCmd::Rect(DrawRect {
                        rect: Rect::new(x0, y_top, w, px + 2.0),
                        color: bg,
                    }));
                }
            }
        }
    }

    out.push(DrawCmd::Text(DrawText {
        pos: [baseline_x, baseline_y],
        color: style.text.color,
        layout: laid,
    }));

    if focused && style.caret_blink_ms > 0 {
        let on = ((now_ms / style.caret_blink_ms) % 2) == 0;
        if on {
            out.push(DrawCmd::Rect(DrawRect {
                rect: Rect::new(caret_x, y_top, style.caret_w, px + 2.0),
                color: style.caret,
            }));
        }
    }

    out
}
