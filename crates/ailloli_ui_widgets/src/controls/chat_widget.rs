use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use crate::layout::{
    Column, Container, FlexItemExt, LayoutExt, ResizeBar, ResizeBarStyle, ResizeDragPhase, Row,
    ScrollView,
};
use crate::primitives::Icon;
use crate::text::Text;
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{
    ChatItemId, ChatMessage, ChatMessageKind, ChatMessageStatus, ChatRole, ChatSessionState,
    ChatSessionStatus, Color, FontId, TextStyle, Theme,
};
use ailloli_ui_runtime::component::{
    ComponentNode, Context, IntoView, Signal, State, View, Widget,
};
use ailloli_ui_runtime::input::EventCtx;
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawCmd, DrawRRect, DrawText};
use ailloli_ui_text::{PreparedTextLayout, TextLayoutParams, TextSystem, WrapMode};
use lucide_icons::Icon as LucideIcon;

use super::button::{Button, ButtonVariant};
use super::popup::PopupPlacement;
use super::select::{Select, SelectSize};
use super::text_input::{TextInput, TextInputStyle};

pub type ChatWidgetActionHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, ChatWidgetAction)>;
pub type ChatWidgetSendHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, String)>;
pub type ChatWidgetCopyHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, ChatItemId, String)>;
pub type ChatWidgetRetryHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, ChatItemId)>;
pub type ChatMessageRenderer<A> = Rc<dyn Fn(&ChatMessage) -> View<A>>;

#[derive(Debug, Clone, PartialEq)]
pub enum ChatWidgetAction {
    Send { text: String },
    Copy { item_id: ChatItemId, text: String },
    Retry { item_id: ChatItemId },
    SetComposerDraft { text: String },
    SetComposerPermission { value: String },
    SetComposerModel { model_id: Option<String> },
    SetComposerReasoning { reasoning_level: Option<String> },
    SetComposerHeight { height: f32 },
    StopTurn,
}

impl Eq for ChatWidgetAction {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatComposerOption {
    pub value: String,
    pub label: String,
}

impl ChatComposerOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatComposerControls {
    pub permission_options: Vec<ChatComposerOption>,
    pub selected_permission: String,
    pub model_options: Vec<ChatComposerOption>,
    pub selected_model_id: Option<String>,
    pub reasoning_options: Vec<ChatComposerOption>,
    pub selected_reasoning_level: Option<String>,
    pub permission_enabled: bool,
    pub model_enabled: bool,
    pub reasoning_enabled: bool,
    pub running: bool,
    pub height: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl Default for ChatComposerControls {
    fn default() -> Self {
        Self {
            permission_options: vec![
                ChatComposerOption::new("read_only", "read_only"),
                ChatComposerOption::new("workspace_write", "write"),
                ChatComposerOption::new("danger_full_access", "full_acess"),
            ],
            selected_permission: "read_only".into(),
            model_options: Vec::new(),
            selected_model_id: None,
            reasoning_options: Vec::new(),
            selected_reasoning_level: None,
            permission_enabled: true,
            model_enabled: true,
            reasoning_enabled: false,
            running: false,
            height: 112.0,
            min_height: 72.0,
            max_height: 220.0,
        }
    }
}

impl ChatComposerControls {
    pub fn clamped_height(&self) -> f32 {
        if self.height.is_finite() {
            self.height.clamp(self.min_height, self.max_height)
        } else {
            112.0
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChatWidgetStyle {
    pub background: Color,
    pub border: Border,
    pub header_background: Color,
    pub user_background: Color,
    pub assistant_background: Color,
    pub tool_background: Color,
    pub error_background: Color,
    pub text: TextStyle,
    pub muted: TextStyle,
    pub role_text: TextStyle,
    pub input: TextInputStyle,
    pub radius: Radius,
    pub header_radius: f32,
    pub message_radius: f32,
    pub inline_button_radius: f32,
    pub padding: f32,
    pub gap: f32,
    pub message_gap: f32,
    pub height: f32,
    pub width: f32,
    pub compact: bool,
}

impl Default for ChatWidgetStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl ChatWidgetStyle {
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        let mut input = TextInputStyle::from_theme(theme);
        input.text = TextStyle::new(FontId::Ui, 13, palette.text);
        input.bg = Color::hex_rgb(0x111827);
        Self {
            background: Color::hex_rgb(0x0B1018),
            border: Border::new(1.0, palette.border.with_alpha(0.78)),
            header_background: Color::hex_rgb(0x111827),
            user_background: palette.accent.with_alpha(0.22),
            assistant_background: Color::hex_rgb(0x17202A),
            tool_background: Color::hex_rgb(0x1E293B),
            error_background: Color::hex_rgb(0x3A1717),
            text: TextStyle::new(FontId::Ui, 13, palette.text),
            muted: TextStyle::new(FontId::Ui, 11, palette.text_muted),
            role_text: TextStyle::new(FontId::Ui, 11, palette.text_muted),
            input,
            radius: Radius::uniform(theme.radius().md),
            header_radius: 6.0,
            message_radius: 6.0,
            inline_button_radius: 5.0,
            padding: 8.0,
            gap: 8.0,
            message_gap: 6.0,
            height: 260.0,
            width: 520.0,
            compact: false,
        }
    }

    pub fn compact(mut self) -> Self {
        self.compact = true;
        self.padding = 6.0;
        self.gap = 6.0;
        self.message_gap = 4.0;
        self.height = 190.0;
        self
    }
}

pub struct ChatWidget<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    session: Signal<Option<ChatSessionState>>,
    draft: Signal<String>,
    style: ChatWidgetStyle,
    on_action: Option<ChatWidgetActionHandler<A>>,
    on_send: Option<ChatWidgetSendHandler<A>>,
    on_copy: Option<ChatWidgetCopyHandler<A>>,
    on_retry: Option<ChatWidgetRetryHandler<A>>,
    message_renderer: Option<ChatMessageRenderer<A>>,
    composer_accessory: Option<View<A>>,
    composer_controls: ChatComposerControls,
    follow_latest: bool,
}

crate::impl_layout_builders!(ChatWidget);

impl<A: 'static> ChatWidget<A> {
    pub fn new(session: Option<ChatSessionState>, draft: impl Into<Signal<String>>) -> Self {
        Self::bind_session(State::new(session).into_signal(), draft)
    }

