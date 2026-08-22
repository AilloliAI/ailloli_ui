//! Mutable paint context for draw commands, clips, and isolated effects.

use crate::input::{InputInteraction, InputSnapshot};
use crate::scene::clip_stack::{ClipStack, ClipStackSnapshot};
use crate::scene::draw_cmd::DrawCmd;
use crate::scene::isolated_effects::IsolatedEffects;
use crate::scene::scene_graph::{Layer, LayerKind, Scene};
use ailloli_ui_core::{ClipShape, Offset, Rect};
use ailloli_ui_text::{ParleyEngine, TextSystem};

/// Accumulates draw commands, clips, overlays, effects, and paint-time services.
///
/// Coordinates are window-space logical pixels unless a widget contract says
/// otherwise. A fresh context contains one empty base layer, an empty clip
/// stack, zero origin and frame time, default interaction state, and no text
/// system. It is single-threaded because it exclusively borrows `TextSystem`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::scene::PaintCtx;
/// let ctx = PaintCtx::new();
/// assert_eq!(ctx.layers.len(), 1);
/// assert!(ctx.overlay_layers.is_empty() && ctx.text_system.is_none());
/// ```
pub struct PaintContext<'a> {
    /// Draw commands grouped by clip layer (scissor / shader / stencil downstream).
    pub layers: Vec<Layer>,
    /// Top-level overlay layers appended after all base content.
    pub overlay_layers: Vec<Layer>,
    /// Mutable active clip entries, ordered outermost to innermost.
    pub clip_stack: ClipStack,
    /// Caller-managed paint translation in logical pixels.
    pub origin: Offset,
    /// Snapshot corresponding to the active clip stack.
    pub clip: ClipStackSnapshot,
    /// Optional shared shaping engine and prepared-layout cache.
    pub text_system: Option<&'a mut TextSystem>,
    input: InputSnapshot,
    current_interaction: InputInteraction,
    frame_time_ms: u128,
    /// Indices into `layers` for active isolated scopes (outermost first).
    isolated_scope_stack: Vec<usize>,
    overlay_target_stack: Vec<usize>,
    default_overlay_layer: Option<usize>,
}

/// Short alias for [`PaintCtx`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::scene::PaintCtx;
/// let ctx = PaintCtx::new();
/// assert_eq!(ctx.origin.x, 0.0);
/// ```
pub type PaintCtx<'a> = PaintContext<'a>;

/// Implements the `Default` contract for `PaintContext<'a>`.
impl<'a> Default for PaintContext<'a> {
    /// Constructs the documented default value.
    fn default() -> Self {
        Self::new()
    }
}

/// Provides the operations defined for `PaintContext<'a>`.
impl<'a> PaintContext<'a> {
    /// Creates an empty paint accumulator with one base layer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::{LayerKind, PaintCtx};
    /// let ctx = PaintCtx::new();
    /// assert_eq!(ctx.layers[0].kind, LayerKind::Base);
    /// assert!(ctx.clip.is_empty());
    /// ```
    pub fn new() -> Self {
        let mut s = Self {
            layers: Vec::new(),
            overlay_layers: Vec::new(),
            clip_stack: ClipStack::new(),
            origin: Offset::default(),
            clip: ClipStackSnapshot::empty(),
            text_system: None,
            input: InputSnapshot::default(),
            current_interaction: InputInteraction::default(),
            frame_time_ms: 0,
            isolated_scope_stack: Vec::new(),
            overlay_target_stack: Vec::new(),
            default_overlay_layer: None,
        };
        s.layers.push(Layer::base(ClipStackSnapshot::empty()));
        s
    }

    /// Creates a fresh context borrowing a text system.
    ///
    /// Input interaction stays at its default and frame time stays zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// use ailloli_ui_text::TextSystem;
    /// let mut text = TextSystem::new();
    /// let ctx = PaintCtx::with_text_system(&mut text);
    /// assert!(ctx.text_system.is_some());
    /// ```
    pub fn with_text_system(text_system: &'a mut TextSystem) -> Self {
        let mut s = Self::new();
        s.text_system = Some(text_system);
        s
    }

