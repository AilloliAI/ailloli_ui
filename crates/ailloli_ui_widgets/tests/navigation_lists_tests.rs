use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{IconId, Point, Theme};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, View};
use ailloli_ui_runtime::input::{dispatch_event_to_target, InputRouter};
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{
    DisclosureRow, DisclosureRowStyle, DisclosureRowVariant, ListItem, ListItemVariant, ListView,
    ListViewStyle, NavItem, NavItemStyle, Sidebar, SidebarStyle,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Open,
    Delete,
}

#[test]
fn navigation_list_styles_use_theme_tokens() {
    let theme = Theme::default();
    let palette = theme.palette();

    let sidebar = SidebarStyle::from_theme(theme);
    assert_eq!(sidebar.background, palette.surface);
    assert_eq!(sidebar.border.colors.top, palette.border);
    assert_eq!(sidebar.width, 220.0);

    let nav = NavItemStyle::from_theme(theme);
    assert_eq!(nav.selected_background, palette.accent.with_alpha(0.22));
    assert_eq!(nav.badge_background, palette.accent);
    assert_eq!(nav.focus_ring.colors.top, palette.focus);

    let list_view = ListViewStyle::from_theme(theme);
    assert_eq!(list_view.background, palette.surface);
    assert_eq!(list_view.border.colors.top, palette.border);

    let row = DisclosureRowStyle::from_theme(theme);
    assert_eq!(row.chevron_tint, palette.text_muted);
    assert_eq!(row.focus_ring.colors.top, palette.focus);
}

#[test]
fn sidebar_composes_items_with_stable_width() {
    let (app, root) = layout_view(
        Sidebar::<()>::new()
            .title("Workspace")
            .nav_item(NavItem::new("Dashboard").selected(true))
            .nav_item(NavItem::new("Messages").badge(3))
            .into_view(),
        260.0,
        180.0,
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();

    assert_eq!(layout.size.w, 220.0);
    assert_eq!(layout.children.len(), 1);
    assert!(paint_cmds(&app)
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Border(_))));
}

#[test]
fn nav_item_selected_paints_fill_badge_icon_and_text() {
    let palette = Theme::default().palette();
    let (app, _) = layout_view(
        NavItem::<()>::new("Dashboard")
            .leading_icon(IconId::Check)
            .badge(3)
            .selected(true)
            .into_view(),
        220.0,
        60.0,
    );
    let cmds = paint_cmds(&app);

    assert!(cmds.iter().any(|cmd| matches!(
        cmd,
        DrawCmd::RRect(r) if r.color == palette.accent.with_alpha(0.22)
    )));
    assert!(cmds.iter().any(|cmd| matches!(cmd, DrawCmd::Image(_))));
    assert!(
        cmds.iter()
            .filter(|cmd| matches!(cmd, DrawCmd::Text(_)))
            .count()
            >= 2
    );
}

#[test]
fn nav_item_dispatches_click_and_keyboard_but_disabled_blocks() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        NavItem::<Action>::new("Messages")
            .on_select(Action::Open)
            .into_view(),
    );
    layout_app(&mut app, 220.0, 80.0);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(20.0, 16.0, false),
    );
    assert_eq!(runtime.take_actions(), vec![Action::Open]);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(20.0, 16.0, true),
    );
    assert_eq!(router.focused(), Some(root));
    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Enter));
    assert_eq!(runtime.take_actions(), vec![Action::Open]);

    let disabled_root = app.reconcile(
        NavItem::<Action>::new("Disabled")
            .disabled(true)
            .on_select(Action::Delete)
            .into_view(),
    );
    layout_app(&mut app, 220.0, 80.0);
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(20.0, 16.0, true),
    );
    assert_eq!(router.focused(), None);
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        disabled_root,
        &pointer_button(20.0, 16.0, false),
    );
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn list_view_measures_all_rows_and_list_item_paints_variants() {
    let (app, root) = layout_view(
        ListView::<()>::new()
            .item(
                ListItem::new("Inbox")
                    .leading_icon(IconId::Check)
                    .trailing_text("12"),
            )
            .item(ListItem::new("Starred").subtitle("Pinned messages"))
            .item(
                ListItem::new("Trash")
                    .variant(ListItemVariant::Danger)
                    .badge(7),
            )
            .into_view(),
        320.0,
        220.0,
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 260.0);
    assert!(layout.size.h >= 130.0, "height={}", layout.size.h);

    let cmds = paint_cmds(&app);
    assert!(cmds.iter().any(|cmd| matches!(cmd, DrawCmd::Image(_))));
    assert!(
        cmds.iter()
            .filter(|cmd| matches!(cmd, DrawCmd::Text(_)))
            .count()
            >= 5
    );
    assert!(cmds.iter().any(|cmd| matches!(cmd, DrawCmd::RRect(_))));
}

#[test]
fn list_item_dispatches_only_inside_bounds() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        ListItem::<Action>::new("Inbox")
            .on_select(Action::Open)
            .into_view(),
    );
    layout_app(&mut app, 220.0, 80.0);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(260.0, 16.0, false),
    );
    assert!(runtime.take_actions().is_empty());

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(20.0, 16.0, false),
    );
    assert_eq!(runtime.take_actions(), vec![Action::Open]);
}

#[test]
fn disclosure_row_paints_chevron_trailing_and_dispatches() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        DisclosureRow::<Action>::new("Wi-Fi")
            .leading_icon(IconId::Check)
            .trailing_text("Connected")
            .on_select(Action::Open)
            .into_view(),
    );
    layout_app(&mut app, 260.0, 80.0);

    let cmds = paint_cmds_action(&app);
    assert!(
        cmds.iter()
            .filter(|cmd| matches!(cmd, DrawCmd::Image(_)))
            .count()
            >= 2
    );
    assert!(
        cmds.iter()
            .filter(|cmd| matches!(cmd, DrawCmd::Text(_)))
            .count()
            >= 2
    );

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(20.0, 18.0, false),
    );
    assert_eq!(runtime.take_actions(), vec![Action::Open]);
}

#[test]
fn disclosure_row_disabled_is_not_focusable_or_dispatching() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        DisclosureRow::<Action>::new("About")
            .variant(DisclosureRowVariant::Danger)
            .disabled(true)
            .on_select(Action::Delete)
            .into_view(),
    );
    layout_app(&mut app, 260.0, 80.0);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(20.0, 18.0, true),
    );
    assert_eq!(router.focused(), None);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(20.0, 18.0, false),
    );
    assert!(runtime.take_actions().is_empty());
}

fn layout_view(view: View<()>, w: f32, h: f32) -> (Runtime<()>, ailloli_ui_core::ElementId) {
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

fn paint_cmds(app: &Runtime<()>) -> Vec<DrawCmd> {
    let mut text_system = TextSystem::new();
    app.paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter().cloned())
        .collect()
}

fn paint_cmds_action(app: &Runtime<Action>) -> Vec<DrawCmd> {
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

fn keyboard_event(key: NamedKey) -> Event {
    Event::Keyboard(KeyEvent {
        state: KeyState::Pressed,
        key: Key::Named(key),
        modifiers: Modifiers::default(),
        repeat: false,
        pointer_pos: Some(Point::new(20.0, 16.0)),
        text: None,
    })
}
