use ailloli_ui_fs::{
    DirectoryLoadState, FileEntry, FileError, FileIdentity, FileKind, FileMetadata, FileTreeDelta,
    FileTreeStore, FileTreeStoreError, FileTreeStoreLimits, FileUri, WatchEvent, WatchEventKind,
};
use std::time::{Duration, Instant};

fn root_store() -> FileTreeStore {
    FileTreeStore::new(
        FileUri::parse("file:///").unwrap(),
        FileMetadata::new(FileKind::Directory),
    )
    .unwrap()
}

fn entry(path: &str, kind: FileKind) -> (FileEntry, Option<FileIdentity>) {
    let uri = FileUri::parse(format!("file:///{path}")).unwrap();
    (
        FileEntry::new(uri, FileMetadata::new(kind)),
        Some(FileIdentity::new("test", path.as_bytes())),
    )
}

#[test]
fn directory_errors_remain_attached_to_the_requested_node() {
    let mut store = root_store();
    let root = store.root();
    let (request, _) = store.begin_directory_load(root).unwrap();
    store
        .apply_directory_result(&request, Err(FileError::PermissionDenied("/root".into())))
        .unwrap();
    assert!(matches!(
        store.node(root).unwrap().directory_state(),
        DirectoryLoadState::Error(FileError::PermissionDenied(path)) if path == "/root"
    ));
    assert_eq!(store.len(), 1);
}

#[test]
fn identity_preserves_node_id_across_an_external_rename() {
    let mut store = root_store();
    let root = store.root();
    let (request, _) = store.begin_directory_load(root).unwrap();
    store
        .apply_directory_result(&request, Ok(vec![entry("foo", FileKind::Directory)]))
        .unwrap();
    let old_uri = FileUri::parse("file:///foo").unwrap();
    let id = store.node_id(&old_uri).unwrap();

    let renamed = FileEntry::new(
        FileUri::parse("file:///bar").unwrap(),
        FileMetadata::new(FileKind::Directory),
    );
    let (request, _) = store.begin_directory_load(root).unwrap();
    store
        .apply_directory_result(
            &request,
            Ok(vec![(renamed, Some(FileIdentity::new("test", b"foo")))]),
        )
        .unwrap();
    assert_eq!(
        store.node_id(&FileUri::parse("file:///bar").unwrap()),
        Some(id)
    );
    assert!(store.node_id(&old_uri).is_none());
}

#[test]
fn hard_links_with_one_identity_remain_distinct_stable_entries() {
    let mut store = root_store();
    let root = store.root();
    let shared = FileIdentity::new("unix", b"same-device-and-inode");
    let entries = ["tool", "tool-a", "tool-b"]
        .into_iter()
        .map(|name| {
            (
                FileEntry::new(
                    FileUri::parse(format!("file:///{name}")).unwrap(),
                    FileMetadata::new(FileKind::File),
                ),
                Some(shared.clone()),
            )
        })
        .collect::<Vec<_>>();

    let (request, _) = store.begin_directory_load(root).unwrap();
    let first = store
        .apply_directory_result(&request, Ok(entries.clone()))
        .unwrap();
    assert_eq!(store.node(root).unwrap().children().len(), 3);
    assert_eq!(
        first
            .changes()
            .iter()
            .filter(|change| matches!(change, FileTreeDelta::Inserted { .. }))
            .count(),
        3
    );
    let ids = ["tool", "tool-a", "tool-b"].map(|name| {
        store
            .node_id(&FileUri::parse(format!("file:///{name}")).unwrap())
            .unwrap()
    });
    assert_ne!(ids[0], ids[1]);
    assert_ne!(ids[1], ids[2]);

    let (request, _) = store.begin_directory_load(root).unwrap();
    store.apply_directory_result(&request, Ok(entries)).unwrap();
    assert_eq!(store.node(root).unwrap().children(), &ids);
}

#[test]
fn stale_worker_responses_never_overwrite_a_new_generation() {
    let mut store = root_store();
    let root = store.root();
    let (request, _) = store.begin_directory_load(root).unwrap();
    store.invalidate_generation().unwrap();
    assert_eq!(
        store
            .apply_directory_result(&request, Ok(vec![entry("late", FileKind::File)]))
            .unwrap_err(),
        FileTreeStoreError::StaleResponse {
            request_id: request.request_id()
        }
    );
    assert_eq!(store.len(), 1);
}

