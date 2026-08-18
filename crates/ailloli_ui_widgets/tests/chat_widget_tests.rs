use std::cell::RefCell;
use std::rc::Rc;

use ailloli_ui_core::event::{Key, KeyEvent, KeyState, Modifiers, WheelDelta};
use ailloli_ui_core::geometry::{Constraints, Rect};
use ailloli_ui_core::ids::ElementId;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{
    ChatItemId, ChatMessage, ChatMessageKind, ChatMessageStatus, ChatRequestId, ChatRole,
    ChatSessionId, ChatSessionState, ChatSessionStatus, Point,
};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{ComponentNode, Context, IntoView, State, View};
use ailloli_ui_runtime::element::ElementKind;
use ailloli_ui_runtime::input::{absolute_paint_bounds, dispatch_event_to_target, InputRouter};
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{ChatComposerControls, ChatWidget, ChatWidgetAction};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    Chat(ChatWidgetAction),
}

#[test]
fn chat_widget_paints_session_messages_in_order() {
    let mut session = ChatSessionState::new(ChatSessionId::from_index(1), "Chat 1");
    session.status = ChatSessionStatus::Running;
    session.messages.push(ChatMessage::user(
        ChatItemId::from_index(1),
        "hello workspace",
    ));
    session.messages.push(ChatMessage::assistant(
        ChatItemId::from_index(2),
        "streamed answer",
    ));

    let draft = State::new(String::new());
    let app = layout_view(ChatWidget::<()>::new(Some(session), draft).into_view());
    let texts = painted_texts(&app);

    assert_text_order(&texts, "hello workspace", "streamed answer");
    assert!(texts.iter().any(|text| text.contains("Chat 1 - running")));
}

#[test]
fn chat_widget_send_dispatches_current_draft_without_clearing_it() {
    let session = ChatSessionState::new(ChatSessionId::from_index(1), "Chat 1");
    let draft = State::new("ship phase 86".to_string());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        ChatWidget::new(Some(session), draft.clone())
            .on_action(Action::Chat)
            .into_view(),
    );
    layout_app(&mut app);

    let send = first_widget_bounds(&app, "Button").expect("send button bounds");
    let mut router = InputRouter::default();
    click(
        &mut router,
        &app,
        runtime.clone(),
        send.x + send.w * 0.5,
        send.y + send.h * 0.5,
    );

    assert_eq!(
        runtime.take_actions(),
        vec![Action::Chat(ChatWidgetAction::Send {
            text: "ship phase 86".into()
        })]
    );
    assert_eq!(draft.read(), "ship phase 86");
}

#[test]
fn chat_widget_renders_no_session_fallback() {
    let draft = State::new(String::new());
    let app = layout_view(ChatWidget::<()>::new(None, draft).into_view());
    let texts = painted_texts(&app);

    assert!(texts.iter().any(|text| text.contains("No session")));
    assert!(texts.iter().any(|text| text.contains("No chat session")));
}

#[test]
fn chat_widget_wraps_transcript_in_scroll_view() {
    let draft = State::new(String::new());
    let app = layout_view(ChatWidget::<()>::new(Some(long_session(4)), draft).into_view());

    assert_eq!(widget_count(&app, "ScrollView"), 1);
    assert_eq!(widget_count(&app, "ChatMessages"), 1);
}

#[test]
fn chat_widget_transcript_scroll_view_has_bounded_clip() {
    let draft = State::new(String::new());
    let app = layout_view(ChatWidget::<()>::new(Some(long_session(18)), draft).into_view());
    let scroll_id = first_widget_id(&app, "ScrollView").expect("scroll view");
    let scroll_layout = app.tree.get(scroll_id).unwrap().layout.as_ref().unwrap();

    assert_eq!(
        scroll_layout.clip,
        Some(ailloli_ui_core::ClipShape::Rect(Rect::new(
            0.0,
            0.0,
            scroll_layout.size.w,
            scroll_layout.size.h
        )))
    );
    assert_eq!(scroll_layout.children.len(), 1);
    assert!(scroll_layout.children[0].size.h > scroll_layout.size.h);
    assert!(scroll_layout.size.h < 220.0);
}

