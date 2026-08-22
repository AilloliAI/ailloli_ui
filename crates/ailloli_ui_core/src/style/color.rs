//! Linear-RGBA color values and sRGB hexadecimal conversion.

/// Linear RGBA color whose normalized components are normally in `0.0..=1.0`.
///
/// Fields are public and [`Self::from_f32_const`] stores values verbatim, so the
/// normalized range is a caller-maintained invariant for those construction paths.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Color;
/// assert_eq!(Color::rgb(255, 0, 0).as_rgba8(), (255, 0, 0, 255));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// Linear red component.
    pub r: f32,
    /// Linear green component.
    pub g: f32,
    /// Linear blue component.
    pub b: f32,
    /// Linear alpha component, where zero is transparent and one is opaque.
    pub a: f32,
}

/// Error when parsing a hex color string.
///
/// Possible values distinguish an unsupported length from a non-hex digit.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, ColorParseError};
/// assert_eq!(Color::hex("#12").unwrap_err(), ColorParseError::InvalidLength);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorParseError {
    /// Wrong number of hex digits.
    InvalidLength,
    /// Non-hex character.
    InvalidChar,
}

impl Color {
    /// Fully transparent black in linear RGBA.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// assert_eq!(Color::TRANSPARENT.a, 0.0);
    /// ```
    pub const TRANSPARENT: Self = Self::from_f32_const(0.0, 0.0, 0.0, 0.0);
    /// Opaque black in linear RGBA.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// assert_eq!(Color::BLACK.as_rgba8(), (0, 0, 0, 255));
    /// ```
    pub const BLACK: Self = Self::from_f32_const(0.0, 0.0, 0.0, 1.0);
    /// Opaque white in linear RGBA.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// assert_eq!(Color::WHITE.as_rgba8(), (255, 255, 255, 255));
    /// ```
    pub const WHITE: Self = Self::from_f32_const(1.0, 1.0, 1.0, 1.0);

    /// Creates a linear color from verbatim already-linear components.
    ///
    /// This const constructor does not clamp or reject non-finite values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// assert_eq!(Color::from_f32_const(2.0, 0.0, 0.0, 1.0).r, 2.0);
    /// ```
    pub const fn from_f32_const(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Creates a linear color from already-linear components.
    ///
    /// Use [`Self::rgba`] or [`Self::rgb`] for normal UI colors written as
    /// sRGB/RGB values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// assert_eq!(Color::new(0.25, 0.5, 0.75, 1.0).to_array(), [0.25, 0.5, 0.75, 1.0]);
    /// ```
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self::f32(r, g, b, a)
    }

    /// Converts 8-bit sRGB channels to linear RGB and clamps alpha to `0.0..=1.0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// assert_eq!(Color::rgba(255, 0, 0, 2.0).as_rgba8(), (255, 0, 0, 255));
    /// ```
    pub fn rgba(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self {
            r: srgb_u8_to_linear(r),
            g: srgb_u8_to_linear(g),
            b: srgb_u8_to_linear(b),
            a: a.clamp(0.0, 1.0),
        }
    }

