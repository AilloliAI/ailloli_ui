use std::hash::{Hash, Hasher};

use lucide_icons::Icon;

use super::svg_source::SvgSource;

/// Icon source for rendering (`DrawCmd::Image` / `Icon` widget).
#[derive(Debug, Clone)]
pub enum IconId {
    /// Window minimize (chrome).
    Minimize,
    /// Window maximize (chrome).
    Maximize,
    /// Window close (chrome).
    Close,
    /// Copy action.
    Copy,
    /// Delete action.
    Trash,
    /// History action.
    History,
    /// Add action.
    Plus,
    /// Checkbox / check mark (Lucide `check`).
    Check,
    /// Arbitrary Lucide glyph.
    Lucide(Icon),
    /// Devicons Nerd Font codepoint.
    Devicon(char),
    /// Custom SVG rasterized at draw time.
    Svg(SvgSource),
}

impl IconId {
    fn discriminant_tag(&self) -> u8 {
        match self {
            IconId::Minimize => 0,
            IconId::Maximize => 1,
            IconId::Close => 2,
            IconId::Copy => 3,
            IconId::Trash => 4,
            IconId::History => 5,
            IconId::Plus => 6,
            IconId::Check => 7,
            IconId::Lucide(_) => 8,
            IconId::Devicon(_) => 9,
            IconId::Svg(_) => 10,
        }
    }
}

impl PartialEq for IconId {
    fn eq(&self, other: &Self) -> bool {
        if self.discriminant_tag() != other.discriminant_tag() {
            return false;
        }
        match (self, other) {
            (IconId::Minimize, IconId::Minimize)
            | (IconId::Maximize, IconId::Maximize)
            | (IconId::Close, IconId::Close)
            | (IconId::Copy, IconId::Copy)
            | (IconId::Trash, IconId::Trash)
            | (IconId::History, IconId::History)
            | (IconId::Plus, IconId::Plus)
            | (IconId::Check, IconId::Check) => true,
            (IconId::Lucide(a), IconId::Lucide(b)) => format!("{a}") == format!("{b}"),
            (IconId::Devicon(a), IconId::Devicon(b)) => a == b,
            (IconId::Svg(a), IconId::Svg(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for IconId {}

impl Hash for IconId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.discriminant_tag().hash(state);
        match self {
            IconId::Minimize
            | IconId::Maximize
            | IconId::Close
            | IconId::Copy
            | IconId::Trash
            | IconId::History
            | IconId::Plus
            | IconId::Check => {}
            IconId::Lucide(icon) => {
                format!("{icon}").hash(state);
            }
            IconId::Devicon(ch) => ch.hash(state),
            IconId::Svg(src) => src.hash(state),
        }
    }
}
