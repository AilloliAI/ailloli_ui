//! Worker-owned local source, native-watch normalization, and stable identity.
//!
//! Native watches are deliberately non-recursive. Raw backend events wait for a
//! 50-millisecond debounce window, rename halves are paired by tracker, and
//! duplicate echo events are suppressed before bounded delivery.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use ailloli_ui_fs::{
    FileEntry, FileError, FileIdentity, FileProvider, FileTreeSource, FileTreeSourceFactory,
    FileUri, WatchEvent, WatchEventKind,
};
use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::LocalFileProvider;

/// Minimum age of raw native events before a poll normalizes them.
const WATCH_DEBOUNCE: Duration = Duration::from_millis(50);
/// Retention window used to suppress self-watch echoes after rename.
const WATCH_RENAME_ECHO_TTL: Duration = Duration::from_millis(500);
/// Default maximum number of simultaneously watched local directories: 1,024.
///
/// The limit is an item count and each watch is non-recursive. It bounds native
/// watcher resources independently of the runtime request/response queues.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs_local::DEFAULT_LOCAL_FILE_TREE_MAX_WATCHERS;
/// assert_eq!(DEFAULT_LOCAL_FILE_TREE_MAX_WATCHERS, 1_024);
/// ```
pub const DEFAULT_LOCAL_FILE_TREE_MAX_WATCHERS: usize = 1_024;

/// Timestamped raw event or backend error from `notify`.
type RawWatchEvent = (Instant, notify::Result<Event>);

/// Thread-safe factory configuring each worker-owned local source.
///
/// Clones copy only the numeric watcher limit. Source construction and native
/// watcher allocation happen later on the filesystem worker.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs_local::LocalFileTreeSourceFactory;
/// let factory = LocalFileTreeSourceFactory::new().max_watchers(64);
/// let _: LocalFileTreeSourceFactory = factory;
/// ```
#[derive(Debug, Clone)]
pub struct LocalFileTreeSourceFactory {
    /// Maximum simultaneous non-recursive directory watches for new sources.
    max_watchers: usize,
}

/// Builder operations for worker source configuration.
impl LocalFileTreeSourceFactory {
    /// Creates a factory with the 1,024-watcher default.
    ///
    /// No native watcher is allocated by this constructor.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_local::LocalFileTreeSourceFactory;
    /// let _: LocalFileTreeSourceFactory = LocalFileTreeSourceFactory::new();
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the per-source watcher ceiling.
    ///
    /// Zero is accepted and prevents every new watch while leaving file I/O
    /// usable. Existing sources are unaffected because the factory is consumed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_local::LocalFileTreeSourceFactory;
    /// let disabled = LocalFileTreeSourceFactory::new().max_watchers(0);
    /// let _: LocalFileTreeSourceFactory = disabled;
    /// ```
    pub fn max_watchers(mut self, max_watchers: usize) -> Self {
        self.max_watchers = max_watchers;
        self
    }
}

/// Selects [`DEFAULT_LOCAL_FILE_TREE_MAX_WATCHERS`] for new sources.
impl Default for LocalFileTreeSourceFactory {
    /// Constructs the conservative default factory without allocating a watcher.
    fn default() -> Self {
        Self {
            max_watchers: DEFAULT_LOCAL_FILE_TREE_MAX_WATCHERS,
        }
    }
}

/// Allocates a configured local source on the calling worker thread.
impl FileTreeSourceFactory for LocalFileTreeSourceFactory {
    /// Returns a source or maps native watcher initialization to [`FileError`].
    ///
    /// # Errors
    ///
    /// Returns [`FileError::Io`] when the platform watcher cannot be created.
    fn create(&self) -> Result<Box<dyn FileTreeSource>, FileError> {
        Ok(Box::new(LocalFileTreeSource::with_max_watchers(
            self.max_watchers,
        )?))
    }
}