#[test]
fn chat_widget_transcript_multiline_message_uses_text_layout_height() {
    let listing = listing_message_text();
    let mut session = ChatSessionState::new(ChatSessionId::from_index(1), "Listing");
    session.messages.push(
        ChatMessage::assistant(ChatItemId::from_index(1), listing.clone())
            .request_id(ChatRequestId::from_index(1)),
    );
    let draft = State::new(String::new());
    let app = layout_view(ChatWidget::<()>::new(Some(session), draft).into_view());

    let body = painted_text_box(&app, &listing).expect("listing body paint");
    let copy = painted_text_box(&app, "Copy").expect("copy action paint");
    let messages = first_widget_bounds(&app, "ChatMessages").expect("messages bounds");

    assert!(
        body.lines >= listing.lines().count(),
        "body lines should include hard breaks: body={body:?}"
    );
    assert!(
        copy.top > body.bottom,
        "copy should be below body, body={body:?} copy={copy:?}"
    );
    assert!(
        messages.h >= body.bottom - messages.y + 24.0,
        "transcript height should include listing body, messages={messages:?} body={body:?}"
    );
}

#[test]
fn chat_widget_copy_retry_hit_tests_multiline_listing_message() {
    let listing = listing_message_text();
    let mut session = ChatSessionState::new(ChatSessionId::from_index(1), "Listing");
    session.messages.push(
        ChatMessage::assistant(ChatItemId::from_index(1), listing.clone())
            .request_id(ChatRequestId::from_index(1)),
    );
    let draft = State::new(String::new());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        ChatWidget::new(Some(session), draft)
            .on_action(Action::Chat)
            .into_view(),
    );
    layout_app(&mut app);

    let scroll = first_widget_bounds(&app, "ScrollView").expect("scroll bounds");
    let body = painted_text_box(&app, &listing).expect("listing body paint");
    let copy = visible_text_position(&app, "Copy", scroll).expect("visible copy");
    let retry = visible_text_position(&app, "Retry", scroll).expect("visible retry");
    assert!(
        copy.1 > body.bottom && retry.1 > body.bottom,
        "actions should be below body, body={body:?} copy={copy:?} retry={retry:?}"
    );

    let messages = first_widget_id(&app, "ChatMessages").expect("messages widget");
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        messages,
        &pointer_button(copy.0 + 2.0, copy.1 - 4.0, true),
    );
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        messages,
        &pointer_button(retry.0 + 2.0, retry.1 - 4.0, true),
    );

    let actions = runtime.take_actions();
    assert!(
        matches!(
            actions.first(),
            Some(Action::Chat(ChatWidgetAction::Copy { text, .. })) if text == &listing
        ),
        "{actions:?}"
    );
    assert!(
        matches!(
            actions.get(1),
            Some(Action::Chat(ChatWidgetAction::Retry { .. }))
        ),
        "{actions:?}"
    );
}

#[test]
fn chat_widget_follow_latest_keeps_new_messages_visible() {
    let session = State::new(Some(long_session(8)));
    let draft = State::new(String::new());
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(ChatWidget::<()>::bind_session(session.clone(), draft).into_view());
    layout_app(&mut app);
    let initial_offset = scroll_child_offset(&app);
    assert!(initial_offset < -1.0);

    session.set(Some(long_session(18)));
    layout_app(&mut app);
    let updated_offset = scroll_child_offset(&app);

    assert!(updated_offset < initial_offset);
}

