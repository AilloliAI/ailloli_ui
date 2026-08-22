//! Pure lifecycle and deferred intents for native window presentations.

use std::fmt;

use ailloli_ui_core::{LogicalWindowId, Size};

use super::WindowChromeOp;

/// Monotonic generation of a native presentation attached to a logical window.
///
/// A generation distinguishes successive native/surface attachments for the
/// same logical window. Public construction permits any `u64`; lifecycle
/// attachment increments with checked arithmetic and never wraps.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::PresentationGeneration;
/// assert_eq!(PresentationGeneration::INITIAL.get(), 0);
/// assert!(PresentationGeneration::new(2) > PresentationGeneration::new(1));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PresentationGeneration(u64);

/// Provides the operations defined for PresentationGeneration.
impl PresentationGeneration {
    /// Initial sentinel before any native presentation has attached.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::PresentationGeneration;
    /// assert_eq!(PresentationGeneration::INITIAL, PresentationGeneration::default());
    /// ```
    pub const INITIAL: Self = Self(0);

    /// Wraps an explicit generation value without validation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::PresentationGeneration;
    /// assert_eq!(PresentationGeneration::new(u64::MAX).get(), u64::MAX);
    /// ```
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying generation integer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::PresentationGeneration;
    /// assert_eq!(PresentationGeneration::new(7).get(), 7);
    /// assert_eq!(PresentationGeneration::new(7).to_string(), "7");
    /// ```
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next generation or `None` at `u64::MAX`.
    fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// Implements the fmt::Display contract for PresentationGeneration.
impl fmt::Display for PresentationGeneration {
    /// Formats the value for human-readable diagnostics.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Provider-neutral lifecycle state for a logical window presentation.
///
/// This enum models host/surface availability without owning a native window or
/// GPU resource. It is non-exhaustive for downstream consumers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{PresentationState, PresentationUnavailableReason};
/// assert_ne!(PresentationState::Declared, PresentationState::Ready);
/// assert_eq!(PresentationState::Unavailable(PresentationUnavailableReason::ZeroExtent), PresentationState::Unavailable(PresentationUnavailableReason::ZeroExtent));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationState {
    /// Logical window exists but creation has not been authorized.
    Declared,
    /// Host may create/attach a native presentation.
    CreationAllowed,
    /// Native presentation is attached and accepts this generation's events.
    Ready,
    /// Presentation is detached during host suspension.
    Suspended,
    /// Presentation cannot currently be used for the recorded reason.
    Unavailable(PresentationUnavailableReason),
    /// Terminal lifecycle state; only another Destroy event is accepted.
    Destroyed,
}

/// Reason a native presentation cannot currently be used.
///
/// The enum is non-exhaustive. It provides diagnosis only; recovery occurs by
/// applying [`PresentationEvent::Retry`] or allowing creation again.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::PresentationUnavailableReason;
/// assert_ne!(PresentationUnavailableReason::ZeroExtent, PresentationUnavailableReason::SurfaceLost);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationUnavailableReason {
    /// Logical width or height was zero/non-positive for surface creation.
    ZeroExtent,
    /// A previously usable rendering surface was lost.
    SurfaceLost,
    /// No surface/configuration compatible with the host was available.
    NoCompatibleSurface,
    /// The native presentation provider itself is unavailable.
    HostUnavailable,
}

/// Input accepted by the pure presentation reducer.
///
/// The enum is non-exhaustive. Events carry no native handles and are safe to
/// use in deterministic lifecycle tests.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{PresentationEvent, PresentationUnavailableReason};
/// assert_ne!(PresentationEvent::AllowCreation, PresentationEvent::Destroy);
/// assert_eq!(PresentationEvent::Unavailable(PresentationUnavailableReason::SurfaceLost), PresentationEvent::Unavailable(PresentationUnavailableReason::SurfaceLost));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationEvent {
    /// Authorize transitioning toward native attachment.
    AllowCreation,
    /// Report successful native attachment and advance generation.
    Attached,
    /// Detach/suspend presentation without destroying the logical window.
    Suspend,
    /// Record a non-fatal unavailable state.
    Unavailable(PresentationUnavailableReason),
    /// Move an unavailable presentation back to creation-allowed.
    Retry,
    /// Enter the terminal destroyed state.
    Destroy,
}

/// Result of one pure lifecycle reduction.
///
/// `generation_changed` is true only for successful attachment and always equals
/// `generation !=` the reducer's input generation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{reduce_presentation, PresentationEvent, PresentationGeneration, PresentationState};
/// let reduced = reduce_presentation(PresentationState::Declared, PresentationGeneration::INITIAL, PresentationEvent::AllowCreation)?;
/// assert_eq!(reduced.state, PresentationState::CreationAllowed);
/// assert!(!reduced.generation_changed);
/// # Ok::<(), ailloli_ui_runtime::app::PresentationTransitionError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationReduction {
    /// State after applying the event.
    pub state: PresentationState,
    /// Generation after applying the event.
    pub generation: PresentationGeneration,
    /// Whether generation advanced during this reduction.
    pub generation_changed: bool,
}

