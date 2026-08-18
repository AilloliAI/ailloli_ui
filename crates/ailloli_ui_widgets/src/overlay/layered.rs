//! Helpers for base + overlay scenes (correct z-order).

use ailloli_ui_core::{ClipShape, Rect};
use ailloli_ui_runtime::{DrawCmd, Scene};

/// Builds a scene with a base layer then an overlay layer on top.
pub fn scene_base_overlay(
    base_clip: Option<Rect>,
    base_cmds: Vec<DrawCmd>,
    overlay_cmds: Vec<DrawCmd>,
) -> Scene {
    let base_clip = base_clip.map(ClipShape::Rect);
    Scene::from_base_and_overlay(base_clip, base_cmds, None, overlay_cmds)
}
