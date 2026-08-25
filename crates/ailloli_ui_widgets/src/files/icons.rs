//! File and directory icon selection backed by the Devicons font catalogue.

use ailloli_ui_core::{Color, IconId};
#[cfg(test)]
use ailloli_ui_devicons_font::GENERIC_FILE_GLYPH;
use ailloli_ui_devicons_font::{glyph_or_fallback, FOLDER_GLYPH};
use ailloli_ui_fs::{FileEntry, FileKind};
use devicons::FileIcon as DeviconsFileIcon;

/// Icon identifier and optional recommended foreground tint.
///
/// `None` leaves tint choice to the caller. The helpers in this module always
/// return `Some`: amber for directories, green for directory symlinks, the
/// Devicons color for recognized files, or light gray for fallbacks.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, IconId};
/// use ailloli_ui_widgets::files::FileIconVisual;
/// let visual = FileIconVisual::new(IconId::Plus, Some(Color::hex_rgb(0xffffff)));
/// assert_eq!(visual.icon, IconId::Plus);
/// assert!(visual.color.is_some());
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct FileIconVisual {
    /// Glyph source to paint.
    pub icon: IconId,
    /// Optional caller-overridable tint.
    pub color: Option<Color>,
}

impl FileIconVisual {
    /// Stores an icon and tint without normalization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::files::FileIconVisual;
    /// let visual = FileIconVisual::new(IconId::Close, None);
    /// assert_eq!(visual.color, None);
    /// ```
    pub fn new(icon: IconId, color: Option<Color>) -> Self {
        Self { icon, color }
    }
}

/// Chooses an icon from an entry's kind, symlink target, and name.
///
/// Directory-like symlinks receive the folder glyph; other symlinks use their
/// filename just like files. The recommended tint is discarded.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::IconId;
/// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
/// use ailloli_ui_widgets::files::file_icon_for_entry;
/// let entry = FileEntry::new(FileUri::parse("file:///repo/src")?, FileMetadata::new(FileKind::Directory));
/// assert!(matches!(file_icon_for_entry(&entry), IconId::Devicon(_)));
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
pub fn file_icon_for_entry(entry: &FileEntry) -> IconId {
    file_icon_visual_for_entry(entry).icon
}

/// Chooses a Devicons glyph for a filename, with a generic-file fallback.
///
/// Matching follows the upstream Devicons mapping and is based on the supplied
/// name only; no filesystem access occurs. The recommended tint is discarded.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::IconId;
/// use ailloli_ui_widgets::files::file_icon_for_name;
/// assert!(matches!(file_icon_for_name("main.rs"), IconId::Devicon(_)));
/// assert!(matches!(file_icon_for_name("unknown"), IconId::Devicon(_)));
/// ```
pub fn file_icon_for_name(name: &str) -> IconId {
    file_icon_visual_for_name(name).icon
}

/// Chooses both icon and recommended tint for a filesystem entry.
///
/// Real directories use amber (`#f59e0b`); symlinks known to target a
/// directory use green (`#22c55e`). All other kinds use filename mapping.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Color;
/// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
/// use ailloli_ui_widgets::files::file_icon_visual_for_entry;
/// let entry = FileEntry::new(FileUri::parse("file:///repo/src")?, FileMetadata::new(FileKind::Directory));
/// assert_eq!(file_icon_visual_for_entry(&entry).color, Some(Color::hex_rgb(0xf59e0b)));
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
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

/// Chooses a filename glyph and its Devicons or fallback tint.
///
/// Missing source-font glyphs, wildcard mappings, and invalid upstream color
/// strings fall back to the generic glyph/color (`#e5e7eb`).
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Color;
/// use ailloli_ui_widgets::files::file_icon_visual_for_name;
/// assert_eq!(file_icon_visual_for_name("unknown").color, Some(Color::hex_rgb(0xe5e7eb)));
/// ```
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

/// Parses an upstream Devicons color or returns the generic fallback color.
fn parse_devicon_color(hex: &str) -> Color {
    Color::hex(hex).unwrap_or_else(|_| default_icon_color())
}

/// Returns the fixed amber tint for real directories.
fn folder_color() -> Color {
    Color::hex_rgb(0xf59e0b)
}

/// Returns the fixed green tint for symlinks to directories.
fn symlink_folder_color() -> Color {
    Color::hex_rgb(0x22c55e)
}

/// Returns the light-gray tint for generic-file fallbacks.
fn default_icon_color() -> Color {
    Color::hex_rgb(0xe5e7eb)
}

#[cfg(test)]
/// Verifies known extensions plus directory, symlink, color, and glyph fallbacks.
mod tests {
    use super::*;
    use ailloli_ui_core::IconId;

    #[test]
    fn maps_supported_extensions_to_devicons_and_fallbacks() {
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
