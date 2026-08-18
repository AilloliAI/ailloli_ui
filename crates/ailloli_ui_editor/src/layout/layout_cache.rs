use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use ailloli_ui_core::TextStyle;
use ailloli_ui_text::{TextLayoutHandle, TextLayoutParams, TextSystem, WrapMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LayoutCacheKey {
    pub paragraph_index: usize,
    pub paragraph_revision: u64,
    pub text_hash: u64,
    pub text_len: usize,
    pub font: ailloli_ui_core::FontId,
    pub px_size: u16,
    pub wrap_mode: WrapMode,
    pub max_width_q: u32,
}

impl LayoutCacheKey {
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
#[derive(Debug, Clone, Default)]
pub struct LayoutCache {
    layouts: HashMap<LayoutCacheKey, TextLayoutHandle>,
}

impl LayoutCache {
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

    pub fn clear(&mut self) {
        self.layouts.clear();
    }
}
