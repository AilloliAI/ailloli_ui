//! Stateful chat transcript and resizable composer control.

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

/// Shared context-aware handler for every [`ChatWidgetAction`].
///
/// # Examples
///
/// ```
/// use std::rc::Rc;
/// use ailloli_ui_widgets::controls::ChatWidgetActionHandler;
/// let handler: ChatWidgetActionHandler<()> = Rc::new(|_ctx, _action| {});
/// let _ = handler;
/// ```
pub type ChatWidgetActionHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, ChatWidgetAction)>;
/// Shared context-aware handler for submitted composer text.
///
/// # Examples
///
/// ```
/// use std::rc::Rc;
/// use ailloli_ui_widgets::controls::ChatWidgetSendHandler;
/// let handler: ChatWidgetSendHandler<()> = Rc::new(|_ctx, text| assert!(!text.is_empty()));
/// let _ = handler;
/// ```
pub type ChatWidgetSendHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, String)>;
/// Shared context-aware handler for a message ID and copied text.
///
/// # Examples
///
/// ```
/// use std::rc::Rc;
/// use ailloli_ui_widgets::controls::ChatWidgetCopyHandler;
/// let handler: ChatWidgetCopyHandler<()> = Rc::new(|_ctx, _id, _text| {});
/// let _ = handler;
/// ```
pub type ChatWidgetCopyHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, ChatItemId, String)>;
/// Shared context-aware handler for a retryable message ID.
///
/// # Examples
///
/// ```
/// use std::rc::Rc;
/// use ailloli_ui_widgets::controls::ChatWidgetRetryHandler;
/// let handler: ChatWidgetRetryHandler<()> = Rc::new(|_ctx, _id| {});
/// let _ = handler;
/// ```
pub type ChatWidgetRetryHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, ChatItemId)>;
/// Shared renderer replacing the built-in message bubble for every message.
///
/// # Examples
///
/// ```
/// use std::rc::Rc;
/// use ailloli_ui_runtime::component::View;
/// use ailloli_ui_widgets::controls::ChatMessageRenderer;
/// let renderer: ChatMessageRenderer<()> = Rc::new(|_message| View::empty());
/// let _ = renderer;
/// ```
pub type ChatMessageRenderer<A> = Rc<dyn Fn(&ChatMessage) -> View<A>>;

#[derive(Debug, Clone, PartialEq)]
/// User intent emitted by the transcript and composer.
///
/// A general action handler runs before any specialized Send/Copy/Retry handler,
/// so configuring both intentionally produces two callbacks. Copy actions do not
/// write the clipboard themselves. `SetComposerHeight` expects a finite logical-
/// pixel value; NaN makes derived `PartialEq` non-reflexive despite the legacy
/// `Eq` marker on this enum.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ChatItemId;
/// use ailloli_ui_widgets::controls::ChatWidgetAction;
/// let actions = [
///     ChatWidgetAction::Send { text: "Hi".into() },
///     ChatWidgetAction::Copy { item_id: ChatItemId::new("1"), text: "Hi".into() },
///     ChatWidgetAction::Retry { item_id: ChatItemId::new("1") },
///     ChatWidgetAction::SetComposerDraft { text: "draft".into() },
///     ChatWidgetAction::SetComposerPermission { value: "read_only".into() },
///     ChatWidgetAction::SetComposerModel { model_id: None },
///     ChatWidgetAction::SetComposerReasoning { reasoning_level: None },
///     ChatWidgetAction::SetComposerHeight { height: 112.0 },
///     ChatWidgetAction::StopTurn,
/// ];
/// assert_eq!(actions.len(), 9);
/// ```
pub enum ChatWidgetAction {
    /// Submit non-blank composer text exactly as stored, without clearing it.
    Send {
        /// Exact UTF-8 composer contents forwarded to the owner.
        text: String,
    },
    /// Request copying one item's current text.
    Copy {
        /// Stable identifier of the item whose copy affordance was activated.
        item_id: ChatItemId,
        /// Exact UTF-8 text to place on the clipboard.
        text: String,
    },
    /// Request retry for an eligible item.
    Retry {
        /// Stable identifier of the item from which retry should resume.
        item_id: ChatItemId,
    },
    /// Notify that the already-bound composer draft changed.
    SetComposerDraft {
        /// Complete replacement UTF-8 draft, including any whitespace.
        text: String,
    },
    /// Select a permission option value.
    SetComposerPermission {
        /// Provider-neutral option value supplied by the composer model.
        value: String,
    },
    /// Select a model ID, or provider default with `None`.
    SetComposerModel {
        /// Selected opaque model ID, or `None` to request the provider default.
        model_id: Option<String>,
    },
    /// Select a reasoning level, or default with `None`.
    SetComposerReasoning {
        /// Selected opaque reasoning value, or `None` for the model default.
        reasoning_level: Option<String>,
    },
    /// Request composer height in logical pixels.
    SetComposerHeight {
        /// Requested composer height in logical pixels before owner-side policy.
        height: f32,
    },
    /// Request stopping the running turn.
    StopTurn,
}

