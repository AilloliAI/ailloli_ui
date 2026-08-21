use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use ailloli_ui_fs::{
    FileEntry, FileError, FileKind, FileMetadata, FileTreeSource, FileTreeSourceFactory,
    FileTreeStore, FileUri,
};
use ailloli_ui_fs_runtime::{FileTreeEnqueueOutcome, FileTreeRuntime};

#[derive(Default)]
struct SourceState {
    creator: Mutex<Option<ThreadId>>,
    reads: AtomicUsize,
}

struct Factory(Arc<SourceState>);

impl FileTreeSourceFactory for Factory {
    fn create(&self) -> Result<Box<dyn FileTreeSource>, FileError> {
        *self.0.creator.lock().unwrap() = Some(std::thread::current().id());
        Ok(Box::new(Source(self.0.clone())))
    }
}

struct Source(Arc<SourceState>);

impl FileTreeSource for Source {
    fn read_dir(&mut self, uri: &FileUri) -> Result<Vec<FileEntry>, FileError> {
        self.0.reads.fetch_add(1, Ordering::Relaxed);
        Ok(vec![FileEntry::new(
            uri.join_child("child")?,
            FileMetadata::new(FileKind::File),
        )])
    }
}

fn store() -> FileTreeStore {
    FileTreeStore::new(
        FileUri::parse("file:///tmp").unwrap(),
        FileMetadata::new(FileKind::Directory),
    )
    .unwrap()
}

#[test]
fn source_is_worker_owned_and_directory_requests_are_coalesced() {
    let state = Arc::new(SourceState::default());
    let ui_thread = std::thread::current().id();
    let mut runtime = FileTreeRuntime::spawn(Arc::new(Factory(state.clone()))).unwrap();
    assert_ne!(*state.creator.lock().unwrap(), Some(ui_thread));
    let mut store = store();
    let (request, _) = store.begin_directory_load(store.root()).unwrap();
    assert_eq!(
        runtime.request_directory(request.clone()).unwrap(),
        FileTreeEnqueueOutcome::Enqueued
    );
    assert_eq!(
        runtime.request_directory(request).unwrap(),
        FileTreeEnqueueOutcome::Coalesced
    );

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let report = runtime.drain_into_store(&mut store).unwrap();
        if !report.deltas.is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "worker response timeout");
        std::thread::yield_now();
    }
    assert_eq!(state.reads.load(Ordering::Relaxed), 1);
    assert_eq!(store.len(), 2);
    assert_eq!(runtime.stats().requests_coalesced, 1);
    runtime.finish().unwrap();
}

#[test]
fn stale_store_generation_drops_owned_response_without_retry() {
    let state = Arc::new(SourceState::default());
    let mut runtime = FileTreeRuntime::spawn(Arc::new(Factory(state))).unwrap();
    let mut store = store();
    let (request, _) = store.begin_directory_load(store.root()).unwrap();
    runtime.request_directory(request).unwrap();
    store.invalidate_generation().unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let report = runtime.drain_into_store(&mut store).unwrap();
        if report.stale_responses == 1 {
            break;
        }
        assert!(Instant::now() < deadline, "worker response timeout");
        std::thread::yield_now();
    }
    assert_eq!(store.len(), 1);
    assert_eq!(runtime.stats().stale_responses, 1);
    runtime.finish().unwrap();
}
