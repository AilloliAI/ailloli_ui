//! Multi-page GPU glyph atlas with per-frame pinning and LRU page eviction.

use std::collections::{HashMap, VecDeque};

use super::glyph_upload::write_subtexture_rgba;
use swash::zeno::Format;
use swash::{
    scale::{Render, ScaleContext, Source, StrikeWith},
    FontRef, GlyphId,
};

/// Maximum atlas pages: eight 1024² RGBA8 textures (about 32 MiB total).
///
/// Each RGBA8 page occupies about 4 MiB, so the absolute texture storage ceiling
/// is approximately 32 MiB across all DPR buckets represented in the keys.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::text::MAX_ATLAS_PAGES;
/// assert_eq!(MAX_ATLAS_PAGES, 8);
/// ```
pub const MAX_ATLAS_PAGES: u8 = 8;

/// Cache key for one rasterized glyph in the text atlas.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::text::GlyphKey;
/// let key = GlyphKey { face_id: 3, font_index: 0, px_size: 16,
///     glyph_id: 42, scale_100: 200 };
/// assert_eq!((key.face_id, key.scale_100), (3, 200));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    /// Stable face identity supplied by the text system.
    pub face_id: u64,
    /// Face index inside a font collection.
    pub font_index: u32,
    /// Raster size in physical pixels, clamped to `8..=128` when used.
    pub px_size: u16,
    /// Font-specific glyph identifier; rasterization narrows it to `u16`.
    pub glyph_id: u32,
    /// `round(dpr * 100)` to separate HiDPI cache buckets.
    pub scale_100: u16,
}

/// UV layout and metrics for a glyph in the atlas.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::text::Glyph;
/// let glyph = Glyph { uv_min: [0.0, 0.0], uv_max: [0.5, 0.5],
///     size_px: [8.0, 10.0], offset_px: [1.0, -2.0], advance_px: 0.0 };
/// assert_eq!(glyph.size_px, [8.0, 10.0]);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    /// Inclusive lower UV corner in normalized atlas coordinates.
    pub uv_min: [f32; 2],
    /// Exclusive upper UV corner in normalized atlas coordinates.
    pub uv_max: [f32; 2],
    /// Raster bitmap width and height in physical pixels.
    pub size_px: [f32; 2],
    /// Bitmap offset from the physical glyph pen position.
    pub offset_px: [f32; 2],
    /// Reserved advance in physical pixels; currently zero because layout owns advance.
    pub advance_px: f32,
}

/// Per-frame atlas cache statistics.
///
/// Counters saturate at `u32::MAX`; `pages_active` reflects the current atlas,
/// including pages allocated in earlier frames.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::text::TextAtlasStats;
/// let stats = TextAtlasStats::default();
/// assert_eq!((stats.hits, stats.misses, stats.pages_active), (0, 0, 0));
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TextAtlasStats {
    /// Cache hits in the current frame.
    pub hits: u32,
    /// Cache misses in the current frame.
    pub misses: u32,
    /// Glyph bitmaps uploaded in the current frame.
    pub rasterized: u32,
    /// Atlas page resets caused by eviction in the current frame.
    pub resets: u32,
    /// Allocations skipped because every candidate page was frame-pinned.
    pub evictions_blocked: u32,
    /// Glyphs skipped for missing faces, raster failures, or allocation failure.
    pub glyphs_skipped: u32,
    /// Total allocated atlas pages after the latest operation.
    pub pages_active: u32,
}

#[derive(Debug)]
/// Shelf allocator cursor for one atlas page.
struct Shelf {
    /// Next free horizontal texel coordinate on the current shelf.
    x: u32,
    /// Top texel coordinate of the current shelf.
    y: u32,
    /// Maximum glyph height in texels on the current shelf.
    h: u32,
}

/// Texture, binding, and allocator state for one atlas page.
struct AtlasPage {
    /// RGBA8 glyph texture owned by this page.
    texture: wgpu::Texture,
    /// Bind group exposing `texture` and the shared sampler to shaders.
    bind_group: wgpu::BindGroup,
    /// Monotonic shelf allocator state for new glyph rectangles.
    shelf: Shelf,
}

/// Multi-page GPU glyph atlas with LRU eviction.
///
/// The atlas allocates one 1024-square RGBA8 page eagerly and grows to
/// [`MAX_ATLAS_PAGES`]. Pinned pages cannot be reset during a frame. Unpinned
/// eviction clears an entire least-recently-used page, not an individual glyph.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::text::TextAtlas;
/// let _: usize = std::mem::size_of::<TextAtlas>();
/// ```
pub struct TextAtlas {
    /// Width and height in physical texels of every square page.
    tex_size: u32,
    /// Maximum number of pages, representable by the `u8` glyph page index.
    max_pages: u8,
    /// Shared sampler used by every atlas page bind group.
    sampler: wgpu::Sampler,

