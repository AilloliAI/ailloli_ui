//! Stateful Parley adapter used by higher-level text systems.

use ailloli_ui_core::{style::TextStyle as UiTextStyle, FontId};

use parley::{
    Alignment, AlignmentOptions, FontContext, Layout, LayoutContext, OverflowWrap, StyleProperty,
    TextStyle,
};

use crate::WrapMode;

/// Bundled monospace font registered by [`ParleyEngine::new`].
const JBM_NERD_REGULAR: &[u8] = include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf");

/// Thin wrapper around Parley:
/// - holds shared `FontContext` / `LayoutContext`
/// - builds `Layout<()>` with fractional positioning (`quantize = false`)
///
/// Reusing one engine preserves Parley's font and layout caches. The type is
/// mutable and not intended for concurrent use without external synchronization.
/// [`Default`] creates empty Parley contexts; the deprecated [`Self::new`]
/// additionally registers the bundled monospace font and probes `assets/fonts`.
/// Application code should normally construct [`crate::TextSystem`] instead.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::ParleyEngine;
/// let engine = ParleyEngine::default();
/// let _font_context = &engine.font_cx;
/// let _plain_layout_context = &engine.layout_cx;
/// ```
#[derive(Default)]
pub struct ParleyEngine {
    /// Parley font collection and font-selection state.
    pub font_cx: FontContext,
    /// Reusable context for layouts whose glyph runs have no color brush.
    pub layout_cx: LayoutContext<()>,
    /// Reusable context for layouts whose brushes are linear-RGBA arrays.
    pub styled_layout_cx: LayoutContext<[f32; 4]>,
}

impl ParleyEngine {
    /// Creates an engine with the bundled mono font and optional asset fonts.
    ///
    /// The method also scans the relative `assets/fonts` path. Failures there
    /// are handled by Fontique and do not make construction fallible.
    /// Prefer [`crate::TextSystem::new`] in widgets and applications because it
    /// owns the font blobs needed by renderers.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::ParleyEngine;
    /// #[allow(deprecated)]
    /// let mut engine = ParleyEngine::new();
    /// assert!(engine.font_cx.collection.family_names().next().is_some());
    /// ```
    #[deprecated(note = "prefer TextSystem::new() for app and widget code")]
    pub fn new() -> Self {
        let mut this = Self::default();

        // Register bundled monospace from assets.
        this.font_cx
            .collection
            .register_fonts(fontique::Blob::from(JBM_NERD_REGULAR.to_vec()), None);

        // Optionally load additional fonts from `assets/fonts` when present.
        this.font_cx
            .collection
            .load_fonts_from_paths(["assets/fonts"]);

        this
    }

