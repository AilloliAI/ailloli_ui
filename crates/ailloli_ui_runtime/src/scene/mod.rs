//! Scene graph: draw commands, layered clips, and paint context.

pub mod clip_stack;
pub mod dirty;
pub mod draw_cmd;
pub mod isolated_effects;
pub mod paint_ctx;
pub mod paint_engine;
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
pub use scene_graph::{Layer, LayerKind, Scene};
