use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Color, IconId, Point, Theme};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State, View};
use ailloli_ui_runtime::input::{dispatch_event_to_target, InputRouter};
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{
    Accordion, AccordionItem, AccordionSize, AccordionStyle, TreeCreateKind, TreeCreateRequest,
    TreeDelete, TreeDropPosition, TreeMove, TreeMutationMode, TreeNode, TreeRename, TreeShortcut,
    TreeView, TreeViewCommand, TreeViewSize, TreeViewStyle,
};
use ailloli_ui_widgets::layout::ScrollView;
use ailloli_ui_widgets::text::Text;
use lucide_icons::Icon as LucideIcon;

#[derive(Clone, Debug, PartialEq, Eq)]
enum NodeId {
    Root,
    Src,
    Components,
    Button,
    Cargo,
    Disabled,
    Missing,
    NewChild,
    NewSibling,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
enum Action {
    ToggleAccordion(String, bool),
    SelectNode(NodeId),
    ActivateNode(NodeId),
    ToggleNode(NodeId, bool),
    MoveNode(TreeMove<NodeId>),
    RenameNode(TreeRename<NodeId>),
    CreateNode(ailloli_ui_widgets::controls::TreeCreate<NodeId>),
    DeleteNode(TreeDelete<NodeId>),
    Shortcut(TreeShortcut<NodeId>),
    TrailingAction(NodeId),
}

#[test]
fn accordion_and_tree_styles_use_theme_tokens() {
    let theme = Theme::default();
    let palette = theme.palette();

    let accordion = AccordionStyle::from_theme(theme, AccordionSize::Default);
    assert_eq!(accordion.background, palette.surface);
    assert_eq!(accordion.border.colors.top, palette.border);
    assert_eq!(accordion.header_open, palette.accent.with_alpha(0.16));
    assert_eq!(accordion.focus_ring.colors.top, palette.focus);

    let tree = TreeViewStyle::from_theme(theme, TreeViewSize::Default);
    assert_eq!(tree.selected_background, palette.accent.with_alpha(0.18));
    assert_eq!(tree.text.color, palette.text);
    assert_eq!(tree.icon_tint, palette.text_muted);
    assert_eq!(tree.focus_ring.colors.top, palette.focus);
}

#[test]
fn accordion_single_open_replaces_previous_and_dispatches() {
    let open = State::new(vec!["one".to_string()]);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Accordion::<Action>::new()
            .single()
            .bind_open_ids(open.clone())
            .item(item("one", "One"))
            .item(item("two", "Two"))
            .on_toggle(Action::ToggleAccordion)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 240.0);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(20.0, 88.0, true),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(20.0, 88.0, false),
    );
    layout_app(&mut app, 360.0, 240.0);

    assert_eq!(open.read(), vec!["two".to_string()]);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::ToggleAccordion("two".to_string(), true)]
    );
}

#[test]
fn accordion_multiple_keeps_existing_open_items() {
    let open = State::new(vec!["one".to_string()]);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Accordion::<Action>::new()
            .multiple()
            .bind_open_ids(open.clone())
            .item(item("one", "One"))
            .item(item("two", "Two"))
            .on_toggle(Action::ToggleAccordion)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 240.0);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(20.0, 88.0, true),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(20.0, 88.0, false),
    );

    assert_eq!(open.read(), vec!["one".to_string(), "two".to_string()]);
}

#[test]
fn accordion_collapsed_omits_content_from_layout() {
    let collapsed = layout_root(
        Accordion::<()>::new()
            .item(item("one", "One"))
            .item(item("two", "Two"))
            .into_view(),
        360.0,
        240.0,
    )
    .0;
    let expanded = layout_root(
        Accordion::<()>::new()
            .default_open("one")
            .item(item("one", "One"))
            .item(item("two", "Two"))
            .into_view(),
        360.0,
        240.0,
    )
    .0;

    let collapsed_size = root_size(&collapsed);
    let expanded_size = root_size(&expanded);
    assert!(
        expanded_size.h > collapsed_size.h,
        "expanded={expanded_size:?} collapsed={collapsed_size:?}"
    );

    let expanded_text = paint_cmds(&expanded)
        .iter()
        .filter(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "Content one"))
        .count();
    assert_eq!(expanded_text, 1);
}

