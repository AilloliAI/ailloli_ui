//! Application loop: runtime instance, handles, scheduler, and shared state.

pub mod command;
pub mod external_url;
pub mod runtime;
pub mod runtime_handle;
pub mod scheduler;
pub mod state_store;

pub use command::Command;
pub use external_url::{
    ExternalUrl, ExternalUrlError, ExternalUrlOpener, MemoryExternalUrlOpener, OpenUrlError,
};
pub use runtime::Runtime;
pub use runtime_handle::{
    ClipboardProvider, MemoryClipboard, RuntimeHandle, RuntimeInner, WindowChromeOp,
};
pub use scheduler::Scheduler;
pub use state_store::{StateSlot, StateStore};
