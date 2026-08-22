//! Local filesystem provider and native non-recursive watcher for `ailloli_ui_fs`.
//!
//! [`LocalFileProvider`] performs synchronous host filesystem calls. The
//! exported source factory creates the worker-owned watcher used by
//! `ailloli_ui_fs_runtime`; neither layer adds sandboxing or path authorization.
//!
//! # Examples
//!
//! ```
//! use ailloli_ui_fs::{FileProvider, FileUri};
//! use ailloli_ui_fs_local::LocalFileProvider;
//! let provider = LocalFileProvider::new();
//! let remote = FileUri::parse("sftp://host/file.txt")?;
//! assert!(provider.read_file(&remote).is_err());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::fs;
use std::path::{Path, PathBuf};

/// Worker-owned native-watch source implementation.
mod source;

pub use source::{
    LocalFileTreeSource, LocalFileTreeSourceFactory, DEFAULT_LOCAL_FILE_TREE_MAX_WATCHERS,
};

use ailloli_ui_fs::{
    FileCapabilities, FileEntry, FileError, FileKind, FileMetadata, FileProvider, FileUri,
};

/// Stateless synchronous provider for `file:` URIs on the host filesystem.
///
/// Paths are neither confined nor normalized before access. Methods inherit OS
/// permissions, follow symlinks except where explicitly documented, and may
/// block the calling thread; use `LocalFileTreeSourceFactory` with the runtime
/// worker for retained file-tree operations.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileProvider;
/// use ailloli_ui_fs_local::LocalFileProvider;
/// let provider = LocalFileProvider::new();
/// assert!(provider.capabilities().read);
/// assert!(provider.capabilities().watch);
/// ```
#[derive(Debug, Clone, Default)]
pub struct LocalFileProvider;

/// Constructs the stateless local provider.
impl LocalFileProvider {
    /// Returns a zero-sized provider with no filesystem access performed yet.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_local::LocalFileProvider;
    /// let provider = LocalFileProvider::new();
    /// assert_eq!(std::mem::size_of_val(&provider), 0);
    /// ```
    pub fn new() -> Self {
        Self
    }
}

/// Maps the generic provider contract to synchronous `std::fs` operations.
impl FileProvider for LocalFileProvider {
    /// Advertises read/write plus native watch support.
    fn capabilities(&self) -> FileCapabilities {
        FileCapabilities {
            watch: true,
            ..FileCapabilities::READ_WRITE
        }
    }