    /// Creates a text-enabled context with an input snapshot and frame timestamp.
    ///
    /// `frame_time_ms` is an opaque monotonic host timestamp in milliseconds;
    /// it is stored verbatim and is not compared with wall-clock time.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::InputSnapshot;
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// use ailloli_ui_text::TextSystem;
    /// let mut text = TextSystem::new();
    /// let ctx = PaintCtx::with_text_system_and_input(
    ///     &mut text, InputSnapshot::default(), 250,
    /// );
    /// assert_eq!(ctx.frame_time_ms(), 250);
    /// ```
    pub fn with_text_system_and_input(
        text_system: &'a mut TextSystem,
        input: InputSnapshot,
        frame_time_ms: u128,
    ) -> Self {
        let mut s = Self::with_text_system(text_system);
        s.input = input;
        s.frame_time_ms = frame_time_ms;
        s
    }

    /// Borrows the underlying Parley engine, or returns `None` without text services.
    ///
    /// Mutations through the engine bypass `TextSystem` cache accounting; prefer
    /// the higher-level text-system APIs unless direct shaping is required.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// let mut ctx = PaintCtx::new();
    /// assert!(ctx.text_engine_mut().is_none());
    /// ```
    pub fn text_engine_mut(&mut self) -> Option<&mut ParleyEngine> {
        self.text_system.as_mut().map(|ts| ts.parley_engine_mut())
    }

    /// Appends a command to the current base or innermost isolated layer.
    ///
    /// If public mutation removed all base layers, this method first recreates
    /// one with the current clip snapshot. Commands preserve call order.
    ///
    /// # Panics
    ///
    /// May panic if caller mutation leaves an active isolated-layer index that
    /// no longer exists in `layers`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, Rect};
    /// use ailloli_ui_runtime::scene::{DrawCmd, DrawRect, PaintCtx};
    /// let mut ctx = PaintCtx::new();
    /// ctx.push(DrawCmd::Rect(DrawRect { rect: Rect::new(0.0, 0.0, 4.0, 4.0), color: Color::WHITE }));
    /// assert_eq!(ctx.layers[0].cmds.len(), 1);
    /// ```
    pub fn push(&mut self, cmd: DrawCmd) {
        if self.layers.is_empty() {
            self.layers.push(Layer::base(self.clip.clone()));
        }
        let idx = self.push_target_layer_index();
        self.layers[idx].cmds.push(cmd);
    }

    /// Pushes a command into the current top-level overlay target.
    ///
    /// Outside an explicit overlay clip, consecutive calls share one unclipped
    /// overlay layer. [`Self::into_scene`] places all overlay layers after base
    /// layers regardless of when this method was called.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, Rect};
    /// use ailloli_ui_runtime::scene::{DrawCmd, DrawRect, PaintCtx};
    /// let mut ctx = PaintCtx::new();
    /// ctx.push_overlay(DrawCmd::Rect(DrawRect { rect: Rect::new(0.0, 0.0, 4.0, 4.0), color: Color::WHITE }));
    /// assert_eq!(ctx.overlay_layers[0].cmds.len(), 1);
    /// ```
    pub fn push_overlay(&mut self, cmd: DrawCmd) {
        let idx = self.overlay_target_layer_index();
        self.overlay_layers[idx].cmds.push(cmd);
    }

    /// Pushes overlay commands into a layer clipped by a window-space rect.
    ///
    /// Overlay clips are top-level and intentionally do not inherit parent widget clips.
    /// Nested overlay scopes target the innermost layer. On normal return, an
    /// unscoped overlay command starts a fresh layer after the clipped scope,
    /// preserving procedural order.
    ///
    /// # Panics
    ///
    /// A panic in `f` propagates without popping the overlay target, so callers
    /// that catch unwinding must discard the context rather than continue it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, Rect};
    /// use ailloli_ui_runtime::scene::{DrawCmd, DrawRect, PaintCtx};
    /// let mut ctx = PaintCtx::new();
    /// let clip = Rect::new(10.0, 10.0, 20.0, 20.0);
    /// ctx.with_overlay_clip(clip, |ctx| ctx.push_overlay(DrawCmd::Rect(DrawRect {
    ///     rect: clip, color: Color::WHITE,
    /// })));
    /// assert_eq!(ctx.overlay_layers[0].clip.scissor_rect(), Some(clip));
    /// ```
    pub fn with_overlay_clip(&mut self, clip: Rect, f: impl FnOnce(&mut Self)) {
        let entered_from_default_target = self.overlay_target_stack.is_empty();
        let idx = self.overlay_layers.len();
        self.overlay_layers
            .push(Layer::overlay_with_clip(Some(ClipShape::Rect(clip)), false));
        self.overlay_target_stack.push(idx);
        f(self);
        self.overlay_target_stack.pop();
        if entered_from_default_target {
            // Preserve procedural paint order. The next unscoped overlay must
            // be appended after this clipped scope, never into its clip layer.
            self.default_overlay_layer = None;
        }
    }

