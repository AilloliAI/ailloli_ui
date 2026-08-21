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

const WATCH_DEBOUNCE: Duration = Duration::from_millis(50);
pub const DEFAULT_LOCAL_FILE_TREE_MAX_WATCHERS: usize = 1_024;

type RawWatchEvent = (Instant, notify::Result<Event>);

#[derive(Debug, Clone)]
pub struct LocalFileTreeSourceFactory {
    max_watchers: usize,
}

impl LocalFileTreeSourceFactory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn max_watchers(mut self, max_watchers: usize) -> Self {
        self.max_watchers = max_watchers;
        self
    }
}

impl Default for LocalFileTreeSourceFactory {
    fn default() -> Self {
        Self {
            max_watchers: DEFAULT_LOCAL_FILE_TREE_MAX_WATCHERS,
        }
    }
}

impl FileTreeSourceFactory for LocalFileTreeSourceFactory {
    fn create(&self) -> Result<Box<dyn FileTreeSource>, FileError> {
        Ok(Box::new(LocalFileTreeSource::with_max_watchers(
            self.max_watchers,
        )?))
    }
}

/// Worker-owned local source with non-recursive native directory watches.
pub struct LocalFileTreeSource {
    provider: LocalFileProvider,
    watcher: RecommendedWatcher,
    receiver: Receiver<RawWatchEvent>,
    pending: VecDeque<RawWatchEvent>,
    normalized: VecDeque<WatchEvent>,
    watched: HashMap<PathBuf, FileUri>,
    next_sequence: u64,
    generation: u64,
    max_watchers: usize,
}

impl LocalFileTreeSource {
    pub fn new() -> Result<Self, FileError> {
        Self::with_max_watchers(DEFAULT_LOCAL_FILE_TREE_MAX_WATCHERS)
    }

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
            watched: HashMap::new(),
            next_sequence: 1,
            generation: 1,
            max_watchers,
        })
    }

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

    fn normalize_events(&mut self, raw: Vec<Event>) -> Result<Vec<WatchEvent>, FileError> {
        let mut rename_halves = HashMap::<usize, (Option<PathBuf>, Option<PathBuf>)>::new();
        let mut complete_trackers = HashSet::new();
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
                }
                _ => {}
            }
        }

        let mut emitted_pairs = HashSet::new();
        let mut normalized = Vec::new();
        for event in raw {
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
        Ok(normalized)
    }

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

impl FileTreeSource for LocalFileTreeSource {
    fn read_dir(&mut self, uri: &FileUri) -> Result<Vec<FileEntry>, FileError> {
        self.provider.read_dir(uri)
    }

    fn identity(&mut self, uri: &FileUri) -> Result<Option<FileIdentity>, FileError> {
        identity_for_path(&uri.to_local_path()?).map(Some)
    }

    fn create_directory(&mut self, uri: &FileUri) -> Result<(), FileError> {
        self.provider.create_dir(uri)
    }

    fn create_file(&mut self, uri: &FileUri) -> Result<(), FileError> {
        self.provider.write_file(uri, &[])
    }

    fn move_entry(&mut self, from: &FileUri, to: &FileUri) -> Result<(), FileError> {
        self.provider.move_entry(from, to)
    }

    fn remove_entry(&mut self, uri: &FileUri, recursive: bool) -> Result<(), FileError> {
        if recursive {
            self.provider.remove_recursive(uri)
        } else {
            self.provider.remove(uri)
        }
    }

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

    fn unwatch_directory(&mut self, uri: &FileUri) -> Result<(), FileError> {
        let path = uri.to_local_path()?;
        if self.watched.remove(&path).is_some() {
            self.watcher.unwatch(&path).map_err(notify_error)?;
            self.generation = self.generation.saturating_add(1);
        }
        Ok(())
    }

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

    fn supports_native_watch(&self) -> bool {
        true
    }
}

#[cfg(unix)]
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
fn identity_for_path(_path: &Path) -> Result<FileIdentity, FileError> {
    Err(FileError::Unsupported(
        "stable local file identity is not available on this target".into(),
    ))
}

fn notify_error(error: notify::Error) -> FileError {
    FileError::Io(format!("filesystem watch: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rename_event(mode: RenameMode, tracker: usize, paths: &[&str]) -> Event {
        let mut event = Event::new(EventKind::Modify(ModifyKind::Name(mode))).set_tracker(tracker);
        for path in paths {
            event = event.add_path(PathBuf::from(path));
        }
        event
    }

    #[test]
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
}
