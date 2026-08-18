#![cfg(feature = "files")]

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent, WheelDelta};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Color, IconId, Point};
use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State};
use ailloli_ui_runtime::element::ElementKind;
use ailloli_ui_runtime::input::{absolute_paint_bounds, dispatch_event_to_target, InputRouter};
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::files::{
    file_icon_for_name, file_icon_visual_for_entry, file_icon_visual_for_name, flatten_file_nodes,
    sort_file_nodes, FileExplorer, FileExplorerAction, FileExplorerCreateDir, FileExplorerNode,
    FileExplorerRename,
};
#[cfg(feature = "files-local")]
use ailloli_ui_widgets::files::{
    local_file_tree_nodes, FileTreeLoadMode, FileTreeOptions, LocalFileExplorer,
};
use ailloli_ui_widgets::layout::Row;
use ailloli_ui_widgets::text::Text;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Select(FileUri),
    Open(FileUri),
    Toggle(FileUri, bool),
    Rename(FileExplorerRename),
    Remove(FileUri, Option<FileUri>),
    CreateDir(FileExplorerCreateDir),
    Any(FileExplorerAction),
}

#[test]
fn file_explorer_models_sort_flatten_and_pick_devicons() {
    let src = uri("/repo/src");
    let mut nodes = vec![
        FileExplorerNode::file(uri("/repo/README.md"), "README.md"),
        FileExplorerNode::file(uri("/repo/main.rs"), "main.rs"),
        FileExplorerNode::directory(src.clone(), "src")
            .child(FileExplorerNode::file(uri("/repo/src/app.ts"), "app.ts")),
    ];

    sort_file_nodes(&mut nodes);
    assert_eq!(nodes[0].name(), "src");

    let collapsed = flatten_file_nodes(&nodes, &[]);
    assert_eq!(collapsed.len(), 3);

    let expanded = flatten_file_nodes(&nodes, &[src]);
    assert_eq!(expanded.len(), 4);
    assert_eq!(expanded[1].depth, 1);

    assert_eq!(file_icon_for_name("main.rs"), IconId::Devicon('\u{e68b}'));
    assert_eq!(
        file_icon_for_name("Cargo.toml"),
        IconId::Devicon('\u{e6b2}')
    );
    assert_eq!(file_icon_for_name("README.md"), IconId::Devicon('\u{f48a}'));
    assert_eq!(file_icon_for_name("app.ts"), IconId::Devicon('\u{e628}'));
    assert_eq!(file_icon_for_name("unknown"), IconId::Devicon('\u{f15b}'));

    let folder = FileExplorerNode::directory(uri("/repo/src"), "src");
    let folder_visual = file_icon_visual_for_entry(&folder.entry);
    assert_eq!(folder_visual.icon, IconId::Devicon('\u{f07b}'));
    assert_eq!(folder_visual.color, Some(Color::hex_rgb(0xf59e0b)));

    let symlink_folder = symlink_node("/repo/bin", "bin", Some(FileKind::Directory));
    assert!(symlink_folder.is_branch());
    let symlink_visual = file_icon_visual_for_entry(&symlink_folder.entry);
    assert_eq!(symlink_visual.icon, IconId::Devicon('\u{f07b}'));
    assert_eq!(symlink_visual.color, Some(Color::hex_rgb(0x22c55e)));

    let rust_visual = file_icon_visual_for_name("main.rs");
    assert_eq!(rust_visual.color, Color::hex("#dea584").ok());
}

#[test]
fn file_explorer_paints_devicon_color_overrides() {
    let root = uri("/repo");
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        FileExplorer::<Action>::new([FileExplorerNode::directory(root.clone(), "repo")
            .child(FileExplorerNode::file(uri("/repo/main.rs"), "main.rs"))])
        .default_expanded(root)
        .width(280.0)
        .height(120.0)
        .into_view(),
    );
    layout_app(&mut app);

    let mut text_system = TextSystem::new();
    let scene = app.paint(&mut text_system);
    assert!(scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .any(|cmd| {
            matches!(
                cmd,
                DrawCmd::Image(img)
                    if img.icon == IconId::Devicon('\u{e68b}')
                        && img.tint == Color::hex("#dea584").expect("rust color")
            )
        }));
    assert!(scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .any(|cmd| {
            matches!(
                cmd,
                DrawCmd::Image(img)
                    if img.icon == IconId::Devicon('\u{f07b}')
                        && img.tint == Color::hex_rgb(0xf59e0b)
            )
        }));
}

#[test]
fn file_explorer_paints_symlink_directory_folder_green() {
    let root = uri("/repo");
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        FileExplorer::<Action>::new([FileExplorerNode::directory(root.clone(), "repo")
            .child(symlink_node("/repo/bin", "bin", Some(FileKind::Directory)))])
        .default_expanded(root)
        .width(280.0)
        .height(120.0)
        .into_view(),
    );
    layout_app(&mut app);

    let mut text_system = TextSystem::new();
    let scene = app.paint(&mut text_system);
    assert!(scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .any(|cmd| {
            matches!(
                cmd,
                DrawCmd::Image(img)
                    if img.icon == IconId::Devicon('\u{f07b}')
                        && img.tint == Color::hex_rgb(0x22c55e)
            )
        }));
}

