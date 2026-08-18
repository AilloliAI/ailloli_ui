use ailloli_ui_core::ClipShape;

use super::{ClipStackSnapshot, DrawCmd, IsolatedEffects};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    Base,
    Overlay,
}

#[derive(Debug, Clone)]
pub struct Layer {
    pub kind: LayerKind,
    pub clip: ClipStackSnapshot,
    pub cmds: Vec<DrawCmd>,
    /// When true, content is rendered offscreen then composited (Phase 31).
    pub isolated: bool,
    /// Nesting depth among isolated scopes (0 = root). Phase 33.
    pub isolated_depth: u8,
    pub effects: IsolatedEffects,
}

impl Layer {
    pub fn base(clip: ClipStackSnapshot) -> Self {
        Self {
            kind: LayerKind::Base,
            clip,
            cmds: Vec::new(),
            isolated: false,
            isolated_depth: 0,
            effects: IsolatedEffects::default(),
        }
    }

    pub fn overlay(clip: ClipStackSnapshot) -> Self {
        Self {
            kind: LayerKind::Overlay,
            clip,
            cmds: Vec::new(),
            isolated: false,
            isolated_depth: 0,
            effects: IsolatedEffects::default(),
        }
    }

    pub fn isolated_base(clip: ClipStackSnapshot, effects: IsolatedEffects, depth: u8) -> Self {
        Self {
            kind: LayerKind::Base,
            clip,
            cmds: Vec::new(),
            isolated: true,
            isolated_depth: depth,
            effects,
        }
    }

    pub fn base_with_clip(clip: Option<ClipShape>, is_window_root: bool) -> Self {
        Self::base(ClipStackSnapshot::from_clip(clip, is_window_root))
    }

    pub fn overlay_with_clip(clip: Option<ClipShape>, is_window_root: bool) -> Self {
        Self::overlay(ClipStackSnapshot::from_clip(clip, is_window_root))
    }
}

/// Ordered paint layers (base + overlays) with optional per-layer clips.
#[derive(Debug, Default, Clone)]
pub struct Scene {
    pub layers: Vec<Layer>,
}

impl Scene {
    pub fn push_layer(&mut self, layer: Layer) {
        self.layers.push(layer);
    }

    pub fn push_base_layer(&mut self, clip: ClipStackSnapshot) {
        self.layers.push(Layer::base(clip));
    }

    pub fn push_overlay_layer(&mut self, clip: ClipStackSnapshot) {
        self.layers.push(Layer::overlay(clip));
    }

    /// Builds a scene with base then overlay layers (overlay on top).
    pub fn from_base_and_overlay(
        base_clip: Option<ClipShape>,
        base_cmds: Vec<DrawCmd>,
        overlay_clip: Option<ClipShape>,
        overlay_cmds: Vec<DrawCmd>,
    ) -> Self {
        let mut layers = Vec::new();
        let mut base = Layer::base_with_clip(base_clip, false);
        base.cmds = base_cmds;
        layers.push(base);
        let mut overlay = Layer::overlay_with_clip(overlay_clip, false);
        overlay.cmds = overlay_cmds;
        layers.push(overlay);
        Self { layers }
    }
}