    /// Resolves the destination base-layer index for a new command.
    fn push_target_layer_index(&self) -> usize {
        self.isolated_scope_stack
            .last()
            .copied()
            .unwrap_or_else(|| self.layers.len() - 1)
    }

    /// Resolves or lazily creates the destination overlay-layer index.
    fn overlay_target_layer_index(&mut self) -> usize {
        if let Some(idx) = self.overlay_target_stack.last().copied() {
            return idx;
        }
        if let Some(idx) = self.default_overlay_layer {
            return idx;
        }
        let idx = self.overlay_layers.len();
        self.overlay_layers
            .push(Layer::overlay(ClipStackSnapshot::empty()));
        self.default_overlay_layer = Some(idx);
        idx
    }

    /// Runs `f` with an additional nested rectangular clip and base layer.
    ///
    /// The full ancestor stack is retained. A new base pass is started both on
    /// entry and normal exit; empty passes are removed by [`Self::into_scene`].
    ///
    /// # Panics
    ///
    /// A panic in `f` propagates before the clip is popped, so a caught unwind
    /// leaves this context unbalanced.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, Rect};
    /// use ailloli_ui_runtime::scene::{DrawCmd, DrawRect, PaintCtx};
    /// let mut ctx = PaintCtx::new();
    /// let clip = Rect::new(0.0, 0.0, 10.0, 10.0);
    /// ctx.with_clip(clip, |ctx| ctx.push(DrawCmd::Rect(DrawRect { rect: clip, color: Color::WHITE })));
    /// let scene = ctx.into_scene();
    /// assert_eq!(scene.layers[0].clip.scissor_rect(), Some(clip));
    /// ```
    pub fn with_clip(&mut self, clip: Rect, f: impl FnOnce(&mut Self)) {
        self.with_clip_shape(ClipShape::Rect(clip), false, f);
    }

    /// Pushes a **single** rectangular clip for a new layer without inheriting the stack.
    ///
    /// Intersects `clip` with the current parent scissor when present. Useful for
    /// widget-local viewports that need a "fresh" clip stack (e.g. embedded
    /// surfaces, off-screen viewports) so they don't re-apply expensive ancestor
    /// masks (round mask, stencil) on every contained draw. Most widgets should
    /// prefer [`Self::with_clip`] which keeps the full ancestor stack.
    ///
    /// This is implemented as an isolated offscreen layer with no effects. If
    /// the requested rectangle does not intersect a parent scissor, the current
    /// compatibility behavior falls back to the requested rectangle rather
    /// than representing an empty clip.
    ///
    /// # Panics
    ///
    /// A panic in `f` propagates before the saved stack and snapshot are
    /// restored; a caught unwind leaves the context unsuitable for reuse.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, Rect};
    /// use ailloli_ui_runtime::scene::{DrawCmd, DrawRect, PaintCtx};
    /// let mut ctx = PaintCtx::new();
    /// let clip = Rect::new(2.0, 3.0, 8.0, 9.0);
    /// ctx.with_clip_isolated(clip, |ctx| ctx.push(DrawCmd::Rect(DrawRect { rect: clip, color: Color::WHITE })));
    /// assert!(ctx.into_scene().layers[0].isolated);
    /// ```
    pub fn with_clip_isolated(&mut self, clip: Rect, f: impl FnOnce(&mut Self)) {
        self.with_isolated_effects(clip, IsolatedEffects::default(), f);
    }