/// Worker-owned local source with non-recursive native directory watches.
///
/// All methods are synchronous and the type is intended to stay on its single
/// runtime worker. Event sequences and watch generations start at one and
/// saturate at `u64::MAX`; a successful watch/unwatch increments generation.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_fs::FileTreeSource;
/// use ailloli_ui_fs_local::LocalFileTreeSource;
/// let source = LocalFileTreeSource::new()?;
/// assert!(source.supports_native_watch());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct LocalFileTreeSource {
    /// Stateless synchronous local provider used for filesystem calls.
    provider: LocalFileProvider,
    /// Platform-recommended native watcher.
    watcher: RecommendedWatcher,
    /// Raw callback channel drained without blocking.
    receiver: Receiver<RawWatchEvent>,
    /// Raw events waiting to become at least 50 milliseconds old.
    pending: VecDeque<RawWatchEvent>,
    /// Normalized events retained across bounded polls.
    normalized: VecDeque<WatchEvent>,
    /// Rename-source paths retained for 500-millisecond echo suppression.
    recent_rename_sources: HashMap<PathBuf, Instant>,
    /// Successfully watched host paths and their original URIs.
    watched: HashMap<PathBuf, FileUri>,
    /// Next saturating event sequence, initially one.
    next_sequence: u64,
    /// Current saturating watch generation, initially one.
    generation: u64,
    /// Inclusive ceiling checked before adding a new distinct watch.
    max_watchers: usize,
}