    pub fn bind_session(
        session: impl Into<Signal<Option<ChatSessionState>>>,
        draft: impl Into<Signal<String>>,
    ) -> Self {
        let style = ChatWidgetStyle::default();
        Self {
            layout: LayoutStyle::default()
                .width(style.width)
                .height(style.height),
            flex_item: FlexItemStyle::default(),
            session: session.into(),
            draft: draft.into(),
            style,
            on_action: None,
            on_send: None,
            on_copy: None,
            on_retry: None,
            message_renderer: None,
            composer_accessory: None,
            composer_controls: ChatComposerControls::default(),
            follow_latest: true,
        }
    }

    pub fn chat_style(mut self, style: ChatWidgetStyle) -> Self {
        self.layout = self.layout.width(style.width).height(style.height);
        self.style = style;
        self
    }

    pub fn compact(mut self, compact: bool) -> Self {
        self.style.compact = compact;
        self
    }

    pub fn on_action(mut self, f: impl Fn(ChatWidgetAction) -> A + 'static) -> Self {
        self.on_action = Some(Rc::new(move |ctx, action| ctx.dispatch(f(action))));
        self
    }

    pub fn on_action_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, ChatWidgetAction) + 'static,
    ) -> Self {
        self.on_action = Some(Rc::new(f));
        self
    }

    pub fn on_send(mut self, f: impl Fn(String) -> A + 'static) -> Self {
        self.on_send = Some(Rc::new(move |ctx, text| ctx.dispatch(f(text))));
        self
    }

    pub fn on_send_ctx(mut self, f: impl Fn(&mut EventCtx<A>, String) + 'static) -> Self {
        self.on_send = Some(Rc::new(f));
        self
    }

    pub fn on_copy(mut self, f: impl Fn(ChatItemId, String) -> A + 'static) -> Self {
        self.on_copy = Some(Rc::new(move |ctx, item_id, text| {
            ctx.dispatch(f(item_id, text))
        }));
        self
    }

    pub fn on_copy_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, ChatItemId, String) + 'static,
    ) -> Self {
        self.on_copy = Some(Rc::new(f));
        self
    }

    pub fn on_retry(mut self, f: impl Fn(ChatItemId) -> A + 'static) -> Self {
        self.on_retry = Some(Rc::new(move |ctx, item_id| ctx.dispatch(f(item_id))));
        self
    }

    pub fn on_retry_ctx(mut self, f: impl Fn(&mut EventCtx<A>, ChatItemId) + 'static) -> Self {
        self.on_retry = Some(Rc::new(f));
        self
    }

    pub fn message_renderer(mut self, f: impl Fn(&ChatMessage) -> View<A> + 'static) -> Self {
        self.message_renderer = Some(Rc::new(f));
        self
    }

    pub fn composer_accessory(mut self, accessory: View<A>) -> Self {
        self.composer_accessory = Some(accessory);
        self
    }

    pub fn composer_controls(mut self, controls: ChatComposerControls) -> Self {
        self.composer_controls = controls;
        self
    }

    pub fn follow_latest(mut self, enabled: bool) -> Self {
        self.follow_latest = enabled;
        self
    }
}

