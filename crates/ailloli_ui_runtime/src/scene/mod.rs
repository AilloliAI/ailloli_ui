//! Scene graph: draw commands, layered clips, and paint context.

/// Clip stack implementation details.
pub mod clip_stack;
/// Dirty implementation details.
pub mod dirty;
/// Draw cmd implementation details.
pub mod draw_cmd;
/// Isolated effects implementation details.
pub mod isolated_effects;
/// Paint ctx implementation details.
pub mod paint_ctx;
/// Paint engine implementation details.
pub mod paint_engine;
/// Scene graph implementation details.
pub mod scene_graph;

pub use clip_stack::{ClipEntry, ClipStack, ClipStackSnapshot};
pub use dirty::DirtyFlags;
pub use draw_cmd::{
    DrawBorder, DrawBoxShadow, DrawCmd, DrawImage, DrawPolyline, DrawRRect, DrawRect,
    DrawRingProgress, DrawText,
};
pub use isolated_effects::{BlendMode, IsolatedEffects};
pub use paint_ctx::{PaintCtx, Painter};
pub use paint_engine::paint_element;
pub(crate) use paint_engine::paint_element_observed;
pub use scene_graph::{Layer, LayerKind, Scene};
