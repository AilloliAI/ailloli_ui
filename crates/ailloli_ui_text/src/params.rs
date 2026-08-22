//! Value types that describe layout requests and results.

use core::ops::Range;

use ailloli_ui_core::style::TextStyle;

/// Line breaking strategy for layout.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::WrapMode;
/// assert_ne!(WrapMode::NoWrap, WrapMode::WordOrAnywhere);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WrapMode {
    /// Do not wrap; [`TextLayoutParams::max_width`] is ignored.
    NoWrap,
    /// Prefer word boundaries when wrapping to the requested maximum width.
    Word,
    /// Prefer words, but split an otherwise overflowing word at shaped clusters.
    WordOrAnywhere,
}

/// Inputs for text layout (content, style, width, wrap).
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::{TextLayoutParams, WrapMode};
///
/// let params = TextLayoutParams::new("hello", TextStyle::new(FontId::Ui, 16, Color::WHITE));
/// assert_eq!(params.max_width, None);
/// assert_eq!(params.wrap_mode, WrapMode::Word);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct TextLayoutParams<'a> {
    /// UTF-8 text to shape; an empty string is valid.
    pub text: &'a str,
    /// Base font, logical-pixel size, color, and paint decoration.
    pub style: TextStyle,
    /// Optional wrap width in logical pixels; `None` means unconstrained.
    ///
    /// This value is ignored when [`Self::wrap_mode`] is [`WrapMode::NoWrap`].
    pub max_width: Option<f32>,
    /// Line-breaking policy.
    pub wrap_mode: WrapMode,
}

impl<'a> TextLayoutParams<'a> {
    /// Creates an unconstrained request using [`WrapMode::Word`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{TextLayoutParams, WrapMode};
    ///
    /// let params = TextLayoutParams::new("Ailloli", TextStyle::new(FontId::Mono, 13, Color::BLACK));
    /// assert_eq!(params.text, "Ailloli");
    /// assert_eq!(params.wrap_mode, WrapMode::Word);
    /// ```
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
///
/// The range must be ordered, lie within the associated text, and use UTF-8
/// boundaries when submitted to the layout engine. This value type does not
/// validate those invariants itself.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::StyledTextSpan;
///
/// let span = StyledTextSpan {
///     range: 0..4,
///     style: TextStyle::new(FontId::Mono, 14, Color::WHITE),
/// };
/// assert_eq!(span.range, 0..4);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct StyledTextSpan {
    /// Half-open UTF-8 byte range receiving the override.
    pub range: Range<usize>,
    /// Complete replacement style for the range.
    pub style: TextStyle,
}

/// Inputs for styled text layout. Spans affect cache invalidation and prepare
/// the pipeline for syntax-colored runs while preserving a single layout pass.
///
/// Overlapping spans are forwarded in slice order to Parley. Every span range
/// must be a valid UTF-8 range in `text`; callers should not rely on this data
/// structure to clip or repair invalid ranges.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::{StyledTextLayoutParams, StyledTextSpan, WrapMode};
///
/// let base = TextStyle::new(FontId::Mono, 14, Color::WHITE);
/// let spans = [StyledTextSpan { range: 0..2, style: base.underline() }];
/// let params = StyledTextLayoutParams {
///     text: "fn main",
///     base_style: base,
///     spans: &spans,
///     max_width: Some(120.0),
///     wrap_mode: WrapMode::Word,
/// };
/// assert_eq!(params.spans.len(), 1);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct StyledTextLayoutParams<'a> {
    /// UTF-8 text to shape; an empty string is valid.
    pub text: &'a str,
    /// Style used outside overridden ranges.
    pub base_style: TextStyle,
    /// Ordered range overrides, borrowed for the duration of the request.
    pub spans: &'a [StyledTextSpan],
    /// Optional wrap width in logical pixels; `None` means unconstrained.
    pub max_width: Option<f32>,
    /// Line-breaking policy.
    pub wrap_mode: WrapMode,
}

/// Metrics for one laid-out line.
///
/// Distances are logical pixels. `text_range` is a half-open UTF-8 byte range
/// in the original string. `descent` follows Parley's metric convention and is
/// normally non-negative.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::LaidOutLine;
/// let line = LaidOutLine {
///     text_range: 0..5,
///     width: 40.0,
///     ascent: 12.0,
///     descent: 4.0,
///     baseline_y: 12.0,
/// };
/// assert_eq!(line.text_range, 0..5);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct LaidOutLine {
    /// Half-open UTF-8 byte range covered by the line.
    pub text_range: Range<usize>,
    /// Advance excluding trailing whitespace, in logical pixels.
    pub width: f32,
    /// Distance above the baseline, in logical pixels.
    pub ascent: f32,
    /// Distance below the baseline, in logical pixels.
    pub descent: f32,
    /// Baseline Y coordinate relative to the layout origin, in logical pixels.
    pub baseline_y: f32,
}

/// Overall width and height of laid-out text.
///
/// Both dimensions are logical pixels and come directly from Parley.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::TextMetrics;
/// let metrics = TextMetrics { width: 80.0, height: 24.0 };
/// assert_eq!(metrics.width, 80.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMetrics {
    /// Layout width in logical pixels, including Parley's layout extent.
    pub width: f32,
    /// Layout height in logical pixels.
    pub height: f32,
}