#[cfg(feature = "files-local")]
#[test]
fn local_file_tree_nodes_build_from_path_and_reveal_selected() {
    let temp = TempDir::new("path_api");
    std::fs::create_dir_all(temp.path.join("sample_app/src/view/panes")).expect("dirs");
    std::fs::write(temp.path.join("sample_app/src/view/panes/left.rs"), b"left").expect("left");
    std::fs::write(temp.path.join("Cargo.toml"), b"cargo").expect("cargo");

    let nodes = local_file_tree_nodes(
        &temp.path,
        [std::path::PathBuf::from("sample_app/src/view/panes")],
        Some(std::path::PathBuf::from(
            "sample_app/src/view/panes/left.rs",
        )),
        FileTreeOptions::default(),
    )
    .expect("nodes");

    let root = &nodes[0];
    let sample = root
        .children
        .iter()
        .find(|node| node.name() == "sample_app")
        .expect("sample_app");
    let src = sample
        .children
        .iter()
        .find(|node| node.name() == "src")
        .expect("src");
    let view = src
        .children
        .iter()
        .find(|node| node.name() == "view")
        .expect("view");
    assert!(view
        .children
        .iter()
        .any(|node| node.name() == "panes" && node.is_branch()));
}

#[cfg(feature = "files-local")]
#[test]
fn local_file_tree_nodes_exposes_lazy_controlled_and_full_loading() {
    let temp = TempDir::new("load_modes");
    std::fs::create_dir_all(temp.path.join("src/nested")).expect("src dirs");
    std::fs::create_dir_all(temp.path.join("sample_app")).expect("sample dir");
    std::fs::write(temp.path.join("src/lib.rs"), b"lib").expect("lib");
    std::fs::write(temp.path.join("src/nested/mod.rs"), b"mod").expect("mod");
    std::fs::write(temp.path.join("sample_app/main.rs"), b"main").expect("main");

    let lazy = local_file_tree_nodes(
        &temp.path,
        std::iter::empty::<std::path::PathBuf>(),
        None,
        FileTreeOptions {
            load_mode: FileTreeLoadMode::Lazy,
            ..FileTreeOptions::default()
        },
    )
    .expect("lazy nodes");
    assert!(child(&lazy[0], "src").children.is_empty());

    let controlled = local_file_tree_nodes(
        &temp.path,
        std::iter::empty::<std::path::PathBuf>(),
        None,
        FileTreeOptions {
            load_mode: FileTreeLoadMode::Controlled { preload_depth: 1 },
            ..FileTreeOptions::default()
        },
    )
    .expect("controlled nodes");
    let src = child(&controlled[0], "src");
    assert!(src.children.iter().any(|node| node.name() == "lib.rs"));
    assert!(child(src, "nested").children.is_empty());
    assert!(child(&controlled[0], "sample_app")
        .children
        .iter()
        .any(|node| node.name() == "main.rs"));

    let full = local_file_tree_nodes(
        &temp.path,
        std::iter::empty::<std::path::PathBuf>(),
        None,
        FileTreeOptions {
            load_mode: FileTreeLoadMode::Full,
            ..FileTreeOptions::default()
        },
    )
    .expect("full nodes");
    assert!(child(child(&full[0], "src"), "nested")
        .children
        .iter()
        .any(|node| node.name() == "mod.rs"));
}

#[cfg(all(feature = "files-local", unix))]
#[test]
fn local_file_tree_nodes_expands_symlink_directory_explicitly() {
    let temp = TempDir::new("symlink_dir");
    std::fs::create_dir_all(temp.path.join("target")).expect("target dir");
    std::fs::write(temp.path.join("target/child.rs"), b"child").expect("child");
    std::os::unix::fs::symlink("target", temp.path.join("linked")).expect("symlink");

    let nodes = local_file_tree_nodes(
        &temp.path,
        [std::path::PathBuf::from("linked")],
        None,
        FileTreeOptions::default(),
    )
    .expect("nodes");

    let linked = child(&nodes[0], "linked");
    assert_eq!(linked.entry.metadata.kind, FileKind::Symlink);
    assert_eq!(
        linked.entry.metadata.symlink_target_kind,
        Some(FileKind::Directory)
    );
    assert!(linked.children.iter().any(|node| node.name() == "child.rs"));
}

