use std::rc::Rc;
use std::time::Duration;

use ailloli_ui_core::event::{
    ActivationKind, Event, Key, KeyEvent, KeyState, Modifiers, MouseButton, NamedKey, PointerEvent,
    PointerId, PointerSample, PointerSource,
};
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Constraints, Offset, Point, Rect};
use ailloli_ui_runtime::app::{
    MemoryExternalUrlOpener, PresentationGeneration, Runtime, RuntimeHandle,
};
use ailloli_ui_runtime::component::{IntoView, State, View, Widget};
use ailloli_ui_runtime::input::{
    ActivationPolicy, EventEnvelope, EventId, EventMeta, EventTimestamp, InputRouter,
};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_text::{TextBuffer, TextSystem};
use ailloli_ui_widgets::controls::{
    Autocomplete, Button, ComboBox, ContextMenu, ContextMenuEntry, ContextMenuItem, Link, ListItem,
    NavItem, RadioButton, RadioGroup, Switch, TextInput,
};
use ailloli_ui_widgets::editor::Editor;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    Button,
    Switch(bool),
    RadioButton,
    RadioGroup(u8),
    Combo(u8),
    Autocomplete(String),
    ContextMenu,
    Nav,
    List,
}

struct FocusOnlyAllowingParent;

impl<A: 'static> Widget<A> for FocusOnlyAllowingParent {
    fn debug_name(&self) -> &'static str {
        "FocusOnlyAllowingParent"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let Some(child) = children.first_mut() else {
            return LayoutResult {
                size: ailloli_ui_core::Size::default(),
                children: Vec::new(),
                paint_bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
                visual_bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
                overlay_hit_bounds: Vec::new(),
                clip: None,
                is_window_root_clip: false,
                artifact: None,
            };
        };
        let child_layout = child.layout(engine, ctx, constraints);
        let size = child_layout.size;
        let bounds = Rect::new(0.0, 0.0, size.w, size.h);
        LayoutResult {
            size,
            children: vec![ChildLayout {
                offset: Offset::default(),
                size,
                paint_bounds: child_layout.paint_bounds,
                visual_bounds: child_layout.visual_bounds,
            }],
            paint_bounds: bounds,
            visual_bounds: child_layout.visual_bounds,
            overlay_hit_bounds: child_layout.overlay_hit_bounds,
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::AllowOnFocusOnly
    }
}

fn under_focus_only_allowing_parent<A: 'static>(child: View<A>) -> View<A> {
    View::node(FocusOnlyAllowingParent, vec![child])
}

#[test]
fn focus_only_button_focuses_without_clicking_and_keyboard_contract_is_unchanged() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Button::with_label("Save")
            .on_click(Action::Button)
            .into_view(),
    );
    layout(&mut app);
    let mut router = InputRouter::default();

    focus_only_click(&mut router, &app, runtime.clone(), 1, Point::new(4.0, 4.0));

    assert!(router.focused().is_some(), "focus-only still grants focus");
    assert!(runtime.take_actions().is_empty());

    router.route_event(&app.tree, runtime.clone(), &key(NamedKey::Enter));
    router.route_event(&app.tree, runtime.clone(), &key(NamedKey::Space));
    assert_eq!(runtime.take_actions(), [Action::Button, Action::Button]);
}

#[test]
fn focus_only_link_does_not_open_but_enter_still_opens_once() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let opener = MemoryExternalUrlOpener::new();
    runtime.set_external_url_opener(Rc::new(opener.clone()));
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Link::with_label("Docs")
            .href("https://example.com/docs")
            .into_view(),
    );
    layout(&mut app);
    let mut router = InputRouter::default();

    focus_only_click(&mut router, &app, runtime.clone(), 2, Point::new(2.0, 2.0));

    assert!(router.focused().is_some(), "focus-only still grants focus");
    assert!(opener.opened_urls().is_empty());

    router.route_event(&app.tree, runtime, &key(NamedKey::Enter));
    assert_eq!(opener.opened_urls(), ["https://example.com/docs"]);
}

