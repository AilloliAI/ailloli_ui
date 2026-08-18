use ailloli_ui_core::{IconId, SvgSource};
use ailloli_ui_render_wgpu::{rasterize_svg, IconKey};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

fn hash_key(key: &IconKey) -> u64 {
    let mut h = DefaultHasher::new();
    key.hash(&mut h);
    h.finish()
}

#[test]
fn icon_keys_differ_by_source_variant() {
    let base = IconKey {
        icon: IconId::Plus,
        px_size: 16,
        scale_100: 100,
    };
    let lucide = IconKey {
        icon: IconId::Lucide(lucide_icons::Icon::Plus),
        px_size: 16,
        scale_100: 100,
    };
    let devicon = IconKey {
        icon: IconId::Devicon('\u{e700}'),
        px_size: 16,
        scale_100: 100,
    };
    let svg = IconKey {
        icon: IconId::Svg(SvgSource::Static(b"<svg/>")),
        px_size: 16,
        scale_100: 100,
    };

    let hashes = [
        hash_key(&base),
        hash_key(&lucide),
        hash_key(&devicon),
        hash_key(&svg),
    ];
    assert_eq!(
        hashes
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );
}

#[test]
fn svg_rasterize_produces_non_empty_buffer() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" fill="red"/></svg>"#;
    let src = SvgSource::Owned(Arc::from(svg.as_bytes()));
    let rgba = rasterize_svg(&src, 32).expect("rasterize");
    assert_eq!(rgba.len(), 32 * 32 * 4);
    assert!(rgba.iter().any(|b| *b > 0));
}
