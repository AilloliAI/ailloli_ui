//! Live native-watch regressions with bounded three-second deadlines.

use std::fs;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ailloli_ui_fs::{FileTreeSource, FileUri, WatchEventKind};
use ailloli_ui_fs_local::LocalFileTreeSource;

/// Recoverable unique temporary directory for one live watch test.
struct TempDir(std::path::PathBuf);

/// Creates the temporary watch root.
impl TempDir {
    /// Allocates a process/time-qualified directory under the OS temp root.
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ailloli_ui_fs_watch_{}_{}",
            std::process::id(),
            nonce
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

/// Recursively removes the temporary root on scope exit.
impl Drop for TempDir {
    /// Performs best-effort cleanup without masking a test result.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
/// Verifies non-recursive delivery and the debounce delay for a direct child.
fn local_watch_is_non_recursive_and_debounced() {
    let temp = TempDir::new();
    let nested = temp.0.join("nested");
    fs::create_dir(&nested).unwrap();
    let root = FileUri::local(&temp.0).unwrap();
    let mut source = LocalFileTreeSource::new().unwrap();
    source.watch_directory(&root).unwrap();
    fs::write(temp.0.join("direct.txt"), b"direct").unwrap();
    fs::write(nested.join("deep.txt"), b"deep").unwrap();

    let deadline = Instant::now() + Duration::from_secs(3);
    let events = loop {
        std::thread::sleep(Duration::from_millis(20));
        let events = source.poll_watch(256).unwrap();
        if !events.is_empty() {
            break events;
        }
        assert!(Instant::now() < deadline, "watch event timeout");
    };
    assert!(events.iter().any(|event| {
        event.kind() == WatchEventKind::Created && event.uri().file_name() == Some("direct.txt")
    }));
    assert!(!events
        .iter()
        .any(|event| event.uri().file_name() == Some("deep.txt")));
}

#[test]
/// Verifies explicit watch ceilings and idempotent duplicate registration.
fn local_watch_limit_is_explicit_and_existing_watches_are_idempotent() {
    let first = TempDir::new();
    let second = TempDir::new();
    let first_uri = FileUri::local(&first.0).unwrap();
    let second_uri = FileUri::local(&second.0).unwrap();
    let mut source = LocalFileTreeSource::with_max_watchers(1).unwrap();
    source.watch_directory(&first_uri).unwrap();
    source.watch_directory(&first_uri).unwrap();
    let error = source.watch_directory(&second_uri).unwrap_err();
    assert!(error.to_string().contains("watcher limit reached (1)"));
    source.unwatch_directory(&first_uri).unwrap();
    source.watch_directory(&second_uri).unwrap();
}

#[test]
/// Verifies a watched-directory rename suppresses native self-watch echoes.
fn renaming_a_watched_directory_emits_only_the_semantic_rename() {
    let temp = TempDir::new();
    let from_path = temp.0.join("foo");
    let to_path = temp.0.join("bar");
    fs::create_dir(&from_path).unwrap();
    let root_uri = FileUri::local(&temp.0).unwrap();
    let from_uri = FileUri::local(&from_path).unwrap();
    let to_uri = FileUri::local(&to_path).unwrap();
    let mut source = LocalFileTreeSource::new().unwrap();
    source.watch_directory(&root_uri).unwrap();
    source.watch_directory(&from_uri).unwrap();

    fs::rename(&from_path, &to_path).unwrap();

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut events = Vec::new();
    let mut last_event_at = None;
    loop {
        std::thread::sleep(Duration::from_millis(20));
        let batch = source.poll_watch(256).unwrap();
        if !batch.is_empty() {
            last_event_at = Some(Instant::now());
            events.extend(batch);
        }
        if last_event_at.is_some_and(|last| last.elapsed() >= Duration::from_millis(150)) {
            break;
        }
        assert!(Instant::now() < deadline, "watch event timeout: {events:?}");
    }

    assert_eq!(events.len(), 1, "unexpected watcher echo: {events:?}");
    assert_eq!(events[0].kind(), WatchEventKind::Renamed);
    assert_eq!(events[0].previous_uri(), Some(&from_uri));
    assert_eq!(events[0].uri(), &to_uri);
}