    /// Runs `f` in a fresh clipped offscreen layer with post-effects.
    ///
    /// The requested logical-pixel clip is intersected with the parent's coarse
    /// scissor when they overlap; disjoint rectangles currently fall back to
    /// the requested clip. Nested depth starts at zero and is cast to `u8`, so
    /// more than 256 nested scopes wrap the stored diagnostic depth.
    ///
    /// # Panics
    ///
    /// A panic in `f` prevents stack restoration. Public mutation of `layers`
    /// inside `f` can also invalidate the recorded isolated index.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, Rect};
    /// use ailloli_ui_runtime::scene::{DrawCmd, DrawRect, IsolatedEffects, PaintCtx};
    /// let mut ctx = PaintCtx::new();
    /// let clip = Rect::new(0.0, 0.0, 20.0, 20.0);
    /// ctx.with_isolated_effects(clip, IsolatedEffects { opacity: 0.5, ..Default::default() },
    ///     |ctx| ctx.push(DrawCmd::Rect(DrawRect { rect: clip, color: Color::WHITE })));
    /// assert_eq!(ctx.into_scene().layers[0].effects.opacity, 0.5);
    /// ```
    pub fn with_isolated_effects(
        &mut self,
        clip: Rect,
        effects: IsolatedEffects,
        f: impl FnOnce(&mut Self),
    ) {
        let clip = self
            .clip
            .scissor_rect()
            .and_then(|parent| clip.intersection(parent))
            .unwrap_or(clip);
        let saved_stack = std::mem::replace(&mut self.clip_stack, ClipStack::new());
        let saved_clip = self.clip.clone();
        self.clip_stack.push(ClipShape::Rect(clip), false);
        self.clip = self.clip_stack.snapshot();
        self.start_isolated_pass(effects);
        f(self);
        self.clip_stack = saved_stack;
        self.clip = saved_clip;
        self.end_isolated_scope();
    }

    /// Runs `f` with one additional clip shape and base-layer pass.
    ///
    /// `is_window_root` is preserved on the clip entry for downstream surface
    /// handling. The active clip snapshot and stack are restored on normal
    /// return, and a new pass is opened after the scope.
    ///
    /// # Panics
    ///
    /// A panic in `f` propagates without restoring the clip stack.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Color, Rect};
    /// use ailloli_ui_runtime::scene::{DrawCmd, DrawRect, PaintCtx};
    /// let mut ctx = PaintCtx::new();
    /// let rect = Rect::new(0.0, 0.0, 20.0, 10.0);
    /// ctx.with_clip_shape(ClipShape::Rect(rect), true, |ctx| {
    ///     ctx.push(DrawCmd::Rect(DrawRect { rect, color: Color::WHITE }));
    /// });
    /// assert!(ctx.into_scene().layers[0].clip.entries()[0].is_window_root);
    /// ```
    pub fn with_clip_shape(
        &mut self,
        clip: ClipShape,
        is_window_root: bool,
        f: impl FnOnce(&mut Self),
    ) {
        self.clip_stack.push(clip, is_window_root);
        self.clip = self.clip_stack.snapshot();
        self.start_new_pass();
        f(self);
        self.clip_stack.pop();
        self.clip = self.clip_stack.snapshot();
        self.start_new_pass();
    }

    /// Clones the active clip snapshot.
    ///
    /// The returned snapshot remains unchanged after later scope transitions.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// assert!(PaintCtx::new().current_clip().is_empty());
    /// ```
    pub fn current_clip(&self) -> ClipStackSnapshot {
        self.clip.clone()
    }

    /// Returns the coarse axis-aligned intersection of active clip bounds.
    ///
    /// `None` means no active clip or a disjoint stack.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// assert_eq!(PaintCtx::new().current_clip_bbox(), None);
    /// ```
    pub fn current_clip_bbox(&self) -> Option<Rect> {
        self.clip.scissor_rect()
    }

    /// Replaces paint-time interaction flags and returns the previous value.
    ///
    /// This does not invalidate any element; the paint traversal uses it to
    /// scope a widget's focused, hovered, and pressed state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::InputInteraction;
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// let mut ctx = PaintCtx::new();
    /// let previous = ctx.set_current_interaction(InputInteraction { hovered: true, ..Default::default() });
    /// assert!(!previous.hovered && ctx.is_hovered());
    /// ```
    pub fn set_current_interaction(&mut self, interaction: InputInteraction) -> InputInteraction {
        let previous = self.current_interaction;
        self.current_interaction = interaction;
        previous
    }

    /// Returns all current paint-time interaction flags by value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// assert_eq!(PaintCtx::new().interaction(), Default::default());
    /// ```
    pub fn interaction(&self) -> InputInteraction {
        self.current_interaction
    }

