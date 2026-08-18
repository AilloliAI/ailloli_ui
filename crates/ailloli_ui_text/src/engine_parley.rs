use ailloli_ui_core::{style::TextStyle as UiTextStyle, FontId};

use parley::{
    Alignment, AlignmentOptions, FontContext, Layout, LayoutContext, OverflowWrap, StyleProperty,
    TextStyle,
};

use crate::WrapMode;

const JBM_NERD_REGULAR: &[u8] = include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf");

/// Thin wrapper around Parley:
/// - holds shared `FontContext` / `LayoutContext`
/// - builds `Layout<()>` with fractional positioning (`quantize = false`)
#[derive(Default)]
pub struct ParleyEngine {
    pub font_cx: FontContext,
    pub layout_cx: LayoutContext<()>,
    pub styled_layout_cx: LayoutContext<[f32; 4]>,
}

impl ParleyEngine {
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

    fn parley_font_family(font: FontId) -> parley::FontFamily<'static> {
        match font {
            FontId::Ui => parley::GenericFamily::SansSerif.into(),
            FontId::Mono => parley::GenericFamily::Monospace.into(),
        }
    }

    fn parley_text_style(style: UiTextStyle) -> TextStyle<'static, 'static, ()> {
        TextStyle {
            font_family: Self::parley_font_family(style.font),
            font_size: style.px_size as f32,
            brush: (),
            ..TextStyle::default()
        }
    }

    fn parley_text_style_color(style: UiTextStyle) -> TextStyle<'static, 'static, [f32; 4]> {
        TextStyle {
            font_family: Self::parley_font_family(style.font),
            font_size: style.px_size as f32,
            brush: style.color.to_array(),
            ..TextStyle::default()
        }
    }

    /// Lays out `text` with the given style and optional max width (word wrap).
    pub fn layout_text(
        &mut self,
        text: &str,
        style: UiTextStyle,
        max_width: Option<f32>,
    ) -> Layout<()> {
        self.layout_text_with_wrap(text, style, max_width, WrapMode::Word)
    }

    /// Lays out `text` with an explicit wrap strategy.
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
