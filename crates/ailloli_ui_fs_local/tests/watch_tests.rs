use std::fs;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ailloli_ui_fs::{FileTreeSource, FileUri, WatchEventKind};
use ailloli_ui_fs_local::LocalFileTreeSource;

struct TempDir(std::path::PathBuf);

impl TempDir {
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

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
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
