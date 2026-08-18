use std::fmt;
use std::path::{Path, PathBuf};

use crate::FileError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FileUri {
    scheme: String,
    authority: Option<String>,
    path: String,
}

impl FileUri {
    pub fn parse(input: impl AsRef<str>) -> Result<Self, FileError> {
        let input = input.as_ref().trim();
        if input.is_empty() {
            return Err(FileError::InvalidUri("empty uri".into()));
        }
        if input.contains('?') || input.contains('#') {
            return Err(FileError::InvalidUri(
                "query and fragment are not supported in v1".into(),
            ));
        }
        let Some((scheme, rest)) = input.split_once("://") else {
            return Err(FileError::InvalidUri(input.into()));
        };
        let scheme = normalize_scheme(scheme)?;
        let (authority, path) = split_authority_and_path(rest)?;
        if scheme == "file" && path == "/" {
            return Err(FileError::InvalidUri("file uri path is empty".into()));
        }
        Ok(Self {
            scheme,
            authority,
            path: normalize_path(path),
        })
    }

    pub fn local(path: impl Into<PathBuf>) -> Result<Self, FileError> {
        let path = path.into();
        if !path.is_absolute() {
            return Err(FileError::InvalidUri(format!(
                "local file uri requires absolute path: {}",
                path.display()
            )));
        }
        let path = path_to_uri_path(&path)?;
        Ok(Self {
            scheme: "file".into(),
            authority: None,
            path,
        })
    }

    pub fn new(
        scheme: impl Into<String>,
        authority: Option<impl Into<String>>,
        path: impl Into<String>,
    ) -> Result<Self, FileError> {
        let scheme = normalize_scheme(&scheme.into())?;
        let authority = authority.map(Into::into).filter(|value| !value.is_empty());
        let path = normalize_path(path.into());
        if path.is_empty() || !path.starts_with('/') {
            return Err(FileError::InvalidUri("uri path must be absolute".into()));
        }
        Ok(Self {
            scheme,
            authority,
            path,
        })
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    pub fn authority(&self) -> Option<&str> {
        self.authority.as_deref()
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn file_name(&self) -> Option<&str> {
        self.path.rsplit('/').find(|segment| !segment.is_empty())
    }

    pub fn file_name_decoded(&self) -> Option<String> {
        self.file_name().map(percent_decode_lossy)
    }

    pub fn parent(&self) -> Option<Self> {
        let trimmed = self.path.trim_end_matches('/');
        let idx = trimmed.rfind('/')?;
        if idx == 0 {
            return None;
        }
        Self::new(self.scheme.clone(), self.authority.clone(), &trimmed[..idx]).ok()
    }

    pub fn join_child(&self, name: impl AsRef<str>) -> Result<Self, FileError> {
        let name = name.as_ref().trim();
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            return Err(FileError::InvalidUri(format!("invalid child name: {name}")));
        }
        let base = self.path.trim_end_matches('/');
        Self::new(
            self.scheme.clone(),
            self.authority.clone(),
            format!("{base}/{}", percent_encode_path(name)),
        )
    }

    pub fn with_file_name(&self, name: impl AsRef<str>) -> Result<Self, FileError> {
        let Some(parent) = self.parent() else {
            return Err(FileError::InvalidUri(format!("uri has no parent: {self}")));
        };
        parent.join_child(name)
    }

    pub fn relative_path_from(&self, root: &Self) -> Option<String> {
        if self.scheme != root.scheme || self.authority != root.authority {
            return None;
        }
        let root_path = root.path.trim_end_matches('/');
        let path = self.path.as_str();
        if path == root_path {
            return Some(".".to_string());
        }
        let prefix = format!("{root_path}/");
        let relative = path.strip_prefix(&prefix)?;
        Some(percent_decode_lossy(relative))
    }

    pub fn to_local_path(&self) -> Result<PathBuf, FileError> {
        if self.scheme != "file" {
            return Err(FileError::UnsupportedScheme(self.scheme.clone()));
        }
        if self
            .authority
            .as_deref()
            .is_some_and(|value| value != "localhost")
        {
            return Err(FileError::UnsupportedScheme(format!(
                "file authority {:?}",
                self.authority
            )));
        }
        Ok(PathBuf::from(percent_decode_lossy(&self.path)))
    }
}

impl fmt::Display for FileUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://", self.scheme)?;
        if let Some(authority) = &self.authority {
            write!(f, "{authority}")?;
        }
        write!(f, "{}", self.path)
    }
}

