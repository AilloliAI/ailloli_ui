//! Minimal multi-span text model rendered with the first span's style.

use ailloli_ui_core::TextStyle;
use ailloli_ui_runtime::{DrawCmd, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

#[derive(Debug, Clone)]
/// Owned text fragment paired with its intended style.
///
/// In the current MVP renderer, only the first span's style is applied to the
/// concatenated string.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_widgets::text::TextSpan;
/// let span = TextSpan { text: "hello".into(), style: TextStyle::new(FontId::Ui, 14, Color::WHITE) };
/// assert_eq!(span.text, "hello");
/// ```
pub struct TextSpan {
    /// Owned UTF-8 fragment; empty spans are allowed.
    pub text: String,
    /// Requested span style; ignored for non-first spans by the MVP renderer.
    pub style: TextStyle,
}

#[derive(Debug, Clone)]
/// Ordered spans plus wrap and optional logical-pixel width constraints.
///
/// `None` means unbounded width. An empty span list draws nothing.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_widgets::text::{RichText, WrapMode};
/// let rich = RichText::plain("hello", TextStyle::new(FontId::Ui, 14, Color::WHITE));
/// assert_eq!(rich.spans.len(), 1);
/// assert_eq!(rich.wrap, WrapMode::NoWrap);
/// assert_eq!(rich.max_width, None);
/// ```
pub struct RichText {
    /// Ordered owned fragments; the first also supplies the rendered style.
    pub spans: Vec<TextSpan>,
    /// Line-breaking policy applied to the concatenated text.
    pub wrap: WrapMode,
    /// Optional maximum line width in logical pixels, passed through unchanged.
    pub max_width: Option<f32>,
}

impl RichText {
    /// Creates one unwrapped span with no maximum width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_widgets::text::RichText;
    /// let rich = RichText::plain("hello", TextStyle::new(FontId::Ui, 14, Color::WHITE));
    /// assert_eq!(rich.spans[0].text, "hello");
    /// ```
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
/// Returns `None` for an empty span list. `baseline_xy` is in logical pixels and
/// its y coordinate is the text baseline. The merged string allocates once.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_text::TextSystem;
/// use ailloli_ui_widgets::text::{draw_rich_text, RichText};
/// let mut text_system = TextSystem::new();
/// let rich = RichText::plain("hello", TextStyle::new(FontId::Ui, 14, Color::WHITE));
/// assert!(draw_rich_text([2.0, 18.0], &rich, &mut text_system).is_some());
/// ```
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