#[test]
fn chat_widget_manual_scroll_prevents_auto_follow() {
    let session = State::new(Some(long_session(12)));
    let draft = State::new(String::new());
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(ChatWidget::<()>::bind_session(session.clone(), draft).into_view());
    layout_app(&mut app);
    let initial_offset = scroll_child_offset(&app);
    assert!(initial_offset < -1.0);

    let scroll = first_widget_bounds(&app, "ScrollView").expect("scroll bounds");
    let mut router = InputRouter::default();
    wheel(
        &mut router,
        &app,
        runtime,
        scroll.x + scroll.w * 0.5,
        scroll.y + scroll.h * 0.5,
        90.0,
    );
    layout_app(&mut app);
    let manual_offset = scroll_child_offset(&app);
    assert!(manual_offset > initial_offset);

    session.set(Some(long_session(20)));
    layout_app(&mut app);

    assert_eq!(scroll_child_offset(&app), manual_offset);
}

#[test]
fn chat_widget_copy_retry_hit_tests_after_scroll() {
    let session = State::new(Some(long_session(10)));
    let draft = State::new(String::new());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        ChatWidget::bind_session(session, draft)
            .on_action(Action::Chat)
            .into_view(),
    );
    layout_app(&mut app);

    let scroll = first_widget_bounds(&app, "ScrollView").expect("scroll bounds");
    let copy = visible_text_position(&app, "Copy", scroll).expect("visible copy");
    let retry = visible_text_position(&app, "Retry", scroll).expect("visible retry");
    let messages = first_widget_id(&app, "ChatMessages").expect("messages widget");
    let message_bounds = first_widget_bounds(&app, "ChatMessages").expect("messages bounds");
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        messages,
        &pointer_button(copy.0 + 2.0, copy.1 - 4.0, true),
    );
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        messages,
        &pointer_button(retry.0 + 2.0, retry.1 - 4.0, true),
    );

    let actions = runtime.take_actions();
    assert!(
        matches!(
            actions.first(),
            Some(Action::Chat(ChatWidgetAction::Copy { .. }))
        ),
        "{actions:?} scroll={scroll:?} messages={message_bounds:?} copy={copy:?} retry={retry:?}"
    );
    assert!(matches!(
        actions.get(1),
        Some(Action::Chat(ChatWidgetAction::Retry { .. }))
    ));
}

#[test]
fn chat_composer_uses_multiline_input_controls_and_resize_bar() {
    let session = ChatSessionState::new(ChatSessionId::from_index(1), "Composer");
    let draft = State::new("line one\nline two".to_string());
    let app = layout_view(
        ChatWidget::<()>::new(Some(session), draft)
            .composer_controls(ChatComposerControls {
                height: 136.0,
                ..ChatComposerControls::default()
            })
            .into_view(),
    );

    let input = first_widget_bounds(&app, "TextInput").expect("composer input");
    assert!(
        input.h > 34.0,
        "input should be multiline height: {input:?}"
    );
    let resize = first_widget_bounds(&app, "ResizeBar").expect("resize bar bounds");
    let scroll = first_widget_bounds(&app, "ScrollView").expect("transcript scroll bounds");
    assert!(
        resize.y >= scroll.bottom(),
        "resize bar should sit below transcript: resize={resize:?} scroll={scroll:?}"
    );
    assert!(
        resize.bottom() <= input.y,
        "resize bar should sit above composer input: resize={resize:?} input={input:?}"
    );
    assert!(
        resize.w >= input.w,
        "resize bar should span the composer width: resize={resize:?} input={input:?}"
    );
    assert_eq!(widget_count(&app, "Select"), 3);
    assert_eq!(widget_count(&app, "ResizeBar"), 1);
    assert_eq!(widget_count(&app, "Button"), 1);
}