#[test]
fn accordion_disabled_header_does_not_focus_or_dispatch() {
    let open = State::new(Vec::<String>::new());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Accordion::<Action>::new()
            .bind_open_ids(open.clone())
            .item(item("one", "One").disabled(true))
            .on_toggle(Action::ToggleAccordion)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 160.0);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(20.0, 20.0, true),
    );
    assert_eq!(router.focused(), None);
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(20.0, 20.0, false),
    );
    assert!(open.read().is_empty());
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn tree_view_flattens_visible_nodes_and_paints_selected_state() {
    let palette = Theme::default().palette();
    let (app, root) = layout_root(
        TreeView::<NodeId>::new()
            .selected(NodeId::Src)
            .default_expanded(NodeId::Root)
            .node(sample_tree())
            .width(320.0)
            .into_view(),
        360.0,
        260.0,
    );
    let tree = first_child(&app, root);
    let layout = app.tree.get(tree).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 320.0);
    assert_eq!(layout.size.h, 96.0);

    let cmds = paint_cmds(&app);
    assert!(cmds.iter().any(|cmd| matches!(
        cmd,
        DrawCmd::RRect(r) if r.color == palette.accent.with_alpha(0.18)
    )));
    assert!(cmds.iter().any(|cmd| matches!(cmd, DrawCmd::Image(_))));
    assert!(
        cmds.iter()
            .filter(|cmd| matches!(cmd, DrawCmd::Text(_)))
            .count()
            >= 3
    );
}

#[test]
fn tree_view_uses_explicit_leading_icon_tint() {
    let icon_tint = Color::hex_rgb(0xdea584);
    let (app, _) = layout_root(
        TreeView::<NodeId>::new()
            .selected(NodeId::Cargo)
            .node(
                TreeNode::leaf(NodeId::Cargo, "Cargo.toml")
                    .leading_icon(IconId::Check)
                    .leading_icon_tint(icon_tint),
            )
            .width(320.0)
            .into_view(),
        360.0,
        120.0,
    );

    assert!(paint_cmds(&app).iter().any(|cmd| matches!(
        cmd,
        DrawCmd::Image(img) if img.icon == IconId::Check && img.tint == icon_tint
    )));
}

#[test]
fn tree_view_trailing_action_paints_only_on_selected_row() {
    let (app, _) = layout_root(
        TreeView::<NodeId>::new()
            .selected(NodeId::Cargo)
            .default_expanded(NodeId::Root)
            .node(
                TreeNode::branch(NodeId::Root, "Project Root")
                    .child(
                        TreeNode::leaf(NodeId::Src, "src")
                            .trailing_action(IconId::Lucide(LucideIcon::ArrowRightFromLine)),
                    )
                    .child(
                        TreeNode::leaf(NodeId::Cargo, "Cargo.toml")
                            .trailing_action(IconId::Lucide(LucideIcon::ArrowRightFromLine)),
                    ),
            )
            .width(320.0)
            .into_view(),
        360.0,
        160.0,
    );

    let action_icons = paint_cmds(&app)
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DrawCmd::Image(img)
                    if img.icon == IconId::Lucide(LucideIcon::ArrowRightFromLine)
            )
        })
        .count();
    assert_eq!(action_icons, 1);
}

#[test]
fn tree_view_trailing_action_dispatches_without_selecting_or_activating() {
    let selected = State::new(NodeId::Cargo);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_selected(selected.clone())
            .default_expanded(NodeId::Root)
            .node(
                TreeNode::branch(NodeId::Root, "Project Root").child(
                    TreeNode::leaf(NodeId::Cargo, "Cargo.toml")
                        .trailing_action(IconId::Lucide(LucideIcon::ArrowRightFromLine)),
                ),
            )
            .on_select(Action::SelectNode)
            .on_activate(Action::ActivateNode)
            .on_trailing_action(Action::TrailingAction)
            .width(320.0)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 160.0);
    let tree = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &pointer_button(296.0, 48.0, true),
    );
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &pointer_button(296.0, 48.0, false),
    );

    assert_eq!(selected.read(), NodeId::Cargo);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::TrailingAction(NodeId::Cargo)]
    );
}

