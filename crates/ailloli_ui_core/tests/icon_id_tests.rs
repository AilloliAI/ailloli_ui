//! Verifies equality and hashing invariants for curated, Lucide, Devicon, and SVG IDs.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use ailloli_ui_core::{IconId, SvgSource};
use lucide_icons::Icon;

/// Hashes a value with the same fresh deterministic test hasher for comparisons.
fn hash_of<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

#[test]
fn lucide_variants_hash_equally_for_same_icon() {
    let a = IconId::Lucide(Icon::Plus);
    let b = IconId::Lucide(Icon::Plus);
    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));
}

#[test]
fn svg_source_identity_hash_uses_pointer_not_content() {
    let bytes: Arc<[u8]> = Arc::from(b"<svg/>".as_slice());
    let a = SvgSource::Owned(bytes.clone());
    let b = SvgSource::Owned(bytes);
    let c = SvgSource::Owned(Arc::from(b"<svg/>".as_slice()));

    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(hash_of(&a), hash_of(&b));
    assert_ne!(hash_of(&a), hash_of(&c));
}

#[test]
fn static_svg_clone_keeps_a_stable_hash_after_move() {
    static SVG: &[u8] = b"<svg/>";
    let original = SvgSource::Static(SVG);
    let cloned = original.clone();
    let mut map = std::collections::HashMap::new();
    map.insert(cloned, 7_u8);

    assert_eq!(map.get(&original), Some(&7));
    assert_eq!(hash_of(&original), hash_of(&original.clone()));
}

#[test]
fn devicon_char_equality() {
    let a = IconId::Devicon('\u{e700}');
    let b = IconId::Devicon('\u{e700}');
    assert_eq!(a, b);
}

#[test]
fn curated_variants_remain_distinct() {
    assert_ne!(IconId::Plus, IconId::Check);
    assert_ne!(IconId::Plus, IconId::Lucide(Icon::Plus));
}