    /// Returns whether the currently painted widget owns keyboard focus.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// assert!(!PaintCtx::new().is_focused());
    /// ```
    pub fn is_focused(&self) -> bool {
        self.current_interaction.focused
    }

    /// Whether this element or a descendant owns keyboard focus.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// assert!(!PaintCtx::new().has_focus_within());
    /// ```
    pub fn has_focus_within(&self) -> bool {
        self.current_interaction.focus_within
    }

    /// Returns whether the pointer currently hovers the painted widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// assert!(!PaintCtx::new().is_hovered());
    /// ```
    pub fn is_hovered(&self) -> bool {
        self.current_interaction.hovered
    }

    /// Returns whether the painted widget owns an active pointer press.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// assert!(!PaintCtx::new().is_pressed());
    /// ```
    pub fn is_pressed(&self) -> bool {
        self.current_interaction.pressed
    }

    /// Returns the opaque host frame timestamp in milliseconds.
    ///
    /// A context created without explicit input uses zero. This is not elapsed
    /// time and arithmetic is the caller's responsibility.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// assert_eq!(PaintCtx::new().frame_time_ms(), 0);
    /// ```
    pub fn frame_time_ms(&self) -> u128 {
        self.frame_time_ms
    }

    /// Returns the copyable input snapshot used to derive widget interaction.
    ///
    /// This is crate-visible because widgets should use the focused/hovered/
    /// pressed accessors rather than depend on router internals.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// let ctx = PaintCtx::new();
    /// assert!(!ctx.is_focused() && !ctx.is_hovered());
    /// ```
    pub(crate) fn input_snapshot(&self) -> InputSnapshot {
        self.input
    }

    /// Opens an empty base layer carrying the active clip snapshot.
    fn start_new_pass(&mut self) {
        self.layers.push(Layer::base(self.clip.clone()));
    }

    /// Opens an isolated base layer and records it as the current push target.
    fn start_isolated_pass(&mut self, effects: IsolatedEffects) {
        let depth = self.isolated_scope_stack.len() as u8;
        self.layers
            .push(Layer::isolated_base(self.clip.clone(), effects, depth));
        let layer_index = self.layers.len() - 1;
        self.isolated_scope_stack.push(layer_index);
    }

    /// Closes one isolated target and starts a base pass after the outermost scope.
    fn end_isolated_scope(&mut self) {
        self.isolated_scope_stack.pop();
        if self.isolated_scope_stack.is_empty() {
            self.start_new_pass();
        }
    }

    /// Consumes the context and returns non-empty layers in final paint order.
    ///
    /// Empty base and overlay layers are removed. Remaining base layers come
    /// first with `kind` normalized to [`LayerKind::Base`], followed by every
    /// overlay layer normalized to [`LayerKind::Overlay`]. Command order within
    /// each layer is unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, Rect};
    /// use ailloli_ui_runtime::scene::{DrawCmd, DrawRect, LayerKind, PaintCtx};
    /// let mut ctx = PaintCtx::new();
    /// ctx.push_overlay(DrawCmd::Rect(DrawRect { rect: Rect::new(0.0, 0.0, 1.0, 1.0), color: Color::WHITE }));
    /// let scene = ctx.into_scene();
    /// assert_eq!(scene.layers.len(), 1);
    /// assert_eq!(scene.layers[0].kind, LayerKind::Overlay);
    /// ```
    pub fn into_scene(mut self) -> Scene {
        self.layers.retain(|l| !l.cmds.is_empty());
        self.overlay_layers.retain(|l| !l.cmds.is_empty());
        let mut scene = Scene::default();
        for mut l in self.layers {
            l.kind = LayerKind::Base;
            scene.push_layer(l);
        }
        for mut l in self.overlay_layers {
            l.kind = LayerKind::Overlay;
            scene.push_layer(l);
        }
        scene
    }
}