#[test]
fn tree_view_chevron_toggles_without_selecting() {
    let selected = State::new(NodeId::Missing);
    let expanded = State::new(vec![NodeId::Root]);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_selected(selected.clone())
            .bind_expanded(expanded.clone())
            .node(sample_tree())
            .on_select(Action::SelectNode)
            .on_toggle(Action::ToggleNode)
            .width(320.0)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 260.0);
    let tree = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &pointer_button(14.0, 16.0, false),
    );

    assert!(expanded.read().is_empty());
    assert_eq!(selected.read(), NodeId::Missing);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::ToggleNode(NodeId::Root, false)]
    );
}

#[test]
fn tree_view_chevron_keeps_toggled_row_active_without_selecting() {
    let selected = State::new(NodeId::Cargo);
    let expanded = State::new(vec![NodeId::Root]);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_selected(selected.clone())
            .bind_expanded(expanded.clone())
            .node(sample_tree())
            .on_select(Action::SelectNode)
            .on_toggle(Action::ToggleNode)
            .width(320.0)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 260.0);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(32.0, 44.0, true),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(32.0, 44.0, false),
    );
    layout_app(&mut app, 360.0, 260.0);

    assert!(expanded.read().contains(&NodeId::Src));
    assert_eq!(selected.read(), NodeId::Cargo);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::ToggleNode(NodeId::Src, true)]
    );

    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Enter));
    assert_eq!(selected.read(), NodeId::Src);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SelectNode(NodeId::Src)]
    );
}

#[test]
fn tree_view_collapse_keeps_parent_active_when_selected_child_is_hidden() {
    let selected = State::new(NodeId::Button);
    let expanded = State::new(vec![NodeId::Root, NodeId::Src, NodeId::Components]);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_selected(selected.clone())
            .bind_expanded(expanded.clone())
            .node(sample_tree())
            .on_select(Action::SelectNode)
            .on_toggle(Action::ToggleNode)
            .width(320.0)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 260.0);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(32.0, 44.0, true),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(32.0, 44.0, false),
    );
    layout_app(&mut app, 360.0, 260.0);

    assert!(!expanded.read().contains(&NodeId::Src));
    assert_eq!(selected.read(), NodeId::Button);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::ToggleNode(NodeId::Src, false)]
    );

    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Enter));
    assert_eq!(selected.read(), NodeId::Src);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SelectNode(NodeId::Src)]
    );
}

#[test]
fn tree_view_row_selects_and_dispatches() {
    let selected = State::new(NodeId::Missing);
    let expanded = State::new(vec![NodeId::Root]);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_selected(selected.clone())
            .bind_expanded(expanded)
            .node(sample_tree())
            .on_select(Action::SelectNode)
            .width(320.0)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 260.0);
    let tree = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &pointer_button(70.0, 44.0, false),
    );

    assert_eq!(selected.read(), NodeId::Src);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SelectNode(NodeId::Src)]
    );
}

#[test]
fn tree_view_activate_uses_double_click_and_enter_when_configured() {
    let selected = State::new(NodeId::Missing);
    let expanded = State::new(vec![NodeId::Root]);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_selected(selected.clone())
            .bind_expanded(expanded)
            .node(sample_tree())
            .on_select(Action::SelectNode)
            .on_activate(Action::ActivateNode)
            .width(320.0)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 260.0);
    let tree = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &pointer_button(70.0, 72.0, false),
    );
    assert_eq!(selected.read(), NodeId::Cargo);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SelectNode(NodeId::Cargo)]
    );

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &pointer_button(70.0, 72.0, false),
    );
    assert_eq!(
        runtime.take_actions(),
        vec![Action::ActivateNode(NodeId::Cargo)]
    );

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &keyboard_event(NamedKey::Enter),
    );
    assert_eq!(
        runtime.take_actions(),
        vec![Action::ActivateNode(NodeId::Cargo)]
    );
}