#[test]
fn chat_composer_resize_bar_between_transcript_and_composer_controls_height() {
    let session = ChatSessionState::new(ChatSessionId::from_index(1), "Composer");
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        ChatWidget::new(Some(session), State::new("resize seam".to_string()))
            .composer_controls(ChatComposerControls {
                height: 112.0,
                ..ChatComposerControls::default()
            })
            .on_action(Action::Chat)
            .into_view(),
    );
    layout_app(&mut app);

    let resize = first_widget_bounds(&app, "ResizeBar").expect("resize bar bounds");
    let input = first_widget_bounds(&app, "TextInput").expect("composer input");
    assert!(
        resize.bottom() <= input.y,
        "resize bar should not be inside the composer bottom: resize={resize:?} input={input:?}"
    );

    let mut router = InputRouter::default();
    drag_resize(&mut router, &app, runtime.clone(), resize, -18.0);
    let actions = runtime.take_actions();
    assert!(
        actions.iter().any(|action| {
            matches!(
                action,
                Action::Chat(ChatWidgetAction::SetComposerHeight { height }) if *height > 112.0
            )
        }),
        "dragging the seam upward should increase composer height: {actions:?}"
    );

    drag_resize(&mut router, &app, runtime.clone(), resize, 18.0);
    let actions = runtime.take_actions();
    assert!(
        actions.iter().any(|action| {
            matches!(
                action,
                Action::Chat(ChatWidgetAction::SetComposerHeight { height }) if *height < 112.0
            )
        }),
        "dragging the seam downward should decrease composer height: {actions:?}"
    );
}

#[test]
fn chat_composer_resize_tracks_multiframe_drag() {
    #[derive(Clone)]
    struct ControlledComposer {
        actions: Rc<RefCell<Vec<ChatWidgetAction>>>,
    }

    impl ComponentNode<()> for ControlledComposer {
        fn build(&self, context: &mut Context<()>) -> View<()> {
            let height = context.signal(112.0);
            let height_for_action = height.clone();
            let actions = self.actions.clone();
            ChatWidget::new(
                Some(ChatSessionState::new(
                    ChatSessionId::from_index(1),
                    "Tracked composer",
                )),
                State::new("tracked resize".to_string()),
            )
            .composer_controls(ChatComposerControls {
                height: height.read(),
                ..ChatComposerControls::default()
            })
            .on_action_ctx(move |_ctx, action| {
                if let ChatWidgetAction::SetComposerHeight { height } = &action {
                    height_for_action.set(*height);
                }
                actions.borrow_mut().push(action);
            })
            .into_view()
        }
    }

    let actions = Rc::new(RefCell::new(Vec::<ChatWidgetAction>::new()));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(View::component(ControlledComposer {
        actions: actions.clone(),
    }));
    layout_app(&mut app);

    let resize = first_widget_bounds(&app, "ResizeBar").expect("resize bar bounds");
    let x = resize.x + resize.w * 0.5;
    let y = resize.y + resize.h * 0.5;
    let mut router = InputRouter::default();

    router.route_event(&app.tree, runtime.clone(), &pointer_button(x, y, true));
    layout_app(&mut app);
    router.route_event(&app.tree, runtime.clone(), &pointer_move(x, y - 8.0));
    layout_app(&mut app);
    router.route_event(&app.tree, runtime.clone(), &pointer_move(x, y - 16.0));
    layout_app(&mut app);
    router.route_event(&app.tree, runtime.clone(), &pointer_move(x, y - 24.0));
    layout_app(&mut app);
    router.route_event(&app.tree, runtime, &pointer_button(x, y - 24.0, false));
    layout_app(&mut app);

    let heights = actions
        .borrow()
        .iter()
        .filter_map(|action| match action {
            ChatWidgetAction::SetComposerHeight { height } => Some(*height),
            _ => None,
        })
        .collect::<Vec<_>>();
    let final_height = *heights.last().expect("resize height action");
    assert!(
        (final_height - 136.0).abs() <= 0.5,
        "multi-frame drag should track pointer delta once, heights={heights:?}"
    );
    assert!(
        heights.iter().all(|height| *height < 150.0),
        "height should not compound total_delta across redraws: {heights:?}"
    );
}

