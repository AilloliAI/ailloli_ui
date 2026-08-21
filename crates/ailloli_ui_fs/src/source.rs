use crate::{FileEntry, FileError, FileIdentity, FileUri, WatchEvent};

/// Synchronous provider instance owned exclusively by a filesystem worker.
/// It may wrap an internally synchronous or asynchronous backend, but no UI
/// handle or callback crosses this boundary.
pub trait FileTreeSource: Send + 'static {
    fn read_dir(&mut self, uri: &FileUri) -> Result<Vec<FileEntry>, FileError>;

    fn identity(&mut self, _uri: &FileUri) -> Result<Option<FileIdentity>, FileError> {
        Ok(None)
    }

    fn create_directory(&mut self, _uri: &FileUri) -> Result<(), FileError> {
        Err(FileError::Unsupported(
            "this filesystem source cannot create directories".into(),
        ))
    }

    fn move_entry(&mut self, _from: &FileUri, _to: &FileUri) -> Result<(), FileError> {
        Err(FileError::Unsupported(
            "this filesystem source cannot move entries".into(),
        ))
    }

    fn remove_entry(&mut self, _uri: &FileUri, _recursive: bool) -> Result<(), FileError> {
        Err(FileError::Unsupported(
            "this filesystem source cannot remove entries".into(),
        ))
    }

    fn watch_directory(&mut self, _uri: &FileUri) -> Result<(), FileError> {
        Err(FileError::Unsupported(
            "this filesystem source does not provide native watch events".into(),
        ))
    }

    fn unwatch_directory(&mut self, _uri: &FileUri) -> Result<(), FileError> {
        Ok(())
    }

    fn poll_watch(&mut self, _limit: usize) -> Result<Vec<WatchEvent>, FileError> {
        Ok(Vec::new())
    }

    fn supports_native_watch(&self) -> bool {
        false
    }
}

/// Thread-safe factory that creates one worker-owned source instance.
pub trait FileTreeSourceFactory: Send + Sync + 'static {
    fn create(&self) -> Result<Box<dyn FileTreeSource>, FileError>;
}
