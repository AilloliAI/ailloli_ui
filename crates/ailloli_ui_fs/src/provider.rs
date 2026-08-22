//! Synchronous provider contract for complete filesystem operations.

use crate::{FileCapabilities, FileEntry, FileError, FileKind, FileMetadata, FileUri};

/// Synchronous, UI-independent filesystem backend.
///
/// Methods do not impose thread-safety bounds, ordering, atomicity, or timeout
/// guarantees; concrete providers document those details. Callers should gate
/// optional operations with [`Self::capabilities`] and still handle failures.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileCapabilities, FileProvider};
/// fn can_write(provider: &dyn FileProvider) -> bool { provider.capabilities().write }
/// # let _ = can_write;
/// ```
pub trait FileProvider {
    /// Returns the provider's advertised operation set.
    ///
    /// The snapshot is advisory and may not account for per-resource permissions.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileCapabilities, FileProvider};
    /// fn capabilities(provider: &dyn FileProvider) -> FileCapabilities { provider.capabilities() }
    /// # let _ = capabilities;
    /// ```
    fn capabilities(&self) -> FileCapabilities;

    /// Returns a complete directory listing in provider-defined order.
    ///
    /// No pagination or sorting is supplied by this trait.
    ///
    /// # Errors
    ///
    /// Returns a provider [`FileError`] when the URI cannot be listed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileError, FileProvider, FileUri};
    /// fn list(provider: &dyn FileProvider, uri: &FileUri) -> Result<Vec<FileEntry>, FileError> { provider.read_dir(uri) }
    /// # let _ = list;
    /// ```
    fn read_dir(&self, uri: &FileUri) -> Result<Vec<FileEntry>, FileError>;

    /// Reads the complete file into memory.
    ///
    /// # Errors
    ///
    /// Returns a provider [`FileError`] when `uri` cannot be read as bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileProvider, FileUri};
    /// fn read(provider: &dyn FileProvider, uri: &FileUri) -> Result<Vec<u8>, FileError> { provider.read_file(uri) }
    /// # let _ = read;
    /// ```
    fn read_file(&self, uri: &FileUri) -> Result<Vec<u8>, FileError>;

    /// Writes the complete byte slice to a file.
    ///
    /// Creation, truncation, atomicity, and permission preservation are
    /// provider-defined; an empty slice is a valid request.
    ///
    /// # Errors
    ///
    /// Returns a provider [`FileError`] when the bytes cannot be written.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileProvider, FileUri};
    /// fn write(provider: &dyn FileProvider, uri: &FileUri) -> Result<(), FileError> { provider.write_file(uri, b"hello") }
    /// # let _ = write;
    /// ```
    fn write_file(&self, uri: &FileUri, bytes: &[u8]) -> Result<(), FileError>;

    /// Returns one metadata snapshot without prescribing symlink-following policy.
    ///
    /// # Errors
    ///
    /// Returns a provider [`FileError`] when metadata cannot be obtained.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileMetadata, FileProvider, FileUri};
    /// fn metadata(provider: &dyn FileProvider, uri: &FileUri) -> Result<FileMetadata, FileError> { provider.metadata(uri) }
    /// # let _ = metadata;
    /// ```
    fn metadata(&self, uri: &FileUri) -> Result<FileMetadata, FileError>;

    /// Returns a canonical provider URI when one is known.
    ///
    /// The default returns `Ok(None)`, meaning canonicalization is unavailable;
    /// that is distinct from returning the unchanged input URI.
    ///
    /// # Errors
    ///
    /// Implementations may return a provider [`FileError`] if canonicalization fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileProvider, FileUri};
    /// fn canonical(provider: &dyn FileProvider, uri: &FileUri) -> Result<Option<FileUri>, FileError> { provider.canonical_uri(uri) }
    /// # let _ = canonical;
    /// ```
    fn canonical_uri(&self, _uri: &FileUri) -> Result<Option<FileUri>, FileError> {
        Ok(None)
    }

    /// Creates one directory.
    ///
    /// Parent creation and behavior for an existing directory are provider-defined.
    ///
    /// # Errors
    ///
    /// Returns a provider [`FileError`] when the directory cannot be created.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileProvider, FileUri};
    /// fn create(provider: &dyn FileProvider, uri: &FileUri) -> Result<(), FileError> { provider.create_dir(uri) }
    /// # let _ = create;
    /// ```
    fn create_dir(&self, uri: &FileUri) -> Result<(), FileError>;

    /// Renames or relocates one entry according to provider semantics.
    ///
    /// Cross-filesystem support and destination replacement are not guaranteed.
    ///
    /// # Errors
    ///
    /// Returns a provider [`FileError`] when the rename cannot complete.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileProvider, FileUri};
    /// fn rename(provider: &dyn FileProvider, from: &FileUri, to: &FileUri) -> Result<(), FileError> { provider.rename(from, to) }
    /// # let _ = rename;
    /// ```
    fn rename(&self, from: &FileUri, to: &FileUri) -> Result<(), FileError>;

    /// Removes one file-like entry or empty directory.
    ///
    /// Recursive behavior is not implied; use [`Self::remove_recursive`] when advertised.
    ///
    /// # Errors
    ///
    /// Returns a provider [`FileError`] when removal fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileProvider, FileUri};
    /// fn remove(provider: &dyn FileProvider, uri: &FileUri) -> Result<(), FileError> { provider.remove(uri) }
    /// # let _ = remove;
    /// ```
    fn remove(&self, uri: &FileUri) -> Result<(), FileError>;

    /// Recursively copies an entry using the provider's primitive methods.
    ///
    /// The default rejects symlinks, creates a destination directory before its
    /// children, traverses listings in provider order, and buffers each file
    /// completely in memory. [`FileKind::Other`] is copied as bytes. The process
    /// is synchronous and non-transactional, so a failure can leave a partial
    /// destination. Deep trees consume call stack.
    ///
    /// # Errors
    ///
    /// Propagates metadata, listing, URI-join, creation, read, or write errors;
    /// returns [`FileError::Unsupported`] for symlinks/unsupported kinds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileProvider, FileUri};
    /// fn copy(provider: &dyn FileProvider, from: &FileUri, to: &FileUri) -> Result<(), FileError> { provider.copy_entry(from, to) }
    /// # let _ = copy;
    /// ```
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

    /// Moves an entry; the default delegates directly to [`Self::rename`].
    ///
    /// It does not fall back to copy-then-remove across filesystems.
    ///
    /// # Errors
    ///
    /// Propagates the provider's rename failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileProvider, FileUri};
    /// fn move_entry(provider: &dyn FileProvider, from: &FileUri, to: &FileUri) -> Result<(), FileError> { provider.move_entry(from, to) }
    /// # let _ = move_entry;
    /// ```
    fn move_entry(&self, from: &FileUri, to: &FileUri) -> Result<(), FileError> {
        self.rename(from, to)
    }

    /// Removes an entry subtree depth-first using primitive provider methods.
    ///
    /// The default lists only entries whose primary kind is
    /// [`FileKind::Directory`], so symlinks are removed without following them.
    /// Children are processed in provider order, then the root is removed. The
    /// operation is synchronous, recursive, and non-transactional.
    ///
    /// # Errors
    ///
    /// Propagates metadata, listing, descendant, or final removal failures; an
    /// error can occur after earlier descendants were already removed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileError, FileProvider, FileUri};
    /// fn remove_tree(provider: &dyn FileProvider, uri: &FileUri) -> Result<(), FileError> { provider.remove_recursive(uri) }
    /// # let _ = remove_tree;
    /// ```
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