#[test]
fn tree_view_keyboard_navigation_and_toggle_skip_disabled() {
    let selected = State::new(NodeId::Root);
    let expanded = State::new(vec![NodeId::Root, NodeId::Src]);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_selected(selected.clone())
            .bind_expanded(expanded.clone())
            .node(sample_tree())
            .on_select(Action::SelectNode)
            .on_toggle(Action::ToggleNode)
            .width(320.0)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 260.0);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(70.0, 16.0, true),
    );
    assert!(router.focused().is_some());

    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::ArrowDown),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::ArrowDown),
    );
    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Enter));
    assert_eq!(selected.read(), NodeId::Components);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SelectNode(NodeId::Components)]
    );

    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::ArrowRight),
    );
    assert_eq!(
        runtime.take_actions(),
        vec![Action::ToggleNode(NodeId::Components, true)]
    );

    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::ArrowLeft),
    );
    assert_eq!(
        runtime.take_actions(),
        vec![Action::ToggleNode(NodeId::Components, false)]
    );
}

#[test]
fn tree_view_absent_selected_value_has_no_visual_selection_or_mutation() {
    let selected = State::new(NodeId::Missing);
    let (app, _) = layout_root(
        TreeView::<NodeId>::new()
            .bind_selected(selected.clone())
            .default_expanded(NodeId::Root)
            .node(sample_tree())
            .width(320.0)
            .into_view(),
        360.0,
        260.0,
    );
    assert_eq!(selected.read(), NodeId::Missing);

    let palette = Theme::default().palette();
    assert!(!paint_cmds(&app).iter().any(|cmd| matches!(
        cmd,
        DrawCmd::RRect(r) if r.color == palette.accent.with_alpha(0.18)
    )));
}

#[test]
fn tree_view_drag_after_mutates_bound_nodes_and_dispatches_move() {
    let nodes = State::new(vec![sample_tree()]);
    let selected = State::new(NodeId::Src);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_nodes(nodes.clone())
            .bind_selected(selected.clone())
            .default_expanded(NodeId::Root)
            .draggable(true)
            .on_move(Action::MoveNode)
            .width(320.0)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 260.0);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(70.0, 44.0, true),
    );
    router.route_event(&app.tree, runtime.clone(), &pointer_move(70.0, 87.0));
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(70.0, 87.0, false),
    );

    let mut snapshot = nodes.read();
    let root = snapshot.remove(0);
    assert_eq!(root.child_nodes()[0].id(), &NodeId::Cargo);
    assert_eq!(root.child_nodes()[1].id(), &NodeId::Src);
    assert_eq!(selected.read(), NodeId::Src);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::MoveNode(TreeMove {
            source: NodeId::Src,
            target: NodeId::Cargo,
            position: TreeDropPosition::After,
        })]
    );
}

#[test]
fn tree_view_drag_inside_reparents_node() {
    let nodes = State::new(vec![sample_tree()]);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_nodes(nodes.clone())
            .default_expanded(NodeId::Root)
            .draggable(true)
            .on_move(Action::MoveNode)
            .width(320.0)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 260.0);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(70.0, 73.0, true),
    );
    router.route_event(&app.tree, runtime.clone(), &pointer_move(70.0, 44.0));
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(70.0, 44.0, false),
    );

    let mut snapshot = nodes.read();
    let root = snapshot.remove(0);
    assert!(root
        .child_nodes()
        .iter()
        .all(|node| node.id() != &NodeId::Cargo));
    assert_eq!(
        root.child_nodes()[0].child_nodes().last().unwrap().id(),
        &NodeId::Cargo
    );
    assert_eq!(
        runtime.take_actions(),
        vec![Action::MoveNode(TreeMove {
            source: NodeId::Cargo,
            target: NodeId::Src,
            position: TreeDropPosition::Inside,
        })]
    );
}

#[test]
fn tree_view_drag_intent_only_dispatches_without_mutating_bound_nodes() {
    let nodes = State::new(vec![sample_tree()]);
    let selected = State::new(NodeId::Src);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_nodes(nodes.clone())
            .bind_selected(selected.clone())
            .default_expanded(NodeId::Root)
            .draggable(true)
            .mutation_mode(TreeMutationMode::IntentOnly)
            .on_move(Action::MoveNode)
            .width(320.0)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 260.0);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(70.0, 44.0, true),
    );
    router.route_event(&app.tree, runtime.clone(), &pointer_move(70.0, 87.0));
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(70.0, 87.0, false),
    );

    let mut snapshot = nodes.read();
    let root = snapshot.remove(0);
    assert_eq!(root.child_nodes()[0].id(), &NodeId::Src);
    assert_eq!(root.child_nodes()[1].id(), &NodeId::Cargo);
    assert_eq!(selected.read(), NodeId::Src);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::MoveNode(TreeMove {
            source: NodeId::Src,
            target: NodeId::Cargo,
            position: TreeDropPosition::After,
        })]
    );
}

