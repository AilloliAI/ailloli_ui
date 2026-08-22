//! Maps `IconId` to Lucide font glyphs (fontdue raster path).

use ailloli_ui_core::IconId;
use lucide_icons::Icon;

/// Maps the framework's curated icon identifiers to Lucide glyphs.
///
/// Existing `IconId::Lucide` values pass through. Devicon and SVG identifiers
/// have separate raster paths and therefore map to `Circle` only as a defensive
/// fallback when this function is called directly.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::IconId;
/// use ailloli_ui_render_wgpu::icons::lucide::curated_to_lucide;
/// assert_eq!(char::from(curated_to_lucide(IconId::Plus)),
///     char::from(lucide_icons::Icon::Plus));
/// ```
pub fn curated_to_lucide(id: IconId) -> Icon {
    match id {
        IconId::Minimize => Icon::Minus,
        IconId::Maximize => Icon::Square,
        IconId::Close => Icon::X,
        IconId::Copy => Icon::Copy,
        IconId::Trash => Icon::Trash2,
        IconId::History => Icon::RotateCcw,
        IconId::Plus => Icon::Plus,
        IconId::Check => Icon::Check,
        IconId::Lucide(icon) => icon,
        IconId::Devicon(_) | IconId::Svg(_) => Icon::Circle,
    }
}

/// Returns the font character for a curated or direct Lucide icon identifier.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::IconId;
/// use ailloli_ui_render_wgpu::icons::lucide::lucide_char;
/// assert_eq!(lucide_char(&IconId::Check), char::from(lucide_icons::Icon::Check));
/// ```
pub fn lucide_char(id: &IconId) -> char {
    char::from(curated_to_lucide(id.clone()))
}
