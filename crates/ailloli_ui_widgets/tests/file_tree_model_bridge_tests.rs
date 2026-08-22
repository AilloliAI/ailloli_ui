#![cfg(feature = "files")]
//! File-store delta projection into retained tree-model scenarios.

use ailloli_ui_fs::{FileEntry, FileIdentity, FileKind, FileMetadata, FileTreeStore, FileUri};
use ailloli_ui_widgets::files::FileTreeModelBridge;

#[test]
fn filesystem_deltas_update_the_retained_model_without_recursive_snapshots() {
    let mut store = FileTreeStore::new(
        FileUri::parse("file:///").unwrap(),
        FileMetadata::new(FileKind::Directory),
    )
    .unwrap();
    let bridge = FileTreeModelBridge::from_store(&store).unwrap();
    let root = store.root();
    let (request, loading) = store.begin_directory_load(root).unwrap();
    bridge.apply_delta(&store, &loading).unwrap();
    let bin_uri = FileUri::parse("file:///bin").unwrap();
    let loaded = store
        .apply_directory_result(
            &request,
            Ok(vec![(
                FileEntry::new(bin_uri.clone(), FileMetadata::new(FileKind::Directory)),
                Some(FileIdentity::new("test", b"bin")),
            )]),
        )
        .unwrap();
    bridge.apply_delta(&store, &loaded).unwrap();
    let bin = store.node_id(&bin_uri).unwrap();
    assert_eq!(bridge.model().read(|model| model.len()), 2);

    let expanded = store.set_expanded(bin, true).unwrap();
    bridge.apply_delta(&store, &expanded).unwrap();
    assert!(bridge.model().read(|model| model.is_expanded(&bin)));
}

#[test]
fn a_pending_clear_followed_by_removal_projects_the_final_store_state() {
    let mut store = FileTreeStore::new(
        FileUri::parse("file:///workspace").unwrap(),
        FileMetadata::new(FileKind::Directory),
    )
    .unwrap();
    let bridge = FileTreeModelBridge::from_store(&store).unwrap();
    let root = store.root();
    let (request, loading) = store.begin_directory_load(root).unwrap();
    bridge.apply_delta(&store, &loading).unwrap();
    let file_uri = FileUri::parse("file:///workspace/main.rs").unwrap();
    let loaded = store
        .apply_directory_result(
            &request,
            Ok(vec![(
                FileEntry::new(file_uri.clone(), FileMetadata::new(FileKind::File)),
                None,
            )]),
        )
        .unwrap();
    bridge.apply_delta(&store, &loaded).unwrap();
    let file = store.node_id(&file_uri).unwrap();
    let pending = store.set_pending_operation(file, true).unwrap();
    bridge.apply_delta(&store, &pending).unwrap();

    let cleared = store.set_pending_operation(file, false).unwrap();
    let removed = store.apply_attested_remove(file).unwrap();
    bridge.apply_delta(&store, &cleared).unwrap();
    bridge.apply_delta(&store, &removed).unwrap();

    assert!(store.node(file).is_none());
    assert!(bridge.model().read(|model| model.item(&file).is_none()));
    assert_eq!(bridge.model().read(|model| model.len()), 1);
}
