//! Toast, dialog, and command-palette overlay interaction scenarios.

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Color, IconId, Point, Theme};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State};
use ailloli_ui_runtime::input::{dispatch_event_to_target, InputRole, InputRouter};
use ailloli_ui_runtime::popup::{PopupFocusPolicy, PopupRole};
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{
    CommandItem, CommandPalette, CommandPaletteSize, CommandPaletteStyle, Dialog, DialogStyle,
    DialogTone, Toast, ToastHost, ToastPosition, ToastStyle, ToastTone,
};
use ailloli_ui_widgets::layout::Container;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Dismiss(String),
    Confirm,
    Cancel,
    Search,
    Submit,
}

#[test]
fn feedback_overlay_styles_use_theme_tokens() {
    let theme = Theme::default();
    let palette = theme.palette();

    let toast = ToastStyle::from_theme(theme);
    assert_eq!(toast.background, palette.surface_elevated);
    assert_eq!(toast.border.colors.top, palette.border);
    assert_eq!(toast.title_text.color, palette.text);
    assert_eq!(toast.description_text.color, palette.text_muted);
    assert_eq!(toast.success, palette.success);
    assert_eq!(toast.warning, palette.warning);
    assert_eq!(toast.danger, palette.danger);
    assert_eq!(toast.info, palette.info);

    let dialog = DialogStyle::from_theme(Theme::default(), DialogTone::Danger);
    assert_eq!(dialog.panel_background, palette.surface_elevated);
    assert_eq!(dialog.border.colors.top, palette.border);
    assert_eq!(dialog.title_text.color, palette.text);
    assert_eq!(dialog.body_text.color, palette.text_muted);
    assert_eq!(dialog.danger_background, palette.danger);

    let palette_style =
        CommandPaletteStyle::from_theme(Theme::default(), CommandPaletteSize::Default);
    assert_eq!(palette_style.panel_background, palette.surface_elevated);
    assert_eq!(palette_style.border.colors.top, palette.border);
    assert_eq!(palette_style.title_text.color, palette.text);
    assert_eq!(palette_style.subtitle_text.color, palette.text_muted);
    assert_eq!(palette_style.input.border_focused, palette.focus);
}

#[test]
fn toast_host_overlay_does_not_change_child_layout_and_close_dismisses() {
    let toasts = State::new(vec![Toast::new("one", "Saved")
        .description("Done")
        .tone(ToastTone::Success)
        .leading_icon(IconId::Check)]);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        ToastHost::<Action>::new()
            .fill()
            .bind_toasts(toasts.clone())
            .position(ToastPosition::TopRight)
            .on_dismiss(Action::Dismiss)
            .child(Container::new().fill().background(Color::hex_rgb(0x111416)))
            .into_view(),
    );
    layout_app(&mut app, 400.0, 220.0);
    let host = first_child(&app, root);
    let layout = app.tree.get(host).unwrap().layout.as_ref().unwrap();

    assert_eq!(layout.size.w, 400.0);
    assert_eq!(layout.size.h, 220.0);
    assert_eq!(layout.children.len(), 1);
    assert_eq!(layout.children[0].size.w, 400.0);
    assert_eq!(layout.overlay_hit_bounds.len(), 1);
    assert!(paint_cmds_action(&app)
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::BoxShadow(_))));

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        host,
        &pointer_button(362.0, 46.0, false),
    );

    assert!(toasts.read().is_empty());
    assert_eq!(runtime.take_actions(), vec![Action::Dismiss("one".into())]);
}