impl<A: 'static> IntoView<A> for ChatWidget<A> {
    fn into_view(self) -> View<A> {
        let layout = self.layout;
        finish_view_sized(
            View::component(ChatWidgetComponent {
                layout,
                session: self.session,
                draft: self.draft,
                style: self.style,
                on_action: self.on_action,
                on_send: self.on_send,
                on_copy: self.on_copy,
                on_retry: self.on_retry,
                message_renderer: self.message_renderer,
                composer_accessory: self.composer_accessory,
                composer_controls: self.composer_controls,
                follow_latest: self.follow_latest,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(layout),
        )
    }
}

struct ChatWidgetComponent<A> {
    layout: LayoutStyle,
    session: Signal<Option<ChatSessionState>>,
    draft: Signal<String>,
    style: ChatWidgetStyle,
    on_action: Option<ChatWidgetActionHandler<A>>,
    on_send: Option<ChatWidgetSendHandler<A>>,
    on_copy: Option<ChatWidgetCopyHandler<A>>,
    on_retry: Option<ChatWidgetRetryHandler<A>>,
    message_renderer: Option<ChatMessageRenderer<A>>,
    composer_accessory: Option<View<A>>,
    composer_controls: ChatComposerControls,
    follow_latest: bool,
}

struct ChatWidgetHandlers<A> {
    on_action: Option<ChatWidgetActionHandler<A>>,
    on_send: Option<ChatWidgetSendHandler<A>>,
    on_copy: Option<ChatWidgetCopyHandler<A>>,
    on_retry: Option<ChatWidgetRetryHandler<A>>,
}

impl<A> Clone for ChatWidgetHandlers<A> {
    fn clone(&self) -> Self {
        Self {
            on_action: self.on_action.clone(),
            on_send: self.on_send.clone(),
            on_copy: self.on_copy.clone(),
            on_retry: self.on_retry.clone(),
        }
    }
}

impl<A: 'static> ComponentNode<A> for ChatWidgetComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let composer_resize_start_height = context.signal(None::<f32>);
        let mut content = Column::new()
            .fill()
            .gap(self.style.gap)
            .child(self.header())
            .child(self.messages().fill());
        let mut composer_area = Column::new().fill_width();
        if let Some(accessory) = self.composer_accessory.clone() {
            composer_area = composer_area.child(accessory);
        }
        composer_area = composer_area
            .child(self.composer_resize_bar(composer_resize_start_height))
            .child(self.composer());
        content = content.child(composer_area);

        let mut root = Container::new()
            .background(self.style.background)
            .border(self.style.border.widths.top, self.style.border.colors.top)
            .radius(self.style.radius.tl)
            .padding(self.style.padding)
            .clip_children(true)
            .child(content);
        *root.layout_mut() = self.layout;
        root.into_view()
    }
}

