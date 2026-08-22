//! Pure color picker values and conversions.

use crate::{Color, ColorParseError};

/// A color expressed as hue, saturation, and value.
///
/// Finite hues use degrees and are normalized to `0.0..360.0`; finite
/// saturation and value components are clamped to `0.0..=1.0`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::HsvColor;
///
/// let hsv = HsvColor::new(420.0, 2.0, 0.5);
/// assert_eq!((hsv.h, hsv.s, hsv.v), (60.0, 1.0, 0.5));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HsvColor {
    /// Hue angle in degrees, normally in `0.0..360.0`.
    pub h: f32,
    /// Saturation fraction, normally in `0.0..=1.0`.
    pub s: f32,
    /// Brightness value fraction, normally in `0.0..=1.0`.
    pub v: f32,
}

impl HsvColor {
    /// Normalizes finite HSV components into their conventional ranges.
    ///
    /// Non-finite components remain non-finite; callers handling untrusted
    /// numeric input should reject those values before conversion.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::HsvColor;
    /// assert_eq!(HsvColor::new(420.0, 2.0, -1.0), HsvColor { h: 60.0, s: 1.0, v: 0.0 });
    /// ```
    pub fn new(h: f32, s: f32, v: f32) -> Self {
        Self {
            h: h.rem_euclid(360.0),
            s: s.clamp(0.0, 1.0),
            v: v.clamp(0.0, 1.0),
        }
    }
}

/// Converts an opaque view of an RGB color to HSV.
///
/// The source alpha channel is intentionally ignored. Returned components are
/// finite and normalized according to [`HsvColor`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{color_to_hsv, Color};
///
/// let hsv = color_to_hsv(Color::rgb(255, 0, 0));
/// assert_eq!((hsv.h, hsv.s, hsv.v), (0.0, 1.0, 1.0));
/// ```
pub fn color_to_hsv(color: Color) -> HsvColor {
    let (r, g, b, _) = color.as_rgba8();
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let h = if delta <= f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };
    let s = if max <= f32::EPSILON {
        0.0
    } else {
        delta / max
    };
    HsvColor::new(h, s, max)
}

/// Converts HSV components to an opaque [`Color`].
///
/// Finite components are normalized with [`HsvColor::new`]. The result always
/// has alpha `1.0`; non-finite input has no useful color interpretation and
/// should be rejected by the caller.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{hsv_to_color, HsvColor};
///
/// assert_eq!(
///     hsv_to_color(HsvColor::new(120.0, 1.0, 1.0)).as_rgba8(),
///     (0, 255, 0, 255),
/// );
/// ```
pub fn hsv_to_color(hsv: HsvColor) -> Color {
    let hsv = HsvColor::new(hsv.h, hsv.s, hsv.v);
    let c = hsv.v * hsv.s;
    let x = c * (1.0 - ((hsv.h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = hsv.v - c;
    let (r, g, b) = match hsv.h {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color::rgba(
        ((r + m).clamp(0.0, 1.0) * 255.0).round() as u8,
        ((g + m).clamp(0.0, 1.0) * 255.0).round() as u8,
        ((b + m).clamp(0.0, 1.0) * 255.0).round() as u8,
        1.0,
    )
}

/// Parses an RGB hexadecimal string and forces its alpha channel to `1.0`.
///
/// # Errors
///
/// Returns [`ColorParseError`] when `input` is not one of the hexadecimal
/// forms accepted by [`Color::hex`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{parse_hex_rgb, Color};
///
/// assert_eq!(parse_hex_rgb("#FF5A00")?, Color::rgb(255, 90, 0));
/// assert!(parse_hex_rgb("orange").is_err());
/// # Ok::<(), ailloli_ui_core::ColorParseError>(())
/// ```
pub fn parse_hex_rgb(input: &str) -> Result<Color, ColorParseError> {
    let color = Color::hex(input)?;
    Ok(color.with_alpha(1.0))
}

/// Formats the RGB channels as uppercase `#RRGGBB`, discarding alpha.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{format_hex_rgb, Color};
///
/// assert_eq!(format_hex_rgb(Color::rgb(255, 90, 0)), "#FF5A00");
/// ```
pub fn format_hex_rgb(color: Color) -> String {
    let (r, g, b, _) = color.as_rgba8();
    format!("#{r:02X}{g:02X}{b:02X}")
}

#[cfg(test)]
mod tests {
    //! Covers hexadecimal normalization and lossless RGB/HSV round trips.

    use super::*;

    #[test]
    fn hex_format_is_uppercase_rgb() {
        assert_eq!(format_hex_rgb(Color::rgb(255, 90, 0)), "#FF5A00");
        assert_eq!(parse_hex_rgb("#ff5a00"), Ok(Color::rgb(255, 90, 0)));
    }

    #[test]
    fn hsv_roundtrip_preserves_primary_color() {
        let hsv = color_to_hsv(Color::rgb(255, 90, 0));
        let color = hsv_to_color(hsv);
        let (r, g, b, _) = color.as_rgba8();
        assert_eq!((r, g, b), (255, 90, 0));
    }

    #[test]
    fn hsv_roundtrip_preserves_non_primary_color() {
        let hsv = color_to_hsv(Color::rgb(72, 120, 188));
        let color = hsv_to_color(hsv);
        let (r, g, b, _) = color.as_rgba8();
        assert_eq!((r, g, b), (72, 120, 188));
    }
}
