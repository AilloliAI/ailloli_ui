//! Normalized native filesystem watch events and correlation metadata.

use crate::{FileIdentity, FileUri};

/// Kind of change reported by a provider watch stream.
///
/// The enum is non-exhaustive so downstream matches need a wildcard arm.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::WatchEventKind;
/// assert_ne!(WatchEventKind::Created, WatchEventKind::Removed);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WatchEventKind {
    /// A new entry appeared at the event URI.
    Created,
    /// Content or metadata changed at the event URI.
    Modified,
    /// The entry at the event URI disappeared.
    Removed,
    /// An entry changed name, normally within one directory.
    Renamed,
    /// An entry changed parent or provider location.
    Moved,
    /// The provider lost one or more events; consumers must invalidate/reload.
    Overflow,
}

/// One normalized provider watch event.
///
/// Sequence numbers are interpreted within a provider generation; this type
/// stores but does not validate monotonicity. Rename/move events can carry a
/// previous URI, and any event can carry a stable identity. The struct is
/// non-exhaustive so it can gain correlation fields compatibly.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileUri, WatchEvent, WatchEventKind};
/// let event = WatchEvent::new(WatchEventKind::Created, FileUri::parse("file:///new")?, 1, 4);
/// assert_eq!((event.sequence(), event.generation()), (1, 4));
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WatchEvent {
    /// Normalized event category.
    kind: WatchEventKind,
    /// Current or affected URI.
    uri: FileUri,
    /// Former URI for a paired rename/move when supplied.
    previous_uri: Option<FileUri>,
    /// Provider sequence number within `generation`.
    sequence: u64,
    /// Provider watch-stream generation.
    generation: u64,
    /// Optional provider-stable entry identity.
    identity: Option<FileIdentity>,
}

impl WatchEvent {
    /// Creates an event without previous URI or stable identity.
    ///
    /// Zero and non-monotone sequence/generation values are accepted verbatim.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileUri, WatchEvent, WatchEventKind};
    /// let event = WatchEvent::new(WatchEventKind::Modified, FileUri::parse("file:///a")?, 0, 0);
    /// assert_eq!(event.previous_uri(), None);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn new(kind: WatchEventKind, uri: FileUri, sequence: u64, generation: u64) -> Self {
        Self {
            kind,
            uri,
            previous_uri: None,
            sequence,
            generation,
            identity: None,
        }
    }

    /// Attaches the entry's former URI.
    ///
    /// The value is accepted for every event kind and replaces an earlier one.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileUri, WatchEvent, WatchEventKind};
    /// let event = WatchEvent::new(WatchEventKind::Renamed, FileUri::parse("file:///new")?, 1, 1)
    ///     .with_previous_uri(FileUri::parse("file:///old")?);
    /// assert_eq!(event.previous_uri().unwrap().path(), "/old");
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn with_previous_uri(mut self, previous_uri: FileUri) -> Self {
        self.previous_uri = Some(previous_uri);
        self
    }

    /// Attaches a provider-stable identity, replacing any previous identity.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileIdentity, FileUri, WatchEvent, WatchEventKind};
    /// let event = WatchEvent::new(WatchEventKind::Created, FileUri::parse("file:///a")?, 1, 1)
    ///     .with_identity(FileIdentity::new("local", [1]));
    /// assert_eq!(event.identity().unwrap().value(), &[1]);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn with_identity(mut self, identity: FileIdentity) -> Self {
        self.identity = Some(identity);
        self
    }

    /// Returns the normalized event category.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileUri, WatchEvent, WatchEventKind};
    /// let event = WatchEvent::new(WatchEventKind::Overflow, FileUri::parse("file:///")?, 1, 1);
    /// assert_eq!(event.kind(), WatchEventKind::Overflow);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub const fn kind(&self) -> WatchEventKind {
        self.kind
    }

    /// Borrows the entry's current/event URI.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileUri, WatchEvent, WatchEventKind};
    /// let event = WatchEvent::new(WatchEventKind::Removed, FileUri::parse("file:///a")?, 1, 1);
    /// assert_eq!(event.uri().path(), "/a");
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub const fn uri(&self) -> &FileUri {
        &self.uri
    }

    /// Borrows the former URI, or returns `None` when it was not supplied.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileUri, WatchEvent, WatchEventKind};
    /// let event = WatchEvent::new(WatchEventKind::Created, FileUri::parse("file:///a")?, 1, 1);
    /// assert!(event.previous_uri().is_none());
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub const fn previous_uri(&self) -> Option<&FileUri> {
        self.previous_uri.as_ref()
    }

    /// Returns the provider sequence number within its generation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileUri, WatchEvent, WatchEventKind};
    /// let event = WatchEvent::new(WatchEventKind::Created, FileUri::parse("file:///a")?, 7, 2);
    /// assert_eq!(event.sequence(), 7);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Returns the provider watch-generation identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileUri, WatchEvent, WatchEventKind};
    /// let event = WatchEvent::new(WatchEventKind::Created, FileUri::parse("file:///a")?, 7, 2);
    /// assert_eq!(event.generation(), 2);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Borrows the stable identity, or returns `None` when unavailable.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileUri, WatchEvent, WatchEventKind};
    /// let event = WatchEvent::new(WatchEventKind::Created, FileUri::parse("file:///a")?, 1, 1);
    /// assert!(event.identity().is_none());
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub const fn identity(&self) -> Option<&FileIdentity> {
        self.identity.as_ref()
    }
}