    /// Reads one directory level and sorts by name then URI path.
    ///
    /// An empty directory returns an empty vector. Per-entry metadata failures
    /// abort the complete read rather than returning a partial list.
    fn read_dir(&self, uri: &FileUri) -> Result<Vec<FileEntry>, FileError> {
        let path = local_path(uri)?;
        let entries = fs::read_dir(&path)
            .map_err(|err| FileError::from_io(&err, path.display().to_string()))?;
        let mut entries = entries
            .map(|entry| {
                let entry =
                    entry.map_err(|err| FileError::from_io(&err, path.display().to_string()))?;
                let entry_path = entry.path();
                let uri = FileUri::local(&entry_path)?;
                let metadata = metadata_for_path(&entry_path)?;
                Ok(FileEntry::new(uri, metadata))
            })
            .collect::<Result<Vec<_>, FileError>>()?;
        entries.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then_with(|| a.uri.path().cmp(b.uri.path()))
        });
        Ok(entries)
    }

    /// Reads the complete file into an owned byte vector.
    fn read_file(&self, uri: &FileUri) -> Result<Vec<u8>, FileError> {
        let path = local_path(uri)?;
        fs::read(&path).map_err(|err| FileError::from_io(&err, path.display().to_string()))
    }

    /// Creates or truncates a file and writes `bytes` non-atomically.
    ///
    /// Empty bytes create an empty file. Missing parents are not created.
    fn write_file(&self, uri: &FileUri, bytes: &[u8]) -> Result<(), FileError> {
        let path = local_path(uri)?;
        fs::write(&path, bytes).map_err(|err| FileError::from_io(&err, path.display().to_string()))
    }

    /// Returns symlink-aware metadata without following the link for `kind`.
    fn metadata(&self, uri: &FileUri) -> Result<FileMetadata, FileError> {
        metadata_for_path(&local_path(uri)?)
    }

    /// Resolves the OS canonical path, returning `None` for any failure.
    ///
    /// This deliberately collapses missing paths, permission failures, and
    /// other canonicalization errors to absence after URI validation succeeds.
    fn canonical_uri(&self, uri: &FileUri) -> Result<Option<FileUri>, FileError> {
        let path = local_path(uri)?;
        match fs::canonicalize(&path) {
            Ok(canonical) => FileUri::local(canonical).map(Some),
            Err(_) => Ok(None),
        }
    }

    /// Creates exactly one directory; parents are not created recursively.
    fn create_dir(&self, uri: &FileUri) -> Result<(), FileError> {
        let path = local_path(uri)?;
        fs::create_dir(&path).map_err(|err| FileError::from_io(&err, path.display().to_string()))
    }

    /// Delegates an OS rename/move without cross-filesystem fallback.
    fn rename(&self, from: &FileUri, to: &FileUri) -> Result<(), FileError> {
        let from = local_path(from)?;
        let to = local_path(to)?;
        fs::rename(&from, &to).map_err(|err| {
            FileError::from_io(&err, format!("{} -> {}", from.display(), to.display()))
        })
    }

    /// Removes one file, symlink, or empty directory non-recursively.
    ///
    /// `symlink_metadata` ensures a link to a directory removes the link with
    /// `remove_file`, never the target directory.
    fn remove(&self, uri: &FileUri) -> Result<(), FileError> {
        let path = local_path(uri)?;
        let metadata = fs::symlink_metadata(&path)
            .map_err(|err| FileError::from_io(&err, path.display().to_string()))?;
        if metadata.is_dir() {
            fs::remove_dir(&path)
                .map_err(|err| FileError::from_io(&err, path.display().to_string()))
        } else {
            fs::remove_file(&path)
                .map_err(|err| FileError::from_io(&err, path.display().to_string()))
        }
    }
}

/// Converts only a local `file:` URI to its host path.
fn local_path(uri: &FileUri) -> Result<PathBuf, FileError> {
    uri.to_local_path()
}

/// Reads link metadata and best-effort target kind for one host path.
fn metadata_for_path(path: &Path) -> Result<FileMetadata, FileError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|err| FileError::from_io(&err, path.display().to_string()))?;
    let file_type = metadata.file_type();
    let kind = if file_type.is_symlink() {
        FileKind::Symlink
    } else if file_type.is_dir() {
        FileKind::Directory
    } else if file_type.is_file() {
        FileKind::File
    } else {
        FileKind::Other
    };
    let symlink_target_kind = if file_type.is_symlink() {
        fs::metadata(path)
            .ok()
            .map(|target_metadata| kind_for_target_type(target_metadata.file_type()))
    } else {
        None
    };
    Ok(FileMetadata {
        kind,
        symlink_target_kind,
        len: metadata.len(),
        readonly: metadata.permissions().readonly(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
    })
}

/// Classifies a followed symlink target as directory, file, or other.
fn kind_for_target_type(file_type: fs::FileType) -> FileKind {
    if file_type.is_dir() {
        FileKind::Directory
    } else if file_type.is_file() {
        FileKind::File
    } else {
        FileKind::Other
    }
}