#[test]
fn focus_only_is_delivered_to_text_input_for_caret_and_editing() {
    let value = State::new("abc".to_string());
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(TextInput::new().bind(value.clone()).into_view());
    layout(&mut app);
    let mut router = InputRouter::default();

    let outcome = router.route_envelope(
        &app.tree,
        runtime.clone(),
        &pointer_envelope(3, Point::new(5.0, 5.0), true, ActivationKind::FocusOnly),
    );

    assert!(outcome.event_dispatched);
    assert!(router.focused().is_some());
    router.route_event(&app.tree, runtime, &character("x"));
    assert_eq!(value.read().len(), 4);
    assert!(value.read().contains('x'));
}

#[test]
fn focus_only_is_delivered_to_editor_for_selection() {
    let buffer = State::new(TextBuffer::from_string("hello"));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(Editor::new(buffer).into_view());
    layout(&mut app);
    let mut router = InputRouter::default();

    let outcome = router.route_envelope(
        &app.tree,
        runtime,
        &pointer_envelope(4, Point::new(5.0, 5.0), true, ActivationKind::FocusOnly),
    );

    assert!(outcome.event_dispatched);
    assert!(router.focused().is_some());
}

#[test]
fn focus_only_switch_does_not_toggle_but_space_still_does() {
    let checked = State::new(false);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(under_focus_only_allowing_parent(
        Switch::new()
            .bind(checked.clone())
            .on_change(Action::Switch)
            .into_view(),
    ));
    layout(&mut app);
    let mut router = InputRouter::default();

    focus_only_click(&mut router, &app, runtime.clone(), 10, Point::new(4.0, 4.0));

    assert!(!checked.read());
    assert!(runtime.take_actions().is_empty());
    router.route_event(&app.tree, runtime.clone(), &key(NamedKey::Space));
    assert!(checked.read());
    assert_eq!(runtime.take_actions(), [Action::Switch(true)]);
}

#[test]
fn focus_only_radio_button_does_not_select_but_enter_still_does() {
    let checked = State::new(false);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(under_focus_only_allowing_parent(
        RadioButton::new("Choice")
            .bind(checked.clone())
            .on_select(Action::RadioButton)
            .into_view(),
    ));
    layout(&mut app);
    let mut router = InputRouter::default();

    focus_only_click(&mut router, &app, runtime.clone(), 11, Point::new(4.0, 4.0));

    assert!(!checked.read());
    assert!(runtime.take_actions().is_empty());
    router.route_event(&app.tree, runtime.clone(), &key(NamedKey::Enter));
    assert!(checked.read());
    assert_eq!(runtime.take_actions(), [Action::RadioButton]);
}

#[test]
fn focus_only_radio_group_does_not_change_but_arrow_navigation_still_does() {
    let selected = State::new(0_u8);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(under_focus_only_allowing_parent(
        RadioGroup::new()
            .bind(selected.clone())
            .option(0, "First")
            .option(1, "Second")
            .on_change(Action::RadioGroup)
            .into_view(),
    ));
    layout(&mut app);
    let mut router = InputRouter::default();

    focus_only_click(
        &mut router,
        &app,
        runtime.clone(),
        12,
        Point::new(4.0, 40.0),
    );

    assert_eq!(selected.read(), 0);
    assert!(runtime.take_actions().is_empty());
    router.route_event(&app.tree, runtime.clone(), &key(NamedKey::ArrowDown));
    assert_eq!(selected.read(), 1);
    assert_eq!(runtime.take_actions(), [Action::RadioGroup(1)]);
}

