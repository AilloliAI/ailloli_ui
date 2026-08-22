//! Host correlation metadata wrapped around provider-neutral runtime events.

use std::time::Duration;

use ailloli_ui_core::event::{Event, PointerSample};
use ailloli_ui_core::LogicalWindowId;

use crate::app::PresentationGeneration;

/// Monotonic event identifier assigned by one UI host.
///
/// The value type does not allocate IDs or enforce ordering/uniqueness. Zero is
/// the default and has no additional meaning unless a host defines one.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::input::EventId;
/// assert_eq!(EventId::default().get(), 0);
/// assert!(EventId::new(2) > EventId::new(1));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventId(u64);

/// Provides the operations defined for EventId.
impl EventId {
    /// Wraps an explicit host-local event ID without validation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::EventId;
    /// assert_eq!(EventId::new(u64::MAX).get(), u64::MAX);
    /// ```
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying host-local integer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::EventId;
    /// assert_eq!(EventId::new(42).get(), 42);
    /// ```
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Monotonic duration since the host's documented time origin.
///
/// This is not a wall-clock timestamp and cannot represent negative durations.
/// Hosts sharing envelopes must agree on the origin; the type does not enforce
/// monotonic event order.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use ailloli_ui_runtime::input::EventTimestamp;
/// assert_eq!(EventTimestamp::default().duration(), Duration::ZERO);
/// assert!(EventTimestamp::new(Duration::from_millis(2)) > EventTimestamp::new(Duration::from_millis(1)));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventTimestamp(Duration);

/// Provides the operations defined for EventTimestamp.
impl EventTimestamp {
    /// Wraps a duration since the host-defined origin.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_runtime::input::EventTimestamp;
    /// assert_eq!(EventTimestamp::new(Duration::from_micros(7)).duration().as_micros(), 7);
    /// ```
    pub const fn new(value: Duration) -> Self {
        Self(value)
    }

    /// Returns the exact stored duration.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_runtime::input::EventTimestamp;
    /// let value = Duration::from_secs(3);
    /// assert_eq!(EventTimestamp::new(value).duration(), value);
    /// ```
    pub const fn duration(self) -> Duration {
        self.0
    }
}

/// Provider-neutral metadata attached to one routed event.
///
/// The metadata correlates one host-local ID and monotonic timestamp with a
/// logical window and native presentation generation. Optional pointer data is
/// explicit and is not inferred from the event variant. The struct is
/// non-exhaustive and its fields remain constructor/accessor controlled.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventId, EventMeta, EventTimestamp}};
/// let meta = EventMeta::new(EventId::new(1), EventTimestamp::new(Duration::from_millis(5)), "main", PresentationGeneration::new(2));
/// assert_eq!(meta.id().get(), 1);
/// assert_eq!(meta.logical_window_id().as_str(), "main");
/// assert_eq!(meta.pointer(), None);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct EventMeta {
    /// Host-local event correlation ID.
    id: EventId,
    /// Duration since the host-defined time origin.
    timestamp: EventTimestamp,
    /// Stable logical-window identity.
    logical_window_id: LogicalWindowId,
    /// Native presentation generation that produced the event.
    presentation_generation: PresentationGeneration,
    /// Optional complete pointer sample.
    pointer: Option<PointerSample>,
}

