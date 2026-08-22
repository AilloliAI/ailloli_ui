//! Provider feature flags used to gate filesystem UI actions.

/// Operations advertised by a filesystem provider.
///
/// Every boolean means that the corresponding operation is intended to be
/// available. Capabilities are informational: trait default methods do not
/// consult them, and an advertised operation can still fail for a resource.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileCapabilities;
/// assert!(FileCapabilities::READ_ONLY.read);
/// assert!(!FileCapabilities::READ_ONLY.write);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileCapabilities {
    /// Directory listing, file reads, and metadata queries are available.
    pub read: bool,
    /// File content writes are available.
    pub write: bool,
    /// Directory creation is available.
    pub create_dir: bool,
    /// Same-provider rename is available.
    pub rename: bool,
    /// Non-recursive removal is available.
    pub remove: bool,
    /// Entry copy is available, either natively or through the trait default.
    pub copy_entry: bool,
    /// Entry move is available, either natively or as rename.
    pub move_entry: bool,
    /// Recursive removal is available, either natively or through the trait default.
    pub remove_recursive: bool,
    /// Native directory watch events are available.
    pub watch: bool,
}

impl FileCapabilities {
    /// Read-only profile: only `read` is `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileCapabilities;
    /// assert_eq!(FileCapabilities::READ_ONLY, FileCapabilities::default());
    /// ```
    pub const READ_ONLY: Self = Self {
        read: true,
        write: false,
        create_dir: false,
        rename: false,
        remove: false,
        copy_entry: false,
        move_entry: false,
        remove_recursive: false,
        watch: false,
    };

    /// Read/write profile with mutation and recursive helpers, but no watch.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileCapabilities;
    /// assert!(FileCapabilities::READ_WRITE.remove_recursive);
    /// assert!(!FileCapabilities::READ_WRITE.watch);
    /// ```
    pub const READ_WRITE: Self = Self {
        read: true,
        write: true,
        create_dir: true,
        rename: true,
        remove: true,
        copy_entry: true,
        move_entry: true,
        remove_recursive: true,
        watch: false,
    };
}

impl Default for FileCapabilities {
    fn default() -> Self {
        Self::READ_ONLY
    }
}
