use ailloli_ui_fs::{
    DirectoryLoadState, FileEntry, FileError, FileIdentity, FileKind, FileMetadata, FileTreeDelta,
    FileTreeStore, FileTreeStoreError, FileUri, WatchEvent, WatchEventKind,
};

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
