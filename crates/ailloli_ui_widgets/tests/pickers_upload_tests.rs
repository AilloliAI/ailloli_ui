use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, FileEvent, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{
    Color, DateValue, MonthValue, Point, Theme, TimeFormat, TimeValue, UploadFile,
};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State, View};
use ailloli_ui_runtime::input::{dispatch_event_to_target, InputRouter};
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{
    ColorPicker, ColorPickerSize, ColorPickerStyle, DatePicker, DatePickerSize, DatePickerStyle,
    TimePicker, TimePickerSize, TimePickerStyle, UploadDropzone, UploadDropzoneStyle,
    UploadDropzoneVariant,
};

#[derive(Clone, Debug, PartialEq)]
enum Action {
    Date(DateValue),
    Time(TimeValue),
    Color(Color),
    Browse,
    Drop(Vec<UploadFile>),
}

#[test]
fn picker_and_upload_styles_use_theme_tokens() {
    let theme = Theme::default();
    let palette = theme.palette();
    let date = DatePickerStyle::from_theme(theme, DatePickerSize::Default);
    assert_eq!(date.base.trigger_background, palette.surface_elevated);
    assert_eq!(date.base.border.colors.top, palette.border);
    assert_eq!(date.base.text.color, palette.text);
    assert_eq!(date.base.muted_text.color, palette.text_muted);
    assert_eq!(date.base.focus_ring.colors.top, palette.focus);

    let time = TimePickerStyle::from_theme(Theme::default(), TimePickerSize::Compact);
    assert_eq!(time.base.height, 30.0);
    assert_eq!(time.row_height, 26.0);

    let color = ColorPickerStyle::from_theme(Theme::default(), ColorPickerSize::Default);
    assert_eq!(color.base.selected, palette.accent);
    assert!(color.popup_width > 240.0);

    let upload = UploadDropzoneStyle::from_theme(Theme::default(), UploadDropzoneVariant::Default);
    assert_eq!(upload.background, palette.surface);
    assert_eq!(upload.border.colors.top, palette.border);
    assert_eq!(upload.button_background, palette.accent);
}

#[test]
fn date_picker_popup_is_overlay_and_selects_enabled_day() {
    let selected = State::new(None::<DateValue>);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        DatePicker::<Action>::new()
            .bind(selected.clone())
            .default_month(MonthValue::new(2026, 5))
            .default_open(true)
            .min(DateValue::new(2026, 5, 1))
            .max(DateValue::new(2026, 5, 31))
            .on_change(Action::Date)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 360.0);
    let picker = first_child(&app, root);
    let layout = app.tree.get(picker).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.h, 36.0);
    assert_eq!(layout.overlay_hit_bounds.len(), 1);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        picker,
        &pointer_button(190.0, 116.0, false),
    );

    assert_eq!(selected.read(), Some(DateValue::new(2026, 5, 1)));
    assert_eq!(
        runtime.take_actions(),
        vec![Action::Date(DateValue::new(2026, 5, 1))]
    );
}

#[test]
fn time_picker_keyboard_and_pointer_snap_to_step() {
    let selected = State::new(Some(TimeValue::new(14, 30)));
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        TimePicker::<Action>::new()
            .bind(selected.clone())
            .default_open(true)
            .step_minutes(15)
            .format(TimeFormat::Hour24)
            .on_change(Action::Time)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 320.0);
    let picker = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        picker,
        &keyboard_event(NamedKey::ArrowDown),
    );
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        picker,
        &keyboard_event(NamedKey::Enter),
    );

    assert_eq!(selected.read(), Some(TimeValue::new(15, 30)));
    assert_eq!(
        runtime.take_actions(),
        vec![Action::Time(TimeValue::new(15, 30))]
    );
}

#[test]
fn color_picker_swatch_updates_signal_and_dispatches() {
    let selected = State::new(Color::hex_rgb(0xFF5A00));
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        ColorPicker::<Action>::new()
            .bind(selected.clone())
            .default_open(true)
            .swatch(Color::hex_rgb(0x22C55E))
            .on_change(Action::Color)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 420.0);
    let picker = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        picker,
        &pointer_button(20.0, 274.0, true),
    );

    assert_eq!(selected.read(), Color::hex_rgb(0x22C55E));
    assert_eq!(
        runtime.take_actions(),
        vec![Action::Color(Color::hex_rgb(0x22C55E))]
    );
}

#[test]
fn color_picker_reports_text_input_role_when_open() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(ColorPicker::<()>::new().default_open(true).into_view());
    layout_app(&mut app, 360.0, 420.0);
    let picker = first_child(&app, root);
    let mut router = InputRouter::default();
    router.route_event(&app.tree, runtime, &pointer_button(20.0, 12.0, true));
    assert_eq!(router.focused(), Some(picker));
    assert_eq!(
        router.focused_input_role(&app.tree),
        ailloli_ui_runtime::input::InputRole::TextSingleLine
    );
}

#[test]
fn upload_dropzone_dispatches_browse_and_filtered_drop() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        UploadDropzone::<Action>::new()
            .accept([".png"])
            .on_browse(Action::Browse)
            .on_drop(Action::Drop)
            .into_view(),
    );
    layout_app(&mut app, 420.0, 220.0);
    let dropzone = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        dropzone,
        &pointer_button(180.0, 96.0, false),
    );
    assert_eq!(runtime.take_actions(), vec![Action::Browse]);

    let files = vec![
        UploadFile::named("avatar.png"),
        UploadFile::named("notes.txt"),
    ];
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        dropzone,
        &Event::File(FileEvent::Drop {
            pos: Point::new(20.0, 20.0),
            files: files.clone(),
        }),
    );

    assert_eq!(
        runtime.take_actions(),
        vec![Action::Drop(vec![UploadFile::named("avatar.png")])]
    );
}

#[test]
fn pickers_paint_overlay_commands() {
    let (app, root) = layout_view(
        DatePicker::<()>::new()
            .default_open(true)
            .default_month(MonthValue::new(2026, 5))
            .into_view(),
    );
    let picker = first_child(&app, root);
    assert!(!app
        .tree
        .get(picker)
        .unwrap()
        .layout
        .as_ref()
        .unwrap()
        .overlay_hit_bounds
        .is_empty());
    let scene = paint_cmds(&app);
    assert!(scene.iter().any(|cmd| matches!(cmd, DrawCmd::BoxShadow(_))));
    assert!(scene.iter().any(|cmd| matches!(cmd, DrawCmd::Text(_))));
}

fn layout_view(view: View<()>) -> (Runtime<()>, ailloli_ui_core::ElementId) {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(view);
    layout_app(&mut app, 360.0, 420.0);
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
        .flat_map(|layer| layer.cmds.clone())
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
        pointer_pos: None,
        text: None,
    })
}
