//! Minimal application view prelude built directly on the public façade.

pub use ailloli_ui::prelude::*;

/// The showcase handles interaction through bound public state and emits no
/// application-level action enum.
pub type Action = ();
