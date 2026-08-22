//! Per-engine paragraph shaping cache and quantized cache keys.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use ailloli_ui_core::TextStyle;
use ailloli_ui_text::{TextLayoutHandle, TextLayoutParams, TextSystem, WrapMode};

/// Internal identity for one shaped paragraph layout.
///
/// The key combines paragraph identity/revision, a full text hash and byte
/// length, font and size, wrap mode, and a width quantized to 1/64 logical
/// pixel. It deliberately omits paint-only color. Callers normally exercise it
/// indirectly through [`crate::EditorEngine`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::EditorEngine;
/// let mut engine = EditorEngine::new();
/// engine.clear_caches(); // clears layouts identified by internal cache keys
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LayoutCacheKey {
    /// Zero-based logical paragraph index.
    pub paragraph_index: usize,
    /// Caller-owned paragraph revision.
    pub paragraph_revision: u64,
    /// Process-local hash of the complete newline-trimmed paragraph text.
    pub text_hash: u64,
    /// UTF-8 byte length used as an additional collision discriminator.
    pub text_len: usize,
    /// Font family identifier participating in shaping.
    pub font: ailloli_ui_core::FontId,
    /// Font size in logical pixels.
    pub px_size: u16,
    /// Line-breaking policy.
    pub wrap_mode: WrapMode,
    /// Width in 1/64 logical-pixel units, or [`u32::MAX`] for no-wrap.
    pub max_width_q: u32,
}

/// Constructs internal paragraph-layout keys.
impl LayoutCacheKey {
    /// Hashes and quantizes inputs into a cache key.
    ///
    /// Under wrapping, negative widths clamp to zero, `None` maps to zero, and
    /// conversion to `u32` follows Rust's saturating float-to-integer cast. In
    /// no-wrap mode the width is ignored and uses the [`u32::MAX`] sentinel.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::EditorEngine;
    /// let mut engine = EditorEngine::new();
    /// engine.clear_caches(); // the engine rebuilds keys from later frame inputs
    /// ```
    pub(crate) fn new(
        paragraph_index: usize,
        paragraph_revision: u64,
        text: &str,
        style: TextStyle,
        wrap_mode: WrapMode,
        max_width: Option<f32>,
    ) -> Self {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let max_width_q = match wrap_mode {
            WrapMode::NoWrap => u32::MAX,
            WrapMode::Word | WrapMode::WordOrAnywhere => max_width
                .map(|w| (w.max(0.0) * 64.0).round() as u32)
                .unwrap_or(0),
        };
        Self {
            paragraph_index,
            paragraph_revision,
            text_hash: hasher.finish(),
            text_len: text.len(),
            font: style.font,
            px_size: style.px_size,
            wrap_mode,
            max_width_q,
        }
    }
}

/// Per-engine paragraph layout cache layered over `TextSystem`.
///
/// Entries have no independent eviction policy; call [`LayoutCache::clear`] at
/// lifecycle boundaries or let the owning engine drop the cache. Stored handles
/// are cheap [`std::sync::Arc`] clones of prepared layouts.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::layout::LayoutCache;
/// let mut cache = LayoutCache::default();
/// cache.clear();
/// ```
#[derive(Debug, Clone, Default)]
pub struct LayoutCache {
    /// Prepared layouts indexed by all geometry-affecting inputs.
    layouts: HashMap<LayoutCacheKey, TextLayoutHandle>,
}

/// Resolves and invalidates cached paragraph layouts.
impl LayoutCache {
    /// Returns a cached layout or shapes and stores a new one.
    ///
    /// Internal callers must build `key` from the exact accompanying text,
    /// style, wrap mode, and width; a reused but inconsistent key returns its
    /// existing layout without comparing those arguments.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::EditorEngine;
    /// let mut engine = EditorEngine::new();
    /// engine.clear_caches(); // frame construction repopulates this internal cache
    /// ```
    pub(crate) fn layout_paragraph(
        &mut self,
        key: LayoutCacheKey,
        text: &str,
        style: TextStyle,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
        text_system: &mut TextSystem,
    ) -> TextLayoutHandle {
        if let Some(layout) = self.layouts.get(&key) {
            return layout.clone();
        }
        let layout = text_system.layout_cached(TextLayoutParams {
            text,
            style,
            max_width,
            wrap_mode,
        });
        self.layouts.insert(key, layout.clone());
        layout
    }

    /// Removes every prepared paragraph layout from this cache.
    ///
    /// This does not clear the separate cache owned by a [`TextSystem`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::layout::LayoutCache;
    /// let mut cache = LayoutCache::default();
    /// cache.clear();
    /// ```
    pub fn clear(&mut self) {
        self.layouts.clear();
    }
}
