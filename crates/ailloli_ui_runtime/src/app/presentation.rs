use std::fmt;

use ailloli_ui_core::{LogicalWindowId, Size};

use super::WindowChromeOp;

/// Monotonic generation of a native presentation attached to a logical window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PresentationGeneration(u64);

impl PresentationGeneration {
    pub const INITIAL: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

impl fmt::Display for PresentationGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Provider-neutral lifecycle state for a logical window presentation.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationState {
    Declared,
    CreationAllowed,
    Ready,
    Suspended,
    Unavailable(PresentationUnavailableReason),
    Destroyed,
}

/// Reason a native presentation cannot currently be used.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationUnavailableReason {
    ZeroExtent,
    SurfaceLost,
    NoCompatibleSurface,
    HostUnavailable,
}

/// Input accepted by the pure presentation reducer.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationEvent {
    AllowCreation,
    Attached,
    Suspend,
    Unavailable(PresentationUnavailableReason),
    Retry,
    Destroy,
}

/// Result of one pure lifecycle reduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationReduction {
    pub state: PresentationState,
    pub generation: PresentationGeneration,
    pub generation_changed: bool,
}

/// Invalid presentation lifecycle transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PresentationTransitionError {
    #[error("a destroyed presentation cannot transition")]
    Destroyed,
    #[error("a presentation can only attach after creation is allowed")]
    AttachmentNotAllowed,
    #[error("presentation generation is exhausted")]
    GenerationExhausted,
    #[error("presentation retry requires an unavailable presentation")]
    RetryNotAllowed,
}

/// Reduces one lifecycle event without accessing a native host or GPU.
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationLifecycle {
    logical_window_id: LogicalWindowId,
    state: PresentationState,
    generation: PresentationGeneration,
}

impl PresentationLifecycle {
    pub fn new(logical_window_id: impl Into<LogicalWindowId>) -> Self {
        Self {
            logical_window_id: logical_window_id.into(),
            state: PresentationState::Declared,
            generation: PresentationGeneration::INITIAL,
        }
    }

    pub fn logical_window_id(&self) -> &LogicalWindowId {
        &self.logical_window_id
    }

    pub const fn state(&self) -> PresentationState {
        self.state
    }

    pub const fn generation(&self) -> PresentationGeneration {
        self.generation
    }

    pub fn accepts(&self, generation: PresentationGeneration) -> bool {
        self.state == PresentationState::Ready && self.generation == generation
    }

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
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationCursor {
    Default,
    Pointer,
    Text,
    ResizeX,
    ResizeY,
}

/// A window operation that can be retained until presentation is ready.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum PresentationIntent {
    SetTitle(String),
    SetInnerSize(Size),
    SetCursor(PresentationCursor),
    WindowChrome(WindowChromeOp),
    Redraw,
}

/// Coalescing store for presentation operations requested while detached.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PendingPresentationIntents {
    title: Option<String>,
    inner_size: Option<Size>,
    cursor: Option<PresentationCursor>,
    chrome: Vec<WindowChromeOp>,
    redraw: bool,
}

impl PendingPresentationIntents {
    pub fn push(&mut self, intent: PresentationIntent) {
        match intent {
            PresentationIntent::SetTitle(title) => self.title = Some(title),
            PresentationIntent::SetInnerSize(size) => self.inner_size = Some(size),
            PresentationIntent::SetCursor(cursor) => self.cursor = Some(cursor),
            PresentationIntent::WindowChrome(operation) => self.chrome.push(operation),
            PresentationIntent::Redraw => self.redraw = true,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.inner_size.is_none()
            && self.cursor.is_none()
            && self.chrome.is_empty()
            && !self.redraw
    }

    /// Drains coalesced intents in deterministic replay order.
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
