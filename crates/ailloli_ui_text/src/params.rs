use core::ops::Range;

use ailloli_ui_core::style::TextStyle;

/// Line breaking strategy for layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WrapMode {
    /// Single line; `max_width` ignored for cache keys.
    NoWrap,
    /// Break at word boundaries within `max_width`.
    Word,
    /// Break at word boundaries, and break long words only when they overflow.
    WordOrAnywhere,
}

/// Inputs for text layout (content, style, width, wrap).
#[derive(Debug, Clone, Copy)]
pub struct TextLayoutParams<'a> {
    pub text: &'a str,
    pub style: TextStyle,
    pub max_width: Option<f32>,
    pub wrap_mode: WrapMode,
}

impl<'a> TextLayoutParams<'a> {
    pub fn new(text: &'a str, style: TextStyle) -> Self {
        Self {
            text,
            style,
            max_width: None,
            wrap_mode: WrapMode::Word,
        }
    }
}

/// Style override for a UTF-8 byte range in a text layout.
#[derive(Debug, Clone, PartialEq)]
pub struct StyledTextSpan {
    pub range: Range<usize>,
    pub style: TextStyle,
}

/// Inputs for styled text layout. Spans affect cache invalidation and prepare
/// the pipeline for syntax-colored runs while preserving a single layout pass.
#[derive(Debug, Clone, Copy)]
pub struct StyledTextLayoutParams<'a> {
    pub text: &'a str,
    pub base_style: TextStyle,
    pub spans: &'a [StyledTextSpan],
    pub max_width: Option<f32>,
    pub wrap_mode: WrapMode,
}

/// Metrics for one laid-out line.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidOutLine {
    pub text_range: Range<usize>,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub baseline_y: f32,
}

/// Overall width and height of laid-out text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
}