#[test]
fn tree_view_rejects_drop_into_descendant() {
    let nodes = State::new(vec![sample_tree()]);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_nodes(nodes.clone())
            .default_expanded_many([NodeId::Root, NodeId::Src, NodeId::Components])
            .draggable(true)
            .on_move(Action::MoveNode)
            .width(320.0)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 260.0);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(70.0, 44.0, true),
    );
    router.route_event(&app.tree, runtime.clone(), &pointer_move(130.0, 104.0));
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(130.0, 104.0, false),
    );

    let mut snapshot = nodes.read();
    let root = snapshot.remove(0);
    assert_eq!(root.child_nodes()[0].id(), &NodeId::Src);
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn tree_view_inline_rename_mutates_label_and_dispatches() {
    let nodes = State::new(vec![sample_tree()]);
    let selected = State::new(NodeId::Src);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_nodes(nodes.clone())
            .bind_selected(selected)
            .default_expanded(NodeId::Root)
            .editable(true)
            .on_rename(Action::RenameNode)
            .width(320.0)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 260.0);
    let tree = first_child(&app, root);

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

    let mut snapshot = nodes.read();
    let root = snapshot.remove(0);
    assert_eq!(root.child_nodes()[0].label(), "X");
    assert_eq!(
        runtime.take_actions(),
        vec![Action::RenameNode(TreeRename {
            id: NodeId::Src,
            old_label: "src".to_string(),
            new_label: "X".to_string(),
        })]
    );
}

#[test]
fn tree_view_inline_rename_command_keeps_focus_and_first_key_replaces_selection() {
    let nodes = State::new(vec![sample_tree()]);
    let selected = State::new(NodeId::Src);
    let command = State::new(None::<TreeViewCommand<NodeId>>);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_nodes(nodes.clone())
            .bind_selected(selected)
            .bind_command(command.clone())
            .default_expanded(NodeId::Root)
            .editable(true)
            .on_rename(Action::RenameNode)
            .width(320.0)
            .into_view()
            .key("rename-tree"),
    );
    layout_app(&mut app, 360.0, 260.0);

    command.set(Some(TreeViewCommand::BeginRename(NodeId::Src)));
    runtime.request_focus_key("rename-tree");
    let mut router = InputRouter::default();
    router.route_event(&app.tree, runtime.clone(), &pointer_move(12.0, 12.0));
    layout_app(&mut app, 360.0, 260.0);

    let key = router.route_event(&app.tree, runtime.clone(), &character_event("Z"));
    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Enter));

    let mut snapshot = nodes.read();
    let root = snapshot.remove(0);
    assert!(key.event_dispatched);
    assert_eq!(root.child_nodes()[0].label(), "Z");
    assert_eq!(
        runtime.take_actions(),
        vec![Action::RenameNode(TreeRename {
            id: NodeId::Src,
            old_label: "src".to_string(),
            new_label: "Z".to_string(),
        })]
    );
}

#[test]
fn tree_view_insert_creates_sibling_and_enters_rename() {
    let nodes = State::new(vec![sample_tree()]);
    let selected = State::new(NodeId::Src);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_nodes(nodes.clone())
            .bind_selected(selected.clone())
            .default_expanded(NodeId::Root)
            .creatable(true)
            .editable(true)
            .create_node_with(|request| {
                assert_eq!(request.kind, TreeCreateKind::SiblingAfter);
                Some(TreeNode::leaf(NodeId::NewSibling, request.default_label))
            })
            .on_create(Action::CreateNode)
            .width(320.0)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 260.0);
    let tree = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &keyboard_event(NamedKey::Insert),
    );

    let mut snapshot = nodes.read();
    let root = snapshot.remove(0);
    assert_eq!(root.child_nodes()[1].id(), &NodeId::NewSibling);
    assert_eq!(selected.read(), NodeId::NewSibling);
    assert!(matches!(
        runtime.take_actions().as_slice(),
        [Action::CreateNode(event)] if event.id == NodeId::NewSibling
    ));
}

