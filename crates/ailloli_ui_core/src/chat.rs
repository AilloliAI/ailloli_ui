//! Transport-neutral chat identifiers, snapshots, and reducer events.
//!
//! The types in this module contain no provider handles or I/O. They model an
//! ordered UI snapshot that a provider adapter can update through [`ChatEvent`].

use std::fmt;

use serde::{Deserialize, Serialize};

/// Stable identifier of one chat session.
///
/// Values are opaque strings. [`ChatSessionId::new`] performs no validation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ChatSessionId;
///
/// assert_eq!(ChatSessionId::from_index(7).as_str(), "chat_session_0000000000000007");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChatSessionId(String);

/// Stable identifier of one ordered message or status item.
///
/// Values are opaque strings. [`ChatItemId::new`] performs no validation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ChatItemId;
///
/// let id = ChatItemId::new("provider-item-42");
/// assert_eq!(id.as_str(), "provider-item-42");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChatItemId(String);

/// Correlation identifier for one provider request.
///
/// Values are opaque strings. [`ChatRequestId::new`] performs no validation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ChatRequestId;
///
/// assert_eq!(ChatRequestId::from_index(2).as_str(), "chat_request_0000000000000002");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChatRequestId(String);

/// Implements the shared opaque-string API for the three typed chat IDs.
macro_rules! impl_chat_id {
    ($ty:ident, $prefix:literal) => {
        impl $ty {
            /// Wraps an opaque identifier exactly as supplied.
            ///
            /// Empty strings and provider-specific formats are accepted.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::ChatSessionId;
            /// assert_eq!(ChatSessionId::new("provider-42").as_str(), "provider-42");
            /// ```
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Builds a deterministic ID from a zero-based or monotonic index.
            ///
            /// The representation is the type-specific prefix, an underscore,
            /// and exactly 16 lowercase hexadecimal digits.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::ChatSessionId;
            /// assert_eq!(ChatSessionId::from_index(1).as_str(), "chat_session_0000000000000001");
            /// ```
            pub fn from_index(index: u64) -> Self {
                Self(format!("{}_{:016x}", $prefix, index))
            }

            /// Returns the opaque identifier without allocation.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::ChatItemId;
            /// assert_eq!(ChatItemId::new("item-1").as_str(), "item-1");
            /// ```
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

impl_chat_id!(ChatSessionId, "chat_session");
impl_chat_id!(ChatItemId, "chat_item");
impl_chat_id!(ChatRequestId, "chat_request");

/// Semantic author of a [`ChatMessage`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ChatRole;
///
/// assert_eq!(ChatRole::default(), ChatRole::User);
/// let provider_output = ChatRole::Assistant;
/// assert_ne!(provider_output, ChatRole::Tool);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    /// Provider or application instructions outside the user/assistant exchange.
    System,
    /// Human-authored input; this is the default role.
    #[default]
    User,
    /// Model-authored output.
    Assistant,
    /// Output produced by, or sent back from, a tool.
    Tool,
}

/// Presentation category of a chat item.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ChatMessageKind;
///
/// assert_eq!(ChatMessageKind::default(), ChatMessageKind::Text);
/// let command = ChatMessageKind::Command;
/// assert_ne!(command, ChatMessageKind::FileChange);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatMessageKind {
    /// Ordinary natural-language content; this is the default.
    #[default]
    Text,
    /// Model reasoning or an equivalent provider explanation.
    Reasoning,
    /// A shell command or command execution record.
    Command,
    /// A proposed or completed file modification.
    FileChange,
    /// A tool invocation and its structured status.
    ToolCall,
    /// Non-message progress or lifecycle information.
    Status,
    /// Error content that should be presented as an item.
    Error,
}

/// Delivery lifecycle of one [`ChatMessage`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ChatMessageStatus;
///
/// assert_eq!(ChatMessageStatus::default(), ChatMessageStatus::Complete);
/// assert_ne!(ChatMessageStatus::Streaming, ChatMessageStatus::Pending);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatMessageStatus {
    /// The item is queued but provider processing has not begun.
    Pending,
    /// More content may be appended or replace the current item.
    Streaming,
    /// The item is final; this is the default.
    #[default]
    Complete,
    /// Production of this item failed.
    Failed,
}