/// Minimal command sink accepted by legacy paint APIs.
///
/// Implementations decide how optional clips are represented. The built-in
/// [`PaintCtx`] implementation opens a nested rectangular clip layer.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, Rect};
/// use ailloli_ui_runtime::scene::{DrawCmd, DrawRect, PaintCtx, Painter};
/// let mut painter = PaintCtx::new();
/// Painter::push(&mut painter, DrawCmd::Rect(DrawRect {
///     rect: Rect::new(0.0, 0.0, 2.0, 2.0), color: Color::WHITE,
/// }));
/// assert_eq!(painter.layers[0].cmds.len(), 1);
/// ```
pub trait Painter {
    /// Submits one command without adding a clip.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, Rect};
    /// use ailloli_ui_runtime::scene::{DrawCmd, DrawRect, PaintCtx, Painter};
    /// let mut painter = PaintCtx::new();
    /// painter.push(DrawCmd::Rect(DrawRect { rect: Rect::new(0.0, 0.0, 1.0, 1.0), color: Color::WHITE }));
    /// assert_eq!(painter.layers[0].cmds.len(), 1);
    /// ```
    fn push(&mut self, cmd: DrawCmd);

    /// Submits one command under `clip`, or unclipped when it is `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, Rect};
    /// use ailloli_ui_runtime::scene::{DrawCmd, DrawRect, PaintCtx, Painter};
    /// let mut painter = PaintCtx::new();
    /// let clip = Rect::new(0.0, 0.0, 3.0, 3.0);
    /// painter.push_clipped(DrawCmd::Rect(DrawRect { rect: clip, color: Color::WHITE }), Some(clip));
    /// assert_eq!(painter.into_scene().layers[0].clip.scissor_rect(), Some(clip));
    /// ```
    fn push_clipped(&mut self, cmd: DrawCmd, clip: Option<Rect>);
}

/// Implements the `Painter` contract for `PaintContext<'a>`.
impl<'a> Painter for PaintContext<'a> {
    /// Implements the push helper used by this module.
    fn push(&mut self, cmd: DrawCmd) {
        PaintContext::push(self, cmd);
    }

    /// Implements the push_clipped helper used by this module.
    fn push_clipped(&mut self, cmd: DrawCmd, clip: Option<Rect>) {
        if let Some(c) = clip {
            self.with_clip(c, |ctx| ctx.push(cmd));
        } else {
            self.push(cmd);
        }
    }
}

#[cfg(test)]
/// Tests implementation details.
mod tests {
    use super::*;
    use ailloli_ui_core::{ClipShape, Color, Rect};

    /// Implements the rect_cmd helper used by this module.
    fn rect_cmd(rect: Rect) -> DrawCmd {
        DrawCmd::Rect(crate::DrawRect {
            rect,
            color: Color::WHITE,
        })
    }

    #[test]
    /// Implements the isolated_clip_does_not_inherit_parent_stack helper used by this module.
    fn isolated_clip_does_not_inherit_parent_stack() {
        let root = ClipShape::RoundRect {
            rect: Rect::new(0.0, 0.0, 100.0, 80.0),
            radius: 12.0,
        };
        let inner = Rect::new(10.0, 10.0, 40.0, 20.0);
        let mut ctx = PaintCtx::new();

        ctx.with_clip_shape(root, true, |ctx| {
            ctx.with_clip_isolated(inner, |ctx| {
                ctx.push(rect_cmd(Rect::new(12.0, 12.0, 8.0, 8.0)));
            });
        });

        let scene = ctx.into_scene();
        assert_eq!(scene.layers.len(), 1);
        assert_eq!(
            scene.layers[0].clip.entries(),
            &[crate::scene::ClipEntry::new(ClipShape::Rect(inner), false)]
        );
    }

    #[test]
    /// Verifies that nested isolated parent cmds stay on parent layer.
    fn nested_isolated_parent_cmds_stay_on_parent_layer() {
        let outer = Rect::new(0.0, 0.0, 100.0, 100.0);
        let inner = Rect::new(10.0, 10.0, 30.0, 30.0);
        let mut ctx = PaintCtx::new();

        ctx.with_isolated_effects(outer, IsolatedEffects::default(), |ctx| {
            ctx.with_isolated_effects(inner, IsolatedEffects::default(), |ctx| {
                ctx.push(rect_cmd(Rect::new(12.0, 12.0, 8.0, 8.0)));
            });
            ctx.push(rect_cmd(Rect::new(50.0, 50.0, 8.0, 8.0)));
        });

        let scene = ctx.into_scene();
        assert_eq!(scene.layers.len(), 2);
        assert!(scene.layers[0].isolated);
        assert_eq!(scene.layers[0].isolated_depth, 0);
        assert_eq!(scene.layers[0].cmds.len(), 1);
        assert!(scene.layers[1].isolated);
        assert_eq!(scene.layers[1].isolated_depth, 1);
        assert_eq!(scene.layers[1].cmds.len(), 1);
    }

