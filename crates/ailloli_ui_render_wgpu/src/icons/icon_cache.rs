//! CPU rasterization and GPU caching for icon textures.

use ailloli_ui_core::IconId;
use ailloli_ui_devicons_font::{glyph_or_fallback, DEVICON_FONT_BYTES};
use fontdue::Font;
use lucide_icons::LUCIDE_FONT_BYTES;

use super::devicons::rasterize_devicon;
use super::lucide::lucide_char;
use super::raster::pad_rgba_rows;
use super::svg::rasterize_svg;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Cache key for a rasterized icon at a given size and DPR bucket.
///
/// `px_size` is the physical square side and `scale_100` is the rounded device
/// pixel ratio multiplied by 100. Both participate in equality and hashing.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::IconId;
/// use ailloli_ui_render_wgpu::IconKey;
/// let key = IconKey { icon: IconId::Plus, px_size: 24, scale_100: 150 };
/// assert_eq!((key.px_size, key.scale_100), (24, 150));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconKey {
    /// Logical icon source identity.
    pub icon: IconId,
    /// Raster texture side length in physical pixels.
    pub px_size: u16,
    /// Device-pixel-ratio bucket, `round(dpr * 100)`, clamped to `1..=u16::MAX`.
    pub scale_100: u16,
}

impl Hash for IconKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.icon.hash(state);
        self.px_size.hash(state);
        self.scale_100.hash(state);
    }
}

/// GPU texture + bind group for one cached icon.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::icons::IconGpu;
/// let _: usize = std::mem::size_of::<IconGpu>();
/// ```
#[derive(Debug)]
pub struct IconGpu {
    /// Owned RGBA8 texture keeping the view alive.
    pub texture: wgpu::Texture,
    /// Full texture view bound for sampling.
    pub view: wgpu::TextureView,
    /// Nearest-neighbor sampler used by icon rendering.
    pub sampler: wgpu::Sampler,
    /// Texture-and-sampler bind group for the textured pipeline.
    pub bind_group: wgpu::BindGroup,
    /// Actual square side length in physical pixels; at least eight.
    pub size_px: u32,
}

/// In-memory cache of rasterized Lucide / Devicon / SVG icons.
///
/// Entries live for the lifetime of the cache; there is no eviction. SVG parse
/// failure is cached as a transparent square, and unsupported Devicons use the
/// framework's generic-file glyph.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::icons::IconCache;
/// let cache = IconCache::new();
/// let _ = cache;
/// ```
pub struct IconCache {
    lucide_font: Font,
    devicon_font: Font,
    cache: HashMap<IconKey, IconGpu>,
}

impl Default for IconCache {
    fn default() -> Self {
        Self::new()
    }
}

impl IconCache {
    /// Loads the bundled Lucide and Devicon fonts and creates an empty cache.
    ///
    /// # Panics
    ///
    /// Panics only if a bundled, build-time font asset is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::icons::IconCache;
    /// let cache = IconCache::new();
    /// let _ = cache;
    /// ```
    pub fn new() -> Self {
        let lucide_font = Font::from_bytes(LUCIDE_FONT_BYTES, fontdue::FontSettings::default())
            .expect("load lucide font");
        let devicon_font = Font::from_bytes(DEVICON_FONT_BYTES, fontdue::FontSettings::default())
            .expect("load devicon font");
        Self {
            lucide_font,
            devicon_font,
            cache: HashMap::new(),
        }
    }

    /// Rasterizes one key to a tight square RGBA buffer on the CPU.
    ///
    /// Callers normalize the size before invoking this helper.
    fn rasterize_icon_rgba(&self, icon: &IconId, px_size: u32) -> Vec<u8> {
        match icon {
            IconId::Devicon(ch) => {
                rasterize_devicon(&self.devicon_font, glyph_or_fallback(*ch), px_size)
            }
            IconId::Svg(src) => rasterize_svg(src, px_size)
                .unwrap_or_else(|| vec![0u8; (px_size * px_size * 4) as usize]),
            _ => {
                let ch = lucide_char(icon);
                super::raster::rasterize_glyph_mask(&self.lucide_font, ch, px_size)
            }
        }
    }

    /// Uploads a tight RGBA square and builds its nearest-neighbor binding.
    fn upload_rgba_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        rgba: Vec<u8>,
        px_size: u32,
        label: &str,
    ) -> IconGpu {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: px_size,
                height: px_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let (padded, padded_bpr) = pad_rgba_rows(&rgba, px_size);
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &padded,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(padded_bpr),
                rows_per_image: Some(px_size),
            },
            wgpu::Extent3d {
                width: px_size,
                height: px_size,
                depth_or_array_layers: 1,
            },
        );

        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("icon sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("icon bind group"),
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        IconGpu {
            texture: tex,
            view,
            sampler,
            bind_group,
            size_px: px_size,
        }
    }

    /// Returns the cached GPU icon or rasterizes and uploads it once.
    ///
    /// `key.px_size` is clamped upward to eight for allocation and rasterization,
    /// while the original key is retained. The returned reference remains valid
    /// until the cache is mutably borrowed again.
    ///
    /// # Panics
    ///
    /// Panics on impossible cache inconsistency or GPU allocation validation
    /// failure. Invalid SVG itself produces a transparent texture instead.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{icons::IconCache, IconKey};
    /// fn upload<'a>(cache: &'a mut IconCache, device: &wgpu::Device,
    ///     queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout,
    ///     key: &IconKey) -> &'a ailloli_ui_render_wgpu::icons::IconGpu {
    ///     cache.get_or_create(device, queue, layout, key)
    /// }
    /// ```
    pub fn get_or_create(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        key: &IconKey,
    ) -> &IconGpu {
        if self.cache.contains_key(key) {
            return self.cache.get(key).expect("cache contains key");
        }

        let px_size = key.px_size.max(8) as u32;
        let rgba = self.rasterize_icon_rgba(&key.icon, px_size);
        let gpu = Self::upload_rgba_texture(
            device,
            queue,
            bind_group_layout,
            rgba,
            px_size,
            "icon texture",
        );
        self.cache.insert(key.clone(), gpu);
        self.cache.get(key).expect("cache contains key")
    }

    /// Looks up a previously uploaded icon without mutating the cache.
    ///
    /// Returns `None` before [`Self::get_or_create`] has inserted the exact key.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_render_wgpu::{icons::IconCache, IconKey};
    /// let cache = IconCache::new();
    /// let key = IconKey { icon: IconId::Plus, px_size: 16, scale_100: 100 };
    /// assert!(cache.get(&key).is_none());
    /// ```
    pub fn get(&self, key: &IconKey) -> Option<&IconGpu> {
        self.cache.get(key)
    }
}

#[cfg(test)]
/// Verifies fallback rasterization for unsupported Devicon glyphs.
mod tests {
    use super::IconCache;
    use ailloli_ui_core::IconId;
    use ailloli_ui_devicons_font::GENERIC_FILE_GLYPH;

    #[test]
    fn unsupported_devicon_rasterizes_as_the_generic_file_glyph() {
        let cache = IconCache::new();
        let unsupported = cache.rasterize_icon_rgba(&IconId::Devicon('\u{f303}'), 24);
        let fallback = cache.rasterize_icon_rgba(&IconId::Devicon(GENERIC_FILE_GLYPH), 24);
        assert_eq!(unsupported, fallback);
        assert!(fallback.iter().any(|channel| *channel != 0));
    }
}