/// Provides the operations defined for EventMeta.
impl EventMeta {
    /// Creates metadata without a pointer sample.
    ///
    /// IDs, timestamp order, logical-window existence, and presentation liveness
    /// are not validated here. Attach pointer data with [`Self::with_pointer`].
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventId, EventMeta, EventTimestamp}};
    /// let meta = EventMeta::new(EventId::new(9), EventTimestamp::new(Duration::ZERO), "editor", PresentationGeneration::INITIAL);
    /// assert_eq!(meta.id(), EventId::new(9));
    /// assert!(meta.pointer().is_none());
    /// ```
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

    /// Returns the host-local event identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventId, EventMeta, EventTimestamp}};
    /// let meta = EventMeta::new(EventId::new(3), EventTimestamp::new(Duration::ZERO), "main", PresentationGeneration::INITIAL);
    /// assert_eq!(meta.id().get(), 3);
    /// ```
    pub const fn id(&self) -> EventId {
        self.id
    }

    /// Returns the duration since the host-defined origin.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventId, EventMeta, EventTimestamp}};
    /// let timestamp = EventTimestamp::new(Duration::from_millis(12));
    /// let meta = EventMeta::new(EventId::new(1), timestamp, "main", PresentationGeneration::INITIAL);
    /// assert_eq!(meta.timestamp(), timestamp);
    /// ```
    pub const fn timestamp(&self) -> EventTimestamp {
        self.timestamp
    }

    /// Borrows the exact logical-window identity supplied at construction.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventId, EventMeta, EventTimestamp}};
    /// let meta = EventMeta::new(EventId::new(1), EventTimestamp::new(Duration::ZERO), "main", PresentationGeneration::INITIAL);
    /// assert_eq!(meta.logical_window_id().as_str(), "main");
    /// ```
    pub fn logical_window_id(&self) -> &LogicalWindowId {
        &self.logical_window_id
    }

    /// Returns the native presentation generation that produced the event.
    ///
    /// Consumers compare this against a ready [`crate::app::PresentationLifecycle`]
    /// to reject stale native events.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventId, EventMeta, EventTimestamp}};
    /// let generation = PresentationGeneration::new(4);
    /// let meta = EventMeta::new(EventId::new(1), EventTimestamp::new(Duration::ZERO), "main", generation);
    /// assert_eq!(meta.presentation_generation(), generation);
    /// ```
    pub const fn presentation_generation(&self) -> PresentationGeneration {
        self.presentation_generation
    }

    /// Borrows the optional pointer sample.
    ///
    /// `None` is the default and does not imply mouse, primary, or coordinate
    /// values. A sample can be attached even when the eventual event variant is
    /// not `Event::Pointer`; consistency is a host responsibility.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventId, EventMeta, EventTimestamp}};
    /// let meta = EventMeta::new(EventId::new(1), EventTimestamp::new(Duration::ZERO), "main", PresentationGeneration::INITIAL);
    /// assert!(meta.pointer().is_none());
    /// ```
    pub const fn pointer(&self) -> Option<&PointerSample> {
        self.pointer.as_ref()
    }

    /// Returns the primary classification carried by pointer metadata.
    ///
    /// Non-pointer events and legacy direct dispatches have no pointer sample
    /// and therefore return `None` rather than inventing a classification.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
    /// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventId, EventMeta, EventTimestamp}};
    /// let sample = PointerSample::new_with_primary(PointerId::new(2), PointerSource::Touch, Point::default(), false)?;
    /// let meta = EventMeta::new(EventId::new(1), EventTimestamp::new(Duration::ZERO), "main", PresentationGeneration::INITIAL).with_pointer(sample);
    /// assert_eq!(meta.pointer_is_primary(), Some(false));
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub const fn pointer_is_primary(&self) -> Option<bool> {
        match self.pointer.as_ref() {
            Some(pointer) => Some(pointer.is_primary()),
            None => None,
        }
    }

    /// Returns this metadata with `pointer` installed or replaced.
    ///
    /// The sample has already validated finite logical position through its own
    /// constructor. This builder performs no event-kind consistency check.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
    /// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventId, EventMeta, EventTimestamp}};
    /// let sample = PointerSample::new(PointerId::MOUSE, PointerSource::Mouse, Point::new(3.0, 4.0))?;
    /// let meta = EventMeta::new(EventId::new(1), EventTimestamp::new(Duration::ZERO), "main", PresentationGeneration::INITIAL).with_pointer(sample);
    /// assert_eq!(meta.pointer().unwrap().position(), Point::new(3.0, 4.0));
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub const fn with_pointer(mut self, pointer: PointerSample) -> Self {
        self.pointer = Some(pointer);
        self
    }
}

/// Runtime event together with host correlation and presentation metadata.
///
/// The envelope does not validate that optional pointer metadata corresponds to
/// an `Event::Pointer`, or that its logical window/generation is currently
/// accepted. Routing/presentation orchestration owns those checks. The struct
/// is non-exhaustive and exposes data through accessors.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use ailloli_ui_core::event::{Event, FocusEvent};
/// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventEnvelope, EventId, EventMeta, EventTimestamp}};
/// let meta = EventMeta::new(EventId::new(1), EventTimestamp::new(Duration::ZERO), "main", PresentationGeneration::INITIAL);
/// let envelope = EventEnvelope::new(meta, Event::Focus(FocusEvent::new(true)));
/// assert!(matches!(envelope.event(), Event::Focus(_)));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct EventEnvelope {
    /// Host correlation and presentation metadata.
    meta: EventMeta,
    /// Provider-neutral runtime event payload.
    event: Event,
}