/// Construction and raw-event normalization operations.
impl LocalFileTreeSource {
    /// Allocates a source with the default 1,024-watcher ceiling.
    ///
    /// # Errors
    ///
    /// Returns [`FileError::Io`] when the platform watcher cannot initialize.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_fs_local::LocalFileTreeSource;
    /// let _: LocalFileTreeSource = LocalFileTreeSource::new()?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new() -> Result<Self, FileError> {
        Self::with_max_watchers(DEFAULT_LOCAL_FILE_TREE_MAX_WATCHERS)
    }

    /// Allocates a source with an explicit simultaneous-watch ceiling.
    ///
    /// Zero disables watch registration. The value is not clamped and does not
    /// affect filesystem reads or mutations.
    ///
    /// # Errors
    ///
    /// Returns [`FileError::Io`] when the platform watcher cannot initialize.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_fs_local::LocalFileTreeSource;
    /// let _: LocalFileTreeSource = LocalFileTreeSource::with_max_watchers(8)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn with_max_watchers(max_watchers: usize) -> Result<Self, FileError> {
        let (sender, receiver) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send((Instant::now(), event));
        })
        .map_err(notify_error)?;
        Ok(Self {
            provider: LocalFileProvider::new(),
            watcher,
            receiver,
            pending: VecDeque::new(),
            normalized: VecDeque::new(),
            recent_rename_sources: HashMap::new(),
            watched: HashMap::new(),
            next_sequence: 1,
            generation: 1,
            max_watchers,
        })
    }

    /// Allocates one normalized event with the current sequence/generation.
    ///
    /// Sequence increments saturatingly, so `u64::MAX` repeats after exhaustion.
    fn next_event(
        &mut self,
        kind: WatchEventKind,
        uri: FileUri,
        previous_uri: Option<FileUri>,
    ) -> WatchEvent {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let mut event = WatchEvent::new(kind, uri, sequence, self.generation);
        if let Some(previous_uri) = previous_uri {
            event = event.with_previous_uri(previous_uri);
        }
        event
    }

    /// Maps one backend event into zero or more provider-neutral events.
    ///
    /// Access and unknown variants are ignored. `Other` becomes `Overflow` when
    /// a path or watched fallback URI exists. Complete renames use the first two
    /// paths and classify same-parent changes as rename, otherwise move.
    ///
    /// # Errors
    ///
    /// Returns [`FileError::InvalidUri`] when any native event path cannot be
    /// represented as a local [`FileUri`].
    fn map_event(&mut self, event: Event) -> Result<Vec<WatchEvent>, FileError> {
        let paths = event.paths;
        match event.kind {
            EventKind::Create(_) => self.events_for_paths(WatchEventKind::Created, paths),
            EventKind::Remove(_) => self.events_for_paths(WatchEventKind::Removed, paths),
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if paths.len() >= 2 => {
                let from = FileUri::local(&paths[0])?;
                let to = FileUri::local(&paths[1])?;
                let kind = if paths[0].parent() == paths[1].parent() {
                    WatchEventKind::Renamed
                } else {
                    WatchEventKind::Moved
                };
                Ok(vec![self.next_event(kind, to, Some(from))])
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                self.events_for_paths(WatchEventKind::Removed, paths)
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                self.events_for_paths(WatchEventKind::Created, paths)
            }
            EventKind::Modify(_) => self.events_for_paths(WatchEventKind::Modified, paths),
            EventKind::Other => {
                let uri = paths
                    .first()
                    .map(FileUri::local)
                    .transpose()?
                    .or_else(|| self.watched.values().next().cloned());
                Ok(uri
                    .map(|uri| self.next_event(WatchEventKind::Overflow, uri, None))
                    .into_iter()
                    .collect())
            }
            EventKind::Access(_) => Ok(Vec::new()),
            _ => Ok(Vec::new()),
        }
    }

    /// Pairs rename halves and suppresses duplicate rename/remove metadata echoes.
    ///
    /// Output order follows the raw burst except paired halves emit at the first
    /// encountered matching half. Conversion failure aborts the full burst.
    ///
    /// # Errors
    ///
    /// Propagates [`FileError::InvalidUri`] from any paired or ordinary native
    /// path conversion; no partial normalized batch is returned.
    fn normalize_events(&mut self, raw: Vec<Event>) -> Result<Vec<WatchEvent>, FileError> {
        let now = Instant::now();
        self.recent_rename_sources
            .retain(|_, emitted_at| now.duration_since(*emitted_at) <= WATCH_RENAME_ECHO_TTL);
        let mut rename_halves = HashMap::<usize, (Option<PathBuf>, Option<PathBuf>)>::new();
        let mut complete_trackers = HashSet::new();
        let mut renamed_from_paths = HashSet::new();
        let mut renamed_paths = HashSet::new();
        for event in &raw {
            let Some(tracker) = event.tracker() else {
                continue;
            };
            match event.kind {
                EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                    rename_halves.entry(tracker).or_default().0 = event.paths.first().cloned();
                }
                EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                    rename_halves.entry(tracker).or_default().1 = event.paths.first().cloned();
                }
                EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
                    complete_trackers.insert(tracker);
                    if let Some(from) = event.paths.first() {
                        renamed_from_paths.insert(from.clone());
                    }
                    renamed_paths.extend(event.paths.iter().cloned());
                }
                _ => {}
            }
        }
        renamed_from_paths.extend(rename_halves.values().filter_map(|(from, to)| {
            to.as_ref()?;
            from.clone()
        }));

        let mut emitted_pairs = HashSet::new();
        let mut normalized = Vec::new();
        for event in raw {
            if matches!(
                event.kind,
                EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(RenameMode::From))
            ) && !event.paths.is_empty()
                && event.paths.iter().all(|path| {
                    renamed_from_paths.contains(path)
                        || self.recent_rename_sources.contains_key(path)
                })
            {
                // A non-recursive watch attached to the renamed directory may
                // report its own old path as removed (or as an unpaired rename
                // source) after the parent already emitted the complete pair.
                // It is a self-watch echo, not a second filesystem mutation.
                continue;
            }
            if matches!(event.kind, EventKind::Modify(ModifyKind::Metadata(_)))
                && !event.paths.is_empty()
                && event.paths.iter().all(|path| renamed_paths.contains(path))
            {
                // Linux may append an inode-metadata notification to the same
                // debounced burst as a complete rename. The semantic rename
                // already updates that retained node; treating this duplicate
                // as a content modification would stale and reread its parent.
                continue;
            }
            let tracker = event.tracker();
            let rename_half = matches!(
                event.kind,
                EventKind::Modify(ModifyKind::Name(RenameMode::From | RenameMode::To))
            );
            if rename_half {
                if tracker.is_some_and(|tracker| complete_trackers.contains(&tracker)) {
                    continue;
                }
                if let Some(tracker) = tracker {
                    if let Some((Some(from), Some(to))) = rename_halves.get(&tracker) {
                        if emitted_pairs.insert(tracker) {
                            let from = FileUri::local(from)?;
                            let to = FileUri::local(to)?;
                            let kind = if from.parent() == to.parent() {
                                WatchEventKind::Renamed
                            } else {
                                WatchEventKind::Moved
                            };
                            normalized.push(self.next_event(kind, to, Some(from)));
                        }
                        continue;
                    }
                }
            }
            normalized.extend(self.map_event(event)?);
        }
        for event in &normalized {
            if matches!(
                event.kind(),
                WatchEventKind::Renamed | WatchEventKind::Moved
            ) {
                if let Some(path) = event
                    .previous_uri()
                    .and_then(|uri| uri.to_local_path().ok())
                {
                    self.recent_rename_sources.insert(path, now);
                }
            }
        }
        Ok(normalized)
    }

    /// Maps every supplied path to one event of `kind`, preserving path order.
    ///
    /// # Errors
    ///
    /// Returns [`FileError::InvalidUri`] when any path cannot become a local URI;
    /// the complete collection is discarded.
    fn events_for_paths(
        &mut self,
        kind: WatchEventKind,
        paths: Vec<PathBuf>,
    ) -> Result<Vec<WatchEvent>, FileError> {
        paths
            .into_iter()
            .map(|path| Ok(self.next_event(kind, FileUri::local(path)?, None)))
            .collect()
    }
}

