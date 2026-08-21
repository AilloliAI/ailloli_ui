//! Local filesystem provider for `ailloli_ui_fs`.

use std::fs;
use std::path::{Path, PathBuf};

mod source;

pub use source::{
    LocalFileTreeSource, LocalFileTreeSourceFactory, DEFAULT_LOCAL_FILE_TREE_MAX_WATCHERS,
};

use ailloli_ui_fs::{
    FileCapabilities, FileEntry, FileError, FileKind, FileMetadata, FileProvider, FileUri,
};

#[derive(Debug, Clone, Default)]
pub struct LocalFileProvider;

impl LocalFileProvider {
    pub fn new() -> Self {
        Self
    }
}

impl FileProvider for LocalFileProvider {
    fn capabilities(&self) -> FileCapabilities {
        FileCapabilities {
            watch: true,
            ..FileCapabilities::READ_WRITE
        }
    }

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

    fn read_file(&self, uri: &FileUri) -> Result<Vec<u8>, FileError> {
        let path = local_path(uri)?;
        fs::read(&path).map_err(|err| FileError::from_io(&err, path.display().to_string()))
    }

    fn write_file(&self, uri: &FileUri, bytes: &[u8]) -> Result<(), FileError> {
        let path = local_path(uri)?;
        fs::write(&path, bytes).map_err(|err| FileError::from_io(&err, path.display().to_string()))
    }

    fn metadata(&self, uri: &FileUri) -> Result<FileMetadata, FileError> {
        metadata_for_path(&local_path(uri)?)
    }

    fn canonical_uri(&self, uri: &FileUri) -> Result<Option<FileUri>, FileError> {
        let path = local_path(uri)?;
        match fs::canonicalize(&path) {
            Ok(canonical) => FileUri::local(canonical).map(Some),
            Err(_) => Ok(None),
        }
    }

    fn create_dir(&self, uri: &FileUri) -> Result<(), FileError> {
        let path = local_path(uri)?;
        fs::create_dir(&path).map_err(|err| FileError::from_io(&err, path.display().to_string()))
    }

    fn rename(&self, from: &FileUri, to: &FileUri) -> Result<(), FileError> {
        let from = local_path(from)?;
        let to = local_path(to)?;
        fs::rename(&from, &to).map_err(|err| {
            FileError::from_io(&err, format!("{} -> {}", from.display(), to.display()))
        })
    }

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

fn local_path(uri: &FileUri) -> Result<PathBuf, FileError> {
    uri.to_local_path()
}

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
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
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

        fn uri(&self) -> FileUri {
            FileUri::local(&self.path).expect("temp uri")
        }

        fn child_uri(&self, name: &str) -> FileUri {
            FileUri::local(self.path.join(name)).expect("child uri")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
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