#[test]
fn focus_only_combo_box_does_not_open_but_arrow_down_still_opens() {
    let selected = State::new(0_u8);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(under_focus_only_allowing_parent(
        ComboBox::new()
            .bind(selected.clone())
            .option(0, "First")
            .option(1, "Second")
            .on_change(Action::Combo)
            .into_view(),
    ));
    layout(&mut app);
    let mut router = InputRouter::default();

    focus_only_click(&mut router, &app, runtime.clone(), 13, Point::new(4.0, 4.0));

    assert_eq!(selected.read(), 0);
    assert_eq!(open_popup_count(&runtime), 0);
    assert!(runtime.take_actions().is_empty());
    router.route_event(&app.tree, runtime.clone(), &key(NamedKey::ArrowDown));
    assert_eq!(open_popup_count(&runtime), 1);
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn focus_only_autocomplete_does_not_open_but_keyboard_selection_still_works() {
    let value = State::new(String::new());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(under_focus_only_allowing_parent(
        Autocomplete::new()
            .bind(value.clone())
            .suggestion("Apple")
            .suggestion("Banana")
            .on_select(Action::Autocomplete)
            .into_view(),
    ));
    layout(&mut app);
    let mut router = InputRouter::default();

    focus_only_click(&mut router, &app, runtime.clone(), 14, Point::new(4.0, 4.0));

    assert!(value.read().is_empty());
    assert_eq!(open_popup_count(&runtime), 0);
    assert!(runtime.take_actions().is_empty());
    router.route_event(&app.tree, runtime.clone(), &key(NamedKey::ArrowDown));
    router.route_event(&app.tree, runtime.clone(), &key(NamedKey::Enter));
    assert_eq!(value.read(), "Apple");
    assert_eq!(
        runtime.take_actions(),
        [Action::Autocomplete("Apple".to_owned())]
    );
}

#[test]
fn focus_only_context_menu_does_not_open_but_normal_right_click_still_opens() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(under_focus_only_allowing_parent(
        ContextMenu::empty()
            .width(120.0)
            .height(40.0)
            .entries(vec![ContextMenuEntry::Item(
                ContextMenuItem::new("Open").on_select(Action::ContextMenu),
            )])
            .into_view(),
    ));
    layout(&mut app);
    let mut router = InputRouter::default();
    let point = Point::new(4.0, 4.0);

    focus_only_gesture(
        &mut router,
        &app,
        runtime.clone(),
        15,
        point,
        MouseButton::Right,
    );

    assert_eq!(open_popup_count(&runtime), 0);
    assert!(runtime.take_actions().is_empty());
    normal_gesture(
        &mut router,
        &app,
        runtime.clone(),
        16,
        point,
        MouseButton::Right,
    );
    assert_eq!(open_popup_count(&runtime), 1);
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn focus_only_navigation_and_list_items_do_not_select_but_keyboard_still_does() {
    for (view, expected) in [
        (
            NavItem::new("Navigation")
                .on_select(Action::Nav)
                .into_view(),
            Action::Nav,
        ),
        (
            ListItem::new("List").on_select(Action::List).into_view(),
            Action::List,
        ),
    ] {
        let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
        let mut app = Runtime::new(runtime.clone());
        app.reconcile(under_focus_only_allowing_parent(view));
        layout(&mut app);
        let mut router = InputRouter::default();

        focus_only_click(&mut router, &app, runtime.clone(), 17, Point::new(4.0, 4.0));

        assert!(runtime.take_actions().is_empty());
        router.route_event(&app.tree, runtime.clone(), &key(NamedKey::Enter));
        assert_eq!(runtime.take_actions(), [expected]);
    }
}

#[test]
fn cancellation_clears_only_its_pointer_gesture_without_clicking() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Button::with_label("Save")
            .on_click(Action::Button)
            .into_view(),
    );
    layout(&mut app);
    let mut router = InputRouter::default();
    let pos = Point::new(4.0, 4.0);

    router.route_envelope(
        &app.tree,
        runtime.clone(),
        &pointer_envelope(5, pos, true, ActivationKind::Normal),
    );
    router.route_envelope(
        &app.tree,
        runtime.clone(),
        &pointer_envelope(6, pos, true, ActivationKind::Normal),
    );
    assert!(router.snapshot_for(PointerId::new(5)).pressed.is_some());
    assert!(router.snapshot_for(PointerId::new(6)).pressed.is_some());

    router.route_envelope(&app.tree, runtime.clone(), &cancel_envelope(5, pos));

    assert_eq!(router.snapshot_for(PointerId::new(5)).pressed, None);
    assert!(
        router.snapshot_for(PointerId::new(6)).pressed.is_some(),
        "cancelling one pointer must not clear another pointer's gesture"
    );
    assert!(runtime.take_actions().is_empty());
}