#[test]
fn tree_view_begin_create_command_focuses_new_node_and_first_key_replaces_selection() {
    let nodes = State::new(vec![sample_tree()]);
    let selected = State::new(NodeId::Src);
    let expanded = State::new(vec![NodeId::Root]);
    let command = State::new(None::<TreeViewCommand<NodeId>>);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_nodes(nodes.clone())
            .bind_selected(selected.clone())
            .bind_expanded(expanded.clone())
            .bind_command(command.clone())
            .default_expanded(NodeId::Root)
            .creatable(true)
            .editable(true)
            .create_node_with(|request| {
                assert_eq!(request.kind, TreeCreateKind::Child);
                assert_eq!(request.default_label, "New_Node");
                Some(TreeNode::leaf(NodeId::NewChild, request.default_label))
            })
            .on_create(Action::CreateNode)
            .width(320.0)
            .into_view()
            .key("create-tree"),
    );
    layout_app(&mut app, 360.0, 260.0);

    command.set(Some(TreeViewCommand::BeginCreate(TreeCreateRequest {
        parent: Some(NodeId::Src),
        after: None,
        kind: TreeCreateKind::Child,
        default_label: "New_Node".to_string(),
    })));
    runtime.request_focus_key("create-tree");
    let mut router = InputRouter::default();
    router.route_event(&app.tree, runtime.clone(), &pointer_move(12.0, 12.0));
    layout_app(&mut app, 360.0, 260.0);

    assert_eq!(selected.read(), NodeId::NewChild);
    assert!(expanded.read().contains(&NodeId::Src));
    let key = router.route_event(&app.tree, runtime.clone(), &character_event("Q"));
    layout_app(&mut app, 360.0, 260.0);
    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Enter));

    let mut snapshot = nodes.read();
    let root = snapshot.remove(0);
    assert!(key.event_dispatched);
    assert!(!root.child_nodes()[0]
        .child_nodes()
        .iter()
        .any(|node| node.id() == &NodeId::NewChild));
    assert!(matches!(
        runtime.take_actions().as_slice(),
        [Action::CreateNode(event)] if event.id == NodeId::NewChild && event.label == "Q"
    ));
}

#[test]
fn tree_view_keyboard_shortcuts_dispatch_without_local_delete() {
    let nodes = State::new(vec![sample_tree()]);
    let selected = State::new(NodeId::Src);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_nodes(nodes.clone())
            .bind_selected(selected)
            .default_expanded(NodeId::Root)
            .deletable(true)
            .mutation_mode(TreeMutationMode::IntentOnly)
            .on_delete(Action::DeleteNode)
            .on_shortcut(Action::Shortcut)
            .width(320.0)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 260.0);
    let tree = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &keyboard_event(NamedKey::Delete),
    );
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

    let mut snapshot = nodes.read();
    let root_node = snapshot.remove(0);
    assert_eq!(root_node.child_nodes()[0].id(), &NodeId::Src);
    assert_eq!(
        runtime.take_actions(),
        vec![
            Action::Shortcut(TreeShortcut::Delete { id: NodeId::Src }),
            Action::Shortcut(TreeShortcut::Copy { id: NodeId::Src }),
            Action::Shortcut(TreeShortcut::Cut { id: NodeId::Src }),
            Action::Shortcut(TreeShortcut::Paste {
                id: Some(NodeId::Src)
            }),
        ]
    );
}

