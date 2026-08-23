//! Pure upload/dropzone metadata and accept matching.

use std::path::{Path, PathBuf};

/// Metadata describing one file offered to an upload control.
///
/// The value does not grant filesystem access and does not read the file.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::UploadFile;
///
/// let file = UploadFile::named("report.pdf");
/// assert_eq!(file.name, "report.pdf");
/// assert_eq!(file.size, None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadFile {
    /// Local path when the producer is allowed to expose one; `None` otherwise.
    pub path: Option<PathBuf>,
    /// Display name used for extension matching; it may be empty.
    pub name: String,
    /// File size in bytes when known; `None` means unavailable, not zero bytes.
    pub size: Option<u64>,
    /// Unverified MIME type hint when known; matching is ASCII case-insensitive.
    pub mime_hint: Option<String>,
}

/// Normalized extension and MIME patterns accepted by an upload control.
///
/// Supported forms are `.ext`, an exact MIME type such as `image/png`, and a
/// top-level wildcard such as `image/*`. An empty pattern set accepts all files.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::UploadAccept;
///
/// let accept = UploadAccept::new([".png", "image/*"]);
/// assert_eq!(accept.patterns(), [".png", "image/*"]);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UploadAccept {
    /// Ordered extension or MIME-style patterns accepted by the target.
    patterns: Vec<String>,
}

impl UploadFile {
    /// Creates pathless metadata with unknown size and MIME type.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::UploadFile;
    ///
    /// let file = UploadFile::named("photo.png");
    /// assert!(file.path.is_none());
    /// ```
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            path: None,
            name: name.into(),
            size: None,
            mime_hint: None,
        }
    }

    /// Creates metadata from a path without reading its target.
    ///
    /// [`Self::name`] is the final UTF-8 path component, or an empty string
    /// when that component is absent or not valid UTF-8. Size and MIME type
    /// remain unknown.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use ailloli_ui_core::UploadFile;
    ///
    /// let file = UploadFile::from_path(PathBuf::from("images/photo.png"));
    /// assert_eq!(file.name, "photo.png");
    /// ```
    pub fn from_path(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        Self {
            path: Some(path),
            name,
            size: None,
            mime_hint: None,
        }
    }
}

impl UploadAccept {
    /// Normalizes and stores the supplied patterns in input order.
    ///
    /// Patterns are trimmed, converted to lowercase, and empty entries are
    /// discarded. Duplicates and unsupported forms are preserved but do not
    /// gain special matching behavior.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::UploadAccept;
    ///
    /// let accept = UploadAccept::new([" .PNG ", ""]);
    /// assert_eq!(accept.patterns(), [".png"]);
    /// ```
    pub fn new(patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            patterns: patterns
                .into_iter()
                .map(|p| p.into().trim().to_ascii_lowercase())
                .filter(|p| !p.is_empty())
                .collect(),
        }
    }

    /// Returns `true` when there are no effective restrictions.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::UploadAccept;
    ///
    /// assert!(UploadAccept::default().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Returns normalized patterns in their original relative order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::UploadAccept;
    ///
    /// assert_eq!(UploadAccept::new([".PNG"]).patterns(), [".png"]);
    /// ```
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    /// Returns whether at least one pattern matches `file`.
    ///
    /// Extension patterns use [`UploadFile::name`]; MIME patterns require a
    /// [`UploadFile::mime_hint`]. With no patterns this returns `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{UploadAccept, UploadFile};
    ///
    /// let images = UploadAccept::new([".png", "image/*"]);
    /// assert!(images.accepts(&UploadFile::named("avatar.PNG")));
    /// assert!(!images.accepts(&UploadFile::named("notes.txt")));
    /// ```
    pub fn accepts(&self, file: &UploadFile) -> bool {
        if self.patterns.is_empty() {
            return true;
        }
        self.patterns.iter().any(|pattern| {
            if let Some(ext) = pattern.strip_prefix('.') {
                extension_matches(&file.name, ext)
            } else if pattern.ends_with("/*") {
                let prefix = pattern.trim_end_matches('*');
                file.mime_hint
                    .as_deref()
                    .is_some_and(|mime| mime.to_ascii_lowercase().starts_with(prefix))
            } else {
                file.mime_hint
                    .as_deref()
                    .is_some_and(|mime| mime.eq_ignore_ascii_case(pattern))
            }
        })
    }
}

/// Matches the final UTF-8 extension without its leading dot.
fn extension_matches(name: &str, ext: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|actual| actual.eq_ignore_ascii_case(ext))
}

#[cfg(test)]
mod tests {
    //! Covers extension, wildcard MIME, and exact MIME acceptance.

    use super::*;

    #[test]
    fn accepts_extension_and_mime_patterns() {
        let accept = UploadAccept::new([".png", "image/*"]);
        let mut png = UploadFile::named("avatar.PNG");
        assert!(accept.accepts(&png));
        png.name = "avatar.webp".into();
        png.mime_hint = Some("image/webp".into());
        assert!(accept.accepts(&png));
        png.mime_hint = Some("application/pdf".into());
        assert!(!accept.accepts(&png));
    }
}