/// Invalid presentation lifecycle transition.
///
/// Errors carry no native-resource detail and leave [`PresentationLifecycle`]
/// unchanged when returned through its [`PresentationLifecycle::apply`] method.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::PresentationTransitionError;
/// assert_eq!(PresentationTransitionError::AttachmentNotAllowed.to_string(), "a presentation can only attach after creation is allowed");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PresentationTransitionError {
    #[error("a destroyed presentation cannot transition")]
    /// The presentation has already reached its terminal destroyed state.
    Destroyed,
    #[error("a presentation can only attach after creation is allowed")]
    /// Attachment was requested before creation became allowed.
    AttachmentNotAllowed,
    #[error("presentation generation is exhausted")]
    /// Incrementing the `u64` presentation generation would overflow.
    GenerationExhausted,
    #[error("presentation retry requires an unavailable presentation")]
    /// Retry was requested from a state other than unavailable.
    RetryNotAllowed,
}

/// Reduces one lifecycle event without accessing a native host or GPU.
///
/// `Attached` is valid only from [`PresentationState::CreationAllowed`] and
/// checked-increments generation. `Retry` is valid only from `Unavailable`.
/// `AllowCreation` is idempotent for already-allowed/ready states and re-enables
/// suspended or unavailable states. Suspend, Unavailable, and Destroy otherwise
/// replace any non-destroyed state. Destroy is idempotent once destroyed; every
/// other event then returns [`PresentationTransitionError::Destroyed`].
///
/// # Errors
///
/// Returns a transition error for disallowed attach/retry, generation overflow,
/// or any non-Destroy event after destruction.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{reduce_presentation, PresentationEvent, PresentationGeneration, PresentationState};
/// let allowed = reduce_presentation(PresentationState::Declared, PresentationGeneration::INITIAL, PresentationEvent::AllowCreation)?;
/// let ready = reduce_presentation(allowed.state, allowed.generation, PresentationEvent::Attached)?;
/// assert_eq!(ready.state, PresentationState::Ready);
/// assert_eq!(ready.generation.get(), 1);
/// assert!(ready.generation_changed);
/// # Ok::<(), ailloli_ui_runtime::app::PresentationTransitionError>(())
/// ```
pub fn reduce_presentation(
    state: PresentationState,
    generation: PresentationGeneration,
    event: PresentationEvent,
) -> Result<PresentationReduction, PresentationTransitionError> {
    use PresentationEvent as Input;
    use PresentationState as State;

    if state == State::Destroyed {
        return if matches!(event, Input::Destroy) {
            Ok(PresentationReduction {
                state,
                generation,
                generation_changed: false,
            })
        } else {
            Err(PresentationTransitionError::Destroyed)
        };
    }

    let mut next_generation = generation;
    let next_state = match event {
        Input::AllowCreation => match state {
            State::Declared | State::Suspended | State::Unavailable(_) => State::CreationAllowed,
            State::CreationAllowed | State::Ready => state,
            State::Destroyed => unreachable!("destroyed is handled above"),
        },
        Input::Attached => {
            if state != State::CreationAllowed {
                return Err(PresentationTransitionError::AttachmentNotAllowed);
            }
            next_generation = generation
                .checked_next()
                .ok_or(PresentationTransitionError::GenerationExhausted)?;
            State::Ready
        }
        Input::Suspend => State::Suspended,
        Input::Unavailable(reason) => State::Unavailable(reason),
        Input::Retry => match state {
            State::Unavailable(_) => State::CreationAllowed,
            _ => return Err(PresentationTransitionError::RetryNotAllowed),
        },
        Input::Destroy => State::Destroyed,
    };

    Ok(PresentationReduction {
        state: next_state,
        generation: next_generation,
        generation_changed: generation != next_generation,
    })
}

/// Pure retained lifecycle model for one logical window.
///
/// The logical ID survives suspension and attachment generations. The type owns
/// no native resource and can be cloned for deterministic state inspection.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{PresentationGeneration, PresentationLifecycle, PresentationState};
/// let lifecycle = PresentationLifecycle::new("main");
/// assert_eq!(lifecycle.logical_window_id().as_str(), "main");
/// assert_eq!(lifecycle.state(), PresentationState::Declared);
/// assert_eq!(lifecycle.generation(), PresentationGeneration::INITIAL);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationLifecycle {
    /// Stable provider-neutral logical-window identity.
    logical_window_id: LogicalWindowId,
    /// Current pure lifecycle state.
    state: PresentationState,
    /// Current attachment generation.
    generation: PresentationGeneration,
}

