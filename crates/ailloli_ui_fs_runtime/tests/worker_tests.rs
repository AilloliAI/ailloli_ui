use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use ailloli_ui_fs::{
    FileEntry, FileError, FileIdentity, FileKind, FileMetadata, FileTreeSource,
    FileTreeSourceFactory, FileTreeStore, FileUri,
};
use ailloli_ui_fs_runtime::{
    FileTreeEnqueueOutcome, FileTreeMutation, FileTreeReconcileScheduler, FileTreeRuntime,
    FILE_TREE_REMOTE_POLL_INTERVAL, FILE_TREE_REMOTE_POLL_MAX_BACKOFF,
};

#[derive(Default)]
struct SourceState {
    creator: Mutex<Option<ThreadId>>,
    reads: AtomicUsize,
    mutations: Mutex<Vec<String>>,
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

    fn identity(&mut self, uri: &FileUri) -> Result<Option<FileIdentity>, FileError> {
        Ok(Some(FileIdentity::new(
            "worker-test",
            uri.to_string().into_bytes(),
        )))
    }

    fn create_directory(&mut self, uri: &FileUri) -> Result<(), FileError> {
        self.0
            .mutations
            .lock()
            .unwrap()
            .push(format!("create:{uri}"));
        Ok(())
    }

    fn move_entry(&mut self, from: &FileUri, to: &FileUri) -> Result<(), FileError> {
        self.0
            .mutations
            .lock()
            .unwrap()
            .push(format!("move:{from}->{to}"));
        Ok(())
    }

    fn remove_entry(&mut self, uri: &FileUri, recursive: bool) -> Result<(), FileError> {
        self.0
            .mutations
            .lock()
            .unwrap()
            .push(format!("remove:{uri}:{recursive}"));
        Ok(())
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
    assert_eq!(runtime.stats().active_directory_requests, 0);
    assert!(runtime.stats().request_queue_max_depth >= 1);
    assert!(!runtime.supports_native_watch());
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

#[test]
fn a_reopened_directory_supersedes_the_cancelled_worker_request() {
    let state = Arc::new(SourceState::default());
    let mut runtime = FileTreeRuntime::spawn(Arc::new(Factory(state.clone()))).unwrap();
    let mut store = store();
    let root = store.root();
    let (cancelled, _) = store.begin_directory_load(root).unwrap();
    runtime.request_directory(cancelled).unwrap();
    store.cancel_directory_load(root).unwrap();
    let (current, _) = store.begin_directory_load(root).unwrap();
    runtime.request_directory(current).unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    while store.len() == 1 {
        let report = runtime.drain_into_store(&mut store).unwrap();
        assert!(report.stale_responses <= 1);
        assert!(Instant::now() < deadline, "worker response timeout");
        std::thread::yield_now();
    }
    assert_eq!(state.reads.load(Ordering::Relaxed), 2);
    assert_eq!(runtime.stats().active_directory_requests, 0);
    runtime.finish().unwrap();
}

#[test]
fn remote_polling_only_schedules_expanded_directories_with_bounded_backoff() {
    let mut scheduler = FileTreeReconcileScheduler::new(false);
    let root = store().root();
    let start = Instant::now();
    scheduler.set_expanded(root, true, start);
    assert!(scheduler.due(start, 256).is_empty());
    assert_eq!(
        scheduler.due(start + FILE_TREE_REMOTE_POLL_INTERVAL, 256),
        vec![root]
    );

    let mut now = start + FILE_TREE_REMOTE_POLL_INTERVAL;
    for _ in 0..8 {
        scheduler.note_error(root, now);
        now += FILE_TREE_REMOTE_POLL_MAX_BACKOFF;
    }
    assert!(scheduler.due(now, 256).contains(&root));
    scheduler.note_success(root, now);
    assert!(scheduler.due(now, 256).is_empty());
    assert!(scheduler
        .due(now + FILE_TREE_REMOTE_POLL_INTERVAL, 256)
        .contains(&root));

    scheduler.set_expanded(root, false, now);
    assert!(scheduler.is_empty());
}

#[test]
fn native_watch_scheduler_never_creates_a_polling_loop() {
    let mut scheduler = FileTreeReconcileScheduler::new(true);
    let root = store().root();
    let now = Instant::now();
    scheduler.set_expanded(root, true, now);
    assert!(scheduler.is_empty());
    assert!(scheduler.due(now + Duration::from_secs(60), 256).is_empty());
}

#[test]
fn provider_mutations_run_on_the_worker_then_update_the_store() {
    let state = Arc::new(SourceState::default());
    let mut runtime = FileTreeRuntime::spawn(Arc::new(Factory(state.clone()))).unwrap();
    let mut store = store();
    let root = store.root();
    let created_uri = FileUri::parse("file:///tmp/created").unwrap();
    let create = FileTreeMutation::CreateDirectory {
        parent: root,
        uri: created_uri.clone(),
    };
    let enqueued = runtime
        .request_mutation(&mut store, create.clone())
        .unwrap();
    assert_eq!(enqueued.outcome, FileTreeEnqueueOutcome::Enqueued);
    assert!(store.node(root).unwrap().is_pinned());
    assert_eq!(
        runtime
            .request_mutation(&mut store, create)
            .unwrap()
            .outcome,
        FileTreeEnqueueOutcome::Coalesced
    );
    wait_until(&mut runtime, &mut store, |store| {
        store.node_id(&created_uri).is_some()
    });

    let created = store.node_id(&created_uri).unwrap();
    let moved_uri = FileUri::parse("file:///tmp/moved").unwrap();
    runtime
        .request_mutation(
            &mut store,
            FileTreeMutation::Move {
                node_id: created,
                from: created_uri,
                to: moved_uri.clone(),
            },
        )
        .unwrap();
    wait_until(&mut runtime, &mut store, |store| {
        store.node_id(&moved_uri) == Some(created)
    });

    runtime
        .request_mutation(
            &mut store,
            FileTreeMutation::Remove {
                node_id: created,
                uri: moved_uri,
                recursive: true,
            },
        )
        .unwrap();
    wait_until(&mut runtime, &mut store, |store| {
        store.node(created).is_none()
    });

    assert_eq!(
        state.mutations.lock().unwrap().as_slice(),
        [
            "create:file:///tmp/created",
            "move:file:///tmp/created->file:///tmp/moved",
            "remove:file:///tmp/moved:true",
        ]
    );
    assert_eq!(runtime.stats().mutations, 3);
    runtime.finish().unwrap();
}

fn wait_until(
    runtime: &mut FileTreeRuntime,
    store: &mut FileTreeStore,
    ready: impl Fn(&FileTreeStore) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !ready(store) {
        let report = runtime.drain_into_store(store).unwrap();
        assert!(report.mutation_errors.is_empty());
        assert!(Instant::now() < deadline, "worker mutation timeout");
        std::thread::yield_now();
    }
}
