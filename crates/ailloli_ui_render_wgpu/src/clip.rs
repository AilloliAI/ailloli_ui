//! GPU clip rendering mode selection (not part of `ailloli_ui_core`).

use ailloli_ui_core::{ClipShape, Rect};
use ailloli_ui_runtime::scene::{ClipEntry, ClipStackSnapshot};

/// How a layer clip is applied on the GPU.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::ClipRenderMode;
/// let mode = ClipRenderMode::Scissor;
/// assert_eq!(mode, ClipRenderMode::Scissor);
/// ```
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
///
/// `scissor` is the rectangular intersection of the full stack. Rounded entries
/// remain separate so window-root identity and nested masks are not discarded.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::resolve_clip_render_plan;
/// use ailloli_ui_runtime::scene::ClipStackSnapshot;
/// let clip = ClipStackSnapshot::from_entries(Vec::new());
/// let plan = resolve_clip_render_plan(&clip, 0);
/// assert!(plan.rounded_masks.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct RenderClipPlan {
    /// Rectangular clip intersection in logical coordinates, or none for an empty stack.
    pub scissor: Option<Rect>,
    /// All rounded entries in original stack order.
    pub rounded_masks: Vec<ClipEntry>,
    /// Window-root rounded entry when present, otherwise the first rounded entry.
    pub primary_round_mask: Option<ClipEntry>,
    /// GPU implementation selected for the primary rounded mask.
    pub clip_mode: ClipRenderMode,
}

/// Above this draw-command count, prefer stencil over per-fragment shader masking.
///
/// The comparison is strict: four commands still use the shader for a non-root
/// rounded clip, while five select stencil.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::clip::STENCIL_DRAW_CMD_THRESHOLD;
/// assert_eq!(STENCIL_DRAW_CMD_THRESHOLD, 4);
/// ```
pub const STENCIL_DRAW_CMD_THRESHOLD: usize = 4;

/// Shader AA band on stencil edges (on by default).
/// Set `AILLOLI_UI_STENCIL_AA=0`; `OCTAVUI_STENCIL_AA` is a legacy fallback.
/// The primary variable wins when both are set; only `0` and case-insensitive
/// `false` disable the band.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::clip::stencil_aa_enabled;
/// let enabled: bool = stencil_aa_enabled();
/// let _ = enabled; // The environment controls the value.
/// ```
pub fn stencil_aa_enabled() -> bool {
    !crate::env_control::falsey("AILLOLI_UI_STENCIL_AA", "OCTAVUI_STENCIL_AA")
}

/// Picks the GPU clip strategy for a layer.
///
/// `None` and rectangular clips always use scissor. A window-root rounded clip
/// always uses stencil unless a force environment variable overrides it. For a
/// non-root rounded clip, command counts above
/// [`STENCIL_DRAW_CMD_THRESHOLD`] use stencil and smaller layers use a shader
/// mask. If both force variables are true, the shader override wins.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::{choose_clip_render_mode, ClipRenderMode};
/// assert_eq!(choose_clip_render_mode(None, false, 100), ClipRenderMode::Scissor);
/// ```
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
///
/// `draw_cmd_count` influences only the rounded-mask strategy. The returned
/// scissor remains in logical coordinates; conversion to physical pixels occurs
/// at pass encoding.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{ClipShape, Rect};
/// use ailloli_ui_render_wgpu::{resolve_clip_render_plan, ClipRenderMode};
/// use ailloli_ui_runtime::scene::{ClipEntry, ClipStackSnapshot};
/// let clip = ClipStackSnapshot::from_entries(vec![ClipEntry::new(
///     ClipShape::Rect(Rect::new(0.0, 0.0, 20.0, 10.0)), false)]);
/// let plan = resolve_clip_render_plan(&clip, 1);
/// assert_eq!(plan.clip_mode, ClipRenderMode::Scissor);
/// ```
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
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::clip::ClipParamsGpu;
/// assert_eq!(std::mem::size_of::<ClipParamsGpu>(), 32);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ClipParamsGpu {
    /// Physical `[x, y, width, height]` bounding rectangle.
    pub rect: [f32; 4],
    /// Rounded-corner radius in physical pixels; zero for rectangles and no clip.
    pub radius: f32,
    /// Shader discriminator, one of `MODE_NONE`, `MODE_RECT`, or `MODE_ROUND`.
    pub mode: u32,
    /// Explicit alignment padding; always zero.
    pub _pad: u32,
    /// Struct tail padding (WGSL expects 32 bytes, not 28).
    pub _struct_pad: u32,
}

const _: () = assert!(std::mem::size_of::<ClipParamsGpu>() == 32);

impl ClipParamsGpu {
    /// Shader mode for an inactive clip.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::clip::ClipParamsGpu;
    /// assert_eq!(ClipParamsGpu::MODE_NONE, 0);
    /// ```
    pub const MODE_NONE: u32 = 0;
    /// Shader mode for an axis-aligned rectangle.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::clip::ClipParamsGpu;
    /// assert_eq!(ClipParamsGpu::MODE_RECT, 1);
    /// ```
    pub const MODE_RECT: u32 = 1;
    /// Shader mode for a rounded rectangle.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::clip::ClipParamsGpu;
    /// assert_eq!(ClipParamsGpu::MODE_ROUND, 2);
    /// ```
    pub const MODE_ROUND: u32 = 2;

    /// Creates the zeroed inactive uniform block.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::clip::ClipParamsGpu;
    /// let params = ClipParamsGpu::none();
    /// assert_eq!((params.mode, params.radius), (ClipParamsGpu::MODE_NONE, 0.0));
    /// ```
    pub fn none() -> Self {
        Self {
            rect: [0.0; 4],
            radius: 0.0,
            mode: Self::MODE_NONE,
            _pad: 0,
            _struct_pad: 0,
        }
    }

    /// Converts a logical clip shape to its physical-pixel shader parameters.
    ///
    /// Every coordinate and the rounded radius are multiplied by `dpr` without
    /// clamping. Callers are responsible for a finite positive scale.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// use ailloli_ui_render_wgpu::clip::ClipParamsGpu;
    /// let params = ClipParamsGpu::from_shape(
    ///     &ClipShape::Rect(Rect::new(1.0, 2.0, 3.0, 4.0)), 2.0);
    /// assert_eq!(params.rect, [2.0, 4.0, 6.0, 8.0]);
    /// assert_eq!(params.mode, ClipParamsGpu::MODE_RECT);
    /// ```
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
///
/// Coordinates are multiplied by `dpr` without clamping or rounding.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{ClipShape, Rect};
/// use ailloli_ui_render_wgpu::clip::clip_bbox_physical;
/// let shape = ClipShape::Rect(Rect::new(1.0, 2.0, 3.0, 4.0));
/// assert_eq!(clip_bbox_physical(&shape, 2.0), Rect::new(2.0, 4.0, 6.0, 8.0));
/// ```
pub fn clip_bbox_physical(shape: &ClipShape, dpr: f32) -> Rect {
    let b = shape.bounding_rect();
    Rect::new(b.x * dpr, b.y * dpr, b.w * dpr, b.h * dpr)
}

#[cfg(test)]
/// Exercises clip-mode policy, environment overrides, stacks, and root clips.
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes tests that mutate the process-wide clip override variables.
    static CLIP_ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Installs temporary legacy clip overrides, runs `f`, then restores state.
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
