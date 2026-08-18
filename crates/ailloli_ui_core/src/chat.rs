use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChatSessionId(String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChatItemId(String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChatRequestId(String);

macro_rules! impl_chat_id {
    ($ty:ident, $prefix:literal) => {
        impl $ty {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn from_index(index: u64) -> Self {
                Self(format!("{}_{:016x}", $prefix, index))
            }

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    System,
    #[default]
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatMessageKind {
    #[default]
    Text,
    Reasoning,
    Command,
    FileChange,
    ToolCall,
    Status,
    Error,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatMessageStatus {
    Pending,
    Streaming,
    #[default]
    Complete,
    Failed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatSessionStatus {
    #[default]
    Idle,
    Ready,
    Running,
    Waiting,
    Failed,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: ChatItemId,
    pub role: ChatRole,
    pub kind: ChatMessageKind,
    pub text: String,
    pub request_id: Option<ChatRequestId>,
    pub status: ChatMessageStatus,
}

impl ChatMessage {
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

    pub fn user(id: ChatItemId, text: impl Into<String>) -> Self {
        Self::new(id, ChatRole::User, ChatMessageKind::Text, text)
    }

    pub fn assistant(id: ChatItemId, text: impl Into<String>) -> Self {
        Self::new(id, ChatRole::Assistant, ChatMessageKind::Text, text)
    }

    pub fn request_id(mut self, request_id: ChatRequestId) -> Self {
        self.request_id = Some(request_id);
        self
    }

    pub fn status(mut self, status: ChatMessageStatus) -> Self {
        self.status = status;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSessionState {
    pub id: ChatSessionId,
    pub title: String,
    pub status: ChatSessionStatus,
    pub messages: Vec<ChatMessage>,
    pub active_request: Option<ChatRequestId>,
    pub last_error: Option<String>,
}

impl ChatSessionState {
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

    pub fn with_event(&self, event: ChatEvent) -> Self {
        let mut next = self.clone();
        next.apply_event(event);
        next
    }

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

    pub fn message(&self, item_id: &ChatItemId) -> Option<&ChatMessage> {
        self.messages.iter().find(|message| &message.id == item_id)
    }

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSessionSummary {
    pub id: ChatSessionId,
    pub title: String,
    pub status: ChatSessionStatus,
    pub message_count: usize,
    pub last_message_preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatEvent {
    MessageAdded(ChatMessage),
    UserMessageSubmitted {
        item_id: ChatItemId,
        request_id: ChatRequestId,
        text: String,
    },
    AssistantMessageStarted {
        item_id: ChatItemId,
        request_id: ChatRequestId,
        kind: ChatMessageKind,
    },
    AssistantMessageDelta {
        item_id: ChatItemId,
        delta: String,
    },
    AssistantMessageFinished {
        item_id: ChatItemId,
    },
    RequestStarted {
        request_id: ChatRequestId,
    },
    RequestFinished {
        request_id: ChatRequestId,
    },
    StatusChanged(ChatSessionStatus),
    Failed {
        message: String,
    },
}

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