/// Adapts local I/O, stable identity, mutations, and native watch to the worker contract.
impl FileTreeSource for LocalFileTreeSource {
    /// Delegates one sorted, non-recursive directory read to the local provider.
    ///
    /// # Errors
    ///
    /// Propagates the URI, directory iteration, and metadata errors documented
    /// by [`LocalFileProvider::read_dir`].
    fn read_dir(&mut self, uri: &FileUri) -> Result<Vec<FileEntry>, FileError> {
        self.provider.read_dir(uri)
    }

    /// Returns Unix device/inode identity or an unsupported error elsewhere.
    ///
    /// # Errors
    ///
    /// Returns [`FileError::InvalidUri`] for a non-local URI, a mapped metadata
    /// error on Unix, or [`FileError::Unsupported`] on non-Unix targets.
    fn identity(&mut self, uri: &FileUri) -> Result<Option<FileIdentity>, FileError> {
        identity_for_path(&uri.to_local_path()?).map(Some)
    }

    /// Creates one directory without recursively creating parents.
    ///
    /// # Errors
    ///
    /// Propagates the URI and host creation errors documented by
    /// [`LocalFileProvider::create_dir`].
    fn create_directory(&mut self, uri: &FileUri) -> Result<(), FileError> {
        self.provider.create_dir(uri)
    }

    /// Creates or truncates an empty file.
    ///
    /// # Errors
    ///
    /// Propagates the URI and host write errors documented by
    /// [`LocalFileProvider::write_file`].
    fn create_file(&mut self, uri: &FileUri) -> Result<(), FileError> {
        self.provider.write_file(uri, &[])
    }

    /// Moves through the provider's rename-only policy; no cross-device fallback exists.
    ///
    /// # Errors
    ///
    /// Propagates invalid local URIs and host rename failures from the provider.
    fn move_entry(&mut self, from: &FileUri, to: &FileUri) -> Result<(), FileError> {
        self.provider.move_entry(from, to)
    }

    /// Chooses recursive or single-entry removal exactly from `recursive`.
    ///
    /// # Errors
    ///
    /// Propagates invalid local URIs and host traversal/removal failures from
    /// the selected provider operation.
    fn remove_entry(&mut self, uri: &FileUri, recursive: bool) -> Result<(), FileError> {
        if recursive {
            self.provider.remove_recursive(uri)
        } else {
            self.provider.remove(uri)
        }
    }