impl std::str::FromStr for FileUri {
    type Err = FileError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

fn normalize_scheme(scheme: &str) -> Result<String, FileError> {
    let scheme = scheme.trim().to_ascii_lowercase();
    let valid = !scheme.is_empty()
        && scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'));
    if valid {
        Ok(scheme)
    } else {
        Err(FileError::InvalidUri(format!("invalid scheme: {scheme}")))
    }
}

fn split_authority_and_path(rest: &str) -> Result<(Option<String>, String), FileError> {
    if rest.is_empty() {
        return Err(FileError::InvalidUri("uri target is empty".into()));
    }
    if rest.starts_with('/') {
        return Ok((None, rest.to_string()));
    }
    let Some((authority, path)) = rest.split_once('/') else {
        return Err(FileError::InvalidUri("uri path is empty".into()));
    };
    if authority.is_empty() {
        return Err(FileError::InvalidUri("uri authority is empty".into()));
    }
    Ok((Some(authority.to_string()), format!("/{path}")))
}

fn normalize_path(path: impl Into<String>) -> String {
    let mut path = path.into().replace('\\', "/");
    while path.contains("//") {
        path = path.replace("//", "/");
    }
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    path
}

fn path_to_uri_path(path: &Path) -> Result<String, FileError> {
    let Some(path) = path.to_str() else {
        return Err(FileError::InvalidUri(
            "local path is not valid UTF-8".into(),
        ));
    };
    Ok(normalize_path(percent_encode_path(path)))
}

fn percent_encode_path(path: &str) -> String {
    path.replace('%', "%25").replace(' ', "%20")
}

fn percent_decode_lossy(path: &str) -> String {
    path.replace("%20", " ").replace("%25", "%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_file_and_remote_uris() {
        let file = FileUri::parse("file:///tmp/a.rs").expect("file uri");
        assert_eq!(file.scheme(), "file");
        assert_eq!(file.authority(), None);
        assert_eq!(file.path(), "/tmp/a.rs");
        assert_eq!(file.to_string(), "file:///tmp/a.rs");

        let sftp = FileUri::parse("sftp://host/var/www").expect("sftp uri");
        assert_eq!(sftp.scheme(), "sftp");
        assert_eq!(sftp.authority(), Some("host"));
        assert_eq!(sftp.path(), "/var/www");

        let ftp = FileUri::parse("ftp://host/path").expect("ftp uri");
        assert_eq!(ftp.scheme(), "ftp");
        assert_eq!(ftp.authority(), Some("host"));
        assert_eq!(ftp.path(), "/path");
    }

    #[test]
    fn rejects_invalid_v1_uris() {
        assert!(matches!(FileUri::parse(""), Err(FileError::InvalidUri(_))));
        assert!(matches!(
            FileUri::parse("file:///tmp/a.rs?download=1"),
            Err(FileError::InvalidUri(_))
        ));
        assert!(matches!(
            FileUri::parse("file://"),
            Err(FileError::InvalidUri(_))
        ));
        assert!(matches!(
            FileUri::parse("file:///"),
            Err(FileError::InvalidUri(_))
        ));
    }

    #[test]
    fn builds_local_file_uri() {
        let uri = FileUri::local("/tmp/octav ui.rs").expect("local uri");
        assert_eq!(uri.scheme(), "file");
        assert_eq!(uri.path(), "/tmp/octav%20ui.rs");
        assert_eq!(
            uri.to_local_path().expect("path"),
            PathBuf::from("/tmp/octav ui.rs")
        );
    }

    #[test]
    fn uri_parent_join_rename_and_relative_paths() {
        let root = FileUri::parse("file:///tmp/octav%20ui").expect("root");
        let file = root.join_child("main file.rs").expect("child");
        assert_eq!(file.to_string(), "file:///tmp/octav%20ui/main%20file.rs");
        assert_eq!(file.file_name_decoded().as_deref(), Some("main file.rs"));
        assert_eq!(file.parent(), Some(root.clone()));
        assert_eq!(
            file.with_file_name("lib.rs").expect("rename").to_string(),
            "file:///tmp/octav%20ui/lib.rs"
        );
        assert_eq!(
            file.relative_path_from(&root),
            Some("main file.rs".to_string())
        );
        assert_eq!(root.relative_path_from(&root), Some(".".to_string()));
        assert!(FileUri::parse("file:///tmp/other")
            .expect("other")
            .relative_path_from(&root)
            .is_none());
    }
}