    /// Allocated pages in stable `u8` index order.
    pages: Vec<AtlasPage>,

    /// Cached glyph metadata paired with its page index.
    glyphs: HashMap<GlyphKey, (u8, Glyph)>,
    /// Least-recently-used glyph keys, oldest at the front.
    lru: VecDeque<GlyphKey>,
    /// Per-page pin flags preventing reset during the active frame.
    frame_pinned_pages: Vec<bool>,
    /// Counters accumulated during the active frame.
    frame_stats: TextAtlasStats,

    /// Swash scale context reused for CPU glyph rasterization.
    scale_cx: ScaleContext,
}

/// Scoped atlas access for one frame (pins pages until drop).
///
/// Dropping the guard calls `finish_frame`, unpinning every page. The guard's
/// mutable borrow prevents other atlas access during the frame.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::text::TextAtlasFrame;
/// let _: usize = std::mem::size_of::<TextAtlasFrame<'static>>();
/// ```
pub struct TextAtlasFrame<'a> {
    /// Exclusively borrowed atlas whose pages remain pinned for this guard.
    atlas: &'a mut TextAtlas,
}

impl<'a> TextAtlasFrame<'a> {
    /// Looks up or rasterizes a glyph and pins its page until this guard drops.
    ///
    /// Returns `None` for invalid font data, a missing glyph image, an oversized
    /// allocation, or when all pages are pinned and full.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::text::{GlyphKey, TextAtlasFrame};
    /// fn lookup(frame: &mut TextAtlasFrame<'_>, device: &wgpu::Device,
    ///     queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, font: &[u8], key: GlyphKey) {
    ///     let _glyph: Option<(u8, ailloli_ui_render_wgpu::text::Glyph)> =
    ///         frame.get_or_rasterize(device, queue, layout, key, font);
    /// }
    /// ```
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

    /// Records one glyph skipped because its face bytes were unavailable.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::text::TextAtlasFrame;
    /// fn missing(frame: &mut TextAtlasFrame<'_>) { frame.record_missing_face(); }
    /// ```
    pub fn record_missing_face(&mut self) {
        self.atlas.record_missing_face();
    }