impl Eq for ChatWidgetAction {}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One value/label pair shown in a composer select.
///
/// Values and labels are stored without validation; duplicates and empty strings
/// are allowed.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ChatComposerOption;
/// let option = ChatComposerOption::new("model-id", "Model name");
/// assert_eq!((option.value.as_str(), option.label.as_str()), ("model-id", "Model name"));
/// ```
pub struct ChatComposerOption {
    /// Value emitted when selected.
    pub value: String,
    /// Visible option label.
    pub label: String,
}

impl ChatComposerOption {
    /// Stores a value and label unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ChatComposerOption;
    /// let option = ChatComposerOption::new("fast", "Fast");
    /// assert_eq!(option.value, "fast");
    /// ```
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Composer select choices, availability, run state, and height bounds.
///
/// The defaults provide permission values `read_only`, `workspace_write`, and
/// `danger_full_access`; model/reasoning lists are empty, reasoning is disabled,
/// and the composer is not running.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ChatComposerControls;
/// let controls = ChatComposerControls::default();
/// assert_eq!(controls.selected_permission, "read_only");
/// assert_eq!(controls.permission_options.len(), 3);
/// assert_eq!((controls.height, controls.min_height, controls.max_height), (112.0, 72.0, 220.0));
/// ```
pub struct ChatComposerControls {
    /// Permission choices in display order.
    pub permission_options: Vec<ChatComposerOption>,
    /// Controlled permission value; it may be absent from the option list.
    pub selected_permission: String,
    /// Model choices in display order; provider default is added separately.
    pub model_options: Vec<ChatComposerOption>,
    /// Controlled model ID, or provider default with `None`.
    pub selected_model_id: Option<String>,
    /// Reasoning choices in display order; default is added separately.
    pub reasoning_options: Vec<ChatComposerOption>,
    /// Controlled reasoning value, or default with `None`.
    pub selected_reasoning_level: Option<String>,
    /// Whether permission selection is enabled when not running.
    pub permission_enabled: bool,
    /// Whether model selection is enabled when not running.
    pub model_enabled: bool,
    /// Whether reasoning selection is enabled when nonempty and not running.
    pub reasoning_enabled: bool,
    /// Whether the composer shows Stop and disables all selectors.
    pub running: bool,
    /// Requested composer height in logical pixels.
    pub height: f32,
    /// Inclusive minimum composer height in logical pixels.
    pub min_height: f32,
    /// Inclusive maximum composer height in logical pixels.
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
    /// Returns finite height clamped to inclusive bounds, else `112.0`.
    ///
    /// # Panics
    ///
    /// Panics when `height` is finite and either bound is NaN or `min_height` is
    /// greater than `max_height`, following [`f32::clamp`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ChatComposerControls;
    /// let controls = ChatComposerControls { height: 500.0, ..Default::default() };
    /// assert_eq!(controls.clamped_height(), 220.0);
    /// let controls = ChatComposerControls { height: f32::NAN, ..Default::default() };
    /// assert_eq!(controls.clamped_height(), 112.0);
    /// ```
    pub fn clamped_height(&self) -> f32 {
        if self.height.is_finite() {
            self.height.clamp(self.min_height, self.max_height)
        } else {
            112.0
        }
    }
}

#[derive(Clone, Debug)]
/// Colors, typography, radii, spacing, and preferred size of a [`ChatWidget`].
///
/// Numeric geometry is expressed in logical pixels. [`Self::from_theme`] uses a
/// `520 x 260` preferred size, 8-pixel outer padding/gap, a 6-pixel message gap,
/// and non-compact rendering.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ChatWidgetStyle;
/// let style = ChatWidgetStyle::default();
/// assert_eq!((style.width, style.height), (520.0, 260.0));
/// assert!(!style.compact);
/// ```
pub struct ChatWidgetStyle {
    /// Root background color.
    pub background: Color,
    /// Root border widths and colors.
    pub border: Border,
    /// Header and inline-action background color.
    pub header_background: Color,
    /// Background used for user messages.
    pub user_background: Color,
    /// Background used for assistant messages.
    pub assistant_background: Color,
    /// Background used for system and tool messages.
    pub tool_background: Color,
    /// Background used for error-kind or failed messages.
    pub error_background: Color,
    /// Message-body and header text style.
    pub text: TextStyle,
    /// Empty-state and inline-action text style.
    pub muted: TextStyle,
    /// Message role, kind, and status text style.
    pub role_text: TextStyle,
    /// Composer input and container style.
    pub input: TextInputStyle,
    /// Root corner radii in logical pixels.
    pub radius: Radius,
    /// Header corner radius in logical pixels.
    pub header_radius: f32,
    /// Message-bubble corner radius in logical pixels.
    pub message_radius: f32,
    /// Composer and inline-action corner radius in logical pixels.
    pub inline_button_radius: f32,
    /// Root inner padding in logical pixels.
    pub padding: f32,
    /// Vertical gap between header, transcript, and composer in logical pixels.
    pub gap: f32,
    /// Vertical gap between message bubbles in logical pixels.
    pub message_gap: f32,
    /// Preferred widget height in logical pixels.
    pub height: f32,
    /// Preferred widget width in logical pixels.
    pub width: f32,
    /// Whether compact header, bubble padding, and density are rendered.
    pub compact: bool,
}

impl Default for ChatWidgetStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl ChatWidgetStyle {
    /// Creates the default chat style from `theme`.
    ///
    /// Palette-derived text and border colors coexist with deliberately dark
    /// fixed transcript surfaces. No dimension is scaled or validated.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::ChatWidgetStyle;
    /// let style = ChatWidgetStyle::from_theme(Theme::default());
    /// assert_eq!(style.padding, 8.0);
    /// assert_eq!(style.message_gap, 6.0);
    /// ```
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

