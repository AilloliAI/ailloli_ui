//! Worker-owned source contract used by the persistent file-tree coordinator.

use crate::{FileEntry, FileError, FileIdentity, FileUri, WatchEvent};

/// Synchronous provider instance owned exclusively by a filesystem worker.
/// It may wrap an internally synchronous or asynchronous backend, but no UI
/// handle or callback crosses this boundary.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileError, FileTreeSource, FileUri};
/// fn list(source: &mut dyn FileTreeSource, uri: &FileUri) -> Result<usize, FileError> {
///     Ok(source.read_dir(uri)?.len())
/// }
/// # let _ = list;
/// ```
pub trait FileTreeSource: Send + 'static {
    /// Returns a complete directory listing in provider-defined order.
    ///
    /// The mutable receiver permits worker-local clients and caches. No
    /// pagination or sorting is imposed.
    ///
    /// # Errors
    ///
    /// Returns a provider [`FileError`] when the directory cannot be listed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileError, FileTreeSource, FileUri};
    /// fn read(source: &mut dyn FileTreeSource, uri: &FileUri) -> Result<Vec<FileEntry>, FileError> { source.read_dir(uri) }
    /// # let _ = read;
    /// ```
    fn read_dir(&mut self, uri: &FileUri) -> Result<Vec<FileEntry>, FileError>;

    /// Returns a stable provider identity for an entry when available.
    ///
    /// The default returns `Ok(None)`. Equal identities can represent hard
    /// links, so tree stores do not assume global one-to-one identity.
    ///
    /// # Errors
    ///
    /// Implementations may return a provider [`FileError`] if lookup fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileIdentity, FileTreeSource, FileUri};
    /// fn identity(source: &mut dyn FileTreeSource, uri: &FileUri) -> Result<Option<FileIdentity>, FileError> { source.identity(uri) }
    /// # let _ = identity;
    /// ```
    fn identity(&mut self, _uri: &FileUri) -> Result<Option<FileIdentity>, FileError> {
        Ok(None)
    }

    /// Creates one directory at `uri`.
    ///
    /// # Errors
    ///
    /// The default always returns [`FileError::Unsupported`]. Implementations
    /// return provider errors for failed creation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileTreeSource, FileUri};
    /// fn create(source: &mut dyn FileTreeSource, uri: &FileUri) -> Result<(), FileError> { source.create_directory(uri) }
    /// # let _ = create;
    /// ```
    fn create_directory(&mut self, _uri: &FileUri) -> Result<(), FileError> {
        Err(FileError::Unsupported(
            "this filesystem source cannot create directories".into(),
        ))
    }

    /// Creates one empty file at `uri`.
    ///
    /// Existing-file behavior is provider-defined.
    ///
    /// # Errors
    ///
    /// The default always returns [`FileError::Unsupported`]. Implementations
    /// return provider errors for failed creation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileTreeSource, FileUri};
    /// fn create(source: &mut dyn FileTreeSource, uri: &FileUri) -> Result<(), FileError> { source.create_file(uri) }
    /// # let _ = create;
    /// ```
    fn create_file(&mut self, _uri: &FileUri) -> Result<(), FileError> {
        Err(FileError::Unsupported(
            "this filesystem source cannot create files".into(),
        ))
    }

    /// Moves or renames one entry.
    ///
    /// Cross-filesystem and destination-replacement behavior are provider-defined.
    ///
    /// # Errors
    ///
    /// The default always returns [`FileError::Unsupported`]. Implementations
    /// return provider errors for failed moves.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileTreeSource, FileUri};
    /// fn move_entry(source: &mut dyn FileTreeSource, from: &FileUri, to: &FileUri) -> Result<(), FileError> { source.move_entry(from, to) }
    /// # let _ = move_entry;
    /// ```
    fn move_entry(&mut self, _from: &FileUri, _to: &FileUri) -> Result<(), FileError> {
        Err(FileError::Unsupported(
            "this filesystem source cannot move entries".into(),
        ))
    }

    /// Removes one entry, optionally including descendants.
    ///
    /// # Errors
    ///
    /// The default always returns [`FileError::Unsupported`]. Implementations
    /// return provider errors for failed removal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileTreeSource, FileUri};
    /// fn remove(source: &mut dyn FileTreeSource, uri: &FileUri) -> Result<(), FileError> { source.remove_entry(uri, true) }
    /// # let _ = remove;
    /// ```
    fn remove_entry(&mut self, _uri: &FileUri, _recursive: bool) -> Result<(), FileError> {
        Err(FileError::Unsupported(
            "this filesystem source cannot remove entries".into(),
        ))
    }

    /// Starts native watch delivery for one directory.
    ///
    /// # Errors
    ///
    /// The default always returns [`FileError::Unsupported`]. Implementations
    /// return provider errors if registration fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileTreeSource, FileUri};
    /// fn watch(source: &mut dyn FileTreeSource, uri: &FileUri) -> Result<(), FileError> { source.watch_directory(uri) }
    /// # let _ = watch;
    /// ```
    fn watch_directory(&mut self, _uri: &FileUri) -> Result<(), FileError> {
        Err(FileError::Unsupported(
            "this filesystem source does not provide native watch events".into(),
        ))
    }

    /// Stops native watch delivery for one directory.
    ///
    /// The default is a successful no-op, including for an unwatched URI.
    ///
    /// # Errors
    ///
    /// Implementations may return a provider [`FileError`] if unregistration fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileTreeSource, FileUri};
    /// fn unwatch(source: &mut dyn FileTreeSource, uri: &FileUri) -> Result<(), FileError> { source.unwatch_directory(uri) }
    /// # let _ = unwatch;
    /// ```
    fn unwatch_directory(&mut self, _uri: &FileUri) -> Result<(), FileError> {
        Ok(())
    }

    /// Drains up to the requested number of queued watch events.
    ///
    /// The default ignores `limit` and returns an empty vector. Implementations
    /// should treat zero as a request to return no events; event ordering and
    /// overflow signaling follow their watch contract.
    ///
    /// # Errors
    ///
    /// Implementations may return a provider [`FileError`] when polling fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileTreeSource, WatchEvent};
    /// fn poll(source: &mut dyn FileTreeSource) -> Result<Vec<WatchEvent>, FileError> { source.poll_watch(64) }
    /// # let _ = poll;
    /// ```
    fn poll_watch(&mut self, _limit: usize) -> Result<Vec<WatchEvent>, FileError> {
        Ok(Vec::new())
    }

    /// Reports whether watch registration/polling is implemented natively.
    ///
    /// The default is `false`; it is independent of whether callers choose to poll.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileTreeSource;
    /// fn has_watch(source: &dyn FileTreeSource) -> bool { source.supports_native_watch() }
    /// # let _ = has_watch;
    /// ```
    fn supports_native_watch(&self) -> bool {
        false
    }
}

/// Thread-safe factory that creates one worker-owned source instance.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileError, FileTreeSource, FileTreeSourceFactory};
/// fn open(factory: &dyn FileTreeSourceFactory) -> Result<Box<dyn FileTreeSource>, FileError> { factory.create() }
/// # let _ = open;
/// ```
pub trait FileTreeSourceFactory: Send + Sync + 'static {
    /// Creates a fresh source instance for exclusive ownership by one worker.
    ///
    /// # Errors
    ///
    /// Returns a provider [`FileError`] when the source cannot be initialized.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileTreeSource, FileTreeSourceFactory};
    /// fn create(factory: &dyn FileTreeSourceFactory) -> Result<Box<dyn FileTreeSource>, FileError> { factory.create() }
    /// # let _ = create;
    /// ```
    fn create(&self) -> Result<Box<dyn FileTreeSource>, FileError>;
}