#[cfg(feature = "files-local")]
#[test]
fn local_file_explorer_lazy_cached_layouts_from_path_without_manual_nodes() {
    let temp = TempDir::new("lazy_cached_widget");
    std::fs::create_dir_all(temp.path.join("src/view")).expect("dirs");
    std::fs::write(temp.path.join("src/lib.rs"), b"lib").expect("lib");
    std::fs::write(temp.path.join("src/view/left.rs"), b"left").expect("left");

    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        LocalFileExplorer::<Action>::new(&temp.path)
            .selected_path("src/view/left.rs")
            .default_expanded_path("src/view")
            .lazy_cached()
            .virtualized(true)
            .width(320.0)
            .height(220.0)
            .into_view(),
    );
    layout_app(&mut app);

    assert_eq!(widget_count(&app, "ScrollView"), 1);
    assert_eq!(widget_count(&app, "TreeView"), 1);
}

#[cfg(feature = "files-local")]
#[test]
fn local_file_explorer_lazy_cached_loads_children_when_root_folder_opens() {
    let temp = TempDir::new("lazy_cached_toggle");
    std::fs::create_dir_all(temp.path.join("src")).expect("src");
    std::fs::write(temp.path.join("src/lib.rs"), b"lib").expect("lib");

    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        LocalFileExplorer::<Action>::new(&temp.path)
            .lazy_cached()
            .virtualized(true)
            .file_size(ailloli_ui_widgets::files::FileExplorerSize::Compact)
            .width(320.0)
            .height(220.0)
            .into_view(),
    );
    layout_app(&mut app);

    assert!(!painted_text_contains(&app, "lib.rs"));

    let mut router = InputRouter::default();
    click(&mut router, &app, runtime, 30.0, 36.0);
    layout_app(&mut app);

    assert!(
        painted_text_contains(&app, "lib.rs"),
        "opening src should dynamically load and render lib.rs"
    );
}

#[test]
fn file_explorer_bound_nodes_sync_after_action_toggle_updates_tree() {
    let src = uri("/repo/src");
    let tests = uri("/repo/tests");
    let nodes = State::new(vec![
        FileExplorerNode::directory(src.clone(), "src"),
        FileExplorerNode::directory(tests.clone(), "tests"),
    ]);
    let nodes_for_action = nodes.clone();
    let src_for_action = src.clone();
    let tests_for_action = tests.clone();
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    app.reconcile(
        FileExplorer::new(Vec::new())
            .bind_nodes(nodes.clone())
            .on_action_ctx(move |_ctx, action| {
                let FileExplorerAction::Toggle {
                    uri: toggled,
                    expanded: true,
                } = action
                else {
                    return;
                };
                let mut next = nodes_for_action.read();
                for node in &mut next {
                    if node.uri() == &toggled && node.children.is_empty() {
                        if toggled == src_for_action {
                            node.children
                                .push(FileExplorerNode::file(uri("/repo/src/lib.rs"), "lib.rs"));
                        } else if toggled == tests_for_action {
                            node.children.push(FileExplorerNode::file(
                                uri("/repo/tests/tree.rs"),
                                "tree.rs",
                            ));
                        }
                    }
                }
                nodes_for_action.set(next);
            })
            .file_size(ailloli_ui_widgets::files::FileExplorerSize::Compact)
            .width(320.0)
            .height(220.0)
            .into_view(),
    );
    layout_app(&mut app);

    assert!(!painted_text_contains(&app, "lib.rs"));
    assert!(!painted_text_contains(&app, "tree.rs"));

    let mut router = InputRouter::default();
    click(&mut router, &app, runtime.clone(), 16.0, 16.0);
    layout_app(&mut app);

    assert!(
        painted_text_contains(&app, "lib.rs"),
        "children added by on_action toggle must render in the same repaint cycle"
    );
    assert!(!painted_text_contains(&app, "tree.rs"));

    click(&mut router, &app, runtime, 16.0, 64.0);
    layout_app(&mut app);

    assert!(painted_text_contains(&app, "lib.rs"));
    assert!(
        painted_text_contains(&app, "tree.rs"),
        "second toggled folder must not wait for a later toggle to display children"
    );
}

