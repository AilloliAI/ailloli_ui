use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent, WheelDelta};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{IconId, Point, Theme};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State, View};
use ailloli_ui_runtime::input::{dispatch_event_to_target, InputRouter};
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{
    BadgeTone, TableAlign, TableCell, TableColumn, TableColumnWidth, TableRow, TableView,
    TableViewSize, TableViewStyle,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum RowId {
    Alex,
    Maya,
    Jordan,
    Taylor,
    Missing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Open(RowId),
}

#[test]
fn table_view_style_uses_theme_tokens() {
    let theme = Theme::default();
    let palette = theme.palette();
    let style = TableViewStyle::from_theme(theme, TableViewSize::Default);

    assert_eq!(style.background, palette.surface);
    assert_eq!(style.header_background, palette.surface_elevated);
    assert_eq!(
        style.row_selected_background,
        palette.accent.with_alpha(0.18)
    );
    assert_eq!(style.border.colors.top, palette.border);
    assert_eq!(style.focus_ring.colors.top, palette.focus);
    assert_eq!(style.progress_fill, palette.accent);
    assert_eq!(style.shadows, vec![theme.shadows().sm]);
}

#[test]
fn table_view_layout_respects_width_columns_and_max_body_height() {
    let (app, root) = layout_root(
        sample_table::<()>()
            .width(420.0)
            .max_body_height(72.0)
            .into_view(),
        800.0,
        400.0,
    );
    let table = first_child(&app, root);
    let layout = app.tree.get(table).unwrap().layout.as_ref().unwrap();

    assert_eq!(layout.size.w, 420.0);
    assert_eq!(layout.size.h, 106.0);
    assert_eq!(layout.paint_bounds.w, 420.0);
    assert!(layout.visual_bounds.w >= layout.paint_bounds.w);
}

#[test]
fn table_view_paints_header_rows_badges_progress_and_border() {
    let palette = Theme::default().palette();
    let (app, _) = layout_root(
        sample_table::<()>()
            .selected(RowId::Alex)
            .width(520.0)
            .max_body_height(140.0)
            .into_view(),
        800.0,
        400.0,
    );
    let cmds = paint_cmds(&app);

    assert!(cmds.iter().any(|cmd| matches!(cmd, DrawCmd::BoxShadow(_))));
    assert!(cmds.iter().any(|cmd| matches!(cmd, DrawCmd::Border(_))));
    assert!(cmds.iter().any(|cmd| matches!(
        cmd,
        DrawCmd::Rect(rect) if rect.color == palette.surface_elevated
    )));
    assert!(cmds.iter().any(|cmd| matches!(
        cmd,
        DrawCmd::Rect(rect) if rect.color == palette.accent.with_alpha(0.18)
    )));
    assert!(cmds.iter().any(|cmd| matches!(cmd, DrawCmd::Image(_))));
    assert!(
        cmds.iter()
            .filter(|cmd| matches!(cmd, DrawCmd::Text(_)))
            .count()
            >= 12
    );
    assert!(
        cmds.iter()
            .filter(|cmd| matches!(cmd, DrawCmd::RRect(_)))
            .count()
            >= 4
    );
}

#[test]
fn table_view_click_selects_enabled_row_and_disabled_blocks() {
    let selected = State::new(RowId::Missing);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        sample_table::<Action>()
            .bind_selected(selected.clone())
            .on_select(Action::Open)
            .width(520.0)
            .max_body_height(140.0)
            .into_view(),
    );
    layout_app(&mut app, 800.0, 400.0);
    let table = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        table,
        &pointer_button(20.0, 52.0, false),
    );
    assert_eq!(selected.read(), RowId::Alex);
    assert_eq!(runtime.take_actions(), vec![Action::Open(RowId::Alex)]);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        table,
        &pointer_button(20.0, 88.0, false),
    );
    assert_eq!(selected.read(), RowId::Alex);
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn table_view_keyboard_navigation_skips_disabled_rows() {
    let selected = State::new(RowId::Missing);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        sample_table::<Action>()
            .bind_selected(selected.clone())
            .on_select(Action::Open)
            .width(520.0)
            .max_body_height(140.0)
            .into_view(),
    );
    layout_app(&mut app, 800.0, 400.0);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(20.0, 52.0, true),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::ArrowDown),
    );
    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Enter));

    assert_eq!(selected.read(), RowId::Jordan);
    assert_eq!(runtime.take_actions(), vec![Action::Open(RowId::Jordan)]);
}