#[cfg(test)]
/// Local I/O and Unix symlink regression tests.
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    /// Recoverable unique temporary directory for one test.
    struct TempDir {
        /// Host path removed recursively on drop.
        path: PathBuf,
    }

    /// Temporary path and URI helpers.
    impl TempDir {
        /// Creates a process/time-qualified directory in the OS temp root.
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "ailloli_ui_fs_local_{name}_{}_{}",
                std::process::id(),
                nanos
            ));
            fs::create_dir(&path).expect("temp dir");
            Self { path }
        }

        /// Converts the temporary root to a local file URI.
        fn uri(&self) -> FileUri {
            FileUri::local(&self.path).expect("temp uri")
        }

        /// Converts one direct child name to a local file URI.
        fn child_uri(&self, name: &str) -> FileUri {
            FileUri::local(self.path.join(name)).expect("child uri")
        }
    }

    /// Recursively removes the test directory on scope exit.
    impl Drop for TempDir {
        /// Performs best-effort cleanup without masking a test result.
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    /// Verifies deterministic name ordering and directory classification.
    fn reads_and_sorts_directory_entries() {
        let temp = TempDir::new("read_dir");
        fs::write(temp.path.join("b.txt"), b"b").expect("b");
        fs::write(temp.path.join("a.txt"), b"a").expect("a");
        fs::create_dir(temp.path.join("dir")).expect("dir");

        let provider = LocalFileProvider::new();
        let entries = provider.read_dir(&temp.uri()).expect("entries");
        let names = entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["a.txt", "b.txt", "dir"]);
        assert_eq!(entries[2].metadata.kind, FileKind::Directory);
    }

    #[test]
    /// Verifies byte preservation and exact file length metadata.
    fn roundtrips_file_bytes_and_metadata() {
        let temp = TempDir::new("roundtrip");
        let file = temp.child_uri("data.bin");
        let provider = LocalFileProvider::new();
        let payload = b"ailloli_ui";

        provider.write_file(&file, payload).expect("write");
        assert_eq!(provider.read_file(&file).expect("read"), payload);
        let metadata = provider.metadata(&file).expect("metadata");
        assert_eq!(metadata.kind, FileKind::File);
        assert_eq!(metadata.len, payload.len() as u64);
    }

    #[test]
    /// Verifies nonrecursive create, rename, and removal operations.
    fn creates_renames_and_removes_entries() {
        let temp = TempDir::new("ops");
        let provider = LocalFileProvider::new();
        let dir = temp.child_uri("created");
        let from = temp.child_uri("from.txt");
        let to = temp.child_uri("to.txt");

        provider.create_dir(&dir).expect("create dir");
        assert_eq!(
            provider.metadata(&dir).expect("dir metadata").kind,
            FileKind::Directory
        );
        provider.write_file(&from, b"x").expect("write");
        provider.rename(&from, &to).expect("rename");
        assert!(matches!(
            provider.read_file(&from),
            Err(FileError::NotFound(_))
        ));
        assert_eq!(provider.read_file(&to).expect("renamed"), b"x");
        provider.remove(&to).expect("remove file");
        provider.remove(&dir).expect("remove dir");
        assert!(matches!(
            provider.metadata(&to),
            Err(FileError::NotFound(_))
        ));
    }

    #[test]
    /// Verifies generic recursive copy, move, and removal through this provider.
    fn copy_move_recursive_entries_through_provider_api() {
        let temp = TempDir::new("copy_move_recursive");
        let provider = LocalFileProvider::new();
        fs::create_dir(temp.path.join("src")).expect("src");
        fs::create_dir(temp.path.join("src/nested")).expect("nested");
        fs::write(temp.path.join("src/main.rs"), b"main").expect("main");
        fs::write(temp.path.join("src/nested/lib.rs"), b"lib").expect("lib");

        let src = temp.child_uri("src");
        let copied = temp.child_uri("copied");
        provider.copy_entry(&src, &copied).expect("copy tree");
        assert_eq!(
            fs::read(temp.path.join("copied/main.rs")).expect("copied main"),
            b"main"
        );
        assert_eq!(
            fs::read(temp.path.join("copied/nested/lib.rs")).expect("copied lib"),
            b"lib"
        );

        let moved = temp.child_uri("moved");
        provider.move_entry(&copied, &moved).expect("move tree");
        assert!(!temp.path.join("copied").exists());
        assert_eq!(
            fs::read(temp.path.join("moved/nested/lib.rs")).expect("moved lib"),
            b"lib"
        );

        provider
            .remove_recursive(&moved)
            .expect("remove recursive tree");
        assert!(!temp.path.join("moved").exists());
    }

    #[test]
    /// Verifies scheme rejection and missing-file error classification.
    fn rejects_non_file_uri_and_reports_missing_file() {
        let provider = LocalFileProvider::new();
        let remote = FileUri::parse("sftp://host/tmp/a.txt").expect("remote");
        assert!(matches!(
            provider.read_file(&remote),
            Err(FileError::UnsupportedScheme(_))
        ));

        let temp = TempDir::new("missing");
        assert!(matches!(
            provider.read_file(&temp.child_uri("missing.txt")),
            Err(FileError::NotFound(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    /// Verifies directory symlink kind and followed target-kind metadata.
    fn symlink_to_directory_keeps_link_kind_and_records_target_directory() {
        let temp = TempDir::new("symlink_dir");
        fs::create_dir(temp.path.join("target")).expect("target dir");
        std::os::unix::fs::symlink("target", temp.path.join("linked")).expect("symlink");

        let provider = LocalFileProvider::new();
        let metadata = provider
            .metadata(&temp.child_uri("linked"))
            .expect("symlink metadata");

        assert_eq!(metadata.kind, FileKind::Symlink);
        assert_eq!(metadata.symlink_target_kind, Some(FileKind::Directory));
        assert!(metadata.is_directory_like());
    }

    #[cfg(unix)]
    #[test]
    /// Verifies file symlink kind and followed target-kind metadata.
    fn symlink_to_file_keeps_link_kind_and_records_target_file() {
        let temp = TempDir::new("symlink_file");
        fs::write(temp.path.join("target.txt"), b"target").expect("target file");
        std::os::unix::fs::symlink("target.txt", temp.path.join("linked.txt")).expect("symlink");

        let provider = LocalFileProvider::new();
        let metadata = provider
            .metadata(&temp.child_uri("linked.txt"))
            .expect("symlink metadata");

        assert_eq!(metadata.kind, FileKind::Symlink);
        assert_eq!(metadata.symlink_target_kind, Some(FileKind::File));
        assert!(metadata.is_file_like());
    }

    #[cfg(unix)]
    #[test]
    /// Verifies broken symlink metadata preserves link kind with no target kind.
    fn broken_symlink_keeps_link_kind_without_target_kind() {
        let temp = TempDir::new("broken_symlink");
        std::os::unix::fs::symlink("missing", temp.path.join("broken")).expect("symlink");

        let provider = LocalFileProvider::new();
        let metadata = provider
            .metadata(&temp.child_uri("broken"))
            .expect("symlink metadata");

        assert_eq!(metadata.kind, FileKind::Symlink);
        assert_eq!(metadata.symlink_target_kind, None);
        assert!(!metadata.is_directory_like());
    }

    #[cfg(unix)]
    #[test]
    /// Verifies removing a directory symlink leaves its target intact.
    fn removing_symlink_to_directory_removes_link_not_target() {
        let temp = TempDir::new("remove_symlink_dir");
        fs::create_dir(temp.path.join("target")).expect("target dir");
        std::os::unix::fs::symlink("target", temp.path.join("linked")).expect("symlink");

        let provider = LocalFileProvider::new();
        provider
            .remove(&temp.child_uri("linked"))
            .expect("remove symlink");

        assert!(temp.path.join("target").is_dir());
        assert!(!temp.path.join("linked").exists());
    }

    #[cfg(unix)]
    #[test]
    /// Verifies canonicalization follows a directory symlink to its target.
    fn canonical_uri_resolves_symlink_to_directory_target() {
        let temp = TempDir::new("canonical_symlink");
        fs::create_dir(temp.path.join("target")).expect("target dir");
        std::os::unix::fs::symlink("target", temp.path.join("linked")).expect("symlink");

        let provider = LocalFileProvider::new();
        let canonical = provider
            .canonical_uri(&temp.child_uri("linked"))
            .expect("canonical uri")
            .expect("canonical value");

        assert_eq!(
            canonical.to_local_path().expect("canonical path"),
            temp.path.join("target")
        );
    }
}
