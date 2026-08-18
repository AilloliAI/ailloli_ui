use std::collections::{HashMap, VecDeque};

use super::glyph_upload::write_subtexture_rgba;
use swash::zeno::Format;
use swash::{
    scale::{Render, ScaleContext, Source, StrikeWith},
    FontRef, GlyphId,
};

/// Max atlas pages (8 × 1024² ≈ 8 MiB per DPR bucket).
pub const MAX_ATLAS_PAGES: u8 = 8;

/// Cache key for one rasterized glyph in the text atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub face_id: u64,
    pub font_index: u32,
    pub px_size: u16,
    pub glyph_id: u32,
    /// `round(dpr * 100)` to separate HiDPI cache buckets.
    pub scale_100: u16,
}

/// UV layout and metrics for a glyph in the atlas.
#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub size_px: [f32; 2],
    pub offset_px: [f32; 2],
    pub advance_px: f32,
}

/// Per-frame atlas cache statistics.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TextAtlasStats {
    pub hits: u32,
    pub misses: u32,
    pub rasterized: u32,
    pub resets: u32,
    pub evictions_blocked: u32,
    pub glyphs_skipped: u32,
    pub pages_active: u32,
}

#[derive(Debug)]
struct Shelf {
    x: u32,
    y: u32,
    h: u32,
}

struct AtlasPage {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    shelf: Shelf,
}

/// Multi-page GPU glyph atlas with LRU eviction.
pub struct TextAtlas {
    tex_size: u32,
    max_pages: u8,
    sampler: wgpu::Sampler,

    pages: Vec<AtlasPage>,

    glyphs: HashMap<GlyphKey, (u8, Glyph)>,
    lru: VecDeque<GlyphKey>,
    frame_pinned_pages: Vec<bool>,
    frame_stats: TextAtlasStats,

    scale_cx: ScaleContext,
}

/// Scoped atlas access for one frame (pins pages until drop).
pub struct TextAtlasFrame<'a> {
    atlas: &'a mut TextAtlas,
}

impl<'a> TextAtlasFrame<'a> {
    pub fn get_or_rasterize(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        key: GlyphKey,
        font_data: &[u8],
    ) -> Option<(u8, Glyph)> {
        self.atlas
            .get_or_rasterize_pinned(device, queue, bind_group_layout, key, font_data)
    }

    pub fn record_missing_face(&mut self) {
        self.atlas.record_missing_face();
    }

    pub fn stats(&self) -> TextAtlasStats {
        self.atlas.stats()
    }
}

impl Drop for TextAtlasFrame<'_> {
    fn drop(&mut self) {
        self.atlas.finish_frame();
    }
}