#[test]
fn file_explorer_bound_nodes_real_new_file_names_do_not_block_sync_after_toggle() {
    let paf = uri("/repo/paf");
    let pif = uri("/repo/pif");
    let nodes = State::new(vec![
        FileExplorerNode::directory(paf.clone(), "paf"),
        FileExplorerNode::directory(pif.clone(), "pif")
            .child(FileExplorerNode::file(
                uri("/repo/pif/New_File"),
                "New_File",
            ))
            .child(FileExplorerNode::directory(
                uri("/repo/pif/New_Folder"),
                "New_Folder",
            )),
    ]);
    let expanded = State::new(vec![pif.clone()]);
    let nodes_for_action = nodes.clone();
    let paf_for_action = paf.clone();
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    app.reconcile(
        FileExplorer::new(Vec::new())
            .bind_nodes(nodes.clone())
            .bind_expanded(expanded)
            .on_action_ctx(move |_ctx, action| {
                let FileExplorerAction::Toggle {
                    uri: toggled,
                    expanded: true,
                } = action
                else {
                    return;
                };
                if toggled != paf_for_action {
                    return;
                }
                let mut next = nodes_for_action.read();
                for node in &mut next {
                    if node.uri() == &toggled && node.children.is_empty() {
                        node.children
                            .push(FileExplorerNode::file(uri("/repo/paf/puf"), "puf"));
                    }
                }
                nodes_for_action.set(next);
            })
            .file_size(ailloli_ui_widgets::files::FileExplorerSize::Compact)
            .width(320.0)
            .height(220.0)
            .into_view(),
    );
    layout_app(&mut app);

    assert!(painted_text_contains(&app, "New_File"));
    assert!(painted_text_contains(&app, "New_Folder"));
    assert!(!painted_text_contains(&app, "puf"));

    let mut router = InputRouter::default();
    click(&mut router, &app, runtime, 16.0, 16.0);
    layout_app(&mut app);

    assert!(
        nodes.read().iter().any(|node| {
            node.uri() == &paf
                && node
                    .children
                    .iter()
                    .any(|child| child.uri() == &uri("/repo/paf/puf"))
        }),
        "toggle callback must update the bound FileExplorerNode snapshot"
    );
    assert!(
        painted_text_contains(&app, "puf"),
        "real New_File/New_Folder entries must not preserve stale tree nodes"
    );
}

#[test]
fn file_explorer_is_scrollable_by_default_and_hit_tests_after_scroll() {
    let selected = State::new(uri("/repo/file_000.rs"));
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    app.reconcile(
        FileExplorer::new(many_file_nodes(80))
            .bind_selected(selected.clone())
            .on_select(Action::Select)
            .file_size(ailloli_ui_widgets::files::FileExplorerSize::Compact)
            .virtualized(true)
            .into_view(),
    );
    layout_app_size(&mut app, 320.0, 72.0);

    assert_eq!(widget_count(&app, "ScrollView"), 1);
    assert_eq!(widget_count(&app, "TreeView"), 1);
    assert!(
        painted_rrect_count(&app) >= 2,
        "FileExplorer should inherit visible ScrollView scrollbars"
    );
    assert!(!painted_text_contains(&app, "file_020.rs"));

    let mut router = InputRouter::default();
    wheel(&mut router, &app, runtime.clone(), 12.0, 12.0, -480.0);
    layout_app_size(&mut app, 320.0, 72.0);

    assert!(
        painted_text_contains(&app, "file_020.rs"),
        "virtualized file explorer should paint rows in the scrolled viewport"
    );

    click(&mut router, &app, runtime.clone(), 56.0, 16.0);
    assert_eq!(selected.read(), uri("/repo/file_020.rs"));
    let actions = runtime.take_actions();
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::Select(uri) if uri == &selected.read())),
        "actions={actions:?}"
    );
}

#[test]
fn file_explorer_scrollable_preserves_declared_width_in_row_layout() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);

    app.reconcile(
        Row::new()
            .width(600.0)
            .height(120.0)
            .child(
                FileExplorer::<Action>::new(many_file_nodes(20))
                    .width(160.0)
                    .fill_height()
                    .virtualized(true),
            )
            .child(Text::new("editor-pane-marker").flex_grow())
            .into_view(),
    );
    layout_app_size(&mut app, 600.0, 120.0);

    let scroll_bounds = first_widget_bounds(&app, "ScrollView").expect("scroll view bounds");
    assert_eq!(scroll_bounds.w, 160.0);
    assert!(
        painted_text_contains(&app, "editor-pane-marker"),
        "editor sibling should stay visible next to the file explorer"
    );
}

#[test]
fn file_explorer_scrollable_false_keeps_legacy_direct_tree() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);

    app.reconcile(
        FileExplorer::<Action>::new(many_file_nodes(12))
            .scrollable(false)
            .width(320.0)
            .height(72.0)
            .into_view(),
    );
    layout_app_size(&mut app, 320.0, 72.0);

    assert_eq!(widget_count(&app, "ScrollView"), 0);
    assert_eq!(widget_count(&app, "TreeView"), 1);
}