#[test]
fn dialog_confirm_cancel_backdrop_and_escape_close_bound_state() {
    let open = State::new(true);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Dialog::<Action>::new()
            .fill()
            .bind_open(open.clone())
            .title("Delete Project")
            .body("Are you sure you want to delete this project? This action cannot be undone.")
            .on_confirm(Action::Confirm)
            .on_cancel(Action::Cancel)
            .child(Container::new().fill().background(Color::hex_rgb(0x111416)))
            .into_view(),
    );
    layout_app(&mut app, 420.0, 280.0);
    let dialog = first_child(&app, root);
    let layout = app.tree.get(dialog).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.overlay_hit_bounds.len(), 1);

    let cmds = paint_cmds_action(&app);
    assert!(
        cmds.iter().any(|cmd| {
            matches!(
                cmd,
                DrawCmd::Text(text)
                    if text.color == Theme::default().palette().text_muted
                        && text.layout.lines.len() > 1
            )
        }),
        "dialog body should wrap long text"
    );
    let button_text_centers: Vec<f32> = cmds
        .iter()
        .filter_map(|cmd| match cmd {
            DrawCmd::Text(text) if (180.0..=214.0).contains(&text.pos[1]) => {
                Some(text.pos[0] + text.layout.metrics.width * 0.5)
            }
            _ => None,
        })
        .collect();
    assert!(
        button_text_centers
            .iter()
            .any(|center| (center - 218.0).abs() <= 2.0),
        "cancel label should be centered, centers={button_text_centers:?}"
    );
    assert!(
        button_text_centers
            .iter()
            .any(|center| (center - 324.0).abs() <= 2.0),
        "confirm label should be centered, centers={button_text_centers:?}"
    );

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        dialog,
        &pointer_button(308.0, 190.0, false),
    );
    assert!(!open.read());
    assert_eq!(runtime.take_actions(), vec![Action::Confirm]);

    open.set(true);
    layout_app(&mut app, 420.0, 280.0);
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        dialog,
        &pointer_button(8.0, 8.0, false),
    );
    assert!(!open.read());
    assert_eq!(runtime.take_actions(), vec![Action::Cancel]);

    open.set(true);
    layout_app(&mut app, 420.0, 280.0);
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        dialog,
        &keyboard_event(NamedKey::Escape),
    );
    assert!(!open.read());
    assert_eq!(runtime.take_actions(), vec![Action::Cancel]);
}

#[test]
fn dialog_composed_content_mounts_a_retained_modal_popup() {
    let open = State::new(true);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Dialog::<Action>::new()
            .fill()
            .bind_open(open.clone())
            .on_submit(Action::Submit)
            .on_cancel(Action::Cancel)
            .modal_content(
                Container::new()
                    .width(300.0)
                    .height(180.0)
                    .background(Color::hex_rgb(0x202428)),
            )
            .child(Container::new().fill().background(Color::hex_rgb(0x111416)))
            .into_view(),
    );
    layout_app(&mut app, 640.0, 360.0);

    let popup_view = {
        let portal = runtime.popup_portal();
        let portal = portal.borrow();
        let popup_id = portal
            .topmost()
            .expect("composed dialog popup should be open");
        let request = portal.request(popup_id).expect("registered dialog request");
        assert_eq!(request.semantics().role(), PopupRole::Dialog);
        assert_eq!(
            request.semantics().focus_policy(),
            PopupFocusPolicy::TrapWithinPopup
        );
        assert!(request.semantics().restores_focus_on_close());
        assert_eq!(
            portal.bounds(popup_id),
            Some(ailloli_ui_core::Rect::new(0.0, 0.0, 640.0, 360.0))
        );
        request.content().build()
    };
    assert_eq!(popup_view.children.len(), 1);

    let mut popup_app = Runtime::new(runtime.clone());
    let surface = popup_app.reconcile(popup_view);
    layout_app(&mut popup_app, 640.0, 360.0);
    dispatch_event_to_target(
        &popup_app.tree,
        runtime.clone(),
        surface,
        &keyboard_event(NamedKey::Enter),
    );
    assert!(!open.read());
    assert_eq!(runtime.take_actions(), vec![Action::Submit]);

    open.set(true);
    layout_app(&mut popup_app, 640.0, 360.0);
    dispatch_event_to_target(
        &popup_app.tree,
        runtime.clone(),
        surface,
        &keyboard_event(NamedKey::Escape),
    );
    assert!(!open.read());
    assert_eq!(runtime.take_actions(), vec![Action::Cancel]);

    open.set(true);
    layout_app(&mut popup_app, 640.0, 360.0);
    dispatch_event_to_target(
        &popup_app.tree,
        runtime.clone(),
        surface,
        &pointer_button(4.0, 4.0, false),
    );
    assert!(!open.read());
    assert_eq!(runtime.take_actions(), vec![Action::Cancel]);
}