/// Provides the operations defined for EventEnvelope.
impl EventEnvelope {
    /// Combines metadata and an event without cross-validating them.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::event::{Event, FocusEvent};
    /// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventEnvelope, EventId, EventMeta, EventTimestamp}};
    /// let meta = EventMeta::new(EventId::new(2), EventTimestamp::new(Duration::ZERO), "main", PresentationGeneration::INITIAL);
    /// let envelope = EventEnvelope::new(meta, Event::Focus(FocusEvent::new(false)));
    /// assert_eq!(envelope.meta().id(), EventId::new(2));
    /// ```
    pub const fn new(meta: EventMeta, event: Event) -> Self {
        Self { meta, event }
    }

    /// Borrows host correlation and presentation metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::event::{Event, FocusEvent};
    /// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventEnvelope, EventId, EventMeta, EventTimestamp}};
    /// let envelope = EventEnvelope::new(EventMeta::new(EventId::new(7), EventTimestamp::new(Duration::ZERO), "main", PresentationGeneration::INITIAL), Event::Focus(FocusEvent::new(true)));
    /// assert_eq!(envelope.meta().id().get(), 7);
    /// ```
    pub const fn meta(&self) -> &EventMeta {
        &self.meta
    }

    /// Borrows the provider-neutral event payload.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::event::{Event, FocusEvent};
    /// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventEnvelope, EventId, EventMeta, EventTimestamp}};
    /// let envelope = EventEnvelope::new(EventMeta::new(EventId::new(1), EventTimestamp::new(Duration::ZERO), "main", PresentationGeneration::INITIAL), Event::Focus(FocusEvent::new(true)));
    /// assert!(matches!(envelope.event(), Event::Focus(event) if event.focused));
    /// ```
    pub const fn event(&self) -> &Event {
        &self.event
    }

    /// Pointer sample carried by this envelope, when the event has one.
    ///
    /// This delegates to metadata rather than inspecting the event variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::event::{Event, FocusEvent};
    /// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventEnvelope, EventId, EventMeta, EventTimestamp}};
    /// let envelope = EventEnvelope::new(EventMeta::new(EventId::new(1), EventTimestamp::new(Duration::ZERO), "main", PresentationGeneration::INITIAL), Event::Focus(FocusEvent::new(true)));
    /// assert_eq!(envelope.pointer(), None);
    /// ```
    pub const fn pointer(&self) -> Option<&PointerSample> {
        self.meta.pointer()
    }

    /// Returns the envelope's explicit primary-pointer classification.
    ///
    /// `None` means no sample was attached, not a secondary pointer.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::event::{Event, FocusEvent};
    /// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventEnvelope, EventId, EventMeta, EventTimestamp}};
    /// let envelope = EventEnvelope::new(EventMeta::new(EventId::new(1), EventTimestamp::new(Duration::ZERO), "main", PresentationGeneration::INITIAL), Event::Focus(FocusEvent::new(true)));
    /// assert_eq!(envelope.pointer_is_primary(), None);
    /// ```
    pub const fn pointer_is_primary(&self) -> Option<bool> {
        self.meta.pointer_is_primary()
    }

    /// Consumes the envelope and returns `(metadata, event)` without cloning.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::event::{Event, FocusEvent};
    /// use ailloli_ui_runtime::{app::PresentationGeneration, input::{EventEnvelope, EventId, EventMeta, EventTimestamp}};
    /// let envelope = EventEnvelope::new(EventMeta::new(EventId::new(5), EventTimestamp::new(Duration::ZERO), "main", PresentationGeneration::INITIAL), Event::Focus(FocusEvent::new(true)));
    /// let (meta, event) = envelope.into_parts();
    /// assert_eq!(meta.id().get(), 5);
    /// assert!(matches!(event, Event::Focus(_)));
    /// ```
    pub fn into_parts(self) -> (EventMeta, Event) {
        (self.meta, self.event)
    }
}
