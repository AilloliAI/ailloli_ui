use crate::{FileCapabilities, FileEntry, FileError, FileKind, FileMetadata, FileUri};

pub trait FileProvider {
    fn capabilities(&self) -> FileCapabilities;
    fn read_dir(&self, uri: &FileUri) -> Result<Vec<FileEntry>, FileError>;
    fn read_file(&self, uri: &FileUri) -> Result<Vec<u8>, FileError>;
    fn write_file(&self, uri: &FileUri, bytes: &[u8]) -> Result<(), FileError>;
    fn metadata(&self, uri: &FileUri) -> Result<FileMetadata, FileError>;
    fn canonical_uri(&self, _uri: &FileUri) -> Result<Option<FileUri>, FileError> {
        Ok(None)
    }
    fn create_dir(&self, uri: &FileUri) -> Result<(), FileError>;
    fn rename(&self, from: &FileUri, to: &FileUri) -> Result<(), FileError>;
    fn remove(&self, uri: &FileUri) -> Result<(), FileError>;

    fn copy_entry(&self, from: &FileUri, to: &FileUri) -> Result<(), FileError> {
        let metadata = self.metadata(from)?;
        if metadata.kind == FileKind::Symlink {
            return Err(FileError::Unsupported(
                "copying symlinks is not supported by this provider".into(),
            ));
        }
        if metadata.kind == FileKind::Directory {
            self.create_dir(to)?;
            for entry in self.read_dir(from)? {
                let child_to = to.join_child(&entry.name)?;
                self.copy_entry(&entry.uri, &child_to)?;
            }
            return Ok(());
        }
        if metadata.is_file_like() || metadata.kind == FileKind::Other {
            let bytes = self.read_file(from)?;
            return self.write_file(to, &bytes);
        }
        Err(FileError::Unsupported(format!(
            "unsupported entry kind for copy: {:?}",
            metadata.kind
        )))
    }

    fn move_entry(&self, from: &FileUri, to: &FileUri) -> Result<(), FileError> {
        self.rename(from, to)
    }

    fn remove_recursive(&self, uri: &FileUri) -> Result<(), FileError> {
        let metadata = self.metadata(uri)?;
        if metadata.kind == FileKind::Directory {
            for entry in self.read_dir(uri)? {
                self.remove_recursive(&entry.uri)?;
            }
        }
        self.remove(uri)
    }
}