#[test]
fn chat_composer_typing_dispatches_draft_action() {
    let session = ChatSessionState::new(ChatSessionId::from_index(1), "Composer");
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let draft = State::new(String::new());
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        ChatWidget::new(Some(session), draft.clone())
            .on_action(Action::Chat)
            .into_view(),
    );
    layout_app(&mut app);

    let input = first_widget_bounds(&app, "TextInput").expect("composer input");
    let mut router = InputRouter::default();
    click(
        &mut router,
        &app,
        runtime.clone(),
        input.x + 12.0,
        input.y + 12.0,
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &ailloli_ui_core::event::Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Character("a".into()),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: Some(Point::new(input.x + 12.0, input.y + 12.0)),
            text: Some("a".into()),
        }),
    );

    assert_eq!(draft.read(), "a");
    assert_eq!(
        runtime.take_actions(),
        vec![Action::Chat(ChatWidgetAction::SetComposerDraft {
            text: "a".into()
        })]
    );
}

#[test]
fn chat_composer_play_and_stop_emit_distinct_actions() {
    let session = ChatSessionState::new(ChatSessionId::from_index(1), "Composer");
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let draft = State::new("ship it".to_string());
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        ChatWidget::new(Some(session.clone()), draft.clone())
            .on_action(Action::Chat)
            .into_view(),
    );
    layout_app(&mut app);

    let button = first_widget_bounds(&app, "Button").expect("play button bounds");
    let mut router = InputRouter::default();
    click(
        &mut router,
        &app,
        runtime.clone(),
        button.x + button.w * 0.5,
        button.y + button.h * 0.5,
    );
    assert_eq!(
        runtime.take_actions(),
        vec![Action::Chat(ChatWidgetAction::Send {
            text: "ship it".into()
        })]
    );

    app.reconcile(
        ChatWidget::new(Some(session), draft)
            .composer_controls(ChatComposerControls {
                running: true,
                ..ChatComposerControls::default()
            })
            .on_action(Action::Chat)
            .into_view(),
    );
    layout_app(&mut app);
    let button = first_widget_bounds(&app, "Button").expect("stop button bounds");
    click(
        &mut router,
        &app,
        runtime.clone(),
        button.x + button.w * 0.5,
        button.y + button.h * 0.5,
    );

    assert_eq!(
        runtime.take_actions(),
        vec![Action::Chat(ChatWidgetAction::StopTurn)]
    );
}

#[test]
fn chat_composer_selects_dispatch_actions() {
    let session = ChatSessionState::new(ChatSessionId::from_index(1), "Composer");
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        ChatWidget::new(Some(session), State::new(String::new()))
            .composer_controls(ChatComposerControls {
                model_options: vec![
                    ailloli_ui_widgets::controls::ChatComposerOption::new("gpt-5", "GPT-5"),
                    ailloli_ui_widgets::controls::ChatComposerOption::new(
                        "gpt-5-mini",
                        "GPT-5 mini",
                    ),
                ],
                ..ChatComposerControls::default()
            })
            .on_action(Action::Chat)
            .into_view(),
    );
    layout_app(&mut app);

    let mut selects = widget_id_bounds(&app, "Select");
    selects.sort_by(|(_, a), (_, b)| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    assert_eq!(selects.len(), 3);
    let (permission_id, permission_bounds) = selects[0];
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        permission_id,
        &pointer_button(
            permission_bounds.x + permission_bounds.w * 0.5,
            permission_bounds.y + permission_bounds.h * 0.5,
            false,
        ),
    );
    layout_app(&mut app);
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        permission_id,
        &pointer_button(
            permission_bounds.x + 12.0,
            popup_option_y(&app, permission_id, permission_bounds, 3, 1),
            false,
        ),
    );

    let (model_id, model_bounds) = selects[1];
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        model_id,
        &pointer_button(
            model_bounds.x + model_bounds.w * 0.5,
            model_bounds.y + model_bounds.h * 0.5,
            false,
        ),
    );
    layout_app(&mut app);
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        model_id,
        &pointer_button(
            model_bounds.x + 12.0,
            popup_option_y(&app, model_id, model_bounds, 2, 1),
            false,
        ),
    );

    let actions = runtime.take_actions();
    assert!(actions.iter().any(|action| {
        matches!(
            action,
            Action::Chat(ChatWidgetAction::SetComposerPermission { value }) if value == "workspace_write"
        )
    }));
    assert!(actions.iter().any(|action| {
        matches!(
            action,
            Action::Chat(ChatWidgetAction::SetComposerModel { model_id }) if model_id.as_deref() == Some("gpt-5-mini")
        )
    }));
}