#[test]
fn tree_view_ctrl_insert_creates_child_and_delete_removes_subtree() {
    let nodes = State::new(vec![sample_tree()]);
    let selected = State::new(NodeId::Src);
    let expanded = State::new(vec![NodeId::Root]);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        TreeView::<NodeId, Action>::new()
            .bind_nodes(nodes.clone())
            .bind_selected(selected.clone())
            .bind_expanded(expanded.clone())
            .creatable(true)
            .deletable(true)
            .editable(true)
            .create_node_with(|request| {
                assert_eq!(request.kind, TreeCreateKind::Child);
                Some(TreeNode::leaf(NodeId::NewChild, request.default_label))
            })
            .on_create(Action::CreateNode)
            .on_delete(Action::DeleteNode)
            .width(320.0)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 260.0);
    let tree = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &keyboard_event_with_modifiers(
            NamedKey::Insert,
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        ),
    );
    let mut snapshot = nodes.read();
    let root_after_create = snapshot.remove(0);
    assert_eq!(
        root_after_create.child_nodes()[0]
            .child_nodes()
            .last()
            .unwrap()
            .id(),
        &NodeId::NewChild
    );
    assert!(expanded.read().contains(&NodeId::Src));
    runtime.take_actions();

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &keyboard_event(NamedKey::Enter),
    );
    runtime.take_actions();
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        tree,
        &keyboard_event(NamedKey::Delete),
    );
    let mut snapshot = nodes.read();
    let root_after_delete = snapshot.remove(0);
    assert!(!root_after_delete.child_nodes()[0]
        .child_nodes()
        .iter()
        .any(|node| node.id() == &NodeId::NewChild));
    assert!(matches!(
        runtime.take_actions().as_slice(),
        [Action::DeleteNode(event)] if event.id == NodeId::NewChild
    ));
}

#[test]
fn tree_view_virtualized_paints_only_rows_inside_clip_with_overscan() {
    let tree = (1..120).fold(TreeNode::branch(0usize, "root"), |node, id| {
        node.child(TreeNode::leaf(id, format!("file_{id}.rs")))
    });
    let (app, _) = layout_root(
        ScrollView::<()>::vertical()
            .child(
                TreeView::<usize>::new()
                    .virtualized(true)
                    .default_expanded(0)
                    .node(tree)
                    .width(320.0),
            )
            .into_view(),
        360.0,
        120.0,
    );

    let text_count = paint_cmds(&app)
        .iter()
        .filter(|cmd| matches!(cmd, DrawCmd::Text(_)))
        .count();
    assert!(
        text_count < 40,
        "virtualized tree should not paint every row, painted {text_count}"
    );
}

fn item<A: 'static>(id: &'static str, title: &'static str) -> AccordionItem<A> {
    AccordionItem::new(id, title).child(Text::new(format!("Content {id}")))
}

fn sample_tree() -> TreeNode<NodeId> {
    TreeNode::branch(NodeId::Root, "Project Root")
        .leading_icon(IconId::History)
        .child(
            TreeNode::branch(NodeId::Src, "src")
                .leading_icon(IconId::Plus)
                .child(
                    TreeNode::branch(NodeId::Components, "components")
                        .leading_icon(IconId::Copy)
                        .child(
                            TreeNode::leaf(NodeId::Button, "button.rs").leading_icon(IconId::Check),
                        ),
                )
                .child(TreeNode::leaf(NodeId::Disabled, "disabled.rs").disabled(true)),
        )
        .child(TreeNode::leaf(NodeId::Cargo, "Cargo.toml").leading_icon(IconId::Check))
}

fn layout_root(view: View<()>, w: f32, h: f32) -> (Runtime<()>, ailloli_ui_core::ElementId) {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(view);
    layout_app(&mut app, w, h);
    (app, root)
}

fn layout_app<A: 'static>(app: &mut Runtime<A>, w: f32, h: f32) -> ailloli_ui_core::Size {
    let mut text_system = TextSystem::new();
    app.layout(Constraints::loose(w, h), Scale::new(1.0), &mut text_system);
    let root = app.tree.root().expect("root element");
    app.tree.get(root).unwrap().layout.as_ref().unwrap().size
}

fn root_size<A>(app: &Runtime<A>) -> ailloli_ui_core::Size {
    let root = app.tree.root().expect("root element");
    app.tree.get(root).unwrap().layout.as_ref().unwrap().size
}

fn first_child<A>(
    app: &Runtime<A>,
    root: ailloli_ui_core::ElementId,
) -> ailloli_ui_core::ElementId {
    app.tree.children_of(root).first().copied().unwrap_or(root)
}

fn paint_cmds<A: 'static>(app: &Runtime<A>) -> Vec<DrawCmd> {
    let mut text_system = TextSystem::new();
    app.paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter().cloned())
        .collect()
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
