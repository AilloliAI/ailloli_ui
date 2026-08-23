//! Normalized, serializable identifiers for local and remote filesystem entries.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::FileError;

/// Version-one filesystem URI with a normalized scheme, optional authority, and path.
///
/// Paths use `/`, collapse repeated separators, and always start with `/`.
/// This is deliberately a small VFS syntax rather than a complete RFC URI
/// implementation: queries/fragments are unsupported, dot segments are not
/// resolved, and only `%` and spaces are encoded/decoded by path helpers.
/// Constructor methods establish these invariants, but derived deserialization
/// stores the three fields verbatim and can therefore produce an unnormalized
/// or otherwise invalid value. Only deserialize data from a trusted compatible
/// producer when later code relies on normalized paths.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// let uri = FileUri::parse("SFTP://host//srv/project")?;
/// assert_eq!(uri.to_string(), "sftp://host/srv/project");
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FileUri {
    /// Usually normalized lowercase scheme; deserialization can bypass validation.
    scheme: String,
    /// Optional case-sensitive provider authority.
    authority: Option<String>,
    /// Usually normalized absolute path; deserialization can bypass validation.
    path: String,
}

impl FileUri {
    /// Parses and normalizes a version-one filesystem URI.
    ///
    /// Outer whitespace is trimmed and the scheme is lowercased. The input must
    /// contain `://`, a non-empty target, and no literal `?` or `#`. Backslashes
    /// in the path become `/` and repeated slashes collapse. Authority text and
    /// existing percent escapes are otherwise retained verbatim.
    ///
    /// # Errors
    ///
    /// Returns [`FileError::InvalidUri`] for empty input, a missing/invalid
    /// scheme or path, a missing path after an authority, or a query/fragment.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// let uri = FileUri::parse(" file:///tmp/a%20b ")?;
    /// assert_eq!(uri.path(), "/tmp/a%20b");
    /// assert!(FileUri::parse("file:///tmp/a?raw=1").is_err());
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
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
        Ok(Self {
            scheme,
            authority,
            path: normalize_path(path),
        })
    }

    /// Converts an absolute local path into an authority-free `file` URI.
    ///
    /// The path must be valid UTF-8. Percent signs are encoded as `%25`, spaces
    /// as `%20`, and platform separators normalize to `/`; other URI-reserved
    /// characters are left verbatim. Relative paths are rejected.
    ///
    /// # Errors
    ///
    /// Returns [`FileError::InvalidUri`] for a relative or non-UTF-8 path.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// let uri = FileUri::local("/tmp/a b")?;
    /// assert_eq!(uri.to_string(), "file:///tmp/a%20b");
    /// assert!(FileUri::local("relative/path").is_err());
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
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

    /// Builds a URI from components using the crate's normalization rules.
    ///
    /// An empty authority becomes `None`. Relative path text is accepted and
    /// receives a leading `/`; backslashes and repeated separators normalize.
    /// Existing percent escapes, dot segments, `?`, and `#` are not validated.
    ///
    /// # Errors
    ///
    /// Returns [`FileError::InvalidUri`] only when the normalized scheme is
    /// empty or contains a character outside ASCII alphanumeric, `+`, `-`, `.`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// let uri = FileUri::new("SFTP", Some("host"), "srv\\project")?;
    /// assert_eq!(uri.to_string(), "sftp://host/srv/project");
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
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

    /// Returns the normalized lowercase scheme.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// assert_eq!(FileUri::parse("FILE:///")?.scheme(), "file");
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// Returns the authority, or `None` for authority-free URIs.
    ///
    /// Authority comparison is case-sensitive and no host normalization occurs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// assert_eq!(FileUri::parse("sftp://host/home")?.authority(), Some("host"));
    /// assert_eq!(FileUri::parse("file:///tmp")?.authority(), None);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn authority(&self) -> Option<&str> {
        self.authority.as_deref()
    }

    /// Returns the normalized, absolute, still-percent-encoded path.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// assert_eq!(FileUri::parse("file:////tmp//a")?.path(), "/tmp/a");
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the final non-empty encoded path segment.
    ///
    /// Root returns `None`; a trailing slash is ignored. No allocation or
    /// percent decoding occurs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// assert_eq!(FileUri::parse("file:///tmp/a%20b/")?.file_name(), Some("a%20b"));
    /// assert_eq!(FileUri::parse("file:///")?.file_name(), None);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn file_name(&self) -> Option<&str> {
        self.path.rsplit('/').find(|segment| !segment.is_empty())
    }

    /// Returns an allocated, partially percent-decoded final segment.
    ///
    /// Decoding replaces uppercase `%20` with spaces and `%25` with `%`, in
    /// that order. Other escapes, lowercase forms, and malformed escapes remain
    /// unchanged. Root returns `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// assert_eq!(FileUri::parse("file:///a%20b")?.file_name_decoded().as_deref(), Some("a b"));
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn file_name_decoded(&self) -> Option<String> {
        self.file_name().map(percent_decode_lossy)
    }

    /// Returns the normalized lexical parent while preserving scheme/authority.
    ///
    /// Files directly below root return the root URI; root itself returns
    /// `None`. Dot segments are ordinary text and are not resolved.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// let uri = FileUri::parse("file:///tmp/a")?;
    /// assert_eq!(uri.parent().unwrap().to_string(), "file:///tmp");
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn parent(&self) -> Option<Self> {
        let trimmed = self.path.trim_end_matches('/');
        let idx = trimmed.rfind('/')?;
        if idx == 0 {
            return Self::new(self.scheme.clone(), self.authority.clone(), "/").ok();
        }
        Self::new(self.scheme.clone(), self.authority.clone(), &trimmed[..idx]).ok()
    }

    /// Appends one trimmed child name and encodes percent signs and spaces.
    ///
    /// Empty names and names containing `/` or `\` are rejected. Other URI
    /// delimiters and dot names are accepted verbatim, so a child containing
    /// `?` or `#` may not round-trip through [`Self::parse`].
    ///
    /// # Errors
    ///
    /// Returns [`FileError::InvalidUri`] for an empty or multi-segment name, or
    /// if rebuilding the URI fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// let root = FileUri::parse("file:///tmp")?;
    /// assert_eq!(root.join_child(" a b ")?.to_string(), "file:///tmp/a%20b");
    /// assert!(root.join_child("a/b").is_err());
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
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

    /// Replaces the final segment using [`Self::join_child`] name rules.
    ///
    /// # Errors
    ///
    /// Returns [`FileError::InvalidUri`] when this URI is root or the new name
    /// is empty or contains a path separator.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// let uri = FileUri::parse("file:///tmp/a")?;
    /// assert_eq!(uri.with_file_name("b")?.to_string(), "file:///tmp/b");
    /// assert!(FileUri::parse("file:///")?.with_file_name("b").is_err());
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn with_file_name(&self, name: impl AsRef<str>) -> Result<Self, FileError> {
        let Some(parent) = self.parent() else {
            return Err(FileError::InvalidUri(format!("uri has no parent: {self}")));
        };
        parent.join_child(name)
    }

    /// Returns a decoded lexical path relative to `root` when compatible.
    ///
    /// Scheme and authority must match exactly, and `self` must equal or be
    /// below the root on a path-segment boundary. Equal non-root paths return
    /// `"."`; equal root or trailing-slash paths return an empty string due to
    /// normalization semantics. The result uses the same limited decoding as
    /// [`Self::file_name_decoded`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// let root = FileUri::parse("file:///tmp/work")?;
    /// let child = FileUri::parse("file:///tmp/work/a%20b")?;
    /// assert_eq!(child.relative_path_from(&root).as_deref(), Some("a b"));
    /// assert_eq!(root.relative_path_from(&root).as_deref(), Some("."));
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
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

    /// Converts an authority-free or `localhost` `file` URI to a local path.
    ///
    /// Authority matching is case-sensitive. Percent decoding is intentionally
    /// limited to uppercase `%20` and `%25`; no existence check is performed.
    ///
    /// # Errors
    ///
    /// Returns [`FileError::UnsupportedScheme`] for non-`file` schemes or a
    /// non-empty authority other than exact `localhost`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use std::path::PathBuf;
    /// assert_eq!(FileUri::parse("file://localhost/tmp/a%20b")?.to_local_path()?, PathBuf::from("/tmp/a b"));
    /// assert!(FileUri::parse("sftp://host/tmp")?.to_local_path().is_err());
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
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