impl<A: 'static> ChatWidgetComponent<A> {
    fn handlers(&self) -> ChatWidgetHandlers<A> {
        ChatWidgetHandlers {
            on_action: self.on_action.clone(),
            on_send: self.on_send.clone(),
            on_copy: self.on_copy.clone(),
            on_retry: self.on_retry.clone(),
        }
    }

    fn header(&self) -> View<A> {
        let session = self.session.clone();
        let title = session.to_text_with(|session| {
            let (title, status, count) = session
                .as_ref()
                .map(|session| {
                    (
                        session.title.clone(),
                        session_status_label(session.status).to_string(),
                        session.messages.len(),
                    )
                })
                .unwrap_or_else(|| ("No session".into(), "idle".into(), 0));
            format!("{title} - {status} - {count} msg")
        });
        Container::new()
            .fill_width()
            .height(if self.style.compact { 32.0 } else { 40.0 })
            .background(self.style.header_background)
            .radius(self.style.header_radius)
            .padding(6.0)
            .child(
                Row::new().gap(8.0).fill_width().child(
                    Container::new()
                        .fill_width()
                        .child(Text::new(title).style(self.style.text).nowrap()),
                ),
            )
            .into_view()
    }

    fn messages(&self) -> View<A> {
        if self.message_renderer.is_none() {
            return ScrollView::vertical()
                .follow_end(self.follow_latest)
                .child(
                    View::leaf(ChatMessagesWidget {
                        layout: LayoutStyle::default().fill_width(),
                        session: self.session.clone(),
                        style: self.style.clone(),
                        handlers: self.handlers(),
                        geometry: Rc::new(RefCell::new(ChatMessagesGeometry::default())),
                    })
                    .fill_width(),
                )
                .fill_width()
                .into_view();
        }

        let mut list = Column::new().gap(self.style.message_gap).fill_width();
        let session = self.session.read();
        match &session {
            Some(session) if !session.messages.is_empty() => {
                for message in &session.messages {
                    let rendered = self
                        .message_renderer
                        .as_ref()
                        .map(|renderer| renderer(message))
                        .unwrap_or_else(|| self.message_bubble(message));
                    list = list.child(rendered);
                }
            }
            Some(_) => {
                list = list.child(Text::new("No messages yet").style(self.style.muted));
            }
            None => {
                list = list.child(Text::new("No chat session").style(self.style.muted));
            }
        }

        ScrollView::vertical()
            .follow_end(self.follow_latest)
            .child(list)
            .fill_width()
            .into_view()
    }

    fn message_bubble(&self, message: &ChatMessage) -> View<A> {
        let background = message_background(&self.style, message);
        let role = format!(
            "{} / {}{}",
            role_label(message.role),
            kind_label(message.kind),
            status_suffix(message.status)
        );
        let copy = ChatWidgetAction::Copy {
            item_id: message.id.clone(),
            text: message.text.clone(),
        };
        let copy_handlers = self.handlers();
        let mut actions = Row::new()
            .gap(6.0)
            .child(action_button("Copy", copy, copy_handlers));
        if message_is_retryable(message) {
            actions = actions.child(action_button(
                "Retry",
                ChatWidgetAction::Retry {
                    item_id: message.id.clone(),
                },
                self.handlers(),
            ));
        }
        Container::new()
            .fill_width()
            .background(background)
            .radius(self.style.message_radius)
            .padding(if self.style.compact { 6.0 } else { 8.0 })
            .child(
                Column::new()
                    .gap(4.0)
                    .fill_width()
                    .child(Text::new(role).style(self.style.role_text).nowrap())
                    .child(
                        Text::new(if message.text.is_empty() {
                            "...".to_string()
                        } else {
                            message.text.clone()
                        })
                        .style(self.style.text)
                        .wrap_anywhere(),
                    )
                    .child(actions),
            )
            .into_view()
    }

    fn composer_resize_bar(&self, resize_start_height: Signal<Option<f32>>) -> View<A> {
        let current_height = self.composer_controls.clamped_height();
        let resize_handlers = self.handlers();
        ResizeBar::<A>::horizontal()
            .fill_width()
            .height(8.0)
            .resize_bar_style(ResizeBarStyle {
                hit_thickness: 8.0,
                line_thickness: 2.0,
                idle_color: self.style.input.border_focused,
                hover_color: self.style.input.border_focused,
                active_color: self.style.input.border_focused,
                ..ResizeBarStyle::default()
            })
            .on_resize_ctx(move |ctx, event| {
                let drag_start_height = match event.phase {
                    ResizeDragPhase::Start => {
                        resize_start_height.set(Some(current_height));
                        current_height
                    }
                    ResizeDragPhase::Drag | ResizeDragPhase::End => {
                        resize_start_height.read().unwrap_or(current_height)
                    }
                };
                emit_chat_widget_action(
                    ctx,
                    &resize_handlers,
                    ChatWidgetAction::SetComposerHeight {
                        height: drag_start_height - event.total_delta,
                    },
                );
                if event.phase == ResizeDragPhase::End {
                    resize_start_height.set(None);
                }
            })
            .into_view()
            .key("ailloli_ui-chat-composer-resize-bar")
    }

    fn composer(&self) -> View<A> {
        let draft = self.draft.clone();
        let handlers = self.handlers();
        let controls = self.composer_controls.clone();
        let draft_handlers = self.handlers();
        let height = controls.clamped_height();
        let content_height = (height - 14.0).max(0.0);
        let input_height = (content_height - 34.0).max(0.0);
        let selected_permission = controls.selected_permission.clone();
        let selected_model = controls.selected_model_id.clone().unwrap_or_default();
        let selected_reasoning = controls
            .selected_reasoning_level
            .clone()
            .unwrap_or_default();
        let mut permission_select = Select::<String, A>::new()
            .placeholder("permissions")
            .selected(selected_permission)
            .popup_placement(PopupPlacement::Top)
            .select_size(SelectSize::Compact)
            .width(122.0)
            .height(30.0)
            .disabled(controls.running || !controls.permission_enabled);
        for option in &controls.permission_options {
            permission_select =
                permission_select.option(option.value.clone(), option.label.clone());
        }
        let permission_handlers = self.handlers();
        permission_select = permission_select.on_change_ctx(move |ctx, value| {
            emit_chat_widget_action(
                ctx,
                &permission_handlers,
                ChatWidgetAction::SetComposerPermission { value },
            );
        });

        let mut model_select = Select::<String, A>::new()
            .placeholder("model")
            .selected(selected_model)
            .popup_placement(PopupPlacement::Top)
            .select_size(SelectSize::Compact)
            .fill_width()
            .height(30.0)
            .disabled(controls.running || !controls.model_enabled)
            .option(String::new(), "Provider default");
        for option in &controls.model_options {
            model_select = model_select.option(option.value.clone(), option.label.clone());
        }
        let model_handlers = self.handlers();
        model_select = model_select.on_change_ctx(move |ctx, value| {
            emit_chat_widget_action(
                ctx,
                &model_handlers,
                ChatWidgetAction::SetComposerModel {
                    model_id: (!value.is_empty()).then_some(value),
                },
            );
        });

        let mut reasoning_select = Select::<String, A>::new()
            .placeholder("reasoning")
            .selected(selected_reasoning)
            .popup_placement(PopupPlacement::Top)
            .select_size(SelectSize::Compact)
            .width(92.0)
            .height(30.0)
            .disabled(
                controls.running
                    || !controls.reasoning_enabled
                    || controls.reasoning_options.is_empty(),
            )
            .option(String::new(), "default");
        for option in &controls.reasoning_options {
            reasoning_select = reasoning_select.option(option.value.clone(), option.label.clone());
        }
        let reasoning_handlers = self.handlers();
        reasoning_select = reasoning_select.on_change_ctx(move |ctx, value| {
            emit_chat_widget_action(
                ctx,
                &reasoning_handlers,
                ChatWidgetAction::SetComposerReasoning {
                    reasoning_level: (!value.is_empty()).then_some(value),
                },
            );
        });

        let button_handlers = handlers.clone();
        let running = controls.running;
        let button_icon = if running {
            LucideIcon::SquareStop
        } else {
            LucideIcon::Play
        };
        let button_variant = if running {
            ButtonVariant::Destructive
        } else {
            ButtonVariant::Primary
        };

        Container::new()
            .fill_width()
            .height(height)
            .background(self.style.input.bg)
            .border(1.0, self.style.input.border)
            .radius(self.style.inline_button_radius)
            .padding(6.0)
            .clip_children(true)
            .child(
                Column::new()
                    .gap(4.0)
                    .fill_width()
                    .child(
                        TextInput::<A>::new()
                            .bind(self.draft.clone())
                            .multiline()
                            .placeholder("Ask...")
                            .input_style(TextInputStyle {
                                bg: Color::TRANSPARENT,
                                border: Color::TRANSPARENT,
                                border_focused: Color::TRANSPARENT,
                                radius: 0.0,
                                ..self.style.input
                            })
                            .on_change_ctx(move |ctx, text| {
                                emit_chat_widget_action(
                                    ctx,
                                    &draft_handlers,
                                    ChatWidgetAction::SetComposerDraft { text },
                                );
                            })
                            .height(input_height)
                            .fill_width()
                            .into_view()
                            .key("ailloli_ui-chat-composer-input"),
                    )
                    .child(
                        Row::new()
                            .gap(6.0)
                            .fill_width()
                            .child(
                                permission_select
                                    .into_view()
                                    .key("ailloli_ui-chat-composer-permission-select"),
                            )
                            .child(
                                model_select
                                    .into_view()
                                    .key("ailloli_ui-chat-composer-model-select"),
                            )
                            .child(
                                reasoning_select
                                    .into_view()
                                    .key("ailloli_ui-chat-composer-reasoning-select"),
                            )
                            .child(
                                Button::new()
                                    .variant(button_variant)
                                    .width(32.0)
                                    .height(30.0)
                                    .radius(self.style.inline_button_radius)
                                    .child(
                                        Icon::lucide(button_icon)
                                            .size(15.0)
                                            .into_view()
                                            .key("ailloli_ui-chat-composer-turn-button-icon"),
                                    )
                                    .on_click_ctx(move |ctx| {
                                        if running {
                                            emit_chat_widget_action(
                                                ctx,
                                                &button_handlers,
                                                ChatWidgetAction::StopTurn,
                                            );
                                            return;
                                        }
                                        let text = draft.read();
                                        if text.trim().is_empty() {
                                            return;
                                        }
                                        emit_chat_widget_action(
                                            ctx,
                                            &button_handlers,
                                            ChatWidgetAction::Send { text },
                                        );
                                    })
                                    .into_view()
                                    .key("ailloli_ui-chat-composer-turn-button"),
                            ),
                    ),
            )
            .into_view()
    }
}

