use ailloli_ui_core::{Color, IconId};
#[cfg(test)]
use ailloli_ui_devicons_font::GENERIC_FILE_GLYPH;
use ailloli_ui_devicons_font::{glyph_or_fallback, FOLDER_GLYPH};
use ailloli_ui_fs::{FileEntry, FileKind};
use devicons::FileIcon as DeviconsFileIcon;

#[derive(Clone, Debug, PartialEq)]
pub struct FileIconVisual {
    pub icon: IconId,
    pub color: Option<Color>,
}

impl FileIconVisual {
    pub fn new(icon: IconId, color: Option<Color>) -> Self {
        Self { icon, color }
    }
}

pub fn file_icon_for_entry(entry: &FileEntry) -> IconId {
    file_icon_visual_for_entry(entry).icon
}

pub fn file_icon_for_name(name: &str) -> IconId {
    file_icon_visual_for_name(name).icon
}

pub fn file_icon_visual_for_entry(entry: &FileEntry) -> FileIconVisual {
    match (entry.metadata.kind, entry.metadata.symlink_target_kind) {
        (FileKind::Symlink, Some(FileKind::Directory)) => {
            FileIconVisual::new(IconId::Devicon(FOLDER_GLYPH), Some(symlink_folder_color()))
        }
        (FileKind::Directory, _) => {
            FileIconVisual::new(IconId::Devicon(FOLDER_GLYPH), Some(folder_color()))
        }
        (FileKind::File | FileKind::Symlink | FileKind::Other, _) => {
            file_icon_visual_for_name(&entry.name)
        }
    }
}

pub fn file_icon_visual_for_name(name: &str) -> FileIconVisual {
    let icon = DeviconsFileIcon::from(name);
    let glyph = glyph_or_fallback(icon.icon);
    let color = if icon.icon == '*' || glyph != icon.icon {
        default_icon_color()
    } else {
        parse_devicon_color(icon.color)
    };
    FileIconVisual::new(IconId::Devicon(glyph), Some(color))
}

fn parse_devicon_color(hex: &str) -> Color {
    Color::hex(hex).unwrap_or_else(|_| default_icon_color())
}

fn folder_color() -> Color {
    Color::hex_rgb(0xf59e0b)
}

fn symlink_folder_color() -> Color {
    Color::hex_rgb(0x22c55e)
}

fn default_icon_color() -> Color {
    Color::hex_rgb(0xe5e7eb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailloli_ui_core::IconId;

    #[test]
    fn maps_mvp_extensions_to_devicons_and_fallbacks() {
        assert_eq!(file_icon_for_name("main.rs"), IconId::Devicon('\u{e68b}'));
        assert_eq!(
            file_icon_for_name("Cargo.toml"),
            IconId::Devicon('\u{e6b2}')
        );
        assert_eq!(file_icon_for_name("README.md"), IconId::Devicon('\u{f48a}'));
        assert_eq!(
            file_icon_for_name("config.json"),
            IconId::Devicon('\u{e60b}')
        );
        assert_eq!(file_icon_for_name("app.js"), IconId::Devicon('\u{e60c}'));
        assert_eq!(file_icon_for_name("app.ts"), IconId::Devicon('\u{e628}'));
        assert_eq!(
            file_icon_for_name("index.html"),
            IconId::Devicon('\u{e736}')
        );
        assert_eq!(file_icon_for_name("style.css"), IconId::Devicon('\u{e749}'));
        assert_eq!(file_icon_for_name("unknown"), IconId::Devicon('\u{f15b}'));
    }

    #[test]
    fn folders_use_nerd_font_folder_with_orange_tint() {
        let entry = FileEntry {
            uri: ailloli_ui_fs::FileUri::parse("file:///repo/src").expect("uri"),
            name: "src".to_string(),
            metadata: ailloli_ui_fs::FileMetadata::new(FileKind::Directory),
        };
        let visual = file_icon_visual_for_entry(&entry);
        assert_eq!(visual.icon, IconId::Devicon(FOLDER_GLYPH));
        assert_eq!(visual.color, Some(Color::hex_rgb(0xf59e0b)));
    }

    #[test]
    fn symlink_directories_use_folder_with_green_tint() {
        let mut metadata = ailloli_ui_fs::FileMetadata::new(FileKind::Symlink);
        metadata.symlink_target_kind = Some(FileKind::Directory);
        let entry = FileEntry {
            uri: ailloli_ui_fs::FileUri::parse("file:///repo/bin").expect("uri"),
            name: "bin".to_string(),
            metadata,
        };

        let visual = file_icon_visual_for_entry(&entry);

        assert_eq!(visual.icon, IconId::Devicon(FOLDER_GLYPH));
        assert_eq!(visual.color, Some(Color::hex_rgb(0x22c55e)));
    }

    #[test]
    fn file_visuals_include_devicons_color() {
        let rust = file_icon_visual_for_name("main.rs");
        assert_eq!(rust.icon, IconId::Devicon('\u{e68b}'));
        assert_eq!(rust.color, Color::hex("#dea584").ok());

        let unknown = file_icon_visual_for_name("file.unknownext");
        assert_eq!(unknown.icon, IconId::Devicon('\u{f15b}'));
        assert_eq!(unknown.color, Some(Color::hex_rgb(0xe5e7eb)));
    }

    #[test]
    fn font_logos_and_source_missing_glyphs_use_the_generic_file_fallback() {
        for name in ["PKGBUILD", "shell.nix", "vercel.json"] {
            let visual = file_icon_visual_for_name(name);
            assert_eq!(visual.icon, IconId::Devicon(GENERIC_FILE_GLYPH));
            assert_eq!(visual.color, Some(Color::hex_rgb(0xe5e7eb)));
        }
    }
}