    /// Applies the predefined compact density and preferred height.
    ///
    /// This sets `compact`, padding, both gaps, and height, while preserving the
    /// width and all colors. By contrast, [`ChatWidget::compact`] changes only
    /// the rendering flag on an already configured widget style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ChatWidgetStyle;
    /// let style = ChatWidgetStyle::default().compact();
    /// assert_eq!((style.padding, style.gap, style.message_gap), (6.0, 6.0, 4.0));
    /// assert_eq!((style.width, style.height, style.compact), (520.0, 190.0, true));
    /// ```
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self.padding = 6.0;
        self.gap = 6.0;
        self.message_gap = 4.0;
        self.height = 190.0;
        self
    }
}

/// Stateful transcript with message actions and a controlled composer.
///
/// `A` is the application action returned by non-context callbacks. The draft
/// is always signal-backed. Use [`Self::bind_session`] when session changes must
/// remain observable after construction; [`Self::new`] stores its session value
/// in private state.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{ChatSessionId, ChatSessionState};
/// use ailloli_ui_runtime::component::State;
/// use ailloli_ui_widgets::controls::ChatWidget;
/// let session = ChatSessionState::new(ChatSessionId::new("demo"), "Demo");
/// let widget: ChatWidget<()> = ChatWidget::new(Some(session), State::new(String::new()));
/// let _ = widget;
/// ```
pub struct ChatWidget<A = ()> {
    /// Root layout, updated by layout builders and [`Self::chat_style`].
    pub(crate) layout: LayoutStyle,
    /// Flex-parent participation configured by generated layout builders.
    pub(crate) flex_item: FlexItemStyle,
    /// Optional live chat session.
    session: Signal<Option<ChatSessionState>>,
    /// Writable composer draft.
    draft: Signal<String>,
    /// Visual configuration.
    style: ChatWidgetStyle,
    /// General callback invoked for every action.
    on_action: Option<ChatWidgetActionHandler<A>>,
    /// Specialized Send callback.
    on_send: Option<ChatWidgetSendHandler<A>>,
    /// Specialized Copy callback.
    on_copy: Option<ChatWidgetCopyHandler<A>>,
    /// Specialized Retry callback.
    on_retry: Option<ChatWidgetRetryHandler<A>>,
    /// Optional replacement renderer for all transcript items.
    message_renderer: Option<ChatMessageRenderer<A>>,
    /// Optional view inserted above the resize bar.
    composer_accessory: Option<View<A>>,
    /// Composer state and options.
    composer_controls: ChatComposerControls,
    /// Whether the transcript scroll follows its end.
    follow_latest: bool,
}