    #[test]
    /// Verifies that nested clip restores window root snapshot.
    fn nested_clip_restores_window_root_snapshot() {
        let root = ClipShape::RoundRect {
            rect: Rect::new(0.0, 0.0, 100.0, 80.0),
            radius: 12.0,
        };
        let inner = Rect::new(10.0, 10.0, 40.0, 20.0);
        let mut ctx = PaintCtx::new();

        ctx.with_clip_shape(root, true, |ctx| {
            ctx.push(rect_cmd(Rect::new(0.0, 0.0, 100.0, 80.0)));
            ctx.with_clip(inner, |ctx| {
                ctx.push(rect_cmd(Rect::new(0.0, 0.0, 100.0, 80.0)));
            });
            ctx.push(rect_cmd(Rect::new(0.0, 60.0, 20.0, 10.0)));
        });

        let scene = ctx.into_scene();

        assert_eq!(scene.layers.len(), 3);
        assert_eq!(
            scene.layers[0].clip.entries(),
            &[crate::scene::ClipEntry::new(root, true)]
        );
        assert_eq!(
            scene.layers[1].clip.entries(),
            &[
                crate::scene::ClipEntry::new(root, true),
                crate::scene::ClipEntry::new(ClipShape::Rect(inner), false)
            ]
        );
        assert_eq!(
            scene.layers[2].clip.entries(),
            &[crate::scene::ClipEntry::new(root, true)]
        );
    }

    #[test]
    /// Verifies that overlay layers are appended after base layers.
    fn overlay_layers_are_appended_after_base_layers() {
        let mut ctx = PaintCtx::new();

        ctx.push(rect_cmd(Rect::new(0.0, 0.0, 10.0, 10.0)));
        ctx.push_overlay(rect_cmd(Rect::new(1.0, 1.0, 8.0, 8.0)));

        let scene = ctx.into_scene();

        assert_eq!(scene.layers.len(), 2);
        assert_eq!(scene.layers[0].kind, LayerKind::Base);
        assert_eq!(scene.layers[1].kind, LayerKind::Overlay);
    }

    #[test]
    /// Verifies that overlay clip creates top level overlay clip.
    fn overlay_clip_creates_top_level_overlay_clip() {
        let clip = Rect::new(10.0, 12.0, 100.0, 80.0);
        let mut ctx = PaintCtx::new();

        ctx.with_overlay_clip(clip, |ctx| {
            ctx.push_overlay(rect_cmd(Rect::new(20.0, 24.0, 10.0, 10.0)));
        });

        let scene = ctx.into_scene();

        assert_eq!(scene.layers.len(), 1);
        assert_eq!(scene.layers[0].kind, LayerKind::Overlay);
        assert_eq!(
            scene.layers[0].clip.entries(),
            &[crate::scene::ClipEntry::new(ClipShape::Rect(clip), false)]
        );
    }

    #[test]
    /// Verifies that unscoped overlay after clipped scope uses a fresh unclipped layer.
    fn unscoped_overlay_after_clipped_scope_uses_a_fresh_unclipped_layer() {
        let clip = Rect::new(10.0, 12.0, 100.0, 80.0);
        let mut ctx = PaintCtx::new();

        ctx.push_overlay(rect_cmd(Rect::new(0.0, 0.0, 8.0, 8.0)));
        ctx.with_overlay_clip(clip, |ctx| {
            ctx.push_overlay(rect_cmd(Rect::new(20.0, 24.0, 10.0, 10.0)));
        });
        ctx.push_overlay(rect_cmd(Rect::new(150.0, 140.0, 12.0, 12.0)));

        let scene = ctx.into_scene();

        assert_eq!(scene.layers.len(), 3);
        assert!(scene.layers[0].clip.is_empty());
        assert_eq!(
            scene.layers[1].clip.entries(),
            &[crate::scene::ClipEntry::new(ClipShape::Rect(clip), false)]
        );
        assert!(scene.layers[2].clip.is_empty());
        assert!(scene.layers.iter().all(|layer| layer.cmds.len() == 1));
    }
}
