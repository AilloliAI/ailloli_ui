//! Covers fill, fixed/percentage lengths, min/max resolution, and bound normalization.

use ailloli_ui_core::geometry::{Constraints, Size};
use ailloli_ui_core::style::{LayoutStyle, Length};

#[test]
fn fill_resolves_to_parent_max() {
    let parent = Constraints::tight(400.0, 300.0);
    let resolved = LayoutStyle::new().fill().resolve(parent);
    assert_eq!(resolved.width, Some(400.0));
    assert_eq!(resolved.height, Some(300.0));
}

#[test]
fn fill_width_only_resolves_width() {
    let parent = Constraints::tight(400.0, 300.0);
    let resolved = LayoutStyle::new().fill_width().resolve(parent);
    assert_eq!(resolved.width, Some(400.0));
    assert_eq!(resolved.height, None);
}

#[test]
fn fill_height_only_resolves_height() {
    let parent = Constraints::tight(400.0, 300.0);
    let resolved = LayoutStyle::new().fill_height().resolve(parent);
    assert_eq!(resolved.width, None);
    assert_eq!(resolved.height, Some(300.0));
}

#[test]
fn px_and_percent_resolve() {
    let parent = Constraints::tight(200.0, 100.0);
    let resolved = LayoutStyle::new()
        .width(Length::px(50.0))
        .height(Length::percent(0.5))
        .resolve(parent);
    assert_eq!(resolved.width, Some(50.0));
    assert_eq!(resolved.height, Some(50.0));
}

#[test]
fn percent_resolve_accepts_css_percent_values() {
    let parent = Constraints::tight(200.0, 100.0);
    let resolved = LayoutStyle::new()
        .width(Length::percent(25.0))
        .height(Length::percent(50.0))
        .resolve(parent);

    assert_eq!(resolved.width, Some(50.0));
    assert_eq!(resolved.height, Some(50.0));
}

#[test]
fn resolved_size_applies_min_max() {
    let parent = Constraints::loose(500.0, 500.0);
    let resolved = LayoutStyle::new()
        .min_width(Length::px(80.0))
        .max_width(Length::px(120.0))
        .resolve(parent);
    let (w, _) = resolved.size(10.0, 10.0, parent);
    assert_eq!(w, 80.0);

    let (w, _) = resolved.size(200.0, 10.0, parent);
    assert_eq!(w, 120.0);
}

#[test]
fn constraints_for_children_tightens_max_when_fill() {
    let inner = Constraints::loose(300.0, 200.0);
    let child = LayoutStyle::new()
        .fill_width()
        .constraints_for_children(inner);
    assert_eq!(child.max_w, 300.0);
}

#[test]
fn constraints_for_children_normalizes_when_parent_is_too_narrow() {
    let inner = Constraints::tight(0.0, 360.0);
    let child = LayoutStyle::new()
        .width(Length::px(280.0))
        .constraints_for_children(inner);
    assert!(child.min_w <= child.max_w);
    assert_eq!(child.min_w, 0.0);
    assert_eq!(child.max_w, 280.0);
}

#[test]
fn constrain_does_not_panic_when_min_exceeds_max() {
    let constraints = Constraints {
        min_w: 280.0,
        max_w: 0.0,
        min_h: 0.0,
        max_h: 360.0,
    };
    let size = constraints.constrain(Size::new(100.0, 200.0));
    assert_eq!(size.w, 100.0);
    assert_eq!(size.h, 200.0);
}