    /// Returns current per-frame counters and allocated page count.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::text::{TextAtlasFrame, TextAtlasStats};
    /// fn stats(frame: &TextAtlasFrame<'_>) -> TextAtlasStats { frame.stats() }
    /// ```
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
    /// Creates an atlas, sampler, and the mandatory first 1024-square page.
    ///
    /// The supplied bind-group layout must contain a filterable 2D texture at
    /// binding 0 and a filtering sampler at binding 1.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::text::TextAtlas;
    /// fn create(device: &wgpu::Device, queue: &wgpu::Queue,
    ///     layout: &wgpu::BindGroupLayout) -> TextAtlas {
    ///     TextAtlas::new(device, queue, layout)
    /// }
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::text::TextAtlas;
    /// fn binding(atlas: &TextAtlas) -> &wgpu::BindGroup { atlas.bind_group() }
    /// ```
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.pages[0].bind_group
    }

    /// Bind group for the given atlas page index.
    ///
    /// # Panics
    ///
    /// Panics if `page_idx >= page_count()`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::text::TextAtlas;
    /// fn first(atlas: &TextAtlas) -> &wgpu::BindGroup { atlas.page_bind_group(0) }
    /// ```
    pub fn page_bind_group(&self, page_idx: u8) -> &wgpu::BindGroup {
        &self.pages[page_idx as usize].bind_group
    }

    /// Returns the number of allocated pages, always in `1..=MAX_ATLAS_PAGES`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::text::TextAtlas;
    /// fn pages(atlas: &TextAtlas) -> usize { atlas.page_count() }
    /// ```
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Starts a frame and returns a guard that unpins pages on drop.
    ///
    /// Calling this discards the previous frame's counters and pins, so guards
    /// must not conceptually overlap.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::text::TextAtlas;
    /// fn frame(atlas: &mut TextAtlas) { let _guard = atlas.begin_frame(); }
    /// ```
    pub fn begin_frame(&mut self) -> TextAtlasFrame<'_> {
        self.start_frame();
        TextAtlasFrame { atlas: self }
    }

    /// Resets per-frame counters and clears all page pins.
    ///
    /// Prefer [`Self::begin_frame`] when scoped access is practical.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::text::TextAtlas;
    /// fn start(atlas: &mut TextAtlas) { atlas.start_frame(); }
    /// ```
    pub fn start_frame(&mut self) {
        self.frame_pinned_pages.clear();
        self.frame_pinned_pages.resize(self.pages.len(), false);
        self.frame_stats = TextAtlasStats {
            pages_active: self.pages.len() as u32,
            ..TextAtlasStats::default()
        };
    }

    /// Clears page pins without changing counters, glyphs, or allocations.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::text::TextAtlas;
    /// fn finish(atlas: &mut TextAtlas) { atlas.finish_frame(); }
    /// ```
    pub fn finish_frame(&mut self) {
        self.frame_pinned_pages.fill(false);
    }

    /// Returns current frame counters with a live page-count snapshot.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::text::{TextAtlas, TextAtlasStats};
    /// fn stats(atlas: &TextAtlas) -> TextAtlasStats { atlas.stats() }
    /// ```
    pub fn stats(&self) -> TextAtlasStats {
        TextAtlasStats {
            pages_active: self.pages.len() as u32,
            ..self.frame_stats
        }
    }

    /// Allocates and zero-fills one atlas page, then creates its sample binding.
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

    /// Clears a page and removes every glyph and LRU entry resident on it.
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

    /// Looks up or rasterizes a glyph without protecting its page from eviction.
    ///
    /// Returns `(page_index, metrics)` or `None` on raster/allocation failure.
    /// Use [`Self::get_or_rasterize_pinned`] during frame preparation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::text::{GlyphKey, TextAtlas};
    /// fn lookup(atlas: &mut TextAtlas, device: &wgpu::Device, queue: &wgpu::Queue,
    ///     layout: &wgpu::BindGroupLayout, key: GlyphKey, font: &[u8]) {
    ///     let _ = atlas.get_or_rasterize(device, queue, layout, key, font);
    /// }
    /// ```
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

    /// Looks up or rasterizes a glyph and pins its page for the current frame.
    ///
    /// If every page is pinned and full, returns `None` and increments both
    /// `evictions_blocked` and `glyphs_skipped`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::text::{GlyphKey, TextAtlas};
    /// fn lookup(atlas: &mut TextAtlas, device: &wgpu::Device, queue: &wgpu::Queue,
    ///     layout: &wgpu::BindGroupLayout, key: GlyphKey, font: &[u8]) {
    ///     let _ = atlas.get_or_rasterize_pinned(device, queue, layout, key, font);
    /// }
    /// ```
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

    /// Saturating-increments the missing-face/skip counter.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::text::TextAtlas;
    /// fn missing(atlas: &mut TextAtlas) { atlas.record_missing_face(); }
    /// ```
    pub fn record_missing_face(&mut self) {
        self.frame_stats.glyphs_skipped = self.frame_stats.glyphs_skipped.saturating_add(1);
    }

    /// Shared lookup, raster, page allocation, upload, and optional pin path.
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

    /// Marks one page unavailable to eviction for the current frame.
    fn pin_page(&mut self, page_idx: u8) {
        let idx = page_idx as usize;
        if idx >= self.frame_pinned_pages.len() {
            self.frame_pinned_pages.resize(idx + 1, false);
        }
        self.frame_pinned_pages[idx] = true;
    }

    /// Moves a glyph key to the most-recently-used end of the queue.
    fn touch_lru(&mut self, key: GlyphKey) {
        self.lru.retain(|k| k != &key);
        self.lru.push_back(key);
    }

    /// Rasterizes one glyph through Swash, returning mask and screen-space placement.
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

    /// Allocates a shelf region, grows a page, or resets the oldest eligible page.
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

    /// Returns the page containing the globally oldest cached glyph.
    fn oldest_page_idx(&self) -> Option<u8> {
        let key = self.lru.front()?;
        let (page, _) = self.glyphs.get(key)?;
        Some(*page)
    }

    /// Returns the oldest page not pinned by the current frame.
    fn oldest_unpinned_page_idx(&self) -> Option<u8> {
        oldest_unpinned_page_idx_from(&self.lru, &self.glyphs, &self.frame_pinned_pages)
    }

    /// Allocates one rectangle in a page's row-shelf cursor.
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

/// Finds the first LRU glyph whose page is not marked pinned.
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
/// Verifies LRU victim selection while current-frame atlas pages are pinned.
mod tests {
    use super::*;

    /// Creates a stable key that varies only by glyph identifier.
    fn key(glyph_id: u32) -> GlyphKey {
        GlyphKey {
            face_id: 1,
            font_index: 0,
            px_size: 14,
            glyph_id,
            scale_100: 100,
        }
    }

    /// Creates a minimal one-pixel glyph entry for page-victim scenarios.
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