/// Provides the operations defined for PresentationLifecycle.
impl PresentationLifecycle {
    /// Creates a declared logical window at generation zero.
    ///
    /// The ID is stored exactly by [`LogicalWindowId`]; empty IDs are
    /// representable at this layer and host declaration logic owns uniqueness.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{PresentationGeneration, PresentationLifecycle, PresentationState};
    /// let lifecycle = PresentationLifecycle::new(String::from("settings"));
    /// assert_eq!(lifecycle.state(), PresentationState::Declared);
    /// assert_eq!(lifecycle.generation(), PresentationGeneration::INITIAL);
    /// ```
    pub fn new(logical_window_id: impl Into<LogicalWindowId>) -> Self {
        Self {
            logical_window_id: logical_window_id.into(),
            state: PresentationState::Declared,
            generation: PresentationGeneration::INITIAL,
        }
    }

    /// Borrows the stable logical-window identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::PresentationLifecycle;
    /// let lifecycle = PresentationLifecycle::new("main");
    /// assert_eq!(lifecycle.logical_window_id().as_str(), "main");
    /// ```
    pub fn logical_window_id(&self) -> &LogicalWindowId {
        &self.logical_window_id
    }

    /// Returns the current lifecycle state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{PresentationLifecycle, PresentationState};
    /// assert_eq!(PresentationLifecycle::new("main").state(), PresentationState::Declared);
    /// ```
    pub const fn state(&self) -> PresentationState {
        self.state
    }

    /// Returns the current native-attachment generation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{PresentationGeneration, PresentationLifecycle};
    /// assert_eq!(PresentationLifecycle::new("main").generation(), PresentationGeneration::INITIAL);
    /// ```
    pub const fn generation(&self) -> PresentationGeneration {
        self.generation
    }

    /// Returns whether events from `generation` belong to the ready presentation.
    ///
    /// Matching generations are rejected unless state is exactly
    /// [`PresentationState::Ready`], preventing stale events during suspension,
    /// recreation, or destruction.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{PresentationEvent, PresentationGeneration, PresentationLifecycle};
    /// let mut lifecycle = PresentationLifecycle::new("main");
    /// assert!(!lifecycle.accepts(PresentationGeneration::INITIAL));
    /// lifecycle.apply(PresentationEvent::AllowCreation)?;
    /// lifecycle.apply(PresentationEvent::Attached)?;
    /// assert!(lifecycle.accepts(PresentationGeneration::new(1)));
    /// # Ok::<(), ailloli_ui_runtime::app::PresentationTransitionError>(())
    /// ```
    pub fn accepts(&self, generation: PresentationGeneration) -> bool {
        self.state == PresentationState::Ready && self.generation == generation
    }

    /// Applies one pure reducer event and commits it only on success.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`reduce_presentation`]. State and generation
    /// remain unchanged when reduction fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{PresentationEvent, PresentationGeneration, PresentationLifecycle, PresentationState};
    /// let mut lifecycle = PresentationLifecycle::new("main");
    /// lifecycle.apply(PresentationEvent::AllowCreation)?;
    /// let attached = lifecycle.apply(PresentationEvent::Attached)?;
    /// assert_eq!(lifecycle.state(), PresentationState::Ready);
    /// assert_eq!(attached.generation, PresentationGeneration::new(1));
    /// # Ok::<(), ailloli_ui_runtime::app::PresentationTransitionError>(())
    /// ```
    pub fn apply(
        &mut self,
        event: PresentationEvent,
    ) -> Result<PresentationReduction, PresentationTransitionError> {
        let reduction = reduce_presentation(self.state, self.generation, event)?;
        self.state = reduction.state;
        self.generation = reduction.generation;
        Ok(reduction)
    }
}

/// Provider-neutral cursor intent retained while a presentation is unavailable.
///
/// Hosts map these roles to their platform cursor set. The enum is
/// non-exhaustive for downstream consumers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::PresentationCursor;
/// assert_ne!(PresentationCursor::Default, PresentationCursor::Pointer);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationCursor {
    /// Platform default arrow/pointer.
    Default,
    /// Clickable-link or button pointer.
    Pointer,
    /// Text insertion cursor.
    Text,
    /// Horizontal resize cursor.
    ResizeX,
    /// Vertical resize cursor.
    ResizeY,
}

