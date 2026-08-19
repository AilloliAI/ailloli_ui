use ailloli_ui_core::TextStyle;
use ailloli_ui_runtime::{DrawCmd, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

#[derive(Debug, Clone)]
pub struct TextSpan {
    pub text: String,
    pub style: TextStyle,
}

#[derive(Debug, Clone)]
pub struct RichText {
    pub spans: Vec<TextSpan>,
    pub wrap: WrapMode,
    pub max_width: Option<f32>,
}

impl RichText {
    pub fn plain(text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            spans: vec![TextSpan {
                text: text.into(),
                style,
            }],
            wrap: WrapMode::NoWrap,
            max_width: None,
        }
    }
}

/// MVP multi-span: concatenates spans into one string using the **first** span's style.
///
/// Full Parley multi-style layout in a single run is planned for a later phase.
pub fn draw_rich_text(
    baseline_xy: [f32; 2],
    rich: &RichText,
    text_system: &mut TextSystem,
) -> Option<DrawCmd> {
    let first = rich.spans.first()?;
    let merged: String = rich.spans.iter().map(|s| s.text.as_str()).collect();
    let style = first.style;
    let color = style.color;
    let prepared = text_system.layout_cached(TextLayoutParams {
        text: merged.as_str(),
        style,
        max_width: rich.max_width,
        wrap_mode: rich.wrap,
    });
    Some(DrawCmd::Text(DrawText {
        pos: baseline_xy,
        color,
        decoration: ailloli_ui_core::TextDecoration::None,
        layout: prepared,
    }))
}