/// Aggregate lifecycle of a [`ChatSessionState`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ChatSessionStatus;
///
/// assert_eq!(ChatSessionStatus::default(), ChatSessionStatus::Idle);
/// let waiting = ChatSessionStatus::Waiting;
/// assert_ne!(waiting, ChatSessionStatus::Running);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatSessionStatus {
    /// No session setup or request is currently active; this is the default.
    #[default]
    Idle,
    /// The session can accept a new request.
    Ready,
    /// A request or streamed response is in progress.
    Running,
    /// Progress is paused pending user, tool, or provider input.
    Waiting,
    /// The most recent session operation failed.
    Failed,
    /// The most recent request reached a terminal successful state.
    Completed,
}

/// One provider-neutral item in a chat's stable display order.
///
/// Fields are public to support serialization and snapshot construction. The
/// type does not enforce combinations of role, kind, request, and status.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{
///     ChatItemId, ChatMessage, ChatMessageStatus, ChatRequestId,
/// };
///
/// let message = ChatMessage::assistant(ChatItemId::from_index(1), "Hello")
///     .request_id(ChatRequestId::from_index(4))
///     .status(ChatMessageStatus::Streaming);
/// assert_eq!(message.text, "Hello");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Stable identity used to replace streamed updates in place.
    pub id: ChatItemId,
    /// Semantic author of the item.
    pub role: ChatRole,
    /// Presentation category of the item.
    pub kind: ChatMessageKind,
    /// Current UTF-8 content; an empty string is valid for a newly started stream.
    pub text: String,
    /// Correlated request, or `None` for uncorrelated history/status entries.
    pub request_id: Option<ChatRequestId>,
    /// Current delivery lifecycle; constructors default to [`ChatMessageStatus::Complete`].
    pub status: ChatMessageStatus,
}

impl ChatMessage {
    /// Creates a complete, uncorrelated chat item.
    ///
    /// Use [`Self::request_id`] and [`Self::status`] to override those defaults.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ChatItemId, ChatMessage, ChatMessageKind, ChatMessageStatus, ChatRole};
    /// let message = ChatMessage::new(ChatItemId::new("1"), ChatRole::System, ChatMessageKind::Status, "Ready");
    /// assert_eq!(message.status, ChatMessageStatus::Complete);
    /// ```
    pub fn new(
        id: ChatItemId,
        role: ChatRole,
        kind: ChatMessageKind,
        text: impl Into<String>,
    ) -> Self {
        Self {
            id,
            role,
            kind,
            text: text.into(),
            request_id: None,
            status: ChatMessageStatus::Complete,
        }
    }

    /// Creates a complete text item with [`ChatRole::User`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ChatItemId, ChatMessage, ChatRole};
    /// assert_eq!(ChatMessage::user(ChatItemId::new("1"), "Hello").role, ChatRole::User);
    /// ```
    pub fn user(id: ChatItemId, text: impl Into<String>) -> Self {
        Self::new(id, ChatRole::User, ChatMessageKind::Text, text)
    }

    /// Creates a complete text item with [`ChatRole::Assistant`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ChatItemId, ChatMessage, ChatRole};
    /// assert_eq!(ChatMessage::assistant(ChatItemId::new("1"), "Hello").role, ChatRole::Assistant);
    /// ```
    pub fn assistant(id: ChatItemId, text: impl Into<String>) -> Self {
        Self::new(id, ChatRole::Assistant, ChatMessageKind::Text, text)
    }

    /// Associates this message with `request_id`, replacing any previous ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ChatItemId, ChatMessage, ChatRequestId};
    /// let request = ChatRequestId::new("request-1");
    /// let message = ChatMessage::user(ChatItemId::new("1"), "Hello").request_id(request.clone());
    /// assert_eq!(message.request_id, Some(request));
    /// ```
    pub fn request_id(mut self, request_id: ChatRequestId) -> Self {
        self.request_id = Some(request_id);
        self
    }

    /// Sets the delivery status, replacing the constructor default.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ChatItemId, ChatMessage, ChatMessageStatus};
    /// let message = ChatMessage::assistant(ChatItemId::new("1"), "").status(ChatMessageStatus::Streaming);
    /// assert_eq!(message.status, ChatMessageStatus::Streaming);
    /// ```
    pub fn status(mut self, status: ChatMessageStatus) -> Self {
        self.status = status;
        self
    }
}

