//! Application loop: runtime instance, handles, scheduler, and shared state.

pub mod command;
pub mod diagnostics;
pub mod external_url;
pub mod invalidation;
pub mod presentation;
pub mod runtime;
pub mod runtime_handle;
pub mod runtime_inbox;
pub mod scheduler;
pub mod state_store;
pub mod ui_inbox;

pub use command::Command;
pub use diagnostics::{
    ElementTreeDiagnosticsSnapshot, ElementWorkCounters, InvalidationDiagnosticsSnapshot,
    InvalidationRecord, InvalidationSource, INVALIDATION_PROVENANCE_CAPACITY,
};
pub use external_url::{
    ExternalUrl, ExternalUrlError, ExternalUrlOpener, MemoryExternalUrlOpener, OpenUrlError,
};
pub use invalidation::{FrameWorkPlan, Invalidation};
pub use presentation::{
    reduce_presentation, PendingPresentationIntents, PresentationCursor, PresentationEvent,
    PresentationGeneration, PresentationIntent, PresentationLifecycle, PresentationReduction,
    PresentationState, PresentationTransitionError, PresentationUnavailableReason,
};
pub use runtime::Runtime;
pub use runtime_handle::{
    ClipboardProvider, MemoryClipboard, RuntimeHandle, RuntimeInner, UiServiceRegistration,
    WindowChromeOp,
};
pub use runtime_inbox::{
    RuntimeDrain, RuntimeInbox, RuntimeInboxStats, RuntimeSendError, RuntimeSender,
    RUNTIME_INBOX_DRAIN_BUDGET,
};
pub use scheduler::Scheduler;
pub use state_store::{StateSlot, StateStore};
pub use ui_inbox::{UiDrain, UiInbox, UiInboxStats, UiSendError, UiSender, UiWake, UiWakeError};
