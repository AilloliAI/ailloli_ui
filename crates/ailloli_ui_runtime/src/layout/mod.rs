//! Layout: measurement context, engine, results, and retained layout nodes.

/// Transactional layout staging implementation details.
pub(crate) mod layout_attempt;
/// Layout commit implementation details.
pub mod layout_commit;
/// Layout ctx implementation details.
pub mod layout_ctx;
/// Layout engine implementation details.
pub mod layout_engine;
/// Layout node implementation details.
pub mod layout_node;
/// Layout result implementation details.
pub mod layout_result;

#[doc(hidden)]
pub use layout_attempt::{layout_staging_allocation_count, LayoutAttemptToken};
pub use layout_commit::commit_layout_element;
pub(crate) use layout_commit::commit_layout_element_observed;
pub use layout_ctx::{LayoutChild, LayoutContext, LayoutCtx, LayoutPass, VirtualViewport};
pub use layout_engine::LayoutEngine;
pub use layout_node::{LayoutNode, Widget};
#[cfg(feature = "devtools")]
pub use layout_result::LayoutDebugInfo;
pub use layout_result::{ChildLayout, LayoutArtifact, LayoutResult};

pub use crate::scene::{PaintCtx, Painter};