/// Serializable, ordered state of one chat session.
///
/// The public fields allow adapters to restore provider snapshots directly.
/// [`Self::apply_event`] provides the canonical incremental update semantics.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{
///     ChatEvent, ChatItemId, ChatSessionId, ChatSessionState,
/// };
///
/// let mut session = ChatSessionState::new(ChatSessionId::from_index(1), "Demo");
/// assert!(session.apply_event(ChatEvent::MessageAdded(
///     ailloli_ui_core::ChatMessage::user(ChatItemId::from_index(1), "Hi"),
/// )));
/// assert_eq!(session.summary().message_count, 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSessionState {
    /// Stable session identity.
    pub id: ChatSessionId,
    /// Consumer-facing title; an empty title is allowed.
    pub title: String,
    /// Aggregate lifecycle of the session.
    pub status: ChatSessionStatus,
    /// Items in insertion order; replacing an existing ID preserves its index.
    pub messages: Vec<ChatMessage>,
    /// In-flight provider request, or `None` when no request is tracked.
    pub active_request: Option<ChatRequestId>,
    /// Latest session-level failure text, or `None` when absent or cleared.
    pub last_error: Option<String>,
}

impl ChatSessionState {
    /// Creates an empty session in [`ChatSessionStatus::Ready`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ChatSessionId, ChatSessionState, ChatSessionStatus};
    /// let session = ChatSessionState::new(ChatSessionId::new("session-1"), "Demo");
    /// assert_eq!(session.status, ChatSessionStatus::Ready);
    /// ```
    pub fn new(id: ChatSessionId, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            status: ChatSessionStatus::Ready,
            messages: Vec::new(),
            active_request: None,
            last_error: None,
        }
    }

    /// Builds a compact owned summary of the current snapshot.
    ///
    /// The preview is `None` when there are no messages. Otherwise it contains
    /// at most 72 Unicode scalar values from the final message followed by
    /// `...` when truncated; an empty final message yields `Some("")`.
    ///
    /// # Performance
    ///
    /// Clones the ID and title and counts the final message text to determine
    /// whether truncation is required.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ChatSessionId, ChatSessionState};
    /// let summary = ChatSessionState::new(ChatSessionId::new("session-1"), "Demo").summary();
    /// assert_eq!(summary.message_count, 0);
    /// ```
    pub fn summary(&self) -> ChatSessionSummary {
        ChatSessionSummary {
            id: self.id.clone(),
            title: self.title.clone(),
            status: self.status,
            message_count: self.messages.len(),
            last_message_preview: self
                .messages
                .last()
                .map(|message| preview_text(&message.text, 72)),
        }
    }

    /// Returns an updated clone, leaving this snapshot unchanged.
    ///
    /// This has the same event semantics as [`Self::apply_event`].
    ///
    /// # Performance
    ///
    /// Clones the complete session before applying the event.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ChatEvent, ChatSessionId, ChatSessionState, ChatSessionStatus};
    /// let session = ChatSessionState::new(ChatSessionId::new("session-1"), "Demo");
    /// let failed = session.with_event(ChatEvent::Failed { message: "offline".into() });
    /// assert_eq!(failed.status, ChatSessionStatus::Failed);
    /// assert_eq!(session.status, ChatSessionStatus::Ready);
    /// ```
    pub fn with_event(&self, event: ChatEvent) -> Self {
        let mut next = self.clone();
        next.apply_event(event);
        next
    }

    /// Applies one reducer event and reports whether the snapshot changed.
    ///
    /// Message IDs are upserted: an existing item is replaced at its current
    /// index and a new item is appended. A delta for an unknown item creates a
    /// streaming assistant text item without a request ID. Finishing an
    /// assistant item marks the session completed and clears the active request
    /// even when that item is unknown. Finishing a request clears
    /// `active_request` only when IDs match, but always marks the session
    /// completed. [`ChatEvent::RequestStarted`] is the event that clears
    /// `last_error`; ordinary message events preserve it.
    ///
    /// # Performance
    ///
    /// Clones the complete pre-event state to compute the boolean result and
    /// performs a linear search through messages for each upsert or lookup.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{
    ///     ChatEvent, ChatItemId, ChatMessageKind, ChatRequestId,
    ///     ChatSessionId, ChatSessionState,
    /// };
    ///
    /// let mut chat = ChatSessionState::new(ChatSessionId::from_index(1), "Demo");
    /// let item = ChatItemId::from_index(1);
    /// chat.apply_event(ChatEvent::AssistantMessageStarted {
    ///     item_id: item.clone(),
    ///     request_id: ChatRequestId::from_index(1),
    ///     kind: ChatMessageKind::Text,
    /// });
    /// chat.apply_event(ChatEvent::AssistantMessageDelta {
    ///     item_id: item.clone(),
    ///     delta: "Hello".into(),
    /// });
    /// assert_eq!(chat.message(&item).unwrap().text, "Hello");
    /// ```
    pub fn apply_event(&mut self, event: ChatEvent) -> bool {
        let before = self.clone();
        match event {
            ChatEvent::MessageAdded(message) => {
                self.upsert_message(message);
                self.status = ChatSessionStatus::Ready;
            }
            ChatEvent::UserMessageSubmitted {
                item_id,
                request_id,
                text,
            } => {
                self.active_request = Some(request_id.clone());
                self.status = ChatSessionStatus::Running;
                self.upsert_message(ChatMessage::user(item_id, text).request_id(request_id));
            }
            ChatEvent::AssistantMessageStarted {
                item_id,
                request_id,
                kind,
            } => {
                self.active_request = Some(request_id.clone());
                self.status = ChatSessionStatus::Running;
                self.upsert_message(
                    ChatMessage::new(item_id, ChatRole::Assistant, kind, "")
                        .request_id(request_id)
                        .status(ChatMessageStatus::Streaming),
                );
            }
            ChatEvent::AssistantMessageDelta { item_id, delta } => {
                self.status = ChatSessionStatus::Running;
                let mut message = self.message(&item_id).cloned().unwrap_or_else(|| {
                    ChatMessage::new(
                        item_id.clone(),
                        ChatRole::Assistant,
                        ChatMessageKind::Text,
                        "",
                    )
                    .status(ChatMessageStatus::Streaming)
                });
                message.text.push_str(&delta);
                if message.status == ChatMessageStatus::Complete {
                    message.status = ChatMessageStatus::Streaming;
                }
                self.upsert_message(message);
            }
            ChatEvent::AssistantMessageFinished { item_id } => {
                if let Some(mut message) = self.message(&item_id).cloned() {
                    message.status = ChatMessageStatus::Complete;
                    self.upsert_message(message);
                }
                self.status = ChatSessionStatus::Completed;
                self.active_request = None;
            }
            ChatEvent::RequestStarted { request_id } => {
                self.active_request = Some(request_id);
                self.status = ChatSessionStatus::Running;
                self.last_error = None;
            }
            ChatEvent::RequestFinished { request_id } => {
                if self.active_request.as_ref() == Some(&request_id) {
                    self.active_request = None;
                }
                self.status = ChatSessionStatus::Completed;
            }
            ChatEvent::StatusChanged(status) => {
                self.status = status;
            }
            ChatEvent::Failed { message } => {
                self.status = ChatSessionStatus::Failed;
                self.last_error = Some(message);
                self.active_request = None;
            }
        }
        before != *self
    }

    /// Finds a message by ID without changing insertion order.
    ///
    /// Returns `None` if the ID has not been observed. Lookup is linear in the
    /// number of messages.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ChatEvent, ChatItemId, ChatMessage, ChatSessionId, ChatSessionState};
    /// let id = ChatItemId::new("item-1");
    /// let mut session = ChatSessionState::new(ChatSessionId::new("session-1"), "Demo");
    /// session.apply_event(ChatEvent::MessageAdded(ChatMessage::user(id.clone(), "Hello")));
    /// assert_eq!(session.message(&id).unwrap().text, "Hello");
    /// ```
    pub fn message(&self, item_id: &ChatItemId) -> Option<&ChatMessage> {
        self.messages.iter().find(|message| &message.id == item_id)
    }

    /// Replaces an existing ID in place or appends a previously unseen item.
    fn upsert_message(&mut self, message: ChatMessage) {
        if let Some(index) = self
            .messages
            .iter()
            .position(|existing| existing.id == message.id)
        {
            self.messages[index] = message;
        } else {
            self.messages.push(message);
        }
    }
}

