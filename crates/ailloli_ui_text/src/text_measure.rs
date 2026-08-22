//! Width measurement abstractions for lightweight text helpers.

use ailloli_ui_core::FontId;

/// Measures text width and per-character advance for simple layout/wrap.
///
/// Values are logical pixels. Implementations may be approximate and do not
/// have to perform shaping, kerning, bidi reordering, or grapheme clustering.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::FontId;
/// use ailloli_ui_text::{ApproxTextMeasure, TextMeasure};
/// assert!(ApproxTextMeasure.measure("abc", FontId::Ui, 12) > 0.0);
/// ```
pub trait TextMeasure {
    /// Returns the estimated logical-pixel advance of `text` at `px_size`.
    ///
    /// Empty text conventionally measures zero. `px_size` is forwarded as-is,
    /// including zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::FontId;
    /// use ailloli_ui_text::{ApproxTextMeasure, TextMeasure};
    /// assert_eq!(ApproxTextMeasure.measure("", FontId::Mono, 14), 0.0);
    /// ```
    fn measure(&self, text: &str, font: FontId, px_size: u16) -> f32;

    /// Returns the logical-pixel advance of one Unicode scalar.
    ///
    /// The default implementation UTF-8 encodes `ch` and delegates to
    /// [`TextMeasure::measure`]. Implementations may override it for efficiency.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::FontId;
    /// use ailloli_ui_text::{ApproxTextMeasure, TextMeasure};
    /// assert_eq!(
    ///     ApproxTextMeasure.advance('é', FontId::Ui, 10),
    ///     ApproxTextMeasure.measure("é", FontId::Ui, 10),
    /// );
    /// ```
    fn advance(&self, ch: char, font: FontId, px_size: u16) -> f32 {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        self.measure(s, font, px_size)
    }
}

/// Fast approximate measurement (fixed char width ratio).
///
/// Every Unicode scalar advances by exactly `0.58 * px_size` logical pixels;
/// font family and scalar contents are ignored. This is deterministic but not
/// suitable when shaped typography or exact caret placement matters.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::FontId;
/// use ailloli_ui_text::{ApproxTextMeasure, TextMeasure};
/// let width = ApproxTextMeasure.measure("ab", FontId::Ui, 10);
/// assert!((width - 11.6).abs() < 0.001);
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct ApproxTextMeasure;

impl TextMeasure for ApproxTextMeasure {
    fn measure(&self, text: &str, _font: FontId, px_size: u16) -> f32 {
        let approx_char_w = (px_size as f32) * 0.58;
        (text.chars().count() as f32) * approx_char_w
    }
}

/// Bundled JetBrains Mono Nerd Font bytes used for the monospace slot.
const JBM_NERD_REGULAR: &[u8] = include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf");

/// `TextMeasure` implementation backed by `fontdue` (UI + bundled mono).
///
/// Construction probes a small list of common Linux sans-serif paths. The
/// monospace slot first uses the bundled font. If a requested face is absent,
/// measurement falls back to [`ApproxTextMeasure`], so construction and
/// measurement are infallible and portable.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::FontId;
/// use ailloli_ui_text::{FontMetrics, TextMeasure};
/// let metrics = FontMetrics::new();
/// assert_eq!(metrics.measure("", FontId::Ui, 16), 0.0);
/// ```
pub struct FontMetrics {
    /// Optional system sans-serif font.
    font_ui: Option<fontdue::Font>,
    /// Bundled or system monospace font.
    font_mono: Option<fontdue::Font>,
    /// Deterministic fallback used when no font is available.
    approx: ApproxTextMeasure,
}

impl FontMetrics {
    /// Loads the bundled monospace face and probes common Linux UI-font paths.
    ///
    /// Missing or invalid system files are silently skipped. This function does
    /// filesystem I/O but never returns an error; unavailable faces use the
    /// approximate fallback during measurement.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::FontId;
    /// use ailloli_ui_text::{FontMetrics, TextMeasure};
    /// let metrics = FontMetrics::new();
    /// assert!(metrics.advance('x', FontId::Mono, 14) >= 0.0);
    /// ```
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

    /// Resolves a built-in font slot, allowing the UI face as mono fallback.
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

/// Returns the first fontdue-compatible font read from `paths`.
///
/// I/O and parsing failures are intentionally skipped so startup is not tied
/// to any one operating-system font layout.
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