struct ChatMessagesWidget<A> {
    layout: LayoutStyle,
    session: Signal<Option<ChatSessionState>>,
    style: ChatWidgetStyle,
    handlers: ChatWidgetHandlers<A>,
    geometry: Rc<RefCell<ChatMessagesGeometry>>,
}

#[derive(Clone, Default)]
struct ChatMessagesGeometry {
    width: f32,
    items: Vec<ChatMessageGeometry>,
    total_height: f32,
}

#[derive(Clone)]
struct ChatMessageGeometry {
    item_id: ChatItemId,
    role_text: String,
    body_text: String,
    y: f32,
    height: f32,
    role_height: f32,
    body_height: f32,
    role_layout: Arc<PreparedTextLayout>,
    body_layout: Arc<PreparedTextLayout>,
    copy_rect: Rect,
    retry_rect: Option<Rect>,
}

impl<A: 'static> Widget<A> for ChatMessagesWidget<A> {
    fn debug_name(&self) -> &'static str {
        "ChatMessages"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let width = constraints.max_w.max(0.0);
        let session = self.session.read();
        let geometry = match session.as_ref() {
            Some(session) if !session.messages.is_empty() => measure_chat_messages_geometry(
                ctx.text_system.as_deref_mut(),
                session,
                &self.style,
                width,
            ),
            _ => ChatMessagesGeometry {
                width,
                items: Vec::new(),
                total_height: self.style.text.px_size as f32 * 1.6,
            },
        };
        let height = match session.as_ref() {
            Some(session) if !session.messages.is_empty() => geometry.total_height,
            _ => self.style.text.px_size as f32 * 1.6,
        };
        *self.geometry.borrow_mut() = geometry;
        let size = apply_layout_size(Size::new(width, height.max(24.0)), self.layout, constraints);
        let viewport = Rect::new(0.0, 0.0, size.w, size.h);
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: viewport,
            visual_bounds: viewport,
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let session = self.session.read();
        match session.as_ref() {
            Some(session) if !session.messages.is_empty() => {
                let Some(geometry) = self.geometry_for_paint(ctx, session, bounds.w) else {
                    return;
                };
                for (message, item) in session.messages.iter().zip(geometry.items.iter()) {
                    paint_message(
                        ctx,
                        bounds.x,
                        bounds.y,
                        bounds.w,
                        &self.style,
                        message,
                        item,
                    );
                }
            }
            Some(_) => paint_plain_text(
                ctx,
                "No messages yet",
                bounds.x,
                bounds.y,
                bounds.w,
                self.style.muted,
            ),
            None => paint_plain_text(
                ctx,
                "No chat session",
                bounds.x,
                bounds.y,
                bounds.w,
                self.style.muted,
            ),
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        let Event::Pointer(PointerEvent::Button {
            pos,
            button: MouseButton::Left,
            pressed: true,
            ..
        }) = event
        else {
            return;
        };
        if !bounds.contains(pos.x, pos.y) {
            return;
        }
        let Some(action) = self.action_at(bounds, pos.x, pos.y) else {
            return;
        };
        emit_chat_widget_action(ctx, &self.handlers, action);
        ctx.request_repaint();
        ctx.stop_propagation();
    }
}