#[test]
fn cancelled_directory_response_is_stale_and_a_new_request_can_start() {
    let mut store = root_store();
    let root = store.root();
    let (cancelled, _) = store.begin_directory_load(root).unwrap();
    let delta = store.cancel_directory_load(root).unwrap();
    assert!(matches!(
        store.node(root).unwrap().directory_state(),
        DirectoryLoadState::Stale
    ));
    assert!(!delta.is_empty());

    let (current, _) = store.begin_directory_load(root).unwrap();
    assert_ne!(cancelled.request_id(), current.request_id());
    assert_eq!(
        store
            .apply_directory_result(&cancelled, Ok(vec![entry("late", FileKind::File)]))
            .unwrap_err(),
        FileTreeStoreError::StaleResponse {
            request_id: cancelled.request_id()
        }
    );
    store
        .apply_directory_result(&current, Ok(vec![entry("current", FileKind::File)]))
        .unwrap();
    assert!(store
        .node_id(&FileUri::parse("file:///current").unwrap())
        .is_some());
    assert!(store
        .node_id(&FileUri::parse("file:///late").unwrap())
        .is_none());
}

#[test]
fn reconcile_emits_deltas_without_reusing_removed_ids() {
    let mut store = root_store();
    let root = store.root();
    let (request, _) = store.begin_directory_load(root).unwrap();
    store
        .apply_directory_result(
            &request,
            Ok(vec![entry("a", FileKind::File), entry("b", FileKind::File)]),
        )
        .unwrap();
    let a = store
        .node_id(&FileUri::parse("file:///a").unwrap())
        .unwrap();

    let (request, _) = store.begin_directory_load(root).unwrap();
    let delta = store
        .apply_directory_result(&request, Ok(vec![entry("c", FileKind::File)]))
        .unwrap();
    assert!(delta
        .changes()
        .iter()
        .any(|change| matches!(change, FileTreeDelta::Removed { id } if *id == a)));
    let c = store
        .node_id(&FileUri::parse("file:///c").unwrap())
        .unwrap();
    assert_ne!(a, c);
}

#[test]
fn paired_watch_rename_preserves_identity_expansion_and_descendant_uris() {
    let mut store = root_store();
    let root = store.root();
    let (request, _) = store.begin_directory_load(root).unwrap();
    store
        .apply_directory_result(&request, Ok(vec![entry("foo", FileKind::Directory)]))
        .unwrap();
    let foo = store
        .node_id(&FileUri::parse("file:///foo").unwrap())
        .unwrap();
    store.set_expanded(foo, true).unwrap();
    store.set_selected(foo, true).unwrap();
    let (request, _) = store.begin_directory_load(foo).unwrap();
    store
        .apply_directory_result(&request, Ok(vec![entry("foo/a", FileKind::File)]))
        .unwrap();
    let child = store
        .node_id(&FileUri::parse("file:///foo/a").unwrap())
        .unwrap();

    let event = WatchEvent::new(
        WatchEventKind::Renamed,
        FileUri::parse("file:///bar").unwrap(),
        1,
        1,
    )
    .with_previous_uri(FileUri::parse("file:///foo").unwrap())
    .with_identity(FileIdentity::new("test", b"foo"));
    store.apply_watch_event(&event).unwrap();

    assert_eq!(
        store.node_id(&FileUri::parse("file:///bar").unwrap()),
        Some(foo)
    );
    assert_eq!(
        store.node_id(&FileUri::parse("file:///bar/a").unwrap()),
        Some(child)
    );
    assert!(store.node(foo).unwrap().is_expanded());
    assert!(store.node(foo).unwrap().is_pinned());
}

#[test]
fn paired_watch_move_changes_parent_without_full_reload() {
    let mut store = root_store();
    let root = store.root();
    let (request, _) = store.begin_directory_load(root).unwrap();
    store
        .apply_directory_result(
            &request,
            Ok(vec![
                entry("a", FileKind::Directory),
                entry("b", FileKind::Directory),
            ]),
        )
        .unwrap();
    let a = store
        .node_id(&FileUri::parse("file:///a").unwrap())
        .unwrap();
    let b = store
        .node_id(&FileUri::parse("file:///b").unwrap())
        .unwrap();
    let (request, _) = store.begin_directory_load(a).unwrap();
    store
        .apply_directory_result(&request, Ok(vec![entry("a/x", FileKind::File)]))
        .unwrap();
    let x = store
        .node_id(&FileUri::parse("file:///a/x").unwrap())
        .unwrap();

    let event = WatchEvent::new(
        WatchEventKind::Moved,
        FileUri::parse("file:///b/x").unwrap(),
        1,
        1,
    )
    .with_previous_uri(FileUri::parse("file:///a/x").unwrap())
    .with_identity(FileIdentity::new("test", b"a/x"));
    let delta = store.apply_watch_event(&event).unwrap();

    assert_eq!(store.node(x).unwrap().parent(), Some(b));
    assert!(delta.changes().iter().any(|change| matches!(
        change,
        FileTreeDelta::Moved { id, new_parent, .. } if *id == x && *new_parent == b
    )));
    assert!(!store.node(a).unwrap().children().contains(&x));
    assert!(store.node(b).unwrap().children().contains(&x));
}