crate::impl_layout_builders!(ChatWidget);

impl<A: 'static> ChatWidget<A> {
    /// Creates a widget from a session snapshot and writable draft signal.
    ///
    /// The snapshot is moved into private state and cannot subsequently be
    /// replaced by the caller. Use [`Self::bind_session`] for a live session.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::ChatWidget;
    /// let widget: ChatWidget<()> = ChatWidget::new(None, State::new("draft".to_string()));
    /// let _ = widget;
    /// ```
    pub fn new(session: Option<ChatSessionState>, draft: impl Into<Signal<String>>) -> Self {
        Self::bind_session(State::new(session).into_signal(), draft)
    }

    /// Creates a widget backed by live session and draft signals.
    ///
    /// `None` renders the no-session state; `Some` with no messages renders the
    /// empty-session state. The draft signal is updated by the text input before
    /// `SetComposerDraft` is emitted.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ChatSessionState;
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::ChatWidget;
    /// let session = State::new(None::<ChatSessionState>);
    /// let draft = State::new(String::new());
    /// let widget: ChatWidget<()> = ChatWidget::bind_session(session, draft);
    /// let _ = widget;
    /// ```
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

    /// Replaces the chat style and resets preferred layout width and height.
    ///
    /// Layout builders called after this method override those two dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::{ChatWidget, ChatWidgetStyle};
    /// let style = ChatWidgetStyle { width: 640.0, height: 320.0, ..Default::default() };
    /// let widget: ChatWidget<()> = ChatWidget::new(None, State::new(String::new())).chat_style(style);
    /// let _ = widget;
    /// ```
    pub fn chat_style(mut self, style: ChatWidgetStyle) -> Self {
        self.layout = self.layout.width(style.width).height(style.height);
        self.style = style;
        self
    }

    /// Enables or disables compact rendering without changing style geometry.
    ///
    /// Use [`ChatWidgetStyle::compact`] through [`Self::chat_style`] to also apply
    /// the compact padding, gaps, and 190-pixel preferred height.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::ChatWidget;
    /// let widget: ChatWidget<()> = ChatWidget::new(None, State::new(String::new())).compact(true);
    /// let _ = widget;
    /// ```
    pub fn compact(mut self, compact: bool) -> Self {
        self.style.compact = compact;
        self
    }

    /// Dispatches the application action returned for every widget action.
    ///
    /// For Send, Copy, and Retry this callback runs before the corresponding
    /// specialized callback when both are configured.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::ChatWidget;
    /// let widget = ChatWidget::new(None, State::new(String::new())).on_action(|_action| ());
    /// let _ = widget;
    /// ```
    pub fn on_action(mut self, f: impl Fn(ChatWidgetAction) -> A + 'static) -> Self {
        self.on_action = Some(Rc::new(move |ctx, action| ctx.dispatch(f(action))));
        self
    }

    /// Handles every widget action with direct event-context access.
    ///
    /// The handler may dispatch zero or multiple application actions and runs
    /// before specialized Send, Copy, or Retry handlers.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::ChatWidget;
    /// let widget: ChatWidget<()> = ChatWidget::new(None, State::new(String::new()))
    ///     .on_action_ctx(|_ctx, _action| {});
    /// let _ = widget;
    /// ```
    pub fn on_action_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, ChatWidgetAction) + 'static,
    ) -> Self {
        self.on_action = Some(Rc::new(f));
        self
    }

    /// Dispatches the application action returned for submitted text.
    ///
    /// Blank-after-trimming drafts are not submitted. Non-blank text is passed
    /// unchanged, including surrounding whitespace, and the draft is not cleared.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::ChatWidget;
    /// let widget = ChatWidget::new(None, State::new("Hello".to_string())).on_send(|text| {
    ///     assert_eq!(text, "Hello");
    /// });
    /// let _ = widget;
    /// ```
    pub fn on_send(mut self, f: impl Fn(String) -> A + 'static) -> Self {
        self.on_send = Some(Rc::new(move |ctx, text| ctx.dispatch(f(text))));
        self
    }

    /// Handles submitted text with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::ChatWidget;
    /// let widget: ChatWidget<()> = ChatWidget::new(None, State::new(String::new()))
    ///     .on_send_ctx(|_ctx, _text| {});
    /// let _ = widget;
    /// ```
    pub fn on_send_ctx(mut self, f: impl Fn(&mut EventCtx<A>, String) + 'static) -> Self {
        self.on_send = Some(Rc::new(f));
        self
    }

    /// Dispatches the application action returned for a copy request.
    ///
    /// The widget supplies the message ID and its current text but performs no
    /// clipboard write.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::ChatWidget;
    /// let widget = ChatWidget::new(None, State::new(String::new()))
    ///     .on_copy(|_item_id, _text| ());
    /// let _ = widget;
    /// ```
    pub fn on_copy(mut self, f: impl Fn(ChatItemId, String) -> A + 'static) -> Self {
        self.on_copy = Some(Rc::new(move |ctx, item_id, text| {
            ctx.dispatch(f(item_id, text))
        }));
        self
    }

    /// Handles a copy request with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::ChatWidget;
    /// let widget: ChatWidget<()> = ChatWidget::new(None, State::new(String::new()))
    ///     .on_copy_ctx(|_ctx, _item_id, _text| {});
    /// let _ = widget;
    /// ```
    pub fn on_copy_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, ChatItemId, String) + 'static,
    ) -> Self {
        self.on_copy = Some(Rc::new(f));
        self
    }

    /// Dispatches the application action returned for a retry request.
    ///
    /// User messages are retryable. Assistant messages are retryable only when
    /// they carry a request ID; system and tool messages are never retryable.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::ChatWidget;
    /// let widget = ChatWidget::new(None, State::new(String::new())).on_retry(|_item_id| ());
    /// let _ = widget;
    /// ```
    pub fn on_retry(mut self, f: impl Fn(ChatItemId) -> A + 'static) -> Self {
        self.on_retry = Some(Rc::new(move |ctx, item_id| ctx.dispatch(f(item_id))));
        self
    }

    /// Handles a retry request with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::ChatWidget;
    /// let widget: ChatWidget<()> = ChatWidget::new(None, State::new(String::new()))
    ///     .on_retry_ctx(|_ctx, _item_id| {});
    /// let _ = widget;
    /// ```
    pub fn on_retry_ctx(mut self, f: impl Fn(&mut EventCtx<A>, ChatItemId) + 'static) -> Self {
        self.on_retry = Some(Rc::new(f));
        self
    }

    /// Replaces the complete built-in bubble for every message.
    ///
    /// A custom view is responsible for body, metadata, and any Copy/Retry UI;
    /// it also bypasses the optimized retained transcript renderer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::{State, View};
    /// use ailloli_ui_widgets::controls::ChatWidget;
    /// let widget: ChatWidget<()> = ChatWidget::new(None, State::new(String::new()))
    ///     .message_renderer(|_message| View::empty());
    /// let _ = widget;
    /// ```
    pub fn message_renderer(mut self, f: impl Fn(&ChatMessage) -> View<A> + 'static) -> Self {
        self.message_renderer = Some(Rc::new(f));
        self
    }

    /// Inserts a prebuilt view above the composer resize bar.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::{State, View};
    /// use ailloli_ui_widgets::controls::ChatWidget;
    /// let widget: ChatWidget<()> = ChatWidget::new(None, State::new(String::new()))
    ///     .composer_accessory(View::empty());
    /// let _ = widget;
    /// ```
    pub fn composer_accessory(mut self, accessory: View<A>) -> Self {
        self.composer_accessory = Some(accessory);
        self
    }

    /// Replaces permission/model/reasoning choices, enabled state, and height.
    ///
    /// The values are controlled: emitted actions do not mutate this snapshot.
    /// While `running`, selectors are disabled and the turn button emits Stop.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::{ChatComposerControls, ChatWidget};
    /// let controls = ChatComposerControls { running: true, ..Default::default() };
    /// let widget: ChatWidget<()> = ChatWidget::new(None, State::new(String::new()))
    ///     .composer_controls(controls);
    /// let _ = widget;
    /// ```
    pub fn composer_controls(mut self, controls: ChatComposerControls) -> Self {
        self.composer_controls = controls;
        self
    }

    /// Sets whether the vertical transcript scroll follows its end.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::ChatWidget;
    /// let widget: ChatWidget<()> = ChatWidget::new(None, State::new(String::new()))
    ///     .follow_latest(false);
    /// let _ = widget;
    /// ```
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

