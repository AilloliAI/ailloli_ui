use std::time::Duration;

use ailloli_ui_core::event::{Event, PointerSample};
use ailloli_ui_core::LogicalWindowId;

use crate::app::PresentationGeneration;

/// Monotonic event identifier assigned by one UI host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventId(u64);

impl EventId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Monotonic duration since the host's documented time origin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventTimestamp(Duration);

impl EventTimestamp {
    pub const fn new(value: Duration) -> Self {
        Self(value)
    }

    pub const fn duration(self) -> Duration {
        self.0
    }
}

/// Provider-neutral metadata attached to one routed event.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct EventMeta {
    id: EventId,
    timestamp: EventTimestamp,
    logical_window_id: LogicalWindowId,
    presentation_generation: PresentationGeneration,
    pointer: Option<PointerSample>,
}

impl EventMeta {
    pub fn new(
        id: EventId,
        timestamp: EventTimestamp,
        logical_window_id: impl Into<LogicalWindowId>,
        presentation_generation: PresentationGeneration,
    ) -> Self {
        Self {
            id,
            timestamp,
            logical_window_id: logical_window_id.into(),
            presentation_generation,
            pointer: None,
        }
    }

    pub const fn id(&self) -> EventId {
        self.id
    }

    pub const fn timestamp(&self) -> EventTimestamp {
        self.timestamp
    }

    pub fn logical_window_id(&self) -> &LogicalWindowId {
        &self.logical_window_id
    }

    pub const fn presentation_generation(&self) -> PresentationGeneration {
        self.presentation_generation
    }

    pub const fn pointer(&self) -> Option<&PointerSample> {
        self.pointer.as_ref()
    }

    /// Returns the primary classification carried by pointer metadata.
    ///
    /// Non-pointer events and legacy direct dispatches have no pointer sample
    /// and therefore return `None` rather than inventing a classification.
    pub const fn pointer_is_primary(&self) -> Option<bool> {
        match self.pointer.as_ref() {
            Some(pointer) => Some(pointer.is_primary()),
            None => None,
        }
    }

    pub const fn with_pointer(mut self, pointer: PointerSample) -> Self {
        self.pointer = Some(pointer);
        self
    }
}

/// Runtime event together with host correlation and presentation metadata.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct EventEnvelope {
    meta: EventMeta,
    event: Event,
}

impl EventEnvelope {
    pub const fn new(meta: EventMeta, event: Event) -> Self {
        Self { meta, event }
    }

    pub const fn meta(&self) -> &EventMeta {
        &self.meta
    }

    pub const fn event(&self) -> &Event {
        &self.event
    }

    /// Pointer sample carried by this envelope, when the event has one.
    pub const fn pointer(&self) -> Option<&PointerSample> {
        self.meta.pointer()
    }

    /// Returns the envelope's explicit primary-pointer classification.
    pub const fn pointer_is_primary(&self) -> Option<bool> {
        self.meta.pointer_is_primary()
    }

    pub fn into_parts(self) -> (EventMeta, Event) {
        (self.meta, self.event)
    }
}
