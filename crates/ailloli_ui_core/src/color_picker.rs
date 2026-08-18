//! Pure color picker values and conversions.

use crate::{Color, ColorParseError};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HsvColor {
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

impl HsvColor {
    pub fn new(h: f32, s: f32, v: f32) -> Self {
        Self {
            h: h.rem_euclid(360.0),
            s: s.clamp(0.0, 1.0),
            v: v.clamp(0.0, 1.0),
        }
    }
}

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

pub fn parse_hex_rgb(input: &str) -> Result<Color, ColorParseError> {
    let color = Color::hex(input)?;
    Ok(color.with_alpha(1.0))
}

pub fn format_hex_rgb(color: Color) -> String {
    let (r, g, b, _) = color.as_rgba8();
    format!("#{r:02X}{g:02X}{b:02X}")
}

#[cfg(test)]
mod tests {
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