/// Reactive component that materializes the configured chat view tree.
struct ChatWidgetComponent<A> {
    /// Root layout copied from the public builder.
    layout: LayoutStyle,
    /// Live session source.
    session: Signal<Option<ChatSessionState>>,
    /// Writable composer draft.
    draft: Signal<String>,
    /// Visual configuration shared by child builders.
    style: ChatWidgetStyle,
    /// General action handler.
    on_action: Option<ChatWidgetActionHandler<A>>,
    /// Specialized Send handler.
    on_send: Option<ChatWidgetSendHandler<A>>,
    /// Specialized Copy handler.
    on_copy: Option<ChatWidgetCopyHandler<A>>,
    /// Specialized Retry handler.
    on_retry: Option<ChatWidgetRetryHandler<A>>,
    /// Optional replacement message renderer.
    message_renderer: Option<ChatMessageRenderer<A>>,
    /// Optional view placed above the resize control.
    composer_accessory: Option<View<A>>,
    /// Controlled composer settings.
    composer_controls: ChatComposerControls,
    /// Transcript follow-end setting.
    follow_latest: bool,
}

/// Cheaply cloned callback bundle shared by generated child controls.
struct ChatWidgetHandlers<A> {
    /// General handler invoked first.
    on_action: Option<ChatWidgetActionHandler<A>>,
    /// Specialized Send handler.
    on_send: Option<ChatWidgetSendHandler<A>>,
    /// Specialized Copy handler.
    on_copy: Option<ChatWidgetCopyHandler<A>>,
    /// Specialized Retry handler.
    on_retry: Option<ChatWidgetRetryHandler<A>>,
}