#[test]
fn file_explorer_rename_and_remove_emit_callbacks_without_io() {
    let src = uri("/repo/src");
    let main = uri("/repo/src/main.rs");
    let selected = State::new(main.clone());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    app.reconcile(
        FileExplorer::new(sample_nodes())
            .bind_selected(selected.clone())
            .expanded(vec![src.clone()])
            .on_rename(Action::Rename)
            .on_remove(Action::Remove)
            .on_create_dir(Action::CreateDir)
            .on_action(Action::Any)
            .width(320.0)
            .height(220.0)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    click(&mut router, &app, runtime.clone(), 56.0, 48.0);
    runtime.take_actions();

    let tree = first_widget_id(&app, "TreeView").expect("tree view");
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &keyboard_event(NamedKey::F(2)),
    );
    dispatch_event_to_target(&app.tree, runtime.clone(), tree, &character_event("X"));
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &keyboard_event(NamedKey::Enter),
    );
    let actions = runtime.take_actions();
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::Rename(event)
                if event.uri == main && event.old_name == "main.rs" && event.new_name == "X"
        )),
        "actions={actions:?}"
    );
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::Any(FileExplorerAction::Rename(event))
                if event.uri == main && event.new_name == "X"
        )),
        "actions={actions:?}"
    );

    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::Delete),
    );
    let actions = runtime.take_actions();
    assert!(
        actions.iter().any(|action| matches!(action, Action::Remove(uri, parent) if uri == &main && parent.as_ref() == Some(&src))),
        "actions={actions:?}"
    );
}

#[test]
fn file_explorer_context_menu_rename_accepts_first_typed_character_without_second_click() {
    let src = uri("/repo/src");
    let main = uri("/repo/src/main.rs");
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    app.reconcile(
        FileExplorer::new(sample_nodes())
            .expanded(vec![src])
            .on_rename(Action::Rename)
            .on_action(Action::Any)
            .width(320.0)
            .height(220.0)
            .into_view(),
    );
    layout_app(&mut app);

    let main_pos = painted_text_position(&app, "main.rs").expect("main.rs text");
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &right_pointer_button(main_pos.x + 8.0, main_pos.y - 6.0),
    );
    layout_app(&mut app);

    let rename_pos = painted_text_position(&app, "Rename...").expect("rename menu item");
    click(
        &mut router,
        &app,
        runtime.clone(),
        rename_pos.x + 8.0,
        rename_pos.y - 6.0,
    );
    layout_app(&mut app);
    runtime.take_actions();

    let key = router.route_event(&app.tree, runtime.clone(), &character_event("Z"));
    layout_app(&mut app);
    assert!(painted_text_contains(&app, "Z"));
    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Enter));
    let actions = runtime.take_actions();

    assert!(key.event_dispatched);
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::Rename(event)
                if event.uri == main && event.old_name == "main.rs" && event.new_name == "Z"
        )),
        "actions={actions:?}"
    );
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::Any(FileExplorerAction::Rename(event))
                if event.uri == main && event.new_name == "Z"
        )),
        "actions={actions:?}"
    );
}

#[test]
fn file_explorer_context_menu_new_file_starts_inline_create() {
    let src = uri("/repo/src");
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    app.reconcile(
        FileExplorer::new(sample_nodes())
            .on_action(Action::Any)
            .width(320.0)
            .height(220.0)
            .into_view(),
    );
    layout_app(&mut app);

    let src_pos = painted_text_position(&app, "src").expect("src text");
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &right_pointer_button(src_pos.x + 8.0, src_pos.y - 6.0),
    );
    layout_app(&mut app);

    let new_file_pos = painted_text_position(&app, "New File").expect("new file item");
    click(
        &mut router,
        &app,
        runtime.clone(),
        new_file_pos.x + 8.0,
        new_file_pos.y - 6.0,
    );
    layout_app(&mut app);
    assert!(painted_text_contains(&app, "New_File"));
    assert!(runtime.take_actions().iter().any(|action| matches!(
        action,
        Action::Any(FileExplorerAction::CreateFileRequested { parent }) if parent == &src
    )));

    let key = router.route_event(&app.tree, runtime.clone(), &character_event("Z"));
    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Enter));
    let actions = runtime.take_actions();

    assert!(key.event_dispatched);
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::Any(FileExplorerAction::CreateFile(event))
                if event.parent.as_ref() == Some(&src)
                    && event.name == "Z"
                    && event.uri == uri("/repo/src/Z")
        )),
        "actions={actions:?}"
    );
}

#[test]
fn file_explorer_context_menu_new_folder_starts_inline_create() {
    let src = uri("/repo/src");
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    app.reconcile(
        FileExplorer::new(sample_nodes())
            .on_create_dir(Action::CreateDir)
            .on_action(Action::Any)
            .width(320.0)
            .height(220.0)
            .into_view(),
    );
    layout_app(&mut app);

    let src_pos = painted_text_position(&app, "src").expect("src text");
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &right_pointer_button(src_pos.x + 8.0, src_pos.y - 6.0),
    );
    layout_app(&mut app);

    let new_folder_pos = painted_text_position(&app, "New Folder").expect("new folder item");
    click(
        &mut router,
        &app,
        runtime.clone(),
        new_folder_pos.x + 8.0,
        new_folder_pos.y - 6.0,
    );
    layout_app(&mut app);
    assert!(painted_text_contains(&app, "New_Folder"));
    assert!(runtime.take_actions().iter().any(|action| matches!(
        action,
        Action::Any(FileExplorerAction::CreateDirRequested { parent }) if parent == &src
    )));

    let key = router.route_event(&app.tree, runtime.clone(), &character_event("D"));
    layout_app(&mut app);
    assert!(painted_text_contains(&app, "D"));
    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Enter));
    let actions = runtime.take_actions();

    assert!(key.event_dispatched);
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::CreateDir(event)
                if event.parent.as_ref() == Some(&src)
                    && event.name == "D"
                    && event.uri == uri("/repo/src/D")
        )),
        "actions={actions:?}"
    );
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::Any(FileExplorerAction::CreateDir(event))
                if event.parent.as_ref() == Some(&src)
                    && event.name == "D"
                    && event.uri == uri("/repo/src/D")
        )),
        "actions={actions:?}"
    );
}