    /// Adds one idempotent non-recursive native directory watch.
    ///
    /// A distinct watch at the configured ceiling returns `FileError::Other`.
    /// Successful additions saturating-increment the watch generation.
    ///
    /// # Errors
    ///
    /// Returns [`FileError::InvalidUri`] for a non-local URI,
    /// [`FileError::Other`] when the configured watcher ceiling is reached, or
    /// [`FileError::Io`] when the native watcher rejects the path.
    fn watch_directory(&mut self, uri: &FileUri) -> Result<(), FileError> {
        let path = uri.to_local_path()?;
        if self.watched.contains_key(&path) {
            return Ok(());
        }
        if self.watched.len() >= self.max_watchers {
            return Err(FileError::Other(format!(
                "local filesystem watcher limit reached ({})",
                self.max_watchers
            )));
        }
        self.watcher
            .watch(&path, RecursiveMode::NonRecursive)
            .map_err(notify_error)?;
        self.watched.insert(path, uri.clone());
        self.generation = self.generation.saturating_add(1);
        Ok(())
    }

    /// Removes a known watch idempotently and increments generation on success.
    ///
    /// Unknown paths return success without calling the native watcher.
    ///
    /// # Errors
    ///
    /// Returns [`FileError::InvalidUri`] for a non-local URI or [`FileError::Io`]
    /// when the native watcher fails to remove a known path.
    fn unwatch_directory(&mut self, uri: &FileUri) -> Result<(), FileError> {
        let path = uri.to_local_path()?;
        if self.watched.remove(&path).is_some() {
            self.watcher.unwatch(&path).map_err(notify_error)?;
            self.generation = self.generation.saturating_add(1);
        }
        Ok(())
    }

    /// Returns up to `limit` normalized events after the 50-millisecond debounce.
    ///
    /// Zero returns immediately without draining the raw channel. Later events
    /// remain pending; normalized overflow remains queued for future calls.
    /// Exact `(kind, uri, previous_uri)` duplicates are removed within each
    /// returned batch. A disconnected callback channel is an I/O error.
    ///
    /// # Errors
    ///
    /// Returns [`FileError::Io`] for a disconnected callback channel or native
    /// watcher error, and [`FileError::InvalidUri`] when normalizing an event path.
    fn poll_watch(&mut self, limit: usize) -> Result<Vec<WatchEvent>, FileError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        loop {
            match self.receiver.try_recv() {
                Ok(event) => self.pending.push_back(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    return Err(FileError::Io("local watch channel disconnected".into()))
                }
            }
        }
        let cutoff = Instant::now()
            .checked_sub(WATCH_DEBOUNCE)
            .unwrap_or_else(Instant::now);
        let mut raw = Vec::new();
        loop {
            let Some((received_at, _)) = self.pending.front() else {
                break;
            };
            if *received_at > cutoff {
                break;
            }
            let (_, event) = self.pending.pop_front().expect("front exists");
            raw.push(event.map_err(notify_error)?);
        }
        let normalized = self.normalize_events(raw)?;
        self.normalized.extend(normalized);

        let mut events = Vec::new();
        while events.len() < limit {
            let Some(event) = self.normalized.pop_front() else {
                break;
            };
            let duplicate = events.iter().any(|existing: &WatchEvent| {
                existing.kind() == event.kind()
                    && existing.uri() == event.uri()
                    && existing.previous_uri() == event.previous_uri()
            });
            if !duplicate {
                events.push(event);
            }
        }
        Ok(events)
    }

    /// Always reports native non-recursive watch support.
    fn supports_native_watch(&self) -> bool {
        true
    }
}

#[cfg(unix)]
/// Builds a 16-byte little-endian `(device, inode)` identity without following links.
///
/// # Errors
///
/// Maps failure to read symlink-aware metadata through [`FileError::from_io`].
fn identity_for_path(path: &Path) -> Result<FileIdentity, FileError> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::symlink_metadata(path)
        .map_err(|error| FileError::from_io(&error, path.display().to_string()))?;
    let mut value = Vec::with_capacity(16);
    value.extend_from_slice(&metadata.dev().to_le_bytes());
    value.extend_from_slice(&metadata.ino().to_le_bytes());
    Ok(FileIdentity::new("local-unix", value))
}

