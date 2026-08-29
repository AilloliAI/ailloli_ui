//! Text-input state synchronization, selection, IME, scrolling, and role scenarios.

use ailloli_ui_core::event::{
    Event, ImeEvent, ImePreedit, Key, KeyEvent, KeyState, Modifiers, MouseButton, NamedKey,
    PointerEvent, WheelDelta,
};
use ailloli_ui_core::geometry::{Constraints, Rect};
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{ClipShape, Color, Point};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State, View, ViewKind};
use ailloli_ui_runtime::element::ElementKind;
use ailloli_ui_runtime::input::InputRouter;
use ailloli_ui_runtime::{DrawCmd, DrawRect, DrawText};
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{TextInput, TextInputStyle};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    Changed(String),
}

#[test]
fn text_input_accepts_public_state_binding() {
    let value = State::new("hello".to_string());
    let view: View<()> = TextInput::new().bind(value).into_view();

    assert!(matches!(view.kind, ViewKind::Component(_)));
}

#[test]
fn text_input_click_then_type_updates_bound_state() {
    let value = State::new(String::new());
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(TextInput::new().bind(value.clone()).into_view());

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(240.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(5.0, 5.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime,
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Character("a".into()),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: Some(Point::new(5.0, 5.0)),
            text: Some("a".into()),
        }),
    );

    assert_eq!(value.read(), "a");
}

#[test]
fn text_input_on_change_dispatches_after_text_change() {
    let value = State::new(String::new());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TextInput::<Action>::new()
            .bind(value.clone())
            .on_change(Action::Changed)
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(240.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(5.0, 5.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Character("a".into()),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: Some(Point::new(5.0, 5.0)),
            text: Some("a".into()),
        }),
    );

    assert_eq!(value.read(), "a");
    assert_eq!(runtime.take_actions(), vec![Action::Changed("a".into())]);
}

#[test]
fn text_input_on_change_ignores_focus_wheel_and_preedit_without_commit() {
    let value = State::new("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TextInput::<Action>::new()
            .bind(value)
            .on_change(Action::Changed)
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(80.0, 40.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(10.0, 10.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Wheel {
            pos: Point::new(40.0, 10.0),
            delta: WheelDelta::PixelDelta { x: -40.0, y: 0.0 },
            modifiers: Modifiers::default(),
            precise: true,
        }),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Ime(ImeEvent::Preedit {
            preedit: ImePreedit::new("é"),
            pos: Some(Point::new(10.0, 10.0)),
        }),
    );

    assert!(runtime.take_actions().is_empty());
}

#[test]
fn text_input_disabled_ends_preedit_in_both_input_modes() {
    for multiline in [false, true] {
        let value = State::new("stable".to_string());
        let runtime: RuntimeHandle<()> = RuntimeHandle::new();
        let mut app = Runtime::new(runtime.clone());
        let input = TextInput::new().bind(value.clone());
        let input = if multiline { input.multiline() } else { input };
        app.reconcile(input.into_view());

        let mut text_system = TextSystem::new();
        app.layout(
            Constraints::tight(240.0, 80.0),
            Scale::new(1.0),
            &mut text_system,
        );

        let mut router = InputRouter::default();
        focus_input(&mut router, &app, runtime.clone());
        router.route_event(
            &app.tree,
            runtime.clone(),
            &Event::Ime(ImeEvent::Preedit {
                preedit: ImePreedit::new("PREEDIT-MARKER"),
                pos: None,
            }),
        );
        app.layout(
            Constraints::tight(240.0, 80.0),
            Scale::new(1.0),
            &mut text_system,
        );
        let during_preedit = app.paint_with_input(&mut text_system, router.snapshot(), 0);
        assert!(
            scene_contains_text_fragment(&during_preedit, "PREEDIT-MARKER"),
            "preedit must be visible before Disabled (multiline={multiline})"
        );

        router.route_event(&app.tree, runtime, &Event::Ime(ImeEvent::Disabled));
        app.layout(
            Constraints::tight(240.0, 80.0),
            Scale::new(1.0),
            &mut text_system,
        );
        let after_disabled = app.paint_with_input(&mut text_system, router.snapshot(), 0);
        assert!(
            !scene_contains_text_fragment(&after_disabled, "PREEDIT-MARKER"),
            "Disabled must clear preedit (multiline={multiline})"
        );
        assert_eq!(value.read(), "stable");
    }
}

#[test]
fn text_input_horizontal_wheel_scrolls_clipped_text() {
    let value = State::new("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(TextInput::new().bind(value).into_view());

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(80.0, 40.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Wheel {
            pos: Point::new(40.0, 10.0),
            delta: WheelDelta::PixelDelta { x: -40.0, y: 0.0 },
            modifiers: Modifiers::default(),
            precise: true,
        }),
    );

    let scene = app.paint(&mut text_system);
    let (text_x, text_clip) = text_draw_x_and_clip(&scene).expect("text draw");

    assert!(text_x < 0.0, "text_x={text_x}");
    assert!(matches!(text_clip, Some(ClipShape::Rect(Rect { .. }))));
}

#[test]
fn text_input_shift_wheel_scrolls_single_line_horizontally() {
    let value = State::new("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(TextInput::new().bind(value).into_view());
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(80.0, 40.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Wheel {
            pos: Point::new(40.0, 10.0),
            delta: WheelDelta::PixelDelta { x: 0.0, y: -40.0 },
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::default()
            },
            precise: true,
        }),
    );

    let scene = app.paint(&mut text_system);
    let (text_x, _) = text_draw_x_and_clip(&scene).expect("text draw");
    assert!(text_x < 0.0, "text_x={text_x}");
}

