//! Maps `IconId` to Lucide font glyphs (fontdue raster path).

use ailloli_ui_core::IconId;
use lucide_icons::Icon;

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

pub fn lucide_char(id: &IconId) -> char {
    char::from(curated_to_lucide(id.clone()))
}