#[test]
fn table_view_value_absent_has_no_visual_selection_or_mutation() {
    let selected = State::new(RowId::Missing);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        sample_table::<Action>()
            .bind_selected(selected.clone())
            .width(520.0)
            .max_body_height(140.0)
            .into_view(),
    );
    layout_app(&mut app, 800.0, 400.0);

    let palette = Theme::default().palette();
    assert!(!paint_cmds_action(&app).iter().any(|cmd| matches!(
        cmd,
        DrawCmd::Rect(rect) if rect.color == palette.accent.with_alpha(0.18)
    )));
    assert_eq!(selected.read(), RowId::Missing);
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn table_view_wheel_scrolls_body_with_core_scroll_state() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        sample_table::<Action>()
            .width(360.0)
            .max_body_height(72.0)
            .into_view(),
    );
    layout_app(&mut app, 800.0, 400.0);
    let table = first_child(&app, root);

    dispatch_event_to_target(&app.tree, runtime, table, &wheel_event(120.0, 52.0, -2.0));
    let cmds = paint_cmds_action(&app);
    let alex_y = cmds.iter().find_map(|cmd| match cmd {
        DrawCmd::Text(text) if text.layout.text() == "Alex Rivera" => Some(text.pos[1]),
        _ => None,
    });
    let jordan_y = cmds.iter().find_map(|cmd| match cmd {
        DrawCmd::Text(text) if text.layout.text() == "Jordan Kim" => Some(text.pos[1]),
        _ => None,
    });

    assert!(alex_y.is_none(), "alex_y={alex_y:?}");
    assert!(
        matches!(jordan_y, Some(y) if y < 60.0),
        "jordan_y={jordan_y:?}"
    );
}

fn sample_table<A: 'static>() -> TableView<RowId, A> {
    TableView::new()
        .column(TableColumn::new("Name").width(150.0))
        .column(TableColumn::new("Role").column_width(TableColumnWidth::Auto))
        .column(TableColumn::new("Status").width(100.0))
        .column(TableColumn::new("Progress").width(120.0))
        .column(TableColumn::new("Date").flex(1.0).align(TableAlign::End))
        .row(
            TableRow::new(RowId::Alex)
                .leading_icon(IconId::Check)
                .cell(TableCell::text("Alex Rivera"))
                .cell(TableCell::muted("Designer"))
                .cell(TableCell::badge("Active", BadgeTone::Success))
                .cell(TableCell::progress(0.72))
                .cell(TableCell::muted("May 14, 2024").align(TableAlign::End)),
        )
        .row(
            TableRow::new(RowId::Maya)
                .disabled(true)
                .cell(TableCell::text("Maya Chen"))
                .cell(TableCell::muted("Developer"))
                .cell(TableCell::badge("Active", BadgeTone::Success))
                .cell(TableCell::progress(0.63))
                .cell(TableCell::muted("May 13, 2024").align(TableAlign::End)),
        )
        .row(
            TableRow::new(RowId::Jordan)
                .cell(TableCell::text("Jordan Kim"))
                .cell(TableCell::muted("Product"))
                .cell(TableCell::badge("Pending", BadgeTone::Warning))
                .cell(TableCell::progress(0.28))
                .cell(TableCell::muted("May 12, 2024").align(TableAlign::End)),
        )
        .row(
            TableRow::new(RowId::Taylor)
                .cell(TableCell::text("Taylor Smith"))
                .cell(TableCell::muted("Marketing"))
                .cell(TableCell::badge("Inactive", BadgeTone::Muted))
                .cell(TableCell::progress(0.0))
                .cell(TableCell::muted("May 11, 2024").align(TableAlign::End)),
        )
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

fn first_child<A>(
    app: &Runtime<A>,
    root: ailloli_ui_core::ElementId,
) -> ailloli_ui_core::ElementId {
    app.tree.children_of(root).first().copied().unwrap_or(root)
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

fn wheel_event(x: f32, y: f32, wheel_y: f32) -> Event {
    Event::Pointer(PointerEvent::Wheel {
        pos: Point::new(x, y),
        delta: WheelDelta::LineDelta { x: 0.0, y: wheel_y },
        modifiers: Modifiers::default(),
        precise: false,
    })
}

fn keyboard_event(key: NamedKey) -> Event {
    Event::Keyboard(KeyEvent {
        state: KeyState::Pressed,
        key: Key::Named(key),
        modifiers: Modifiers::default(),
        repeat: false,
        pointer_pos: Some(Point::new(20.0, 52.0)),
        text: None,
    })
}