    /// Creates an opaque linear color from standard sRGB/RGB channels.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// assert_eq!(Color::rgb(0, 128, 255).as_rgba8(), (0, 128, 255, 255));
    /// ```
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 1.0)
    }

    /// Creates a linear color by clamping already-linear components to `0.0..=1.0`.
    ///
    /// NaN components remain NaN under floating-point clamp semantics.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// assert_eq!(Color::f32(-1.0, 0.5, 2.0, 1.0).to_array(), [0.0, 0.5, 1.0, 1.0]);
    /// ```
    pub fn f32(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: r.clamp(0.0, 1.0),
            g: g.clamp(0.0, 1.0),
            b: b.clamp(0.0, 1.0),
            a: a.clamp(0.0, 1.0),
        }
    }

    /// Parses `#RGB`, `#RGBA`, `#RRGGBB`, or `#RRGGBBAA` sRGB.
    ///
    /// The leading `#` is optional and hexadecimal digits are case-insensitive.
    /// Three/four-digit forms duplicate each nibble. RGB-only forms use opaque
    /// alpha before conversion to linear RGB.
    ///
    /// # Errors
    ///
    /// Returns [`ColorParseError::InvalidLength`] unless the post-prefix input
    /// has 3, 4, 6, or 8 bytes, and [`ColorParseError::InvalidChar`] for a
    /// non-ASCII-hex digit.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// assert_eq!(Color::hex("#0f08")?.as_rgba8(), (0, 255, 0, 136));
    /// assert!(Color::hex("#xyz").is_err());
    /// # Ok::<(), ailloli_ui_core::ColorParseError>(())
    /// ```
    pub fn hex(s: &str) -> Result<Self, ColorParseError> {
        let hex = s.strip_prefix('#').unwrap_or(s);
        let expanded = expand_hex_shorthand(hex)?;
        let (r, g, b, a) = parse_hex_channels(&expanded)?;
        Ok(Self::rgba(r, g, b, a as f32 / 255.0))
    }

    /// Creates an opaque linear color from the low 24 bits of `0xRRGGBB` sRGB.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// assert_eq!(Color::hex_rgb(0x12_34_56).as_rgba8(), (0x12, 0x34, 0x56, 255));
    /// ```
    pub fn hex_rgb(v: u32) -> Self {
        Self::rgb(
            ((v >> 16) & 0xFF) as u8,
            ((v >> 8) & 0xFF) as u8,
            (v & 0xFF) as u8,
        )
    }

    /// Replaces alpha after clamping it to `0.0..=1.0`.
    ///
    /// RGB components are preserved verbatim; NaN alpha remains NaN.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// assert_eq!(Color::WHITE.with_alpha(0.5).a, 0.5);
    /// ```
    pub fn with_alpha(self, a: f32) -> Self {
        Self {
            a: a.clamp(0.0, 1.0),
            ..self
        }
    }

    /// Returns linear components in `[red, green, blue, alpha]` order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// assert_eq!(Color::BLACK.to_array(), [0.0, 0.0, 0.0, 1.0]);
    /// ```
    pub fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Converts linear RGB to 8-bit sRGB and alpha to an 8-bit linear channel.
    ///
    /// Normalized components are clamped before rounding. Rust float-to-integer
    /// cast semantics map NaN to zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// assert_eq!(Color::WHITE.with_alpha(0.5).as_rgba8(), (255, 255, 255, 128));
    /// ```
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
///
/// # Examples
///
/// ```
/// #![allow(deprecated)]
/// use ailloli_ui_core::{Color, Rgba};
/// let color: Rgba = Color::WHITE;
/// assert_eq!(color, Color::WHITE);
/// ```
#[deprecated(note = "use Color instead")]
pub type Rgba = Color;

/// Converts one normalized sRGB component through the standard transfer curve.
fn srgb_to_linear_component(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Converts one normalized linear component through the standard sRGB curve.
fn linear_to_srgb_component(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Normalizes and converts one 8-bit sRGB component to linear space.
fn srgb_u8_to_linear(v: u8) -> f32 {
    srgb_to_linear_component(v as f32 / 255.0)
}

/// Converts one linear component to rounded 8-bit sRGB.
fn linear_to_srgb_u8(v: f32) -> u8 {
    (linear_to_srgb_component(v) * 255.0).round() as u8
}

/// Expands three/four-nibble notation and validates the accepted byte lengths.
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

/// Parses expanded six/eight-digit hexadecimal channels, defaulting alpha to 255.
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

/// Parses exactly two ASCII hexadecimal digits.
fn parse_hex_byte(bytes: &[u8]) -> Result<u8, ColorParseError> {
    let hi = hex_digit(bytes[0])?;
    let lo = hex_digit(bytes[1])?;
    Ok((hi << 4) | lo)
}

/// Converts one ASCII hexadecimal digit to its numeric nibble.
fn hex_digit(byte: u8) -> Result<u8, ColorParseError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(ColorParseError::InvalidChar),
    }
}