#[test]
fn dialog_legacy_text_waits_for_committed_layout_artifacts() {
    let title = State::new("Old title".to_string());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        Dialog::<Action>::new()
            .fill()
            .default_open(true)
            .title(title.clone())
            .child(Container::new().fill())
            .into_view(),
    );
    layout_app(&mut app, 420.0, 280.0);
    assert!(paint_cmds_action(&app)
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "Old title")));

    title.set("Fresh title".to_string());
    assert!(!paint_cmds_action(&app)
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "Fresh title")));

    layout_app(&mut app, 420.0, 280.0);
    assert!(paint_cmds_action(&app)
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "Fresh title")));
}

#[test]
fn command_palette_filters_selects_and_reports_text_input_role() {
    let open = State::new(true);
    let query = State::new("se".to_string());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        CommandPalette::<Action>::new()
            .fill()
            .bind_open(open.clone())
            .bind_query(query.clone())
            .item(
                CommandItem::new("Search")
                    .subtitle("Find text")
                    .shortcut("Ctrl+F")
                    .keyword("find")
                    .leading_icon(IconId::History)
                    .on_select(Action::Search),
            )
            .item(
                CommandItem::new("Settings")
                    .keyword("preferences")
                    .disabled(true),
            )
            .child(Container::new().fill().background(Color::hex_rgb(0x111416)))
            .into_view(),
    );
    layout_app(&mut app, 640.0, 360.0);
    let palette = first_child(&app, root);
    let layout = app.tree.get(palette).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.overlay_hit_bounds.len(), 1);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(320.0, 90.0, true),
    );
    assert_eq!(router.focused(), Some(palette));
    assert_eq!(
        router.focused_input_role(&app.tree),
        InputRole::TextSingleLine
    );

    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Enter));
    assert!(!open.read());
    assert_eq!(runtime.take_actions(), vec![Action::Search]);

    open.set(true);
    query.set("settings".to_string());
    layout_app(&mut app, 640.0, 360.0);
    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Enter));
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn command_palette_query_waits_for_a_matching_layout_artifact() {
    let open = State::new(true);
    let query = State::new("old-query".to_string());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CommandPalette::<Action>::new()
            .fill()
            .bind_open(open)
            .bind_query(query.clone())
            .item(CommandItem::new("Search").keyword("old-query"))
            .child(Container::new().fill().background(Color::hex_rgb(0x111416)))
            .into_view(),
    );
    layout_app(&mut app, 640.0, 360.0);
    assert!(paint_cmds_action(&app)
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "old-query")));

    query.set("fresh-query".to_string());

    let plan = runtime.frame_work_plan();
    assert!(plan.needs_layout());
    let stale = paint_cmds_action(&app);
    assert!(
        !stale
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "fresh-query")),
        "fresh query must not be shaped into stale overlay geometry"
    );

    layout_app(&mut app, 640.0, 360.0);
    assert!(paint_cmds_action(&app)
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "fresh-query")));
}

fn layout_app<A: 'static>(app: &mut Runtime<A>, w: f32, h: f32) -> ailloli_ui_core::Size {
    let mut text_system = TextSystem::new();
    app.layout(Constraints::loose(w, h), Scale::new(1.0), &mut text_system);
    let root = app.tree.root().expect("root element");
    app.tree.get(root).unwrap().layout.as_ref().unwrap().size
}

fn first_child<A>(
    app: &Runtime<A>,
    root: ailloli_ui_core::ElementId,
) -> ailloli_ui_core::ElementId {
    app.tree.children_of(root).first().copied().unwrap_or(root)
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
        pointer_pos: Some(Point::new(20.0, 20.0)),
        text: None,
    })
}