impl<A> ChatMessagesWidget<A> {
    fn geometry_for_paint(
        &self,
        ctx: &mut PaintCtx<'_>,
        session: &ChatSessionState,
        width: f32,
    ) -> Option<ChatMessagesGeometry> {
        let cached = self.geometry.borrow().clone();
        if cached.matches(session, width) {
            return Some(cached);
        }

        let geometry = measure_chat_messages_geometry(
            ctx.text_system.as_deref_mut(),
            session,
            &self.style,
            width,
        );
        if geometry.matches(session, width) {
            *self.geometry.borrow_mut() = geometry.clone();
            Some(geometry)
        } else {
            None
        }
    }

    fn action_at(&self, bounds: Rect, x: f32, y: f32) -> Option<ChatWidgetAction> {
        let session = self.session.read()?;
        let geometry = self.geometry.borrow();
        if !geometry.matches(&session, bounds.w) {
            return None;
        }
        let local_x = x - bounds.x;
        let local_y = y - bounds.y;
        for (message, item) in session.messages.iter().zip(geometry.items.iter()) {
            if item.copy_rect.contains(local_x, local_y) {
                return Some(ChatWidgetAction::Copy {
                    item_id: message.id.clone(),
                    text: message.text.clone(),
                });
            }
            if message_is_retryable(message)
                && item
                    .retry_rect
                    .is_some_and(|retry| retry.contains(local_x, local_y))
            {
                return Some(ChatWidgetAction::Retry {
                    item_id: message.id.clone(),
                });
            }
        }
        None
    }
}

fn paint_message(
    ctx: &mut PaintCtx<'_>,
    x: f32,
    y: f32,
    width: f32,
    style: &ChatWidgetStyle,
    message: &ChatMessage,
    geometry: &ChatMessageGeometry,
) -> f32 {
    let padding = message_padding(style);
    let message_y = y + geometry.y;
    let rect = Rect::new(x, message_y, width.max(0.0), geometry.height);
    debug_assert!(geometry.body_height >= 0.0);

    ctx.push(DrawCmd::RRect(DrawRRect {
        rect,
        radius: style.message_radius,
        color: message_background(style, message),
    }));
    paint_layout(
        ctx,
        geometry.role_layout.clone(),
        x + padding,
        message_y + padding,
        style.role_text.color,
    );
    paint_layout(
        ctx,
        geometry.body_layout.clone(),
        x + padding,
        message_y + padding + geometry.role_height + 4.0,
        style.text.color,
    );
    paint_inline_button(
        ctx,
        rect_to_absolute(geometry.copy_rect, x, y),
        "Copy",
        style,
    );
    if let Some(retry) = geometry.retry_rect {
        paint_inline_button(ctx, rect_to_absolute(retry, x, y), "Retry", style);
    }

    geometry.height
}

