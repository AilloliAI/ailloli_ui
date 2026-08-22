//! Verifies every [`Length`](ailloli_ui_core::style::Length) resolution mode and clamping.

use ailloli_ui_core::style::Length;

#[test]
fn length_resolve_auto_is_none() {
    assert_eq!(Length::Auto.resolve(100.0), None);
}

#[test]
fn length_resolve_px_clamps_to_zero() {
    assert_eq!(Length::Px(12.5).resolve(100.0), Some(12.5));
    assert_eq!(Length::Px(-1.0).resolve(100.0), Some(0.0));
}

#[test]
fn length_resolve_fill_uses_available() {
    assert_eq!(Length::Fill.resolve(123.0), Some(123.0));
    assert_eq!(Length::Fill.resolve(-5.0), Some(0.0));
}

#[test]
fn length_resolve_percent_scales_available() {
    assert_eq!(Length::Percent(0.5).resolve(200.0), Some(100.0));
    assert_eq!(Length::Percent(1.5).resolve(10.0), Some(15.0));
    assert_eq!(Length::Percent(-1.0).resolve(10.0), Some(0.0));
}