impl<A> Clone for ChatWidgetHandlers<A> {
    /// Clones only the reference-counted callback handles.
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
    /// Builds the header, transcript, optional accessory, resize bar, and composer.
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
    /// Takes cheap clones of the configured callback handles.
    fn handlers(&self) -> ChatWidgetHandlers<A> {
        ChatWidgetHandlers {
            on_action: self.on_action.clone(),
            on_send: self.on_send.clone(),
            on_copy: self.on_copy.clone(),
            on_retry: self.on_retry.clone(),
        }
    }

    /// Builds the reactive `title - status - count msg` header.
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

    /// Builds either the optimized transcript leaf or custom-renderer column.
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

    /// Builds one non-optimized message bubble with eligible action buttons.
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

    /// Builds the horizontal drag target and emits unclamped requested heights.
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

    /// Builds controlled selects, text input, and Send/Stop turn button.
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

/// Retained transcript leaf that caches measured text and hit geometry.
struct ChatMessagesWidget<A> {
    /// Size behavior supplied to the runtime layout engine.
    layout: LayoutStyle,
    /// Live session whose message sequence is rendered.
    session: Signal<Option<ChatSessionState>>,
    /// Message styles and geometry constants.
    style: ChatWidgetStyle,
    /// Copy and Retry action destinations.
    handlers: ChatWidgetHandlers<A>,
    /// Last measured geometry, shared across layout, paint, and events.
    geometry: Rc<RefCell<ChatMessagesGeometry>>,
}

#[derive(Clone, Default)]
/// Cached geometry for one transcript width and exact message identity/content.
struct ChatMessagesGeometry {
    /// Measured available width in logical pixels.
    width: f32,
    /// Per-message measurement and action-hit geometry in session order.
    items: Vec<ChatMessageGeometry>,
    /// Sum of message heights and inter-message gaps in logical pixels.
    total_height: f32,
}

#[derive(Clone)]
/// Cached text layouts and local rectangles for a single message.
struct ChatMessageGeometry {
    /// Message identity used for cache validation.
    item_id: ChatItemId,
    /// Rendered role/kind/status string used for cache validation.
    role_text: String,
    /// Rendered body or empty-body sentinel used for cache validation.
    body_text: String,
    /// Top offset in transcript-local logical pixels.
    y: f32,
    /// Full bubble height in logical pixels.
    height: f32,
    /// Role line height in logical pixels.
    role_height: f32,
    /// Body text height in logical pixels.
    body_height: f32,
    /// Prepared role text retained for painting.
    role_layout: Arc<PreparedTextLayout>,
    /// Prepared body text retained for painting.
    body_layout: Arc<PreparedTextLayout>,
    /// Transcript-local Copy hit rectangle.
    copy_rect: Rect,
    /// Transcript-local Retry hit rectangle when the message is eligible.
    retry_rect: Option<Rect>,
}

impl<A: 'static> Widget<A> for ChatMessagesWidget<A> {
    /// Returns the stable diagnostic widget name.
    fn debug_name(&self) -> &'static str {
        "ChatMessages"
    }

    /// Measures the transcript, caches geometry, and clamps its runtime size.
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

    /// Paints cached messages or the appropriate empty-state text.
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

    /// Handles only pressed primary-pointer events within cached action rectangles.
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
    /// Returns matching cached geometry or remeasures it for the paint context.
    ///
    /// `None` means the remeasured data still cannot be proven to match the
    /// current session and width, so painting must be skipped for this frame.
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

    /// Hit-tests absolute coordinates against validated local Copy/Retry regions.
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

