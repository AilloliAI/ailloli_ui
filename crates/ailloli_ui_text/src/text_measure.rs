use ailloli_ui_core::FontId;

/// Measures text width and per-character advance for simple layout/wrap.
pub trait TextMeasure {
    fn measure(&self, text: &str, font: FontId, px_size: u16) -> f32;

    fn advance(&self, ch: char, font: FontId, px_size: u16) -> f32 {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        self.measure(s, font, px_size)
    }
}

/// Fast approximate measurement (fixed char width ratio).
#[derive(Debug, Default, Clone, Copy)]
pub struct ApproxTextMeasure;

impl TextMeasure for ApproxTextMeasure {
    fn measure(&self, text: &str, _font: FontId, px_size: u16) -> f32 {
        let approx_char_w = (px_size as f32) * 0.58;
        (text.chars().count() as f32) * approx_char_w
    }
}

const JBM_NERD_REGULAR: &[u8] = include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf");

/// `TextMeasure` implementation backed by `fontdue` (UI + bundled mono).
pub struct FontMetrics {
    font_ui: Option<fontdue::Font>,
    font_mono: Option<fontdue::Font>,
    approx: ApproxTextMeasure,
}

impl FontMetrics {
    pub fn new() -> Self {
        let font_ui = load_font_from_common_paths(&[
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        ]);
        let font_mono =
            fontdue::Font::from_bytes(JBM_NERD_REGULAR, fontdue::FontSettings::default())
                .ok()
                .or_else(|| {
                    load_font_from_common_paths(&[
                        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
                        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
                        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
                    ])
                });
        Self {
            font_ui,
            font_mono,
            approx: ApproxTextMeasure,
        }
    }

    fn font(&self, font: FontId) -> Option<&fontdue::Font> {
        match font {
            FontId::Ui => self.font_ui.as_ref(),
            FontId::Mono => self.font_mono.as_ref().or(self.font_ui.as_ref()),
        }
    }
}

impl Default for FontMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl TextMeasure for FontMetrics {
    fn measure(&self, text: &str, font: FontId, px_size: u16) -> f32 {
        let Some(f) = self.font(font) else {
            return self.approx.measure(text, font, px_size);
        };
        let px = px_size as f32;
        let mut w = 0.0_f32;
        for ch in text.chars() {
            let m = f.metrics(ch, px);
            w += m.advance_width;
        }
        w
    }

    fn advance(&self, ch: char, font: FontId, px_size: u16) -> f32 {
        let Some(f) = self.font(font) else {
            return self.approx.advance(ch, font, px_size);
        };
        let px = px_size as f32;
        f.metrics(ch, px).advance_width
    }
}

fn load_font_from_common_paths(paths: &[&str]) -> Option<fontdue::Font> {
    for p in paths {
        if let Ok(bytes) = std::fs::read(p) {
            if let Ok(font) = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()) {
                return Some(font);
            }
        }
    }
    None
}