/// A window operation that can be retained until presentation is ready.
///
/// Values perform no host operation themselves. Titles and sizes are stored
/// without validation; size components are logical pixels and may be negative
/// or non-finite at this layer. The enum is non-exhaustive.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Size;
/// use ailloli_ui_runtime::app::PresentationIntent;
/// assert_eq!(PresentationIntent::SetInnerSize(Size::new(800.0, 600.0)), PresentationIntent::SetInnerSize(Size::new(800.0, 600.0)));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum PresentationIntent {
    /// Replace the native window title; an empty title is valid.
    SetTitle(String),
    /// Request a logical inner size without normalization.
    SetInnerSize(Size),
    /// Replace the current provider-neutral cursor role.
    SetCursor(PresentationCursor),
    /// Queue a non-coalesced title-bar/window-chrome operation.
    WindowChrome(WindowChromeOp),
    /// Request at least one redraw.
    Redraw,
}

/// Coalescing store for presentation operations requested while detached.
///
/// Title, size, cursor, and redraw use last-value/boolean coalescing. Chrome
/// operations retain every value in insertion order without a capacity bound.
/// Cloning duplicates all owned titles and queued operations.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{PendingPresentationIntents, PresentationIntent};
/// let mut pending = PendingPresentationIntents::default();
/// pending.push(PresentationIntent::SetTitle("first".into()));
/// pending.push(PresentationIntent::SetTitle("last".into()));
/// assert_eq!(pending.drain(), [PresentationIntent::SetTitle("last".into())]);
/// ```
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PendingPresentationIntents {
    /// Last requested title.
    title: Option<String>,
    /// Last requested logical inner size.
    inner_size: Option<Size>,
    /// Last requested cursor role.
    cursor: Option<PresentationCursor>,
    /// All queued chrome operations in request order.
    chrome: Vec<WindowChromeOp>,
    /// Coalesced redraw request bit.
    redraw: bool,
}

/// Provides the operations defined for PendingPresentationIntents.
impl PendingPresentationIntents {
    /// Coalesces or appends one deferred operation.
    ///
    /// State-setting intents overwrite the prior value; redraw becomes true;
    /// chrome operations append and can grow the vector without bound.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{PendingPresentationIntents, PresentationIntent};
    /// let mut pending = PendingPresentationIntents::default();
    /// pending.push(PresentationIntent::Redraw);
    /// pending.push(PresentationIntent::Redraw);
    /// assert_eq!(pending.drain(), [PresentationIntent::Redraw]);
    /// ```
    pub fn push(&mut self, intent: PresentationIntent) {
        match intent {
            PresentationIntent::SetTitle(title) => self.title = Some(title),
            PresentationIntent::SetInnerSize(size) => self.inner_size = Some(size),
            PresentationIntent::SetCursor(cursor) => self.cursor = Some(cursor),
            PresentationIntent::WindowChrome(operation) => self.chrome.push(operation),
            PresentationIntent::Redraw => self.redraw = true,
        }
    }

    /// Returns whether no coalesced or queued operation is pending.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{PendingPresentationIntents, PresentationIntent};
    /// let mut pending = PendingPresentationIntents::default();
    /// assert!(pending.is_empty());
    /// pending.push(PresentationIntent::Redraw);
    /// assert!(!pending.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.inner_size.is_none()
            && self.cursor.is_none()
            && self.chrome.is_empty()
            && !self.redraw
    }

    /// Drains coalesced intents in deterministic replay order.
    ///
    /// Replay order is title, size, cursor, every chrome operation in insertion
    /// order, then redraw—regardless of original interleaving. All stored state
    /// is cleared; a second drain is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Size;
    /// use ailloli_ui_runtime::app::{PendingPresentationIntents, PresentationCursor, PresentationIntent};
    /// let mut pending = PendingPresentationIntents::default();
    /// pending.push(PresentationIntent::Redraw);
    /// pending.push(PresentationIntent::SetCursor(PresentationCursor::Text));
    /// pending.push(PresentationIntent::SetInnerSize(Size::new(10.0, 20.0)));
    /// assert_eq!(pending.drain(), [
    ///     PresentationIntent::SetInnerSize(Size::new(10.0, 20.0)),
    ///     PresentationIntent::SetCursor(PresentationCursor::Text),
    ///     PresentationIntent::Redraw,
    /// ]);
    /// assert!(pending.is_empty());
    /// ```
    pub fn drain(&mut self) -> Vec<PresentationIntent> {
        let mut intents = Vec::with_capacity(4 + self.chrome.len());
        if let Some(title) = self.title.take() {
            intents.push(PresentationIntent::SetTitle(title));
        }
        if let Some(size) = self.inner_size.take() {
            intents.push(PresentationIntent::SetInnerSize(size));
        }
        if let Some(cursor) = self.cursor.take() {
            intents.push(PresentationIntent::SetCursor(cursor));
        }
        intents.extend(self.chrome.drain(..).map(PresentationIntent::WindowChrome));
        if std::mem::take(&mut self.redraw) {
            intents.push(PresentationIntent::Redraw);
        }
        intents
    }
}