#[test]
fn chat_composer_runtime_controls() {
    let session = ChatSessionState::new(ChatSessionId::from_index(1), "Runtime controls");
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        ChatWidget::new(Some(session), State::new(String::new()))
            .composer_controls(ChatComposerControls {
                model_options: vec![ailloli_ui_widgets::controls::ChatComposerOption::new(
                    "gpt-5.6-terra",
                    "GPT-5.6 Terra",
                )],
                selected_model_id: Some("gpt-5.6-terra".into()),
                reasoning_options: vec![
                    ailloli_ui_widgets::controls::ChatComposerOption::new("low", "low"),
                    ailloli_ui_widgets::controls::ChatComposerOption::new("medium", "medium"),
                ],
                selected_reasoning_level: Some("medium".into()),
                reasoning_enabled: true,
                ..ChatComposerControls::default()
            })
            .on_action(Action::Chat)
            .into_view(),
    );
    layout_app(&mut app);

    let mut selects = widget_id_bounds(&app, "Select");
    selects.sort_by(|(_, a), (_, b)| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    assert_eq!(selects.len(), 3);
    let (reasoning_id, reasoning_bounds) = selects[2];
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        reasoning_id,
        &pointer_button(
            reasoning_bounds.x + reasoning_bounds.w * 0.5,
            reasoning_bounds.y + reasoning_bounds.h * 0.5,
            false,
        ),
    );
    layout_app(&mut app);
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        reasoning_id,
        &pointer_button(
            reasoning_bounds.x + 12.0,
            popup_option_y(&app, reasoning_id, reasoning_bounds, 3, 1),
            false,
        ),
    );

    assert!(runtime.take_actions().iter().any(|action| {
        matches!(
            action,
            Action::Chat(ChatWidgetAction::SetComposerReasoning { reasoning_level })
                if reasoning_level.as_deref() == Some("low")
        )
    }));
}

#[test]
fn chat_widget_tool_message_has_copy_without_retry() {
    let mut session = ChatSessionState::new(ChatSessionId::from_index(1), "Tools");
    session.messages.push(ChatMessage::new(
        ChatItemId::new("tool-command"),
        ChatRole::Tool,
        ChatMessageKind::Command,
        "cargo test",
    ));
    let draft = State::new(String::new());
    let app = layout_view(ChatWidget::<()>::new(Some(session), draft).into_view());
    let texts = painted_texts(&app);

    assert!(texts.iter().any(|text| text == "Copy"));
    assert!(!texts.iter().any(|text| text == "Retry"));
}

#[test]
fn chat_widget_assistant_without_request_has_copy_without_retry() {
    let mut session = ChatSessionState::new(ChatSessionId::from_index(1), "Assistant");
    session.messages.push(ChatMessage::assistant(
        ChatItemId::new("assistant"),
        "answer",
    ));
    let draft = State::new(String::new());
    let app = layout_view(ChatWidget::<()>::new(Some(session), draft).into_view());
    let texts = painted_texts(&app);

    assert!(texts.iter().any(|text| text == "Copy"));
    assert!(!texts.iter().any(|text| text == "Retry"));
}

