//! Pure upload/dropzone metadata and accept matching.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadFile {
    pub path: Option<PathBuf>,
    pub name: String,
    pub size: Option<u64>,
    pub mime_hint: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UploadAccept {
    patterns: Vec<String>,
}

impl UploadFile {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            path: None,
            name: name.into(),
            size: None,
            mime_hint: None,
        }
    }

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
    pub fn new(patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            patterns: patterns
                .into_iter()
                .map(|p| p.into().trim().to_ascii_lowercase())
                .filter(|p| !p.is_empty())
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

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

fn extension_matches(name: &str, ext: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|actual| actual.eq_ignore_ascii_case(ext))
}

#[cfg(test)]
mod tests {
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