impl TextAtlas {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        tex_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let tex_size = 1024;
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("text atlas sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let mut atlas = Self {
            tex_size,
            max_pages: MAX_ATLAS_PAGES,
            sampler,
            pages: Vec::new(),
            glyphs: HashMap::new(),
            lru: VecDeque::new(),
            frame_pinned_pages: Vec::new(),
            frame_stats: TextAtlasStats::default(),
            scale_cx: ScaleContext::new(),
        };
        atlas.allocate_page(device, queue, tex_bind_group_layout);
        atlas
    }

    /// Page 0 bind group (always allocated; legacy renderer path).
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.pages[0].bind_group
    }

    /// Bind group for the given atlas page index.
    pub fn page_bind_group(&self, page_idx: u8) -> &wgpu::BindGroup {
        &self.pages[page_idx as usize].bind_group
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn begin_frame(&mut self) -> TextAtlasFrame<'_> {
        self.start_frame();
        TextAtlasFrame { atlas: self }
    }

    pub fn start_frame(&mut self) {
        self.frame_pinned_pages.clear();
        self.frame_pinned_pages.resize(self.pages.len(), false);
        self.frame_stats = TextAtlasStats {
            pages_active: self.pages.len() as u32,
            ..TextAtlasStats::default()
        };
    }

    pub fn finish_frame(&mut self) {
        self.frame_pinned_pages.fill(false);
    }

    pub fn stats(&self) -> TextAtlasStats {
        TextAtlasStats {
            pages_active: self.pages.len() as u32,
            ..self.frame_stats
        }
    }

    fn allocate_page(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
    ) {
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("text atlas page"),
            size: wgpu::Extent3d {
                width: self.tex_size,
                height: self.tex_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("text atlas page bind group"),
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        let zero = vec![0u8; (self.tex_size * self.tex_size * 4) as usize];
        write_subtexture_rgba(
            queue,
            &texture,
            wgpu::Origin3d::ZERO,
            self.tex_size,
            self.tex_size,
            &zero,
        );

        self.pages.push(AtlasPage {
            texture,
            bind_group,
            shelf: Shelf { x: 0, y: 0, h: 0 },
        });
    }

    fn reset_page(&mut self, page_idx: u8, queue: &wgpu::Queue) {
        self.frame_stats.resets = self.frame_stats.resets.saturating_add(1);
        let zero = vec![0u8; (self.tex_size * self.tex_size * 4) as usize];
        write_subtexture_rgba(
            queue,
            &self.pages[page_idx as usize].texture,
            wgpu::Origin3d::ZERO,
            self.tex_size,
            self.tex_size,
            &zero,
        );
        self.pages[page_idx as usize].shelf = Shelf { x: 0, y: 0, h: 0 };

        let evicted: Vec<GlyphKey> = self
            .glyphs
            .iter()
            .filter_map(|(k, (p, _))| if *p == page_idx { Some(*k) } else { None })
            .collect();
        for k in evicted {
            self.glyphs.remove(&k);
            self.lru.retain(|key| key != &k);
        }
    }

    pub fn get_or_rasterize(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        key: GlyphKey,
        font_data: &[u8],
    ) -> Option<(u8, Glyph)> {
        self.get_or_rasterize_impl(device, queue, bind_group_layout, key, font_data, false)
    }

    pub fn get_or_rasterize_pinned(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        key: GlyphKey,
        font_data: &[u8],
    ) -> Option<(u8, Glyph)> {
        self.get_or_rasterize_impl(device, queue, bind_group_layout, key, font_data, true)
    }

    pub fn record_missing_face(&mut self) {
        self.frame_stats.glyphs_skipped = self.frame_stats.glyphs_skipped.saturating_add(1);
    }

    fn get_or_rasterize_impl(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        key: GlyphKey,
        font_data: &[u8],
        pin_for_frame: bool,
    ) -> Option<(u8, Glyph)> {
        if let Some((page, g)) = self.glyphs.get(&key).copied() {
            self.touch_lru(key);
            self.frame_stats.hits = self.frame_stats.hits.saturating_add(1);
            if pin_for_frame {
                self.pin_page(page);
            }
            return Some((page, g));
        }

        self.frame_stats.misses = self.frame_stats.misses.saturating_add(1);
        let px = key.px_size.clamp(8, 128) as f32;
        let Some((bmp, w, h, offset_x, offset_y)) =
            self.rasterize(font_data, key.font_index, key.glyph_id, px)
        else {
            self.frame_stats.glyphs_skipped = self.frame_stats.glyphs_skipped.saturating_add(1);
            return None;
        };
        if w == 0 || h == 0 || bmp.is_empty() {
            let g = Glyph {
                uv_min: [0.0, 0.0],
                uv_max: [0.0, 0.0],
                size_px: [0.0, 0.0],
                offset_px: [0.0, 0.0],
                advance_px: 0.0,
            };
            self.glyphs.insert(key, (0, g));
            self.touch_lru(key);
            if pin_for_frame {
                self.pin_page(0);
            }
            return Some((0, g));
        }
        let w = w.max(1);
        let h = h.max(1);

        let pad = 1u32;
        let alloc_w = w + pad * 2;
        let alloc_h = h + pad * 2;

        let Some((page_idx, x, y)) = self.alloc_or_grow(
            device,
            queue,
            bind_group_layout,
            alloc_w,
            alloc_h,
            pin_for_frame,
        ) else {
            self.frame_stats.glyphs_skipped = self.frame_stats.glyphs_skipped.saturating_add(1);
            return None;
        };

        let mut rgba = vec![0u8; (alloc_w * alloc_h * 4) as usize];
        for yy in 0..h {
            for xx in 0..w {
                let src_i = (yy * w + xx) as usize;
                let a = bmp.get(src_i).copied().unwrap_or(0);
                let dst_x = xx + pad;
                let dst_y = yy + pad;
                let dst_i = ((dst_y * alloc_w + dst_x) * 4) as usize;
                rgba[dst_i] = 255;
                rgba[dst_i + 1] = 255;
                rgba[dst_i + 2] = 255;
                rgba[dst_i + 3] = a;
            }
        }

        write_subtexture_rgba(
            queue,
            &self.pages[page_idx as usize].texture,
            wgpu::Origin3d { x, y, z: 0 },
            alloc_w,
            alloc_h,
            &rgba,
        );

        let uv_min = [
            (x + pad) as f32 / self.tex_size as f32,
            (y + pad) as f32 / self.tex_size as f32,
        ];
        let uv_max = [
            (x + pad + w) as f32 / self.tex_size as f32,
            (y + pad + h) as f32 / self.tex_size as f32,
        ];

        let g = Glyph {
            uv_min,
            uv_max,
            size_px: [w as f32, h as f32],
            offset_px: [offset_x, offset_y],
            advance_px: 0.0,
        };
        self.glyphs.insert(key, (page_idx, g));
        self.touch_lru(key);
        self.frame_stats.rasterized = self.frame_stats.rasterized.saturating_add(1);
        if pin_for_frame {
            self.pin_page(page_idx);
        }
        Some((page_idx, g))
    }

    fn pin_page(&mut self, page_idx: u8) {
        let idx = page_idx as usize;
        if idx >= self.frame_pinned_pages.len() {
            self.frame_pinned_pages.resize(idx + 1, false);
        }
        self.frame_pinned_pages[idx] = true;
    }

    fn touch_lru(&mut self, key: GlyphKey) {
        self.lru.retain(|k| k != &key);
        self.lru.push_back(key);
    }

    fn rasterize(
        &mut self,
        font_data: &[u8],
        font_index: u32,
        glyph_id: u32,
        px: f32,
    ) -> Option<(Vec<u8>, u32, u32, f32, f32)> {
        let font = FontRef::from_index(font_data, font_index as usize)?;
        let mut scaler = self.scale_cx.builder(font).size(px).hint(true).build();

        let gid: GlyphId = glyph_id as u16;
        let image = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ])
        .format(Format::Alpha)
        .render(&mut scaler, gid)?;

        let w = image.placement.width as u32;
        let h = image.placement.height as u32;
        let offset_x = image.placement.left as f32;
        // Swash `Placement::top` uses font coordinates; quads use screen pixels (Y down).
        // Same convention as cosmic-text (`y = -placement.top` when blitting the mask).
        let offset_y = -(image.placement.top as f32);
        Some((image.data, w, h, offset_x, offset_y))
    }

    fn alloc_or_grow(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        w: u32,
        h: u32,
        respect_frame_pins: bool,
    ) -> Option<(u8, u32, u32)> {
        if w > self.tex_size || h > self.tex_size {
            return None;
        }
        if let Some((x, y)) = self.try_alloc_in(self.pages.len() - 1, w, h) {
            return Some(((self.pages.len() - 1) as u8, x, y));
        }
        if (self.pages.len() as u8) < self.max_pages {
            self.allocate_page(device, queue, bind_group_layout);
            if let Some((x, y)) = self.try_alloc_in(self.pages.len() - 1, w, h) {
                return Some(((self.pages.len() - 1) as u8, x, y));
            }
        }
        let oldest_page = if respect_frame_pins {
            match self.oldest_unpinned_page_idx() {
                Some(page) => page,
                None => {
                    self.frame_stats.evictions_blocked =
                        self.frame_stats.evictions_blocked.saturating_add(1);
                    return None;
                }
            }
        } else {
            self.oldest_page_idx().unwrap_or(0)
        };
        self.reset_page(oldest_page, queue);
        let (x, y) = self.try_alloc_in(oldest_page as usize, w, h)?;
        Some((oldest_page, x, y))
    }

    fn oldest_page_idx(&self) -> Option<u8> {
        let key = self.lru.front()?;
        let (page, _) = self.glyphs.get(key)?;
        Some(*page)
    }

    fn oldest_unpinned_page_idx(&self) -> Option<u8> {
        oldest_unpinned_page_idx_from(&self.lru, &self.glyphs, &self.frame_pinned_pages)
    }

    fn try_alloc_in(&mut self, page_idx: usize, w: u32, h: u32) -> Option<(u32, u32)> {
        let page = self.pages.get_mut(page_idx)?;
        if page.shelf.h == 0 {
            page.shelf.h = h;
        }
        if h > page.shelf.h {
            page.shelf.x = 0;
            page.shelf.y += page.shelf.h;
            page.shelf.h = h;
        }
        if page.shelf.x + w > self.tex_size {
            page.shelf.x = 0;
            page.shelf.y += page.shelf.h;
            page.shelf.h = h;
        }
        if page.shelf.y + page.shelf.h > self.tex_size {
            return None;
        }
        let x = page.shelf.x;
        let y = page.shelf.y;
        page.shelf.x += w;
        Some((x, y))
    }
}