#[test]
fn text_input_end_key_reveals_caret_with_horizontal_scroll() {
    let value = State::new("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(TextInput::new().bind(value).into_view());

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(80.0, 40.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(10.0, 10.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime,
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Named(NamedKey::End),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: Some(Point::new(220.0, 10.0)),
            text: None,
        }),
    );

    let scene = app.paint(&mut text_system);
    let (text_x, _) = text_draw_x_and_clip(&scene).expect("text draw");

    assert!(text_x < -100.0, "text_x={text_x}");
}

#[test]
fn text_input_single_line_enter_does_not_insert_newline() {
    let value = State::new("hello".to_string());
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(TextInput::new().bind(value.clone()).into_view());

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(240.0, 40.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    focus_input(&mut router, &app, runtime.clone());
    send_enter(&mut router, &app, runtime);

    assert_eq!(value.read(), "hello");
}

#[test]
fn text_input_multiline_enter_inserts_newline_and_reports_role() {
    let value = State::new("hello".to_string());
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TextInput::new()
            .bind(value.clone())
            .multiline()
            .height(96.0)
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(240.0, 96.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let role = app.tree.iter_elements().find_map(|(_, element)| {
        let ElementKind::Widget(widget) = &element.kind else {
            return None;
        };
        (widget.debug_name() == "TextInput").then(|| widget.input_role())
    });
    assert_eq!(
        role,
        Some(ailloli_ui_runtime::input::InputRole::TextMultiLine)
    );

    let mut router = InputRouter::default();
    focus_input(&mut router, &app, runtime.clone());
    send_enter(&mut router, &app, runtime);

    assert_eq!(value.read().matches('\n').count(), 1);
}

#[test]
fn text_input_multiline_wheel_scrolls_clipped_text_vertically() {
    let value = State::new(
        (0..16)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TextInput::new()
            .bind(value)
            .multiline()
            .height(70.0)
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(220.0, 70.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Wheel {
            pos: Point::new(10.0, 10.0),
            delta: WheelDelta::PixelDelta { x: 0.0, y: -48.0 },
            modifiers: Modifiers::default(),
            precise: true,
        }),
    );

    let scene = app.paint(&mut text_system);
    let (_, text_y, text_clip) = text_draw_pos_and_clip(&scene).expect("text draw");

    assert!(text_y < 20.0, "text_y={text_y}");
    assert!(matches!(text_clip, Some(ClipShape::Rect(Rect { .. }))));
}

#[test]
fn text_input_multiline_selection_ctrl_a_paints_full_line_rects() {
    let selection_color = Color::f32(0.18, 0.62, 0.31, 0.72);
    let style = TextInputStyle {
        selection_bg: selection_color,
        caret_blink_ms: 0,
        ..TextInputStyle::default()
    };
    let value = State::new(
        [
            "Line 01  Long text wraps inside the bounded multiline input surface.",
            "Line 02  This line should receive a full-width selection rectangle.",
            "Line 03  Hard breaks and soft wrapping are selected together.",
            "Line 04  Final selected line remains part of Ctrl+A.",
        ]
        .join("\n"),
    );
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TextInput::new()
            .bind(value)
            .multiline()
            .input_style(style)
            .width(260.0)
            .height(130.0)
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 130.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    focus_input(&mut router, &app, runtime.clone());
    send_ctrl_a(&mut router, &app, runtime);

    let scene = app.paint(&mut text_system);
    let text = text_draw(&scene).expect("text draw");
    let rects = rects_with_color(&scene, selection_color);
    let expected_lines = text
        .layout
        .lines
        .iter()
        .filter(|line| line.width > 1.0)
        .count();

    assert_eq!(
        rects.len(),
        expected_lines,
        "selection rect count should match laid out lines"
    );
    for (idx, (rect, line)) in rects.iter().zip(text.layout.lines.iter()).enumerate() {
        if line.width <= 1.0 {
            continue;
        }
        assert!(
            rect.w >= line.width - 1.0,
            "line {idx}: selection width {} < layout width {}",
            rect.w,
            line.width
        );
    }
}

#[test]
fn text_input_multiline_selection_drag_paints_full_line_rects() {
    let selection_color = Color::f32(0.46, 0.19, 0.71, 0.76);
    let style = TextInputStyle {
        selection_bg: selection_color,
        caret_blink_ms: 0,
        ..TextInputStyle::default()
    };
    let value = State::new(
        [
            "alpha bravo charlie",
            "delta echo foxtrot",
            "golf hotel india",
            "juliet kilo lima",
        ]
        .join("\n"),
    );
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TextInput::new()
            .bind(value)
            .multiline()
            .input_style(style)
            .width(260.0)
            .height(110.0)
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 110.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(230.0, 72.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Moved {
            pos: Point::new(12.0, 12.0),
            modifiers: Modifiers::default(),
        }),
    );

    let scene = app.paint(&mut text_system);
    let text = text_draw(&scene).expect("text draw");
    let rects = rects_with_color(&scene, selection_color);

    assert!(
        rects.len() >= 3,
        "drag selection should cover multiple line rects: {rects:?}"
    );
    for (idx, rect) in rects
        .iter()
        .enumerate()
        .skip(1)
        .take(rects.len().saturating_sub(2))
    {
        let line = &text.layout.lines[idx];
        assert!(
            rect.w >= line.width - 1.0,
            "middle line {idx}: selection width {} < layout width {}",
            rect.w,
            line.width
        );
    }
}

#[test]
fn text_input_multiline_reveal_insert_after_overflow_keeps_caret_visible() {
    let caret_color = Color::f32(0.91, 0.17, 0.74, 1.0);
    let style = TextInputStyle {
        caret: caret_color,
        caret_blink_ms: 500,
        ..TextInputStyle::default()
    };
    let value = State::new(
        (0..18)
            .map(|idx| format!("line {idx:02} keeps enough words for wrapped multiline input"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TextInput::new()
            .bind(value.clone())
            .multiline()
            .input_style(style)
            .width(220.0)
            .height(70.0)
            .into_view(),
    );

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(220.0, 70.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    focus_input(&mut router, &app, runtime.clone());
    send_ctrl_end(&mut router, &app, runtime.clone());
    send_text_key(&mut router, &app, runtime, "x");
    app.layout(
        Constraints::tight(220.0, 70.0),
        Scale::new(1.0),
        &mut text_system,
    );

    assert!(value.read().ends_with('x'));

    let scene = app.paint_with_input(&mut text_system, router.snapshot(), 0);
    let (text_x, _, _) = text_draw_pos_and_clip(&scene).expect("text draw");
    let caret = first_rect_with_color(&scene, caret_color).expect("caret rect");
    let content = text_input_content_rect_for_test(Rect::new(0.0, 0.0, 220.0, 70.0), style);

    assert!(
        text_x >= content.x - 1.0,
        "text should not be shifted horizontally out of view: text_x={text_x}, content={content:?}"
    );
    assert!(
        caret.rect.y >= content.y - 1.0
            && caret.rect.y + caret.rect.h <= content.y + content.h + 1.0,
        "caret should remain vertically visible: caret={:?}, content={content:?}",
        caret.rect
    );
}

#[test]
fn text_input_multiline_scrollbar_drag_has_no_change_callback() {
    let original = (0..18)
        .map(|idx| format!("line {idx:02} with enough text to remain visible"))
        .collect::<Vec<_>>()
        .join("\n");
    let value = State::new(original.clone());
    let style = TextInputStyle::default();
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TextInput::new()
            .bind(value.clone())
            .multiline()
            .input_style(style)
            .on_change(Action::Changed)
            .width(220.0)
            .height(70.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(220.0, 70.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let input_layout = app
        .tree
        .iter_elements()
        .find_map(|(_, element)| match &element.kind {
            ElementKind::Widget(widget) if widget.debug_name() == "TextInput" => {
                element.layout.as_ref()
            }
            _ => None,
        })
        .expect("text input layout");
    assert_eq!(input_layout.overlay_hit_bounds.len(), 1);

    let initial = app.paint(&mut text_system);
    let initial_y = text_draw_pos_and_clip(&initial).expect("initial text").1;
    let thumb = first_rect_with_color(&initial, style.border.with_alpha(0.62))
        .expect("multiline scrollbar thumb");
    let press = Point::new(
        thumb.rect.x + thumb.rect.w * 0.5,
        thumb.rect.y + thumb.rect.h * 0.5,
    );
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: press,
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Moved {
            pos: Point::new(press.x, 1_000.0),
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(press.x, 1_000.0),
            button: MouseButton::Left,
            pressed: false,
            modifiers: Modifiers::default(),
        }),
    );

    let after = app.paint(&mut text_system);
    let after_y = text_draw_pos_and_clip(&after).expect("scrolled text").1;
    assert!(after_y < initial_y, "initial={initial_y}, after={after_y}");
    assert_eq!(value.read(), original);
    assert!(runtime.take_actions().is_empty());
}

fn focus_input(router: &mut InputRouter, app: &Runtime<()>, runtime: RuntimeHandle<()>) {
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(10.0, 10.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
}

fn send_ctrl_a(router: &mut InputRouter, app: &Runtime<()>, runtime: RuntimeHandle<()>) {
    router.route_event(
        &app.tree,
        runtime,
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Character("a".into()),
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            repeat: false,
            pointer_pos: Some(Point::new(10.0, 10.0)),
            text: None,
        }),
    );
}

fn send_ctrl_end(router: &mut InputRouter, app: &Runtime<()>, runtime: RuntimeHandle<()>) {
    router.route_event(
        &app.tree,
        runtime,
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Named(NamedKey::End),
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            repeat: false,
            pointer_pos: Some(Point::new(10.0, 10.0)),
            text: None,
        }),
    );
}

fn send_text_key(
    router: &mut InputRouter,
    app: &Runtime<()>,
    runtime: RuntimeHandle<()>,
    text: &str,
) {
    router.route_event(
        &app.tree,
        runtime,
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Character(text.into()),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: Some(Point::new(10.0, 10.0)),
            text: Some(text.into()),
        }),
    );
}