/// Paints one bubble from premeasured geometry and returns its height.
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

/// Paints a single unwrapped line when a text system is available.
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

/// Paints a retained transcript action surface and vertically centered label.
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

/// Creates a cached layout with a nonnegative maximum width.
///
/// Returns `None` when the paint context has no text system.
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

/// Emits a text draw command positioned from the first-line baseline.
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
        decoration: ailloli_ui_core::TextDecoration::None,
        layout,
    }));
}

/// Returns measured height or a `1.25 * font size` logical-pixel fallback.
fn text_layout_height(layout: Option<&Arc<PreparedTextLayout>>, style: TextStyle) -> f32 {
    layout
        .map(|layout| layout.metrics.height)
        .unwrap_or(style.px_size as f32 * 1.25)
}

impl ChatMessagesGeometry {
    /// Checks width tolerance, item count, IDs, and rendered text cache keys.
    ///
    /// Widths within 0.5 logical pixels are treated as equivalent.
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

/// Measures all transcript geometry with the supplied or a temporary text system.
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

/// Measures messages in order and inserts `message_gap` before every item but the first.
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

/// Measures one message's text, bubble height, and local action rectangles.
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

/// Translates a transcript-local rectangle to paint-space coordinates.
fn rect_to_absolute(rect: Rect, x: f32, y: f32) -> Rect {
    Rect::new(x + rect.x, y + rect.y, rect.w, rect.h)
}

/// Formats the lowercase role, kind, and non-complete status suffix.
fn message_role_text(message: &ChatMessage) -> String {
    format!(
        "{} / {}{}",
        role_label(message.role),
        kind_label(message.kind),
        status_suffix(message.status)
    )
}

/// Returns message text or the visible `...` sentinel for an empty body.
fn message_body_text(message: &ChatMessage) -> String {
    if message.text.is_empty() {
        "...".to_string()
    } else {
        message.text.clone()
    }
}

/// Reports whether Retry is available for the message's role and request ID.
fn message_is_retryable(message: &ChatMessage) -> bool {
    match message.role {
        ChatRole::User => true,
        ChatRole::Assistant => message.request_id.is_some(),
        ChatRole::System | ChatRole::Tool => false,
    }
}

/// Returns 6 compact or 8 regular logical pixels of bubble padding.
fn message_padding(style: &ChatWidgetStyle) -> f32 {
    if style.compact {
        6.0
    } else {
        8.0
    }
}

/// Returns the retained action hit height in logical pixels.
fn action_button_height() -> f32 {
    22.0
}

/// Builds a tree-based ghost action button for custom-renderer fallback paths.
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

/// Invokes the general handler first, then the matching specialized handler.
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

/// Chooses error background first, otherwise maps the message role to a surface.
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

/// Returns the stable lowercase role label.
fn role_label(role: ChatRole) -> &'static str {
    match role {
        ChatRole::System => "system",
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
        ChatRole::Tool => "tool",
    }
}

/// Returns the stable lowercase message-kind label.
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

/// Returns an optional leading-space status suffix; complete has none.
fn status_suffix(status: ChatMessageStatus) -> &'static str {
    match status {
        ChatMessageStatus::Pending => " pending",
        ChatMessageStatus::Streaming => " streaming",
        ChatMessageStatus::Complete => "",
        ChatMessageStatus::Failed => " failed",
    }
}

/// Returns the stable lowercase session-status label.
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
