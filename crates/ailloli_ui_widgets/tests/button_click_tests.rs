//! Button static/deferred action and keyboard/pointer activation scenarios.

use std::cell::Cell;
use std::rc::Rc;

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::Point;
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::IntoView;
use ailloli_ui_runtime::input::{dispatch_event_to_target, DeferredAction, InputRouter};
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::Button;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Static,
    Deferred,
}

#[test]
fn button_on_click_accepts_static_action() {
    let runtime = click_button(Button::<Action>::with_label("Run").on_click(Action::Static));

    assert_eq!(runtime.take_actions(), vec![Action::Static]);
}

#[test]
fn button_on_click_accepts_deferred_action() {
    let runtime = click_button(
        Button::<Action>::with_label("Run").on_click(DeferredAction::new(|| Action::Deferred)),
    );

    assert_eq!(runtime.take_actions(), vec![Action::Deferred]);
}

#[test]
fn button_on_click_ctx_keeps_low_level_handler_compat() {
    let clicked = Rc::new(Cell::new(false));
    let seen = clicked.clone();

    let _runtime = click_button(Button::<()>::with_label("Run").on_click_ctx(move |_| {
        seen.set(true);
    }));

    assert!(clicked.get());
}

#[test]
fn focused_button_activates_with_enter() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Button::<Action>::with_label("Run")
            .on_click(Action::Static)
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(200.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    router.route_event(&app.tree, runtime.clone(), &pointer_button(true));
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Named(NamedKey::Enter),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: Some(Point::new(5.0, 5.0)),
            text: None,
        }),
    );

    assert_eq!(runtime.take_actions(), vec![Action::Static]);
}

fn click_button<A: Clone + 'static>(button: Button<A>) -> RuntimeHandle<A> {
    let runtime: RuntimeHandle<A> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(button.into_view());

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(200.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let press = pointer_button(true);
    dispatch_event_to_target(&app.tree, runtime.clone(), root, &press);

    let release = pointer_button(false);
    dispatch_event_to_target(&app.tree, runtime.clone(), root, &release);

    runtime
}

fn pointer_button(pressed: bool) -> Event {
    Event::Pointer(PointerEvent::Button {
        pos: Point::new(5.0, 5.0),
        button: MouseButton::Left,
        pressed,
        modifiers: Modifiers::default(),
    })
}