#[test]
fn chat_widget_user_and_request_assistant_are_retryable() {
    let mut session = ChatSessionState::new(ChatSessionId::from_index(1), "Retryable");
    session
        .messages
        .push(ChatMessage::user(ChatItemId::new("user"), "prompt"));
    session.messages.push(
        ChatMessage::assistant(ChatItemId::new("assistant"), "answer")
            .request_id(ChatRequestId::new("request-1"))
            .status(ChatMessageStatus::Complete),
    );
    let draft = State::new(String::new());
    let app = layout_view(ChatWidget::<()>::new(Some(session), draft).into_view());
    let texts = painted_texts(&app);

    assert_eq!(
        texts.iter().filter(|text| text.as_str() == "Retry").count(),
        2
    );
}

fn layout_view(view: View<()>) -> Runtime<()> {
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(view);
    layout_app(&mut app);
    app
}

fn long_session(count: usize) -> ChatSessionState {
    let mut session = ChatSessionState::new(ChatSessionId::from_index(1), "Chat 1");
    for idx in 0..count {
        let text = format!(
            "message {idx}: a wrapped chat transcript line with enough content to occupy space"
        );
        let id = ChatItemId::from_index((idx + 1) as u64);
        if idx % 2 == 0 {
            session.messages.push(ChatMessage::user(id, text));
        } else {
            session.messages.push(
                ChatMessage::assistant(id, text)
                    .request_id(ChatRequestId::from_index((idx / 2 + 1) as u64)),
            );
        }
    }
    session
}

fn listing_message_text() -> String {
    [
        "Current directory files/folders:",
        "",
        "Cargo.lock",
        "Cargo.toml",
        "LICENSE",
        "README.md",
        "artifacts",
        "crates",
        "ailloli_ui",
        "ailloli_ui_app_storage",
        "ailloli_ui_bench",
        "ailloli_ui_core",
        "ailloli_ui_devicons_font",
        "ailloli_ui_devtools_core",
        "ailloli_ui_devtools_ui",
        "ailloli_ui_editor",
        "ailloli_ui_fs",
        "ailloli_ui_fs_local",
        "ailloli_ui_icon",
        "ailloli_ui_openxr",
        "ailloli_ui_packaging",
        "ailloli_ui_render_vulkan",
        "ailloli_ui_render_wgpu",
        "ailloli_ui_runtime",
        "ailloli_ui_terminal_core",
        "ailloli_ui_terminal_pty",
        "ailloli_ui_text",
        "ailloli_ui_widgets",
        "ailloli_ui_winit",
    ]
    .join("\n")
}

fn layout_app<A: 'static>(app: &mut Runtime<A>) {
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(560.0, 280.0),
        Scale::new(1.0),
        &mut text_system,
    );
}

fn painted_texts<A: 'static>(app: &Runtime<A>) -> Vec<String> {
    let mut text_system = TextSystem::new();
    app.paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .filter_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some(text.layout.text().to_string()),
            _ => None,
        })
        .collect()
}

#[derive(Debug)]
struct PaintedTextBox {
    top: f32,
    bottom: f32,
    lines: usize,
}

fn painted_text_box<A: 'static>(app: &Runtime<A>, needle: &str) -> Option<PaintedTextBox> {
    let mut text_system = TextSystem::new();
    app.paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .filter_map(|cmd| match cmd {
            DrawCmd::Text(text) if text.layout.text() == needle => {
                let baseline = text
                    .layout
                    .lines
                    .first()
                    .map(|line| line.baseline_y)
                    .unwrap_or(0.0);
                let top = text.pos[1] - baseline;
                Some(PaintedTextBox {
                    top,
                    bottom: top + text.layout.metrics.height,
                    lines: text.layout.lines.len(),
                })
            }
            _ => None,
        })
        .next_back()
}

fn visible_text_position<A: 'static>(
    app: &Runtime<A>,
    needle: &str,
    bounds: Rect,
) -> Option<(f32, f32)> {
    let mut text_system = TextSystem::new();
    app.paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .filter_map(|cmd| match cmd {
            DrawCmd::Text(text)
                if text.layout.text() == needle && bounds.contains(text.pos[0], text.pos[1]) =>
            {
                Some((text.pos[0], text.pos[1]))
            }
            _ => None,
        })
        .next_back()
}