    /// Maps Ailloli's two font slots to Parley generic families.
    fn parley_font_family(font: FontId) -> parley::FontFamily<'static> {
        match font {
            FontId::Ui => parley::GenericFamily::SansSerif.into(),
            FontId::Mono => parley::GenericFamily::Monospace.into(),
        }
    }

    /// Converts family and size into an uncolored Parley style.
    fn parley_text_style(style: UiTextStyle) -> TextStyle<'static, 'static, ()> {
        TextStyle {
            font_family: Self::parley_font_family(style.font),
            font_size: style.px_size as f32,
            brush: (),
            ..TextStyle::default()
        }
    }

    /// Converts family, size, and color into a styled-layout Parley style.
    fn parley_text_style_color(style: UiTextStyle) -> TextStyle<'static, 'static, [f32; 4]> {
        TextStyle {
            font_family: Self::parley_font_family(style.font),
            font_size: style.px_size as f32,
            brush: style.color.to_array(),
            ..TextStyle::default()
        }
    }

    /// Lays out `text` with the given style and optional max width (word wrap).
    ///
    /// `max_width` is in logical pixels; `None` leaves line breaking
    /// unconstrained apart from explicit line separators. Color and decoration
    /// are not stored in the returned `Layout<()>`. Fractional glyph positions
    /// are retained.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::ParleyEngine;
    /// #[allow(deprecated)]
    /// let mut engine = ParleyEngine::new();
    /// let layout = engine.layout_text(
    ///     "hello world",
    ///     TextStyle::new(FontId::Ui, 16, Color::WHITE),
    ///     Some(80.0),
    /// );
    /// assert!(layout.height() > 0.0);
    /// ```
    pub fn layout_text(
        &mut self,
        text: &str,
        style: UiTextStyle,
        max_width: Option<f32>,
    ) -> Layout<()> {
        self.layout_text_with_wrap(text, style, max_width, WrapMode::Word)
    }

    /// Lays out `text` with an explicit wrap strategy.
    ///
    /// This low-level method always forwards `max_width` to Parley's line
    /// breaker. In particular, `WrapMode::NoWrap` does not discard a supplied
    /// width here; use [`crate::layout_text`] for the higher-level no-wrap
    /// contract. [`crate::WrapMode::WordOrAnywhere`] additionally enables
    /// cluster-level overflow breaking. Positions remain fractional.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{ParleyEngine, WrapMode};
    /// #[allow(deprecated)]
    /// let mut engine = ParleyEngine::new();
    /// let layout = engine.layout_text_with_wrap(
    ///     "an_unbroken_identifier",
    ///     TextStyle::new(FontId::Mono, 14, Color::WHITE),
    ///     Some(40.0),
    ///     WrapMode::WordOrAnywhere,
    /// );
    /// assert!(layout.lines().count() >= 1);
    /// ```
    pub fn layout_text_with_wrap(
        &mut self,
        text: &str,
        style: UiTextStyle,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> Layout<()> {
        let scale = 1.0_f32;
        let quantize = false;
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, scale, quantize);

        let style = Self::parley_text_style(style);
        builder.push_default(StyleProperty::FontFamily(style.font_family));
        builder.push_default(StyleProperty::FontSize(style.font_size));
        if matches!(wrap_mode, WrapMode::WordOrAnywhere) {
            builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
        }

        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(max_width);
        layout.align(Alignment::Start, AlignmentOptions::default());
        layout
    }

    /// Lays out `text` with ranged text styles and an explicit wrap strategy.
    ///
    /// Each span replaces family, logical-pixel size, and linear-RGBA brush in
    /// its half-open UTF-8 byte range. Ranges are passed to Parley in slice
    /// order and must be valid for `text`. As in [`Self::layout_text_with_wrap`],
    /// this low-level method does not suppress `max_width` for `NoWrap`.
    ///
    /// # Panics
    ///
    /// Parley may panic when a span range is reversed, out of bounds, or not on
    /// UTF-8 boundaries; callers must validate externally sourced ranges.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_text::{ParleyEngine, StyledTextSpan, WrapMode};
    /// #[allow(deprecated)]
    /// let mut engine = ParleyEngine::new();
    /// let base = TextStyle::new(FontId::Mono, 14, Color::WHITE);
    /// let spans = [StyledTextSpan { range: 0..2, style: base.underline() }];
    /// let layout = engine.layout_styled_text_with_wrap(
    ///     "fn main", base, &spans, None, WrapMode::NoWrap,
    /// );
    /// assert!(layout.height() > 0.0);
    /// ```
    pub fn layout_styled_text_with_wrap(
        &mut self,
        text: &str,
        base_style: UiTextStyle,
        spans: &[crate::StyledTextSpan],
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> Layout<[f32; 4]> {
        let scale = 1.0_f32;
        let quantize = false;
        let mut builder =
            self.styled_layout_cx
                .ranged_builder(&mut self.font_cx, text, scale, quantize);

        let style = Self::parley_text_style_color(base_style);
        builder.push_default(StyleProperty::FontFamily(style.font_family));
        builder.push_default(StyleProperty::FontSize(style.font_size));
        builder.push_default(StyleProperty::Brush(style.brush));
        if matches!(wrap_mode, WrapMode::WordOrAnywhere) {
            builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
        }

        for span in spans {
            let span_style = Self::parley_text_style_color(span.style);
            builder.push(
                StyleProperty::FontFamily(span_style.font_family),
                span.range.clone(),
            );
            builder.push(
                StyleProperty::FontSize(span_style.font_size),
                span.range.clone(),
            );
            builder.push(StyleProperty::Brush(span_style.brush), span.range.clone());
        }

        let mut layout: Layout<[f32; 4]> = builder.build(text);
        layout.break_all_lines(max_width);
        layout.align(Alignment::Start, AlignmentOptions::default());
        layout
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailloli_ui_core::style::Color;

    #[test]
    fn builds_layout_without_quantize() {
        #[allow(deprecated)]
        let mut eng = ParleyEngine::new();
        let style = UiTextStyle::new(FontId::Ui, 16, Color::new(1.0, 1.0, 1.0, 1.0));
        let layout = eng.layout_text("hello parley", style, Some(120.0));
        assert!(layout.height() > 0.0);
    }
}
