//! Helpers for base + overlay scenes (correct z-order).

use ailloli_ui_core::{ClipShape, Rect};
use ailloli_ui_runtime::{DrawCmd, Scene};

/// Builds a scene with a base layer then an overlay layer on top.
///
/// `base_clip` is an optional logical-pixel rectangular clip applied only to
/// base commands. Overlay commands are unclipped and paint after the base;
/// command order inside each input vector is preserved and vectors are moved.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_runtime::Scene;
/// use ailloli_ui_widgets::overlay::scene_base_overlay;
/// let scene: Scene = scene_base_overlay(Some(Rect::new(0.0, 0.0, 80.0, 40.0)), vec![], vec![]);
/// let _ = scene;
/// ```
pub fn scene_base_overlay(
    base_clip: Option<Rect>,
    base_cmds: Vec<DrawCmd>,
    overlay_cmds: Vec<DrawCmd>,
) -> Scene {
    let base_clip = base_clip.map(ClipShape::Rect);
    Scene::from_base_and_overlay(base_clip, base_cmds, None, overlay_cmds)
}