/// Compact owned projection of a [`ChatSessionState`] for lists and navigation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{ChatSessionId, ChatSessionState, ChatSessionStatus};
///
/// let summary = ChatSessionState::new(ChatSessionId::from_index(1), "Demo").summary();
/// assert_eq!(summary.status, ChatSessionStatus::Ready);
/// assert_eq!(summary.last_message_preview, None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSessionSummary {
    /// Stable session identity.
    pub id: ChatSessionId,
    /// Consumer-facing session title.
    pub title: String,
    /// Aggregate session lifecycle.
    pub status: ChatSessionStatus,
    /// Number of ordered items in the source snapshot.
    pub message_count: usize,
    /// Truncated final-message text, or `None` when the session has no messages.
    pub last_message_preview: Option<String>,
}

/// Incremental change understood by [`ChatSessionState::apply_event`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{ChatEvent, ChatRequestId};
///
/// let event = ChatEvent::RequestStarted {
///     request_id: ChatRequestId::from_index(3),
/// };
/// assert!(matches!(event, ChatEvent::RequestStarted { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatEvent {
    /// Upserts a complete caller-constructed item and marks the session ready.
    MessageAdded(ChatMessage),
    /// Adds/replaces a user text item and starts the correlated request.
    UserMessageSubmitted {
        /// Stable ID of the user item.
        item_id: ChatItemId,
        /// Request that becomes active.
        request_id: ChatRequestId,
        /// Complete user-authored UTF-8 text.
        text: String,
    },
    /// Creates/replaces an empty streaming assistant item and starts its request.
    AssistantMessageStarted {
        /// Stable ID of the assistant item.
        item_id: ChatItemId,
        /// Request that becomes active and is attached to the item.
        request_id: ChatRequestId,
        /// Presentation category of the streamed item.
        kind: ChatMessageKind,
    },
    /// Appends a UTF-8 fragment to an assistant item, creating it if absent.
    AssistantMessageDelta {
        /// Stable ID of the assistant item.
        item_id: ChatItemId,
        /// Fragment appended exactly as supplied; an empty fragment is allowed.
        delta: String,
    },
    /// Marks an existing item complete and ends the active session request.
    AssistantMessageFinished {
        /// Item to finish; an unknown ID still completes the session.
        item_id: ChatItemId,
    },
    /// Starts tracking a request and clears the previous session error.
    RequestStarted {
        /// Request that becomes active.
        request_id: ChatRequestId,
    },
    /// Marks the session completed and conditionally clears a matching request.
    RequestFinished {
        /// Completed request; a non-matching ID leaves `active_request` intact.
        request_id: ChatRequestId,
    },
    /// Replaces only the aggregate session status.
    StatusChanged(ChatSessionStatus),
    /// Records a session-level failure and clears the active request.
    Failed {
        /// Human-readable provider or adapter error text.
        message: String,
    },
}