fn paint_plain_text(
    ctx: &mut PaintCtx<'_>,
    text: &str,
    x: f32,
    y: f32,
    width: f32,
    style: TextStyle,
) {
    if let Some(layout) = layout_text(ctx, text, style, width, WrapMode::NoWrap) {
        paint_layout(ctx, layout, x, y, style.color);
    }
}

fn paint_inline_button(ctx: &mut PaintCtx<'_>, rect: Rect, label: &str, style: &ChatWidgetStyle) {
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect,
        radius: style.inline_button_radius,
        color: style.header_background.with_alpha(0.72),
    }));
    if let Some(layout) = layout_text(ctx, label, style.muted, rect.w, WrapMode::NoWrap) {
        let text_y = rect.y + (rect.h - text_layout_height(Some(&layout), style.muted)) * 0.5;
        paint_layout(ctx, layout, rect.x + 8.0, text_y, style.muted.color);
    }
}

fn layout_text(
    ctx: &mut PaintCtx<'_>,
    text: &str,
    style: TextStyle,
    max_width: f32,
    wrap_mode: WrapMode,
) -> Option<Arc<PreparedTextLayout>> {
    let ts = ctx.text_system.as_deref_mut()?;
    Some(ts.layout_cached(TextLayoutParams {
        text,
        style,
        max_width: Some(max_width.max(0.0)),
        wrap_mode,
    }))
}

fn paint_layout(
    ctx: &mut PaintCtx<'_>,
    layout: Arc<PreparedTextLayout>,
    x: f32,
    y: f32,
    color: Color,
) {
    let baseline = layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0);
    ctx.push(DrawCmd::Text(DrawText {
        pos: [x, y + baseline],
        color,
        layout,
    }));
}

fn text_layout_height(layout: Option<&Arc<PreparedTextLayout>>, style: TextStyle) -> f32 {
    layout
        .map(|layout| layout.metrics.height)
        .unwrap_or(style.px_size as f32 * 1.25)
}

impl ChatMessagesGeometry {
    fn matches(&self, session: &ChatSessionState, width: f32) -> bool {
        if (self.width - width).abs() > 0.5 || self.items.len() != session.messages.len() {
            return false;
        }
        self.items
            .iter()
            .zip(session.messages.iter())
            .all(|(item, message)| {
                item.item_id == message.id
                    && item.role_text == message_role_text(message)
                    && item.body_text == message_body_text(message)
            })
    }
}

fn measure_chat_messages_geometry(
    text_system: Option<&mut TextSystem>,
    session: &ChatSessionState,
    style: &ChatWidgetStyle,
    width: f32,
) -> ChatMessagesGeometry {
    if let Some(text_system) = text_system {
        measure_chat_messages_geometry_with_text_system(text_system, session, style, width)
    } else {
        let mut text_system = TextSystem::new();
        measure_chat_messages_geometry_with_text_system(&mut text_system, session, style, width)
    }
}

fn measure_chat_messages_geometry_with_text_system(
    text_system: &mut TextSystem,
    session: &ChatSessionState,
    style: &ChatWidgetStyle,
    width: f32,
) -> ChatMessagesGeometry {
    let mut y = 0.0;
    let mut items = Vec::with_capacity(session.messages.len());
    for (idx, message) in session.messages.iter().enumerate() {
        if idx > 0 {
            y += style.message_gap;
        }
        let item = measure_message_geometry(text_system, message, style, width, y);
        y += item.height;
        items.push(item);
    }
    ChatMessagesGeometry {
        width,
        items,
        total_height: y,
    }
}

fn measure_message_geometry(
    text_system: &mut TextSystem,
    message: &ChatMessage,
    style: &ChatWidgetStyle,
    width: f32,
    y: f32,
) -> ChatMessageGeometry {
    let padding = message_padding(style);
    let content_width = (width - padding * 2.0).max(0.0);
    let role_text = message_role_text(message);
    let body_text = message_body_text(message);
    let role_layout = text_system.layout_cached(TextLayoutParams {
        text: &role_text,
        style: style.role_text,
        max_width: Some(content_width),
        wrap_mode: WrapMode::NoWrap,
    });
    let body_layout = text_system.layout_cached(TextLayoutParams {
        text: &body_text,
        style: style.text,
        max_width: Some(content_width),
        wrap_mode: WrapMode::WordOrAnywhere,
    });
    let role_height = text_layout_height(Some(&role_layout), style.role_text);
    let body_height = text_layout_height(Some(&body_layout), style.text);
    let action_y = y + padding + role_height + 4.0 + body_height + 6.0;
    let copy_rect = Rect::new(padding, action_y, 48.0, action_button_height());
    let retry_rect = message_is_retryable(message)
        .then(|| Rect::new(padding + 54.0, action_y, 50.0, action_button_height()));
    ChatMessageGeometry {
        item_id: message.id.clone(),
        role_text,
        body_text,
        y,
        height: padding * 2.0 + role_height + 4.0 + body_height + 6.0 + action_button_height(),
        role_height,
        body_height,
        role_layout,
        body_layout,
        copy_rect,
        retry_rect,
    }
}

