//! GPU clip rendering mode selection (not part of `ailloli_ui_core`).

use ailloli_ui_core::{ClipShape, Rect};
use ailloli_ui_runtime::scene::{ClipEntry, ClipStackSnapshot};

/// How a layer clip is applied on the GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ClipRenderMode {
    /// Axis-aligned scissor rect.
    Scissor,
    /// Per-fragment rounded-rect alpha in the shader.
    ShaderMask,
    /// Depth/stencil mask for large or window-root rounded clips.
    Stencil,
}

/// GPU-ready interpretation of a non-destructive clip stack.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderClipPlan {
    pub scissor: Option<Rect>,
    pub rounded_masks: Vec<ClipEntry>,
    pub primary_round_mask: Option<ClipEntry>,
    pub clip_mode: ClipRenderMode,
}

/// Above this draw-command count, prefer stencil over per-fragment shader masking.
pub const STENCIL_DRAW_CMD_THRESHOLD: usize = 4;

/// Shader AA band on stencil edges (on by default).
/// Set `AILLOLI_UI_STENCIL_AA=0`; `OCTAVUI_STENCIL_AA` is a legacy fallback.
pub fn stencil_aa_enabled() -> bool {
    !crate::env_control::falsey("AILLOLI_UI_STENCIL_AA", "OCTAVUI_STENCIL_AA")
}

/// Picks the GPU clip strategy for a layer.
pub fn choose_clip_render_mode(
    clip: Option<&ClipShape>,
    is_window_root: bool,
    draw_cmd_count: usize,
) -> ClipRenderMode {
    match clip {
        None => ClipRenderMode::Scissor,
        Some(ClipShape::Rect(_)) => ClipRenderMode::Scissor,
        Some(ClipShape::RoundRect { .. }) => {
            if crate::env_control::truthy(
                "AILLOLI_UI_CLIP_FORCE_SHADER",
                "OCTAVUI_CLIP_FORCE_SHADER",
            ) {
                return ClipRenderMode::ShaderMask;
            }
            if crate::env_control::truthy(
                "AILLOLI_UI_CLIP_FORCE_STENCIL",
                "OCTAVUI_CLIP_FORCE_STENCIL",
            ) {
                return ClipRenderMode::Stencil;
            }
            if is_window_root || draw_cmd_count > STENCIL_DRAW_CMD_THRESHOLD {
                ClipRenderMode::Stencil
            } else {
                ClipRenderMode::ShaderMask
            }
        }
    }
}

/// Resolves a stack of clips without fusing away root/window metadata.
pub fn resolve_clip_render_plan(clip: &ClipStackSnapshot, draw_cmd_count: usize) -> RenderClipPlan {
    let scissor = clip.scissor_rect();
    let rounded_masks: Vec<ClipEntry> = clip
        .entries()
        .iter()
        .copied()
        .filter(|entry| matches!(entry.shape, ClipShape::RoundRect { .. }))
        .collect();
    let primary_round_mask = rounded_masks
        .iter()
        .copied()
        .find(|entry| entry.is_window_root)
        .or_else(|| rounded_masks.first().copied());
    let clip_mode = primary_round_mask.map_or(ClipRenderMode::Scissor, |entry| {
        choose_clip_render_mode(Some(&entry.shape), entry.is_window_root, draw_cmd_count)
    });

    RenderClipPlan {
        scissor,
        rounded_masks,
        primary_round_mask,
        clip_mode,
    }
}

/// Uniform block for `clip_alpha` in WGSL (32 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ClipParamsGpu {
    pub rect: [f32; 4],
    pub radius: f32,
    pub mode: u32,
    pub _pad: u32,
    /// Struct tail padding (WGSL expects 32 bytes, not 28).
    pub _struct_pad: u32,
}

const _: () = assert!(std::mem::size_of::<ClipParamsGpu>() == 32);

impl ClipParamsGpu {
    pub const MODE_NONE: u32 = 0;
    pub const MODE_RECT: u32 = 1;
    pub const MODE_ROUND: u32 = 2;