fn assert_text_order(texts: &[String], before: &str, after: &str) {
    let before_index = texts
        .iter()
        .position(|text| text.contains(before))
        .expect("before text");
    let after_index = texts
        .iter()
        .position(|text| text.contains(after))
        .expect("after text");
    assert!(before_index < after_index, "{texts:?}");
}

fn widget_count<A: 'static>(app: &Runtime<A>, debug_name: &str) -> usize {
    app.tree
        .iter_elements()
        .filter(|(_, el)| {
            matches!(&el.kind, ElementKind::Widget(widget) if widget.debug_name() == debug_name)
        })
        .count()
}

fn first_widget_id<A: 'static>(app: &Runtime<A>, debug_name: &str) -> Option<ElementId> {
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

fn widget_id_bounds<A: 'static>(
    app: &Runtime<A>,
    debug_name: &str,
) -> Vec<(ElementId, ailloli_ui_core::Rect)> {
    app.tree
        .iter_elements()
        .filter_map(|(id, el)| match &el.kind {
            ElementKind::Widget(widget) if widget.debug_name() == debug_name => {
                absolute_paint_bounds(&app.tree, id).map(|bounds| (id, bounds))
            }
            _ => None,
        })
        .collect()
}

fn popup_option_y<A: 'static>(
    app: &Runtime<A>,
    select_id: ElementId,
    bounds: ailloli_ui_core::Rect,
    option_count: usize,
    index: usize,
) -> f32 {
    let popup = app
        .tree
        .get(select_id)
        .and_then(|el| el.layout.as_ref())
        .and_then(|layout| layout.overlay_hit_bounds.first().copied())
        .expect("open select popup bounds");
    bounds.y + popup.y + (index as f32 + 0.5) * (popup.h / option_count as f32)
}

fn scroll_child_offset<A: 'static>(app: &Runtime<A>) -> f32 {
    let scroll_id = first_widget_id(app, "ScrollView").expect("scroll view");
    let scroll_layout = app.tree.get(scroll_id).unwrap().layout.as_ref().unwrap();
    scroll_layout.children[0].offset.y
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
        &ailloli_ui_core::event::Event::Pointer(
            ailloli_ui_core::event::pointer::PointerEvent::Wheel {
                pos: Point::new(x, y),
                delta: WheelDelta::PixelDelta { x: 0.0, y: delta_y },
                modifiers: Modifiers::default(),
                precise: true,
            },
        ),
    );
}

fn drag_resize<A: Clone + 'static>(
    router: &mut InputRouter,
    app: &Runtime<A>,
    runtime: RuntimeHandle<A>,
    bounds: Rect,
    delta_y: f32,
) {
    let x = bounds.x + bounds.w * 0.5;
    let y = bounds.y + bounds.h * 0.5;
    router.route_event(&app.tree, runtime.clone(), &pointer_button(x, y, true));
    router.route_event(&app.tree, runtime.clone(), &pointer_move(x, y + delta_y));
    router.route_event(&app.tree, runtime, &pointer_button(x, y + delta_y, false));
}

fn pointer_button(x: f32, y: f32, pressed: bool) -> ailloli_ui_core::event::Event {
    ailloli_ui_core::event::Event::Pointer(ailloli_ui_core::event::pointer::PointerEvent::Button {
        pos: ailloli_ui_core::Point::new(x, y),
        button: ailloli_ui_core::event::pointer::MouseButton::Left,
        pressed,
        modifiers: Default::default(),
    })
}

fn pointer_move(x: f32, y: f32) -> ailloli_ui_core::event::Event {
    ailloli_ui_core::event::Event::Pointer(ailloli_ui_core::event::pointer::PointerEvent::Moved {
        pos: ailloli_ui_core::Point::new(x, y),
        modifiers: Default::default(),
    })
}
