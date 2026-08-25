//! Immutable renderer-facing scene graph produced by the paint engine.

use ailloli_ui_core::ClipShape;

use super::{ClipStackSnapshot, DrawCmd, IsolatedEffects};

/// Paint ordering stratum for a scene layer.
///
/// Base layers are normally submitted first; overlays are intended to appear
/// above all base content. [`Scene`] preserves insertion order rather than
/// sorting by this discriminant.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::scene::LayerKind;
/// assert_ne!(LayerKind::Base, LayerKind::Overlay);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    /// Ordinary retained-tree content.
    Base,
    /// Top-level content intended to paint above base layers.
    Overlay,
}

/// Ordered draw commands sharing a clip and optional isolated effects.
///
/// `isolated_depth` is meaningful only when `isolated` is true. Constructors
/// initialize commands empty; public fields allow consumers to assemble custom
/// layers without validation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::scene::{ClipStackSnapshot, Layer, LayerKind};
/// let layer = Layer::base(ClipStackSnapshot::empty());
/// assert_eq!(layer.kind, LayerKind::Base);
/// assert!(layer.cmds.is_empty() && !layer.isolated);
/// ```
#[derive(Debug, Clone)]
pub struct Layer {
    /// Base or overlay paint stratum.
    pub kind: LayerKind,
    /// Immutable clip stack applied to every command in this layer.
    pub clip: ClipStackSnapshot,
    /// Draw commands in painter submission order.
    pub cmds: Vec<DrawCmd>,
    /// When true, content is rendered offscreen then composited (isolated compositor).
    pub isolated: bool,
    /// Nesting depth among isolated scopes (0 = root). nested isolated compositor.
    pub isolated_depth: u8,
    /// Opacity, blend, and filter parameters for an isolated layer.
    pub effects: IsolatedEffects,
}

/// Provides the operations defined for Layer.
impl Layer {
    /// Creates an empty, non-isolated base layer with no effects.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::{ClipStackSnapshot, Layer, LayerKind};
    /// let layer = Layer::base(ClipStackSnapshot::empty());
    /// assert_eq!(layer.kind, LayerKind::Base);
    /// ```
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

    /// Creates an empty, non-isolated overlay layer with no effects.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::{ClipStackSnapshot, Layer, LayerKind};
    /// let layer = Layer::overlay(ClipStackSnapshot::empty());
    /// assert_eq!(layer.kind, LayerKind::Overlay);
    /// ```
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

    /// Creates an empty isolated base layer at caller-supplied nesting `depth`.
    ///
    /// Depth uses `0` for the outermost isolated scope and is stored verbatim;
    /// this constructor does not check nesting consistency or overflow.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::{ClipStackSnapshot, IsolatedEffects, Layer};
    /// let layer = Layer::isolated_base(ClipStackSnapshot::empty(), IsolatedEffects::default(), 2);
    /// assert!(layer.isolated);
    /// assert_eq!(layer.isolated_depth, 2);
    /// ```
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

    /// Creates a base layer from zero or one clip shape.
    ///
    /// The root flag is ignored when `clip` is `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::Layer;
    /// assert!(Layer::base_with_clip(None, true).clip.is_empty());
    /// ```
    pub fn base_with_clip(clip: Option<ClipShape>, is_window_root: bool) -> Self {
        Self::base(ClipStackSnapshot::from_clip(clip, is_window_root))
    }

    /// Creates an overlay layer from zero or one clip shape.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::{Layer, LayerKind};
    /// assert_eq!(Layer::overlay_with_clip(None, false).kind, LayerKind::Overlay);
    /// ```
    pub fn overlay_with_clip(clip: Option<ClipShape>, is_window_root: bool) -> Self {
        Self::overlay(ClipStackSnapshot::from_clip(clip, is_window_root))
    }
}

/// Ordered paint layers (base + overlays) with optional per-layer clips.
///
/// The vector is the final submission order; [`Scene`] does not sort or merge
/// layers. An empty scene is valid and renders nothing.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::scene::Scene;
/// assert!(Scene::default().layers.is_empty());
/// ```
#[derive(Debug, Default, Clone)]
pub struct Scene {
    /// Layers in back-to-front submission order.
    pub layers: Vec<Layer>,
}

/// Provides the operations defined for Scene.
impl Scene {
    /// Appends a layer without reordering it by [`LayerKind`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::{ClipStackSnapshot, Layer, Scene};
    /// let mut scene = Scene::default();
    /// scene.push_layer(Layer::base(ClipStackSnapshot::empty()));
    /// assert_eq!(scene.layers.len(), 1);
    /// ```
    pub fn push_layer(&mut self, layer: Layer) {
        self.layers.push(layer);
    }

    /// Appends an empty base layer using `clip`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::{ClipStackSnapshot, LayerKind, Scene};
    /// let mut scene = Scene::default();
    /// scene.push_base_layer(ClipStackSnapshot::empty());
    /// assert_eq!(scene.layers[0].kind, LayerKind::Base);
    /// ```
    pub fn push_base_layer(&mut self, clip: ClipStackSnapshot) {
        self.layers.push(Layer::base(clip));
    }

    /// Appends an empty overlay layer using `clip`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::{ClipStackSnapshot, LayerKind, Scene};
    /// let mut scene = Scene::default();
    /// scene.push_overlay_layer(ClipStackSnapshot::empty());
    /// assert_eq!(scene.layers[0].kind, LayerKind::Overlay);
    /// ```
    pub fn push_overlay_layer(&mut self, clip: ClipStackSnapshot) {
        self.layers.push(Layer::overlay(clip));
    }

    /// Builds exactly two layers: base first and overlay second.
    ///
    /// Command vectors are moved without cloning. `None` clips create empty
    /// clip snapshots, and both root flags are `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::{LayerKind, Scene};
    /// let scene = Scene::from_base_and_overlay(None, vec![], None, vec![]);
    /// assert_eq!(scene.layers.len(), 2);
    /// assert_eq!((scene.layers[0].kind, scene.layers[1].kind), (LayerKind::Base, LayerKind::Overlay));
    /// ```
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