#[test]
fn a_new_watch_generation_restarts_sequence_without_dropping_events() {
    let mut store = root_store();
    let old = WatchEvent::new(
        WatchEventKind::Modified,
        FileUri::parse("file:///old").unwrap(),
        12,
        4,
    );
    store.apply_watch_event(&old).unwrap();
    let next_generation = WatchEvent::new(
        WatchEventKind::Created,
        FileUri::parse("file:///new").unwrap(),
        1,
        5,
    );
    store.apply_watch_event(&next_generation).unwrap();

    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.watch_events, 2);
    assert_eq!(diagnostics.duplicate_watch_events, 0);

    let stale_generation = WatchEvent::new(
        WatchEventKind::Created,
        FileUri::parse("file:///stale").unwrap(),
        99,
        4,
    );
    assert!(store
        .apply_watch_event(&stale_generation)
        .unwrap()
        .is_empty());
    assert_eq!(store.diagnostics().duplicate_watch_events, 1);
}

#[test]
fn collapsed_cache_eviction_retains_root_metadata_and_respects_pins() {
    let limits = FileTreeStoreLimits {
        collapsed_ttl: Duration::ZERO,
        ..FileTreeStoreLimits::default()
    };
    let mut store = FileTreeStore::with_limits(
        FileUri::parse("file:///").unwrap(),
        FileMetadata::new(FileKind::Directory),
        limits,
    )
    .unwrap();
    let root = store.root();
    store.set_expanded(root, true).unwrap();
    let (request, _) = store.begin_directory_load(root).unwrap();
    store
        .apply_directory_result(&request, Ok(vec![entry("cached", FileKind::Directory)]))
        .unwrap();
    let cached = store
        .node_id(&FileUri::parse("file:///cached").unwrap())
        .unwrap();
    store.set_expanded(cached, true).unwrap();
    let (request, _) = store.begin_directory_load(cached).unwrap();
    store
        .apply_directory_result(&request, Ok(vec![entry("cached/item", FileKind::File)]))
        .unwrap();
    let item = store
        .node_id(&FileUri::parse("file:///cached/item").unwrap())
        .unwrap();
    store.set_selected(item, true).unwrap();
    store.set_expanded(cached, false).unwrap();

    assert!(store.evict_expired(Instant::now()).unwrap().is_empty());
    assert!(
        store.node(item).is_some(),
        "a selected descendant pins the cache"
    );

    store.set_selected(item, false).unwrap();
    let delta = store.evict_expired(Instant::now()).unwrap();
    assert!(!delta.is_empty());
    assert!(store.node(cached).is_some());
    assert!(store.node(item).is_none());
    assert!(store.node(cached).unwrap().children().is_empty());
    assert!(matches!(
        store.node(cached).unwrap().directory_state(),
        DirectoryLoadState::Unloaded
    ));
    assert_eq!(store.diagnostics().evicted_nodes, 1);
}

#[test]
fn cache_pressure_evicts_collapsed_children_before_ttl_without_busy_timer() {
    let limits = FileTreeStoreLimits {
        max_nodes: 2,
        max_payload_bytes: usize::MAX,
        collapsed_ttl: Duration::from_secs(3_600),
    };
    let mut store = FileTreeStore::with_limits(
        FileUri::parse("file:///").unwrap(),
        FileMetadata::new(FileKind::Directory),
        limits,
    )
    .unwrap();
    let root = store.root();
    store.set_expanded(root, true).unwrap();
    let (request, _) = store.begin_directory_load(root).unwrap();
    store
        .apply_directory_result(&request, Ok(vec![entry("cached", FileKind::Directory)]))
        .unwrap();
    let cached = store
        .node_id(&FileUri::parse("file:///cached").unwrap())
        .unwrap();
    store.set_expanded(cached, true).unwrap();
    let (request, _) = store.begin_directory_load(cached).unwrap();
    store
        .apply_directory_result(&request, Ok(vec![entry("cached/item", FileKind::File)]))
        .unwrap();
    store.set_expanded(cached, false).unwrap();

    let now = Instant::now();
    assert_eq!(store.next_cache_maintenance_due(now), Some(now));
    let delta = store.evict_expired(now).unwrap();
    assert!(!delta.is_empty());
    assert_eq!(store.len(), 2);
    assert!(store.node(cached).unwrap().children().is_empty());
    assert!(matches!(
        store.node(cached).unwrap().directory_state(),
        DirectoryLoadState::Unloaded
    ));
    assert_eq!(store.next_cache_maintenance_due(now), None);
}

