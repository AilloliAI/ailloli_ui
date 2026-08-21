use std::collections::{HashMap, VecDeque};
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

type RawWatchEvent = (Instant, notify::Result<Event>);

#[derive(Debug, Clone, Default)]
pub struct LocalFileTreeSourceFactory;

impl FileTreeSourceFactory for LocalFileTreeSourceFactory {
    fn create(&self) -> Result<Box<dyn FileTreeSource>, FileError> {
        Ok(Box::new(LocalFileTreeSource::new()?))
    }
}

/// Worker-owned local source with non-recursive native directory watches.
pub struct LocalFileTreeSource {
    provider: LocalFileProvider,
    watcher: RecommendedWatcher,
    receiver: Receiver<RawWatchEvent>,
    pending: VecDeque<RawWatchEvent>,
    watched: HashMap<PathBuf, FileUri>,
    next_sequence: u64,
    generation: u64,
}

impl LocalFileTreeSource {
    pub fn new() -> Result<Self, FileError> {
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
            watched: HashMap::new(),
            next_sequence: 1,
            generation: 1,
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

    fn watch_directory(&mut self, uri: &FileUri) -> Result<(), FileError> {
        let path = uri.to_local_path()?;
        if self.watched.contains_key(&path) {
            return Ok(());
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
        let mut events = Vec::new();
        while events.len() < limit {
            let Some((received_at, _)) = self.pending.front() else {
                break;
            };
            if *received_at > cutoff {
                break;
            }
            let (_, event) = self.pending.pop_front().expect("front exists");
            let mapped = self.map_event(event.map_err(notify_error)?)?;
            for event in mapped {
                let duplicate = events.iter().any(|existing: &WatchEvent| {
                    existing.kind() == event.kind()
                        && existing.uri() == event.uri()
                        && existing.previous_uri() == event.previous_uri()
                });
                if !duplicate {
                    events.push(event);
                    if events.len() == limit {
                        break;
                    }
                }
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