/// Trims/lowercases a scheme and accepts only ASCII alphanumeric, `+`, `-`, `.`.
///
/// # Errors
///
/// Returns [`FileError::InvalidUri`] when the trimmed scheme is empty or
/// contains any other character.
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

/// Splits the post-scheme target into optional authority and absolute path text.
///
/// # Errors
///
/// Returns [`FileError::InvalidUri`] for an empty target, a remote target with
/// no slash-delimited path, or an empty authority.
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

/// Converts separators, collapses repeats, and ensures one leading slash.
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

/// Converts one UTF-8 platform path to the crate's minimally encoded URI path.
///
/// # Errors
///
/// Returns [`FileError::InvalidUri`] when `path` is not valid UTF-8.
fn path_to_uri_path(path: &Path) -> Result<String, FileError> {
    let Some(path) = path.to_str() else {
        return Err(FileError::InvalidUri(
            "local path is not valid UTF-8".into(),
        ));
    };
    Ok(normalize_path(percent_encode_path(path)))
}

/// Encodes percent signs first and spaces second, leaving other characters unchanged.
fn percent_encode_path(path: &str) -> String {
    path.replace('%', "%25").replace(' ', "%20")
}

/// Decodes uppercase `%20` then `%25` without validating other escapes.
fn percent_decode_lossy(path: &str) -> String {
    path.replace("%20", " ").replace("%25", "%")
}

#[cfg(test)]
mod tests {
    //! Covers v1 parsing, invalid syntax, local conversion, and lexical path helpers.

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
        let root = FileUri::parse("file:///").expect("filesystem root");
        assert_eq!(root.path(), "/");
        assert_eq!(root.parent(), None);
        assert_eq!(root.join_child("bin").unwrap().to_string(), "file:///bin");
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
            FileUri::parse("file:///bin").unwrap().parent(),
            Some(FileUri::parse("file:///").unwrap())
        );
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
