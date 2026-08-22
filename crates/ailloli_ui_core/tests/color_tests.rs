//! Verifies linear storage, sRGB conversion, constants, alpha, and every hex form.

use ailloli_ui_core::style::{Color, ColorParseError};

#[test]
fn rgb_opaque() {
    let c = Color::rgb(255, 128, 0);
    assert_eq!(c.as_rgba8(), (255, 128, 0, 255));
    assert!((c.r - 1.0).abs() < f32::EPSILON);
    assert!((c.g - 0.215_860_53).abs() < 0.000_001);
    assert!((c.b - 0.0).abs() < f32::EPSILON);
    assert!((c.a - 1.0).abs() < f32::EPSILON);
}

#[test]
fn rgba_converts_u8_channels() {
    let c = Color::rgba(10, 20, 30, 0.5);
    let (r, g, b, a) = c.as_rgba8();
    assert_eq!((r, g, b), (10, 20, 30));
    assert_eq!(a, 128);
}

#[test]
fn rgb_converts_srgb_to_linear_storage() {
    let c = Color::rgb(128, 128, 128);
    assert!((c.to_array()[0] - 0.215_860_53).abs() < 0.000_001);
    assert_eq!(c.as_rgba8(), (128, 128, 128, 255));
}

#[test]
fn f32_is_linear_and_clamps() {
    let c = Color::f32(1.5, -0.1, 0.5, 2.0);
    assert_eq!(c.to_array(), [1.0, 0.0, 0.5, 1.0]);
    assert_eq!(c.as_rgba8(), (255, 0, 188, 255));
}

#[test]
fn new_aliases_f32() {
    assert_eq!(
        Color::new(1.5, 0.0, 0.0, 1.0),
        Color::f32(1.5, 0.0, 0.0, 1.0)
    );
}

#[test]
fn constants() {
    assert_eq!(Color::BLACK.as_rgba8(), (0, 0, 0, 255));
    assert_eq!(Color::WHITE.as_rgba8(), (255, 255, 255, 255));
    assert_eq!(Color::TRANSPARENT.a, 0.0);
}

#[test]
fn hex_rgb_six_digits() {
    let c = Color::hex("#3b82f6").unwrap();
    assert_eq!(c.as_rgba8(), (59, 130, 246, 255));
}

#[test]
fn hex_rgb_three_digits() {
    let c = Color::hex("#f0f").unwrap();
    assert_eq!(c.as_rgba8(), (255, 0, 255, 255));
}

#[test]
fn hex_rgba_eight_digits() {
    let c = Color::hex("#ff00ff80").unwrap();
    let (_, _, _, a) = c.as_rgba8();
    assert_eq!(a, 128);
}

#[test]
fn hex_rgb_u32() {
    assert_eq!(Color::hex_rgb(0xFF00FF).as_rgba8(), (255, 0, 255, 255));
}

#[test]
fn hex_invalid_length() {
    assert_eq!(Color::hex("#ff"), Err(ColorParseError::InvalidLength));
}

#[test]
fn hex_invalid_char() {
    assert_eq!(Color::hex("#gggggg"), Err(ColorParseError::InvalidChar));
}

#[test]
fn with_alpha_preserves_rgb() {
    let c = Color::rgb(10, 20, 30).with_alpha(0.25);
    assert_eq!(c.as_rgba8().0, 10);
    assert_eq!(c.as_rgba8().1, 20);
    assert_eq!(c.as_rgba8().2, 30);
    assert!((c.a - 0.25).abs() < 0.01);
}
