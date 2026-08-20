//! Application loop: runtime instance, handles, scheduler, and shared state.

pub mod command;
pub mod external_url;
pub mod presentation;
pub mod runtime;
pub mod runtime_handle;
pub mod runtime_inbox;
pub mod scheduler;
pub mod state_store;

pub use command::Command;
pub use external_url::{
    ExternalUrl, ExternalUrlError, ExternalUrlOpener, MemoryExternalUrlOpener, OpenUrlError,
};
pub use presentation::{
    reduce_presentation, PendingPresentationIntents, PresentationCursor, PresentationEvent,
    PresentationGeneration, PresentationIntent, PresentationLifecycle, PresentationReduction,
    PresentationState, PresentationTransitionError, PresentationUnavailableReason,
};
pub use runtime::Runtime;
pub use runtime_handle::{
    ClipboardProvider, MemoryClipboard, RuntimeHandle, RuntimeInner, WindowChromeOp,
};
pub use runtime_inbox::{
    RuntimeDrain, RuntimeInbox, RuntimeInboxStats, RuntimeSendError, RuntimeSender, UiWake,
    UiWakeError, RUNTIME_INBOX_DRAIN_BUDGET,
};
pub use scheduler::Scheduler;
pub use state_store::{StateSlot, StateStore};