#[test]
fn store_diagnostics_count_io_results_errors_and_stale_responses() {
    let mut store = root_store();
    let root = store.root();
    let (failed, _) = store.begin_directory_load(root).unwrap();
    store
        .apply_directory_result(&failed, Err(FileError::PermissionDenied("root".into())))
        .unwrap();
    let (stale, _) = store.begin_directory_load(root).unwrap();
    store.invalidate_generation().unwrap();
    assert!(matches!(
        store.apply_directory_result(&stale, Ok(Vec::new())),
        Err(FileTreeStoreError::StaleResponse { .. })
    ));

    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.directory_loads_started, 2);
    assert_eq!(diagnostics.directory_results_applied, 1);
    assert_eq!(diagnostics.directory_errors, 1);
    assert_eq!(diagnostics.stale_responses, 1);
    assert_eq!(diagnostics.nodes, 1);
    assert!(diagnostics.estimated_payload_bytes > 0);
}

#[test]
fn attested_operations_apply_immediately_and_deduplicate_watcher_echoes() {
    let mut store = root_store();
    let root = store.root();
    let (request, _) = store.begin_directory_load(root).unwrap();
    store
        .apply_directory_result(
            &request,
            Ok(vec![
                entry("a", FileKind::Directory),
                entry("b", FileKind::Directory),
            ]),
        )
        .unwrap();
    let a = store
        .node_id(&FileUri::parse("file:///a").unwrap())
        .unwrap();
    let b = store
        .node_id(&FileUri::parse("file:///b").unwrap())
        .unwrap();

    let created_uri = FileUri::parse("file:///a/new").unwrap();
    store
        .apply_attested_insert(
            a,
            FileEntry::new(created_uri.clone(), FileMetadata::new(FileKind::Directory)),
            Some(FileIdentity::new("test", b"created")),
        )
        .unwrap();
    let created = store.node_id(&created_uri).unwrap();
    store.set_expanded(created, true).unwrap();
    store.set_selected(created, true).unwrap();
    let create_echo = WatchEvent::new(WatchEventKind::Created, created_uri.clone(), 1, 1);
    assert!(store.apply_watch_event(&create_echo).unwrap().is_empty());

    let moved_uri = FileUri::parse("file:///b/new").unwrap();
    store
        .apply_attested_move(
            created,
            moved_uri.clone(),
            Some(FileIdentity::new("test", b"created")),
        )
        .unwrap();
    assert_eq!(store.node(created).unwrap().parent(), Some(b));
    assert!(store.node(created).unwrap().is_expanded());
    assert!(store.node(created).unwrap().is_pinned());
    let move_echo = WatchEvent::new(WatchEventKind::Moved, moved_uri.clone(), 2, 1)
        .with_previous_uri(created_uri);
    assert!(store.apply_watch_event(&move_echo).unwrap().is_empty());

    store.apply_attested_remove(created).unwrap();
    assert!(store.node(created).is_none());
    let remove_echo = WatchEvent::new(WatchEventKind::Removed, moved_uri, 3, 1);
    assert!(store.apply_watch_event(&remove_echo).unwrap().is_empty());
    assert_eq!(store.diagnostics().duplicate_watch_events, 3);
}

#[test]
fn reserved_create_identity_is_committed_once_and_never_reused() {
    let mut store = root_store();
    let root = store.root();
    let reserved = store.reserve_node_id().unwrap();
    let uri = FileUri::parse("file:///draft.txt").unwrap();
    let delta = store
        .apply_attested_insert_reserved(
            root,
            reserved,
            FileEntry::new(uri.clone(), FileMetadata::new(FileKind::File)),
            None,
        )
        .unwrap();
    assert!(!delta.is_empty());
    assert_eq!(store.node_id(&uri), Some(reserved));
    assert!(matches!(
        store.apply_attested_insert_reserved(
            root,
            reserved,
            FileEntry::new(uri, FileMetadata::new(FileKind::File)),
            None,
        ),
        Err(FileTreeStoreError::InvalidReservedNodeId(id)) if id == reserved
    ));

    let cancelled = store.reserve_node_id().unwrap();
    store.discard_reserved_node_id(cancelled).unwrap();
    let next = store.reserve_node_id().unwrap();
    assert!(next.get() > cancelled.get());
}