#[test]
fn file_explorer_drag_move_emits_move_entry_without_local_tree_mutation() {
    let src = uri("/repo/src");
    let cargo = uri("/repo/Cargo.toml");
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    app.reconcile(
        FileExplorer::new(sample_nodes())
            .on_action(Action::Any)
            .width(320.0)
            .height(220.0)
            .into_view(),
    );
    layout_app(&mut app);

    let cargo_pos = painted_text_position(&app, "Cargo.toml").expect("Cargo.toml text");
    let src_pos = painted_text_position(&app, "src").expect("src text");
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(cargo_pos.x + 8.0, cargo_pos.y - 6.0, true),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_move(src_pos.x + 8.0, src_pos.y - 6.0),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(src_pos.x + 8.0, src_pos.y - 6.0, false),
    );

    assert!(painted_text_contains(&app, "Cargo.toml"));
    let actions = runtime.take_actions();
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::Any(FileExplorerAction::MoveEntry(event))
                if event.from == cargo
                    && event.to == uri("/repo/src/Cargo.toml")
                    && event.source_parent.as_ref() == Some(&uri("/repo"))
                    && event.target_parent == src
        )),
        "actions={actions:?}"
    );
}

#[test]
fn file_explorer_keyboard_shortcuts_emit_file_actions_without_inline_edit_conflict() {
    let src = uri("/repo/src");
    let main = uri("/repo/src/main.rs");
    let selected = State::new(main.clone());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    app.reconcile(
        FileExplorer::new(sample_nodes())
            .bind_selected(selected)
            .expanded(vec![src.clone()])
            .clipboard_can_paste(true)
            .on_action(Action::Any)
            .width(320.0)
            .height(220.0)
            .into_view(),
    );
    layout_app(&mut app);
    let tree = first_widget_id(&app, "TreeView").expect("tree view");

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &character_event_with_modifiers(
            "c",
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        ),
    );
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &character_event_with_modifiers(
            "x",
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        ),
    );
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &character_event_with_modifiers(
            "v",
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        ),
    );
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &keyboard_event(NamedKey::Delete),
    );

    let actions = runtime.take_actions();
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::Any(FileExplorerAction::CopyFile { uri }) if uri == &main
        )),
        "actions={actions:?}"
    );
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::Any(FileExplorerAction::CutFile { uri }) if uri == &main
        )),
        "actions={actions:?}"
    );
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::Any(FileExplorerAction::PasteInto { target_dir }) if target_dir == &src
        )),
        "actions={actions:?}"
    );
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::Any(FileExplorerAction::RemoveRequested { uri, parent })
                if uri == &main && parent.as_ref() == Some(&src)
        )),
        "actions={actions:?}"
    );
}

#[test]
fn file_explorer_create_dir_emits_callback_without_io() {
    let src = uri("/repo/src");
    let selected = State::new(src.clone());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    app.reconcile(
        FileExplorer::new(sample_nodes())
            .bind_selected(selected)
            .expanded(vec![src.clone()])
            .on_create_dir(Action::CreateDir)
            .on_action(Action::Any)
            .width(320.0)
            .height(220.0)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    click(&mut router, &app, runtime.clone(), 56.0, 20.0);
    runtime.take_actions();

    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event_with_modifiers(
            NamedKey::Insert,
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        ),
    );
    let actions = runtime.take_actions();
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::CreateDir(event)
                if event.parent.as_ref() == Some(&src) && event.name == "New_Folder"
        )),
        "actions={actions:?}"
    );
}

#[test]
fn file_explorer_select_toggle_and_open_emit_callbacks_without_io() {
    let src = uri("/repo/src");
    let main = uri("/repo/src/main.rs");
    let selected = State::new(uri("/repo/Cargo.toml"));
    let expanded = State::new(vec![src.clone()]);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    app.reconcile(
        FileExplorer::new(sample_nodes())
            .bind_selected(selected.clone())
            .bind_expanded(expanded.clone())
            .on_select(Action::Select)
            .on_open(Action::Open)
            .on_toggle(Action::Toggle)
            .on_action(Action::Any)
            .width(320.0)
            .height(220.0)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    click(&mut router, &app, runtime.clone(), 56.0, 48.0);
    assert_eq!(selected.read(), main);
    let actions = runtime.take_actions();
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::Select(uri) if uri == &main)),
        "actions={actions:?}"
    );

    click(&mut router, &app, runtime.clone(), 56.0, 48.0);
    let actions = runtime.take_actions();
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::Open(uri) if uri == &main)),
        "actions={actions:?}"
    );

    click(&mut router, &app, runtime.clone(), 16.0, 20.0);
    assert!(expanded.read().is_empty());
    let actions = runtime.take_actions();
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::Toggle(uri, false) if uri == &src)),
        "actions={actions:?}"
    );
}

