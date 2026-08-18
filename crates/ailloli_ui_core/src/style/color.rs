/// Linear RGBA color with components in `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// Error when parsing a hex color string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorParseError {
    /// Wrong number of hex digits.
    InvalidLength,
    /// Non-hex character.
    InvalidChar,
}

impl Color {
    pub const TRANSPARENT: Self = Self::from_f32_const(0.0, 0.0, 0.0, 0.0);
    pub const BLACK: Self = Self::from_f32_const(0.0, 0.0, 0.0, 1.0);
    pub const WHITE: Self = Self::from_f32_const(1.0, 1.0, 1.0, 1.0);

    /// Creates a linear color from already-linear components.
    pub const fn from_f32_const(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Creates a linear color from already-linear components.
    ///
    /// Use [`Self::rgba`] or [`Self::rgb`] for normal UI colors written as
    /// sRGB/RGB values.
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self::f32(r, g, b, a)
    }

    /// Creates a linear color from standard sRGB/RGBA channels.
    pub fn rgba(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self {
            r: srgb_u8_to_linear(r),
            g: srgb_u8_to_linear(g),
            b: srgb_u8_to_linear(b),
            a: a.clamp(0.0, 1.0),
        }
    }

    /// Creates an opaque linear color from standard sRGB/RGB channels.
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 1.0)
    }

    /// Creates a linear color from already-linear components.
    pub fn f32(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: r.clamp(0.0, 1.0),
            g: g.clamp(0.0, 1.0),
            b: b.clamp(0.0, 1.0),
            a: a.clamp(0.0, 1.0),
        }
    }

    /// Parses `#RGB`, `#RGBA`, `#RRGGBB`, or `#RRGGBBAA` (`#` prefix optional).
    pub fn hex(s: &str) -> Result<Self, ColorParseError> {
        let hex = s.strip_prefix('#').unwrap_or(s);
        let expanded = expand_hex_shorthand(hex)?;
        let (r, g, b, a) = parse_hex_channels(&expanded)?;
        Ok(Self::rgba(r, g, b, a as f32 / 255.0))
    }

    /// Creates an opaque linear color from a `0xRRGGBB` sRGB value.
    pub fn hex_rgb(v: u32) -> Self {
        Self::rgb(
            ((v >> 16) & 0xFF) as u8,
            ((v >> 8) & 0xFF) as u8,
            (v & 0xFF) as u8,
        )
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self {
            a: a.clamp(0.0, 1.0),
            ..self
        }
    }

    pub fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub fn as_rgba8(self) -> (u8, u8, u8, u8) {
        (
            linear_to_srgb_u8(self.r),
            linear_to_srgb_u8(self.g),
            linear_to_srgb_u8(self.b),
            (self.a.clamp(0.0, 1.0) * 255.0).round() as u8,
        )
    }
}

/// Deprecated alias — use [`Color`].
#[deprecated(note = "use Color instead")]
pub type Rgba = Color;

fn srgb_to_linear_component(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb_component(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_u8_to_linear(v: u8) -> f32 {
    srgb_to_linear_component(v as f32 / 255.0)
}

fn linear_to_srgb_u8(v: f32) -> u8 {
    (linear_to_srgb_component(v) * 255.0).round() as u8
}

fn expand_hex_shorthand(hex: &str) -> Result<String, ColorParseError> {
    let len = hex.len();
    if !matches!(len, 3 | 4 | 6 | 8) {
        return Err(ColorParseError::InvalidLength);
    }
    if len == 3 || len == 4 {
        let mut out = String::with_capacity(len * 2);
        for ch in hex.chars() {
            out.push(ch);
            out.push(ch);
        }
        Ok(out)
    } else {
        Ok(hex.to_string())
    }
}

fn parse_hex_channels(hex: &str) -> Result<(u8, u8, u8, u8), ColorParseError> {
    let bytes = hex.as_bytes();
    let (r, g, b, a) = match bytes.len() {
        6 => (
            parse_hex_byte(&bytes[0..2])?,
            parse_hex_byte(&bytes[2..4])?,
            parse_hex_byte(&bytes[4..6])?,
            255u8,
        ),
        8 => (
            parse_hex_byte(&bytes[0..2])?,
            parse_hex_byte(&bytes[2..4])?,
            parse_hex_byte(&bytes[4..6])?,
            parse_hex_byte(&bytes[6..8])?,
        ),
        _ => return Err(ColorParseError::InvalidLength),
    };
    Ok((r, g, b, a))
}

fn parse_hex_byte(bytes: &[u8]) -> Result<u8, ColorParseError> {
    let hi = hex_digit(bytes[0])?;
    let lo = hex_digit(bytes[1])?;
    Ok((hi << 4) | lo)
}

fn hex_digit(byte: u8) -> Result<u8, ColorParseError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(ColorParseError::InvalidChar),
    }
}
