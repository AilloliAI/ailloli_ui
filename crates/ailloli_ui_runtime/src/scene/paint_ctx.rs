use crate::input::{InputInteraction, InputSnapshot};
use crate::scene::clip_stack::{ClipStack, ClipStackSnapshot};
use crate::scene::draw_cmd::DrawCmd;
use crate::scene::isolated_effects::IsolatedEffects;
use crate::scene::scene_graph::{Layer, LayerKind, Scene};
use ailloli_ui_core::{ClipShape, Offset, Rect};
use ailloli_ui_text::{ParleyEngine, TextSystem};

pub struct PaintContext<'a> {
    /// Draw commands grouped by clip layer (scissor / shader / stencil downstream).
    pub layers: Vec<Layer>,
    /// Top-level overlay layers appended after all base content.
    pub overlay_layers: Vec<Layer>,
    pub clip_stack: ClipStack,
    pub origin: Offset,
    pub clip: ClipStackSnapshot,
    pub text_system: Option<&'a mut TextSystem>,
    input: InputSnapshot,
    current_interaction: InputInteraction,
    frame_time_ms: u128,
    /// Indices into `layers` for active isolated scopes (outermost first).
    isolated_scope_stack: Vec<usize>,
    overlay_target_stack: Vec<usize>,
}

pub type PaintCtx<'a> = PaintContext<'a>;

impl<'a> Default for PaintContext<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> PaintContext<'a> {
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
        };
        s.layers.push(Layer::base(ClipStackSnapshot::empty()));
        s
    }

    pub fn with_text_system(text_system: &'a mut TextSystem) -> Self {
        let mut s = Self::new();
        s.text_system = Some(text_system);
        s
    }

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

    pub fn text_engine_mut(&mut self) -> Option<&mut ParleyEngine> {
        self.text_system.as_mut().map(|ts| ts.parley_engine_mut())
    }

    pub fn push(&mut self, cmd: DrawCmd) {
        if self.layers.is_empty() {
            self.layers.push(Layer::base(self.clip.clone()));
        }
        let idx = self.push_target_layer_index();
        self.layers[idx].cmds.push(cmd);
    }

    /// Pushes a draw command into the top-level overlay target, painted after base layers.
    pub fn push_overlay(&mut self, cmd: DrawCmd) {
        let idx = self.overlay_target_layer_index();
        self.overlay_layers[idx].cmds.push(cmd);
    }

    /// Pushes overlay commands into a layer clipped by a window-space rect.
    ///
    /// Overlay clips are top-level and intentionally do not inherit parent widget clips.
    pub fn with_overlay_clip(&mut self, clip: Rect, f: impl FnOnce(&mut Self)) {
        let idx = self.overlay_layers.len();
        self.overlay_layers
            .push(Layer::overlay_with_clip(Some(ClipShape::Rect(clip)), false));
        self.overlay_target_stack.push(idx);
        f(self);
        self.overlay_target_stack.pop();
    }

    fn push_target_layer_index(&self) -> usize {
        self.isolated_scope_stack
            .last()
            .copied()
            .unwrap_or_else(|| self.layers.len() - 1)
    }

    fn overlay_target_layer_index(&mut self) -> usize {
        if let Some(idx) = self.overlay_target_stack.last().copied() {
            return idx;
        }
        if self.overlay_layers.is_empty() {
            self.overlay_layers
                .push(Layer::overlay(ClipStackSnapshot::empty()));
        }
        self.overlay_layers.len() - 1
    }

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
    pub fn with_clip_isolated(&mut self, clip: Rect, f: impl FnOnce(&mut Self)) {
        self.with_isolated_effects(clip, IsolatedEffects::default(), f);
    }

    /// Pushes an isolated layer (fresh clip stack) with optional post-effects.
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

    pub fn current_clip(&self) -> ClipStackSnapshot {
        self.clip.clone()
    }

    pub fn current_clip_bbox(&self) -> Option<Rect> {
        self.clip.scissor_rect()
    }

    pub fn set_current_interaction(&mut self, interaction: InputInteraction) -> InputInteraction {
        let previous = self.current_interaction;
        self.current_interaction = interaction;
        previous
    }

    pub fn interaction(&self) -> InputInteraction {
        self.current_interaction
    }

    pub fn is_focused(&self) -> bool {
        self.current_interaction.focused
    }

    pub fn is_hovered(&self) -> bool {
        self.current_interaction.hovered
    }

    pub fn is_pressed(&self) -> bool {
        self.current_interaction.pressed
    }

    pub fn frame_time_ms(&self) -> u128 {
        self.frame_time_ms
    }

    pub(crate) fn input_snapshot(&self) -> InputSnapshot {
        self.input
    }

    fn start_new_pass(&mut self) {
        self.layers.push(Layer::base(self.clip.clone()));
    }

    fn start_isolated_pass(&mut self, effects: IsolatedEffects) {
        let depth = self.isolated_scope_stack.len() as u8;
        self.layers
            .push(Layer::isolated_base(self.clip.clone(), effects, depth));
        let layer_index = self.layers.len() - 1;
        self.isolated_scope_stack.push(layer_index);
    }

    fn end_isolated_scope(&mut self) {
        self.isolated_scope_stack.pop();
        if self.isolated_scope_stack.is_empty() {
            self.start_new_pass();
        }
    }

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

pub trait Painter {
    fn push(&mut self, cmd: DrawCmd);
    fn push_clipped(&mut self, cmd: DrawCmd, clip: Option<Rect>);
}

impl<'a> Painter for PaintContext<'a> {
    fn push(&mut self, cmd: DrawCmd) {
        PaintContext::push(self, cmd);
    }

    fn push_clipped(&mut self, cmd: DrawCmd, clip: Option<Rect>) {
        if let Some(c) = clip {
            self.with_clip(c, |ctx| ctx.push(cmd));
        } else {
            self.push(cmd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailloli_ui_core::{ClipShape, Color, Rect};

    fn rect_cmd(rect: Rect) -> DrawCmd {
        DrawCmd::Rect(crate::DrawRect {
            rect,
            color: Color::WHITE,
        })
    }

    #[test]
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
}