fn focus_only_click<A: 'static>(
    router: &mut InputRouter,
    app: &Runtime<A>,
    runtime: RuntimeHandle<A>,
    pointer_id: u64,
    pos: Point,
) {
    focus_only_gesture(router, app, runtime, pointer_id, pos, MouseButton::Left);
}

fn focus_only_gesture<A: 'static>(
    router: &mut InputRouter,
    app: &Runtime<A>,
    runtime: RuntimeHandle<A>,
    pointer_id: u64,
    pos: Point,
    button: MouseButton,
) {
    router.route_envelope(
        &app.tree,
        runtime.clone(),
        &pointer_envelope_with_button(pointer_id, pos, button, true, ActivationKind::FocusOnly),
    );
    router.route_envelope(
        &app.tree,
        runtime,
        &pointer_envelope_with_button(pointer_id, pos, button, false, ActivationKind::Normal),
    );
}

fn normal_gesture<A: 'static>(
    router: &mut InputRouter,
    app: &Runtime<A>,
    runtime: RuntimeHandle<A>,
    pointer_id: u64,
    pos: Point,
    button: MouseButton,
) {
    for pressed in [true, false] {
        router.route_envelope(
            &app.tree,
            runtime.clone(),
            &pointer_envelope_with_button(pointer_id, pos, button, pressed, ActivationKind::Normal),
        );
    }
}

fn pointer_envelope(
    id: u64,
    pos: Point,
    pressed: bool,
    activation: ActivationKind,
) -> EventEnvelope {
    pointer_envelope_with_button(id, pos, MouseButton::Left, pressed, activation)
}

fn pointer_envelope_with_button(
    id: u64,
    pos: Point,
    button: MouseButton,
    pressed: bool,
    activation: ActivationKind,
) -> EventEnvelope {
    let pointer = PointerSample::new(PointerId::new(id), PointerSource::Mouse, pos)
        .unwrap()
        .with_activation(activation);
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(id),
            EventTimestamp::new(Duration::from_millis(id)),
            "main",
            PresentationGeneration::new(1),
        )
        .with_pointer(pointer),
        Event::Pointer(PointerEvent::Button {
            pos,
            button,
            pressed,
            modifiers: Modifiers::default(),
        }),
    )
}

fn open_popup_count<A: 'static>(runtime: &RuntimeHandle<A>) -> usize {
    let portal = runtime.popup_portal();
    let portal = portal.borrow();
    portal.open_ids().count()
}

fn cancel_envelope(id: u64, pos: Point) -> EventEnvelope {
    let pointer = PointerSample::new(PointerId::new(id), PointerSource::Mouse, pos).unwrap();
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(id + 100),
            EventTimestamp::new(Duration::from_millis(id + 100)),
            "main",
            PresentationGeneration::new(1),
        )
        .with_pointer(pointer),
        Event::Pointer(PointerEvent::Cancelled {
            pos,
            modifiers: Modifiers::default(),
        }),
    )
}

fn key(key: NamedKey) -> Event {
    Event::Keyboard(KeyEvent {
        state: KeyState::Pressed,
        key: Key::Named(key),
        modifiers: Modifiers::default(),
        repeat: false,
        pointer_pos: None,
        text: None,
    })
}

fn character(text: &str) -> Event {
    Event::Keyboard(KeyEvent {
        state: KeyState::Pressed,
        key: Key::Character(text.into()),
        modifiers: Modifiers::default(),
        repeat: false,
        pointer_pos: None,
        text: Some(text.into()),
    })
}

fn layout<A: 'static>(app: &mut Runtime<A>) {
    let mut text = TextSystem::new();
    app.layout(Constraints::loose(320.0, 160.0), Scale::new(1.0), &mut text);
}