fn send_enter(router: &mut InputRouter, app: &Runtime<()>, runtime: RuntimeHandle<()>) {
    router.route_event(
        &app.tree,
        runtime,
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Named(NamedKey::Enter),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: Some(Point::new(10.0, 10.0)),
            text: Some("\n".into()),
        }),
    );
}

fn text_draw(scene: &ailloli_ui_runtime::Scene) -> Option<DrawText> {
    scene.layers.iter().find_map(|layer| {
        layer.cmds.iter().find_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some(text.clone()),
            _ => None,
        })
    })
}

fn scene_contains_text_fragment(scene: &ailloli_ui_runtime::Scene, needle: &str) -> bool {
    scene.layers.iter().any(|layer| {
        layer
            .cmds
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text().contains(needle)))
    })
}

fn rects_with_color(scene: &ailloli_ui_runtime::Scene, color: Color) -> Vec<Rect> {
    scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .filter_map(|cmd| match cmd {
            DrawCmd::Rect(rect) if rect.color == color => Some(rect.rect),
            _ => None,
        })
        .collect()
}

fn first_rect_with_color(scene: &ailloli_ui_runtime::Scene, color: Color) -> Option<DrawRect> {
    scene.layers.iter().find_map(|layer| {
        layer.cmds.iter().find_map(|cmd| match cmd {
            DrawCmd::Rect(rect) if rect.color == color => Some(*rect),
            _ => None,
        })
    })
}

fn text_input_content_rect_for_test(bounds: Rect, style: TextInputStyle) -> Rect {
    Rect::new(
        bounds.x + style.pad_x,
        bounds.y + style.pad_y,
        (bounds.w - style.pad_x * 2.0).max(0.0),
        (bounds.h - style.pad_y * 2.0).max(0.0),
    )
}

fn text_draw_x_and_clip(scene: &ailloli_ui_runtime::Scene) -> Option<(f32, Option<ClipShape>)> {
    scene.layers.iter().find_map(|layer| {
        layer.cmds.iter().find_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some((
                text.pos[0],
                layer.clip.entries().last().map(|entry| entry.shape),
            )),
            _ => None,
        })
    })
}

fn text_draw_pos_and_clip(
    scene: &ailloli_ui_runtime::Scene,
) -> Option<(f32, f32, Option<ClipShape>)> {
    scene.layers.iter().find_map(|layer| {
        layer.cmds.iter().find_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some((
                text.pos[0],
                text.pos[1],
                layer.clip.entries().last().map(|entry| entry.shape),
            )),
            _ => None,
        })
    })
}