#[cfg(not(unix))]
/// Reports that this implementation has no stable non-Unix local identity.
///
/// # Errors
///
/// Always returns [`FileError::Unsupported`] on non-Unix targets.
fn identity_for_path(_path: &Path) -> Result<FileIdentity, FileError> {
    Err(FileError::Unsupported(
        "stable local file identity is not available on this target".into(),
    ))
}

/// Redacts backend structure into the public filesystem-watch I/O context.
fn notify_error(error: notify::Error) -> FileError {
    FileError::Io(format!("filesystem watch: {error}"))
}

#[cfg(test)]
/// Pure raw-event normalization regressions independent of live OS delivery.
mod tests {
    use super::*;

    /// Constructs a tracked rename event with ordered synthetic paths.
    fn rename_event(mode: RenameMode, tracker: usize, paths: &[&str]) -> Event {
        let mut event = Event::new(EventKind::Modify(ModifyKind::Name(mode))).set_tracker(tracker);
        for path in paths {
            event = event.add_path(PathBuf::from(path));
        }
        event
    }

    #[test]
    /// Verifies duplicate halves and a complete rename collapse to one rename.
    fn rename_from_to_and_both_collapse_to_one_semantic_event() {
        let mut source = LocalFileTreeSource::new().unwrap();
        let events = source
            .normalize_events(vec![
                rename_event(RenameMode::From, 7, &["/tmp/foo"]),
                rename_event(RenameMode::To, 7, &["/tmp/bar"]),
                rename_event(RenameMode::Both, 7, &["/tmp/foo", "/tmp/bar"]),
            ])
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind(), WatchEventKind::Renamed);
        assert_eq!(events[0].previous_uri().unwrap().path(), "/tmp/foo");
        assert_eq!(events[0].uri().path(), "/tmp/bar");
    }

    #[test]
    /// Verifies paired halves become one cross-parent move without `Both`.
    fn paired_halves_are_normalized_when_backend_omits_both() {
        let mut source = LocalFileTreeSource::new().unwrap();
        let events = source
            .normalize_events(vec![
                rename_event(RenameMode::From, 9, &["/tmp/a/foo"]),
                rename_event(RenameMode::To, 9, &["/tmp/b/foo"]),
            ])
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind(), WatchEventKind::Moved);
        assert_eq!(events[0].previous_uri().unwrap().path(), "/tmp/a/foo");
        assert_eq!(events[0].uri().path(), "/tmp/b/foo");
    }

    #[test]
    /// Verifies metadata echo after a complete rename is suppressed.
    fn metadata_echo_for_a_complete_rename_does_not_force_reconciliation() {
        let mut source = LocalFileTreeSource::new().unwrap();
        let metadata = Event::new(EventKind::Modify(ModifyKind::Metadata(
            notify::event::MetadataKind::Any,
        )))
        .add_path(PathBuf::from("/tmp/bar"));
        let events = source
            .normalize_events(vec![
                rename_event(RenameMode::Both, 11, &["/tmp/foo", "/tmp/bar"]),
                metadata,
            ])
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind(), WatchEventKind::Renamed);
    }

    #[test]
    /// Verifies a watched old-path removal echo does not duplicate rename.
    fn watched_path_remove_echo_for_a_complete_rename_is_suppressed() {
        let from = PathBuf::from("/tmp/foo");
        let to = PathBuf::from("/tmp/bar");
        let mut source = LocalFileTreeSource::new().unwrap();
        source
            .watched
            .insert(from.clone(), FileUri::local(&from).unwrap());
        let events = source
            .normalize_events(vec![
                rename_event(
                    RenameMode::Both,
                    12,
                    &[from.to_str().unwrap(), to.to_str().unwrap()],
                ),
                Event::new(EventKind::Remove(notify::event::RemoveKind::Folder)).add_path(from),
            ])
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind(), WatchEventKind::Renamed);
    }
}
