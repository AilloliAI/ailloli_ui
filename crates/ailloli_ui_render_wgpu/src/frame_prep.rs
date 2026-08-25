//! Frame resource preparation (single-pass compositing).
//!
//! [`PreparedResources::prepare`] is the **only** step that touches the GPU /
//! atlas / icon cache before the per-frame plan is built. It walks the frame's
//! layers, ensures every text glyph is rasterized + pinned in the atlas, and
//! every icon is allocated in the icon cache. The resulting [`PreparedResources`]
//! is then consumed by the **pure-CPU** [`crate::frame_plan::FrameRenderPlan::build_cpu`]
//! to assemble vertex arenas and batches without any device/queue access.
//!
//! This split keeps the plan testable without a GPU and matches the single-pass compositing
//! invariant "no `device.create_*` / `queue.write_*` between `begin_render_pass`
//! and `drop(rpass)`".

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ailloli_ui_core::math::Scale;
use ailloli_ui_runtime::DrawCmd;

use crate::icons::{IconCache, IconKey};
use crate::renderer::LayerPass;
use crate::text::{Glyph, GlyphKey, TextAtlas};

/// Resources pinned / allocated for the upcoming frame.
///
/// - `glyphs` : every glyph requested by any `DrawCmd::Text` in the frame's
///   layers, looked up (or rasterized + uploaded) and pinned in the text atlas
///   for the duration of the frame.
/// - `icons`  : every icon requested by any `DrawCmd::Image`, materialized in
///   the icon cache (bind group + texture).
///
/// `build_cpu` later reads these maps to emit `PlannedBatch`es with the
/// correct `TextureBindKind`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::PreparedResources;
/// let prepared = PreparedResources::default();
/// assert!(prepared.glyphs.is_empty() && prepared.icons.is_empty());
/// ```
#[derive(Debug, Default)]
pub struct PreparedResources {
    /// Pinned glyphs keyed by face, glyph, physical size, and DPR bucket.
    ///
    /// Each value is `(atlas_page, glyph_metrics)`.
    pub glyphs: HashMap<GlyphKey, (u8, Glyph)>,
    /// Exact icon keys uploaded or found in the persistent icon cache.
    pub icons: HashSet<IconKey>,
}

impl PreparedResources {
    /// Walks `layers` and prepares atlas glyphs + icon cache entries.
    ///
    /// Touches `device`, `queue`, `atlas` and `icon_cache`. Callers must have
    /// already called `atlas.start_frame()` (Renderer keeps this contract).
    /// Physical glyph sizes are rounded and clamped to `8..=128`; icon sizes to
    /// `8..=256`; the DPR bucket is `round(dpr * 100)` clamped to
    /// `1..=u16::MAX`. Missing face blobs increment atlas diagnostics and omit
    /// that glyph rather than panicking.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::{collections::HashMap, sync::Arc};
    /// use ailloli_ui_core::math::Scale;
    /// use ailloli_ui_render_wgpu::{frame_prep::PreparedResources,
    ///     icons::IconCache, text::TextAtlas};
    /// fn prepare(atlas: &mut TextAtlas, icons: &mut IconCache,
    ///     device: &wgpu::Device, queue: &wgpu::Queue,
    ///     layout: &wgpu::BindGroupLayout) -> PreparedResources {
    ///     atlas.start_frame();
    ///     let faces: HashMap<u64, Arc<[u8]>> = HashMap::new();
    ///     PreparedResources::prepare(&[], Scale::new(1.0), atlas, icons,
    ///         device, queue, layout, &faces)
    /// }
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        layers: &[LayerPass<'_>],
        scale: Scale,
        atlas: &mut TextAtlas,
        icon_cache: &mut IconCache,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture_bind_group_layout: &wgpu::BindGroupLayout,
        face_blobs: &HashMap<u64, Arc<[u8]>>,
    ) -> Self {
        let mut glyphs: HashMap<GlyphKey, (u8, Glyph)> = HashMap::new();
        let mut icons: HashSet<IconKey> = HashSet::new();
        let scale_100 = (scale.dpr * 100.0).round().clamp(1.0, u16::MAX as f32) as u16;

        for layer in layers {
            for cmd in layer.cmds {
                match cmd {
                    DrawCmd::Text(dt) => {
                        for gi in dt.layout.glyphs() {
                            let physical_px_size = ((gi.px_size as f32) * scale.dpr).round();
                            let key = GlyphKey {
                                face_id: gi.face_id,
                                font_index: gi.font_index,
                                px_size: physical_px_size.clamp(8.0, 128.0) as u16,
                                glyph_id: gi.glyph_id,
                                scale_100,
                            };
                            if glyphs.contains_key(&key) {
                                // Already pinned via earlier layer this frame.
                                continue;
                            }
                            let Some(blob) = face_blobs.get(&gi.face_id) else {
                                atlas.record_missing_face();
                                continue;
                            };
                            if let Some(entry) = atlas.get_or_rasterize_pinned(
                                device,
                                queue,
                                texture_bind_group_layout,
                                key,
                                blob.as_ref(),
                            ) {
                                glyphs.insert(key, entry);
                            }
                        }
                    }
                    DrawCmd::Image(img) => {
                        let physical_px_size = img.rect.w.max(img.rect.h) * scale.dpr;
                        let key = IconKey {
                            icon: img.icon.clone(),
                            px_size: physical_px_size.round().clamp(8.0, 256.0) as u16,
                            scale_100,
                        };
                        if icons.contains(&key) {
                            continue;
                        }
                        let _ = icon_cache.get_or_create(
                            device,
                            queue,
                            texture_bind_group_layout,
                            &key,
                        );
                        icons.insert(key);
                    }
                    DrawCmd::Rect(_)
                    | DrawCmd::RRect(_)
                    | DrawCmd::Border(_)
                    | DrawCmd::BoxShadow(_)
                    | DrawCmd::RingProgress(_)
                    | DrawCmd::Polyline(_) => {
                        // Solid primitives don't need GPU resources beyond the
                        // pipelines + clip bindings, which the renderer owns.
                    }
                }
            }
        }

        Self { glyphs, icons }
    }
}