fn rect_to_absolute(rect: Rect, x: f32, y: f32) -> Rect {
    Rect::new(x + rect.x, y + rect.y, rect.w, rect.h)
}

fn message_role_text(message: &ChatMessage) -> String {
    format!(
        "{} / {}{}",
        role_label(message.role),
        kind_label(message.kind),
        status_suffix(message.status)
    )
}

fn message_body_text(message: &ChatMessage) -> String {
    if message.text.is_empty() {
        "...".to_string()
    } else {
        message.text.clone()
    }
}

fn message_is_retryable(message: &ChatMessage) -> bool {
    match message.role {
        ChatRole::User => true,
        ChatRole::Assistant => message.request_id.is_some(),
        ChatRole::System | ChatRole::Tool => false,
    }
}

fn message_padding(style: &ChatWidgetStyle) -> f32 {
    if style.compact {
        6.0
    } else {
        8.0
    }
}

fn action_button_height() -> f32 {
    22.0
}

fn action_button<A: 'static>(
    label: &'static str,
    action: ChatWidgetAction,
    handlers: ChatWidgetHandlers<A>,
) -> Button<A> {
    Button::with_label_variant(label, ButtonVariant::Ghost)
        .height(24.0)
        .on_click_ctx(move |ctx| {
            emit_chat_widget_action(ctx, &handlers, action.clone());
        })
}

fn emit_chat_widget_action<A>(
    ctx: &mut EventCtx<A>,
    handlers: &ChatWidgetHandlers<A>,
    action: ChatWidgetAction,
) {
    if let Some(handler) = &handlers.on_action {
        handler(ctx, action.clone());
    }
    match action {
        ChatWidgetAction::Send { text } => {
            if let Some(handler) = &handlers.on_send {
                handler(ctx, text);
            }
        }
        ChatWidgetAction::Copy { item_id, text } => {
            if let Some(handler) = &handlers.on_copy {
                handler(ctx, item_id, text);
            }
        }
        ChatWidgetAction::Retry { item_id } => {
            if let Some(handler) = &handlers.on_retry {
                handler(ctx, item_id);
            }
        }
        ChatWidgetAction::SetComposerDraft { .. }
        | ChatWidgetAction::SetComposerPermission { .. }
        | ChatWidgetAction::SetComposerModel { .. }
        | ChatWidgetAction::SetComposerReasoning { .. }
        | ChatWidgetAction::SetComposerHeight { .. }
        | ChatWidgetAction::StopTurn => {}
    }
}

fn message_background(style: &ChatWidgetStyle, message: &ChatMessage) -> Color {
    if message.kind == ChatMessageKind::Error || message.status == ChatMessageStatus::Failed {
        return style.error_background;
    }
    match message.role {
        ChatRole::User => style.user_background,
        ChatRole::Assistant => style.assistant_background,
        ChatRole::System | ChatRole::Tool => style.tool_background,
    }
}

fn role_label(role: ChatRole) -> &'static str {
    match role {
        ChatRole::System => "system",
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
        ChatRole::Tool => "tool",
    }
}

fn kind_label(kind: ChatMessageKind) -> &'static str {
    match kind {
        ChatMessageKind::Text => "text",
        ChatMessageKind::Reasoning => "reasoning",
        ChatMessageKind::Command => "command",
        ChatMessageKind::FileChange => "file",
        ChatMessageKind::ToolCall => "tool",
        ChatMessageKind::Status => "status",
        ChatMessageKind::Error => "error",
    }
}

fn status_suffix(status: ChatMessageStatus) -> &'static str {
    match status {
        ChatMessageStatus::Pending => " pending",
        ChatMessageStatus::Streaming => " streaming",
        ChatMessageStatus::Complete => "",
        ChatMessageStatus::Failed => " failed",
    }
}

fn session_status_label(status: ChatSessionStatus) -> &'static str {
    match status {
        ChatSessionStatus::Idle => "idle",
        ChatSessionStatus::Ready => "ready",
        ChatSessionStatus::Running => "running",
        ChatSessionStatus::Waiting => "waiting",
        ChatSessionStatus::Failed => "failed",
        ChatSessionStatus::Completed => "completed",
    }
}