    pub fn none() -> Self {
        Self {
            rect: [0.0; 4],
            radius: 0.0,
            mode: Self::MODE_NONE,
            _pad: 0,
            _struct_pad: 0,
        }
    }

    pub fn from_shape(shape: &ClipShape, dpr: f32) -> Self {
        let bbox = shape.bounding_rect();
        let x = bbox.x * dpr;
        let y = bbox.y * dpr;
        let w = bbox.w * dpr;
        let h = bbox.h * dpr;
        match shape {
            ClipShape::Rect(_) => Self {
                rect: [x, y, w, h],
                radius: 0.0,
                mode: Self::MODE_RECT,
                _pad: 0,
                _struct_pad: 0,
            },
            ClipShape::RoundRect { radius, .. } => Self {
                rect: [x, y, w, h],
                radius: radius * dpr,
                mode: Self::MODE_ROUND,
                _pad: 0,
                _struct_pad: 0,
            },
        }
    }
}

/// Clip bounding box in physical pixels.
pub fn clip_bbox_physical(shape: &ClipShape, dpr: f32) -> Rect {
    let b = shape.bounding_rect();
    Rect::new(b.x * dpr, b.y * dpr, b.w * dpr, b.h * dpr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static CLIP_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_clip_env(shader: Option<&str>, stencil: Option<&str>, f: impl FnOnce()) {
        let _guard = CLIP_ENV_LOCK.lock().expect("clip env lock");
        let old_shader = std::env::var("OCTAVUI_CLIP_FORCE_SHADER").ok();
        let old_stencil = std::env::var("OCTAVUI_CLIP_FORCE_STENCIL").ok();

        match shader {
            Some(value) => std::env::set_var("OCTAVUI_CLIP_FORCE_SHADER", value),
            None => std::env::remove_var("OCTAVUI_CLIP_FORCE_SHADER"),
        }
        match stencil {
            Some(value) => std::env::set_var("OCTAVUI_CLIP_FORCE_STENCIL", value),
            None => std::env::remove_var("OCTAVUI_CLIP_FORCE_STENCIL"),
        }

        f();

        match old_shader {
            Some(value) => std::env::set_var("OCTAVUI_CLIP_FORCE_SHADER", value),
            None => std::env::remove_var("OCTAVUI_CLIP_FORCE_SHADER"),
        }
        match old_stencil {
            Some(value) => std::env::set_var("OCTAVUI_CLIP_FORCE_STENCIL", value),
            None => std::env::remove_var("OCTAVUI_CLIP_FORCE_STENCIL"),
        }
    }

    #[test]
    fn rect_uses_scissor() {
        with_clip_env(None, None, || {
            let r = Rect::new(0.0, 0.0, 10.0, 10.0);
            assert_eq!(
                choose_clip_render_mode(Some(&ClipShape::Rect(r)), false, 100),
                ClipRenderMode::Scissor
            );
        });
    }

    #[test]
    fn stacked_root_round_and_editor_rect_keep_scissor_and_stencil_mask() {
        with_clip_env(None, None, || {
            let root = ClipShape::RoundRect {
                rect: Rect::new(0.0, 0.0, 100.0, 80.0),
                radius: 12.0,
            };
            let editor = ClipShape::Rect(Rect::new(10.0, 10.0, 40.0, 20.0));
            let clip = ClipStackSnapshot::from_entries(vec![
                ClipEntry::new(root, true),
                ClipEntry::new(editor, false),
            ]);

            let plan = resolve_clip_render_plan(&clip, 2);

            assert_eq!(plan.scissor, Some(Rect::new(10.0, 10.0, 40.0, 20.0)));
            assert_eq!(plan.rounded_masks, vec![ClipEntry::new(root, true)]);
            assert_eq!(plan.primary_round_mask, Some(ClipEntry::new(root, true)));
            assert_eq!(plan.clip_mode, ClipRenderMode::Stencil);
        });
    }

    #[test]
    fn rect_stack_resolves_to_scissor_only() {
        with_clip_env(None, None, || {
            let clip = ClipStackSnapshot::from_entries(vec![
                ClipEntry::new(ClipShape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0)), false),
                ClipEntry::new(ClipShape::Rect(Rect::new(5.0, 2.0, 10.0, 4.0)), false),
            ]);

            let plan = resolve_clip_render_plan(&clip, 10);

            assert_eq!(plan.scissor, Some(Rect::new(5.0, 2.0, 5.0, 4.0)));
            assert!(plan.rounded_masks.is_empty());
            assert_eq!(plan.primary_round_mask, None);
            assert_eq!(plan.clip_mode, ClipRenderMode::Scissor);
        });
    }

    #[test]
    fn small_non_root_round_prefers_shader_mask() {
        with_clip_env(None, None, || {
            let round = ClipShape::RoundRect {
                rect: Rect::new(0.0, 0.0, 40.0, 30.0),
                radius: 6.0,
            };
            let clip = ClipStackSnapshot::from_entries(vec![ClipEntry::new(round, false)]);

            let plan = resolve_clip_render_plan(&clip, 1);

            assert_eq!(plan.scissor, Some(Rect::new(0.0, 0.0, 40.0, 30.0)));
            assert_eq!(plan.primary_round_mask, Some(ClipEntry::new(round, false)));
            assert_eq!(plan.clip_mode, ClipRenderMode::ShaderMask);
        });
    }

    #[test]
    fn none_and_rect_ignore_forced_round_modes() {
        with_clip_env(Some("1"), Some("1"), || {
            let r = Rect::new(0.0, 0.0, 10.0, 10.0);
            assert_eq!(
                choose_clip_render_mode(None, false, 100),
                ClipRenderMode::Scissor
            );
            assert_eq!(
                choose_clip_render_mode(Some(&ClipShape::Rect(r)), false, 100),
                ClipRenderMode::Scissor
            );
        });
    }

    #[test]
    fn window_root_uses_stencil() {
        with_clip_env(None, None, || {
            let r = Rect::new(0.0, 0.0, 100.0, 100.0);
            assert_eq!(
                choose_clip_render_mode(
                    Some(&ClipShape::RoundRect {
                        rect: r,
                        radius: 14.0
                    }),
                    true,
                    1
                ),
                ClipRenderMode::Stencil
            );
        });
    }

    #[test]
    fn small_round_uses_shader() {
        with_clip_env(None, None, || {
            let r = Rect::new(0.0, 0.0, 50.0, 50.0);
            assert_eq!(
                choose_clip_render_mode(
                    Some(&ClipShape::RoundRect {
                        rect: r,
                        radius: 8.0
                    }),
                    false,
                    2
                ),
                ClipRenderMode::ShaderMask
            );
        });
    }

    #[test]
    fn many_draws_use_stencil() {
        with_clip_env(None, None, || {
            let r = Rect::new(0.0, 0.0, 50.0, 50.0);
            assert_eq!(
                choose_clip_render_mode(
                    Some(&ClipShape::RoundRect {
                        rect: r,
                        radius: 8.0
                    }),
                    false,
                    10
                ),
                ClipRenderMode::Stencil
            );
        });
    }

    #[test]
    fn forced_round_modes_apply_only_to_round_rect_with_shader_priority() {
        let r = Rect::new(0.0, 0.0, 50.0, 50.0);
        let clip = ClipShape::RoundRect {
            rect: r,
            radius: 8.0,
        };

        with_clip_env(None, Some("1"), || {
            assert_eq!(
                choose_clip_render_mode(Some(&clip), false, 1),
                ClipRenderMode::Stencil
            );
        });
        with_clip_env(Some("1"), Some("1"), || {
            assert_eq!(
                choose_clip_render_mode(Some(&clip), true, 100),
                ClipRenderMode::ShaderMask
            );
        });
    }
}