#[test]
fn file_explorer_chevron_keeps_toggled_folder_focused_without_selecting() {
    let src = uri("/repo/src");
    let tests = uri("/repo/tests");
    let selected = State::new(uri("/repo/missing.rs"));
    let expanded = State::new(Vec::<FileUri>::new());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    app.reconcile(
        FileExplorer::new([
            FileExplorerNode::directory(src.clone(), "src")
                .child(FileExplorerNode::file(uri("/repo/src/lib.rs"), "lib.rs")),
            FileExplorerNode::directory(tests.clone(), "tests").child(FileExplorerNode::file(
                uri("/repo/tests/tree.rs"),
                "tree.rs",
            )),
        ])
        .bind_selected(selected.clone())
        .bind_expanded(expanded.clone())
        .on_select(Action::Select)
        .on_open(Action::Open)
        .on_toggle(Action::Toggle)
        .width(320.0)
        .height(220.0)
        .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    click(&mut router, &app, runtime.clone(), 16.0, 44.0);
    layout_app(&mut app);

    assert_eq!(selected.read(), uri("/repo/missing.rs"));
    assert_eq!(expanded.read(), vec![tests.clone()]);
    assert_eq!(runtime.take_actions(), vec![Action::Toggle(tests, true)]);

    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Enter));
    assert_eq!(selected.read(), uri("/repo/missing.rs"));
    assert_eq!(
        runtime.take_actions(),
        vec![Action::Open(uri("/repo/tests"))]
    );
}

#[test]
fn file_explorer_context_menu_paints_file_actions_on_right_click() {
    let root_uri = uri("/repo");
    let src_uri = uri("/repo/src");
    let file_uri = uri("/repo/src/main.rs");
    let nodes = vec![FileExplorerNode::directory(root_uri.clone(), "repo").child(
        FileExplorerNode::directory(src_uri.clone(), "src")
            .child(FileExplorerNode::file(file_uri, "main.rs")),
    )];
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        FileExplorer::new(nodes)
            .expanded(vec![root_uri, src_uri])
            .on_action(Action::Any)
            .into_view(),
    );
    layout_app_size(&mut app, 360.0, 260.0);

    let mut router = InputRouter::default();
    router.route_event(&app.tree, runtime, &right_pointer_button(32.0, 76.0));
    layout_app_size(&mut app, 360.0, 260.0);

    assert!(painted_text_contains(&app, "Open"));
    assert!(painted_text_contains(&app, "Cut"));
    assert!(painted_text_contains(&app, "Copy File"));
    assert!(painted_text_contains(&app, "Copy Path"));
    assert!(painted_text_contains(&app, "Rename..."));
    assert!(painted_text_contains(&app, "Delete"));
}

fn sample_nodes() -> Vec<FileExplorerNode> {
    vec![
        FileExplorerNode::directory(uri("/repo/src"), "src")
            .child(FileExplorerNode::file(uri("/repo/src/main.rs"), "main.rs")),
        FileExplorerNode::file(uri("/repo/Cargo.toml"), "Cargo.toml"),
    ]
}

fn many_file_nodes(count: usize) -> Vec<FileExplorerNode> {
    (0..count)
        .map(|idx| {
            let name = format!("file_{idx:03}.rs");
            FileExplorerNode::file(
                FileUri::parse(format!("file:///repo/{name}")).expect("file uri"),
                name,
            )
        })
        .collect()
}

#[cfg(feature = "files-local")]
fn child<'a>(node: &'a FileExplorerNode, name: &str) -> &'a FileExplorerNode {
    node.children
        .iter()
        .find(|child| child.name() == name)
        .unwrap_or_else(|| panic!("missing child {name} in {}", node.name()))
}

fn uri(path: &str) -> FileUri {
    FileUri::parse(format!("file://{path}")).expect("file uri")
}

fn symlink_node(path: &str, name: impl Into<String>, target: Option<FileKind>) -> FileExplorerNode {
    let mut metadata = FileMetadata::new(FileKind::Symlink);
    metadata.symlink_target_kind = target;
    FileExplorerNode::new(FileEntry {
        uri: uri(path),
        name: name.into(),
        metadata,
    })
}

#[cfg(feature = "files-local")]
struct TempDir {
    path: std::path::PathBuf,
}