fn oldest_unpinned_page_idx_from(
    lru: &VecDeque<GlyphKey>,
    glyphs: &HashMap<GlyphKey, (u8, Glyph)>,
    pinned_pages: &[bool],
) -> Option<u8> {
    for key in lru {
        let Some((page, _)) = glyphs.get(key) else {
            continue;
        };
        if !pinned_pages.get(*page as usize).copied().unwrap_or(false) {
            return Some(*page);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(glyph_id: u32) -> GlyphKey {
        GlyphKey {
            face_id: 1,
            font_index: 0,
            px_size: 14,
            glyph_id,
            scale_100: 100,
        }
    }

    fn glyph() -> Glyph {
        Glyph {
            uv_min: [0.0, 0.0],
            uv_max: [1.0, 1.0],
            size_px: [1.0, 1.0],
            offset_px: [0.0, 0.0],
            advance_px: 0.0,
        }
    }

    #[test]
    fn oldest_unpinned_page_skips_pages_used_by_current_frame() {
        let mut lru = VecDeque::new();
        let a = key(1);
        let b = key(2);
        lru.push_back(a);
        lru.push_back(b);

        let mut glyphs = HashMap::new();
        glyphs.insert(a, (0, glyph()));
        glyphs.insert(b, (1, glyph()));

        assert_eq!(
            oldest_unpinned_page_idx_from(&lru, &glyphs, &[true, false]),
            Some(1)
        );
    }

    #[test]
    fn oldest_unpinned_page_returns_none_when_all_pages_are_pinned() {
        let mut lru = VecDeque::new();
        let a = key(1);
        let b = key(2);
        lru.push_back(a);
        lru.push_back(b);

        let mut glyphs = HashMap::new();
        glyphs.insert(a, (0, glyph()));
        glyphs.insert(b, (1, glyph()));

        assert_eq!(
            oldest_unpinned_page_idx_from(&lru, &glyphs, &[true, true]),
            None
        );
    }
}