/// Truncates text at a Unicode scalar-value boundary and appends `...` if needed.
fn preview_text(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in text.chars().take(max_chars) {
        out.push(ch);
    }
    if text.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

#[cfg(test)]
mod tests {
    //! Covers typed ID serialization, streamed upserts, ordering, and previews.

    use super::*;

    #[test]
    fn chat_ids_are_typed_serializable_and_stable() {
        let session = ChatSessionId::from_index(7);
        let item = ChatItemId::new("item-x");
        let request = ChatRequestId::from_index(2);

        assert_eq!(session.as_str(), "chat_session_0000000000000007");
        assert_eq!(item.as_str(), "item-x");
        assert_eq!(request.as_str(), "chat_request_0000000000000002");

        let json = serde_json::to_string(&session).expect("serialize");
        assert_eq!(json, "\"chat_session_0000000000000007\"");
        let roundtrip: ChatSessionId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(roundtrip, session);
    }

    #[test]
    fn chat_streaming_events_accumulate_by_replacing_items() {
        let mut session = ChatSessionState::new(ChatSessionId::from_index(1), "Chat 1");
        let request = ChatRequestId::from_index(1);
        let item = ChatItemId::from_index(1);

        assert!(session.apply_event(ChatEvent::AssistantMessageStarted {
            item_id: item.clone(),
            request_id: request.clone(),
            kind: ChatMessageKind::Text,
        }));
        assert!(session.apply_event(ChatEvent::AssistantMessageDelta {
            item_id: item.clone(),
            delta: "Hel".into(),
        }));
        assert!(session.apply_event(ChatEvent::AssistantMessageDelta {
            item_id: item.clone(),
            delta: "lo".into(),
        }));
        assert!(session.apply_event(ChatEvent::AssistantMessageFinished {
            item_id: item.clone(),
        }));

        assert_eq!(session.messages.len(), 1);
        let message = session.message(&item).expect("streamed message");
        assert_eq!(message.text, "Hello");
        assert_eq!(message.status, ChatMessageStatus::Complete);
        assert_eq!(message.request_id.as_ref(), Some(&request));
        assert_eq!(session.status, ChatSessionStatus::Completed);
    }

    #[test]
    fn chat_summary_keeps_stable_order_and_preview() {
        let mut session = ChatSessionState::new(ChatSessionId::from_index(1), "Chat 1");
        session.apply_event(ChatEvent::MessageAdded(ChatMessage::user(
            ChatItemId::from_index(1),
            "first",
        )));
        session.apply_event(ChatEvent::MessageAdded(ChatMessage::assistant(
            ChatItemId::from_index(2),
            "second message",
        )));

        let summary = session.summary();
        assert_eq!(summary.message_count, 2);
        assert_eq!(
            summary.last_message_preview.as_deref(),
            Some("second message")
        );
        assert_eq!(session.messages[0].id, ChatItemId::from_index(1));
        assert_eq!(session.messages[1].id, ChatItemId::from_index(2));
    }
}