#[cfg(feature = "files-local")]
impl TempDir {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ailloli_ui_widgets_file_explorer_{name}_{}_{}",
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        Self { path }
    }
}

#[cfg(feature = "files-local")]
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn layout_app<A: 'static>(app: &mut Runtime<A>) {
    layout_app_size(app, 320.0, 220.0);
}

fn layout_app_size<A: 'static>(app: &mut Runtime<A>, w: f32, h: f32) {
    let mut text_system = TextSystem::new();
    app.layout(Constraints::tight(w, h), Scale::new(1.0), &mut text_system);
}

fn painted_text_contains<A: 'static>(app: &Runtime<A>, needle: &str) -> bool {
    let mut text_system = TextSystem::new();
    app.paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text().contains(needle)))
}

fn painted_text_position<A: 'static>(app: &Runtime<A>, needle: &str) -> Option<Point> {
    let mut text_system = TextSystem::new();
    app.paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .find_map(|cmd| match cmd {
            DrawCmd::Text(text) if text.layout.text().contains(needle) => {
                Some(Point::new(text.pos[0], text.pos[1]))
            }
            _ => None,
        })
}

fn painted_rrect_count<A: 'static>(app: &Runtime<A>) -> usize {
    let mut text_system = TextSystem::new();
    app.paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .filter(|cmd| matches!(cmd, DrawCmd::RRect(_)))
        .count()
}

fn widget_count<A: 'static>(app: &Runtime<A>, debug_name: &str) -> usize {
    app.tree
        .iter_elements()
        .filter(|(_, el)| match &el.kind {
            ElementKind::Widget(widget) => widget.debug_name() == debug_name,
            _ => false,
        })
        .count()
}

fn first_widget_id<A: 'static>(
    app: &Runtime<A>,
    debug_name: &str,
) -> Option<ailloli_ui_core::ElementId> {
    app.tree
        .iter_elements()
        .find_map(|(id, el)| match &el.kind {
            ElementKind::Widget(widget) if widget.debug_name() == debug_name => Some(id),
            _ => None,
        })
}

fn first_widget_bounds<A: 'static>(
    app: &Runtime<A>,
    debug_name: &str,
) -> Option<ailloli_ui_core::Rect> {
    app.tree
        .iter_elements()
        .find_map(|(id, el)| match &el.kind {
            ElementKind::Widget(widget) if widget.debug_name() == debug_name => {
                absolute_paint_bounds(&app.tree, id)
            }
            _ => None,
        })
}

fn click<A: Clone + 'static>(
    router: &mut InputRouter,
    app: &Runtime<A>,
    runtime: RuntimeHandle<A>,
    x: f32,
    y: f32,
) {
    router.route_event(&app.tree, runtime.clone(), &pointer_button(x, y, true));
    router.route_event(&app.tree, runtime, &pointer_button(x, y, false));
}

fn wheel<A: Clone + 'static>(
    router: &mut InputRouter,
    app: &Runtime<A>,
    runtime: RuntimeHandle<A>,
    x: f32,
    y: f32,
    delta_y: f32,
) {
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Wheel {
            pos: Point::new(x, y),
            delta: WheelDelta::PixelDelta { x: 0.0, y: delta_y },
            modifiers: Modifiers::default(),
            precise: true,
        }),
    );
}

fn pointer_button(x: f32, y: f32, pressed: bool) -> Event {
    Event::Pointer(PointerEvent::Button {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        pressed,
        modifiers: Modifiers::default(),
    })
}

fn pointer_move(x: f32, y: f32) -> Event {
    Event::Pointer(PointerEvent::Moved {
        pos: Point::new(x, y),
        modifiers: Modifiers::default(),
    })
}

fn right_pointer_button(x: f32, y: f32) -> Event {
    Event::Pointer(PointerEvent::Button {
        pos: Point::new(x, y),
        button: MouseButton::Right,
        pressed: true,
        modifiers: Modifiers::default(),
    })
}

fn keyboard_event(key: NamedKey) -> Event {
    keyboard_event_with_modifiers(key, Modifiers::default())
}

fn keyboard_event_with_modifiers(key: NamedKey, modifiers: Modifiers) -> Event {
    Event::Keyboard(KeyEvent {
        state: KeyState::Pressed,
        key: Key::Named(key),
        modifiers,
        repeat: false,
        pointer_pos: Some(Point::new(20.0, 16.0)),
        text: None,
    })
}

fn character_event(text: &str) -> Event {
    character_event_with_modifiers(text, Modifiers::default())
}

fn character_event_with_modifiers(text: &str, modifiers: Modifiers) -> Event {
    Event::Keyboard(KeyEvent {
        state: KeyState::Pressed,
        key: Key::Character(text.to_string()),
        modifiers,
        repeat: false,
        pointer_pos: Some(Point::new(20.0, 16.0)),
        text: Some(text.to_string()),
    })
}
