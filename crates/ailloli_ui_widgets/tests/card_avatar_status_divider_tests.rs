//! Card, avatar, status indicator, and divider layout/paint scenarios.

use ailloli_ui_core::event::{Event, Modifiers, MouseButton, PointerEvent};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Color, IconId, Point};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, View, Widget};
use ailloli_ui_runtime::input::InputRouter;
use ailloli_ui_runtime::layout::{LayoutCtx, LayoutEngine, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawCmd, DrawRect};
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{
    Avatar, AvatarTone, Card, CardStyle, CardVariant, Divider, DividerVariant, StatusIndicator,
    StatusTone, StatusVariant,
};

struct Leaf {
    size: Size,
    color: Option<Color>,
}

impl Widget<()> for Leaf {
    fn debug_name(&self) -> &'static str {
        "Leaf"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [ailloli_ui_runtime::layout::LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = constraints.constrain(self.size);
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        if let Some(color) = self.color {
            ctx.push(DrawCmd::Rect(DrawRect {
                rect: bounds,
                color,
            }));
        }
    }

    fn event(
        &self,
        _ctx: &mut ailloli_ui_runtime::input::EventCtx<()>,
        _event: &Event,
        _bounds: Rect,
        _layout: &LayoutResult,
    ) {
    }
}

#[test]
fn card_style_from_theme_uses_box_tokens() {
    let theme = ailloli_ui_core::Theme::default();
    let palette = theme.palette();

    let surface = CardStyle::from_theme(theme, CardVariant::Surface);
    assert_eq!(surface.background, palette.surface);
    assert_eq!(surface.border.colors.top, palette.border);
    assert_eq!(surface.radius, theme.radius().panel());
    assert!(surface.shadows.is_empty());

    let elevated = CardStyle::from_theme(theme, CardVariant::Elevated);
    assert_eq!(elevated.background, palette.surface_elevated);
    assert!(!elevated.shadows.is_empty());

    let accent = CardStyle::from_theme(theme, CardVariant::Accent);
    assert_eq!(accent.border.colors.top, palette.accent.with_alpha(0.55));
    assert!(!accent.shadows.is_empty());
}

#[test]
fn card_reuses_container_box_model_and_shadow_visual_bounds() {
    let root_view: View<()> = Card::elevated()
        .child(leaf(Size::new(20.0, 10.0)))
        .into_view();
    let (app, root) = layout_root(root_view, Constraints::loose(200.0, 120.0));
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();

    assert_eq!(layout.children.len(), 1);
    assert_eq!(layout.size, Size::new(46.0, 36.0));
    assert_eq!(layout.children[0].offset.x, 13.0);
    assert_eq!(layout.children[0].offset.y, 13.0);
    assert_eq!(layout.paint_bounds, Rect::new(0.0, 0.0, 46.0, 36.0));
    assert_ne!(layout.visual_bounds, layout.paint_bounds);

    let cmds = paint_cmds(&app);
    let shadow_idx = cmds
        .iter()
        .position(|cmd| matches!(cmd, DrawCmd::BoxShadow(_)))
        .expect("card shadow");
    let child_idx = cmds
        .iter()
        .position(|cmd| matches!(cmd, DrawCmd::Rect(rect) if rect.color == Color::rgb(5, 6, 7)))
        .expect("card child paint");
    let border_idx = cmds
        .iter()
        .position(|cmd| matches!(cmd, DrawCmd::Border(_)))
        .expect("card border");
    assert!(shadow_idx < border_idx);
    assert!(child_idx < border_idx);
}

#[test]
fn avatar_initials_layout_and_paint_are_stable() {
    let (app, root) = layout_root(
        Avatar::new("Alex Rivera").into_view(),
        Constraints::loose(80.0, 80.0),
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size, Size::new(40.0, 40.0));

    let cmds = paint_cmds(&app);
    assert!(cmds.iter().any(|cmd| matches!(cmd, DrawCmd::RRect(_))));
    assert!(cmds
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "AR")));
}

#[test]
fn avatar_icon_emits_draw_image() {
    let (app, root) = layout_root(
        Avatar::icon(IconId::Check)
            .tone(AvatarTone::Accent)
            .size(48.0)
            .into_view(),
        Constraints::loose(80.0, 80.0),
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size, Size::new(48.0, 48.0));

    let cmds = paint_cmds(&app);
    assert!(cmds.iter().any(|cmd| matches!(cmd, DrawCmd::Image(_))));
}

#[test]
fn status_indicator_paints_dot_ring_and_bars() {
    let palette = ailloli_ui_core::Theme::default().palette();

    let (dot_app, _) = layout_root(
        StatusIndicator::new(StatusTone::Success).into_view(),
        Constraints::loose(40.0, 40.0),
    );
    assert!(paint_cmds(&dot_app)
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.color == palette.success)));

    let (ring_app, _) = layout_root(
        StatusIndicator::new(StatusTone::Warning)
            .variant(StatusVariant::Ring)
            .size(14.0)
            .into_view(),
        Constraints::loose(40.0, 40.0),
    );
    assert!(paint_cmds(&ring_app)
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Border(_))));

    let (bars_app, _) = layout_root(
        StatusIndicator::new(StatusTone::Info)
            .variant(StatusVariant::Bars)
            .size(16.0)
            .into_view(),
        Constraints::loose(60.0, 40.0),
    );
    assert!(
        paint_cmds(&bars_app)
            .iter()
            .filter(|cmd| matches!(cmd, DrawCmd::RRect(_)))
            .count()
            >= 3
    );
}

#[test]
fn divider_layout_and_segments_are_stable() {
    let (solid_app, solid_root) = layout_root(
        Divider::horizontal()
            .length(80.0)
            .thickness(2.0)
            .into_view(),
        Constraints::loose(120.0, 20.0),
    );
    let layout = solid_app
        .tree
        .get(solid_root)
        .unwrap()
        .layout
        .as_ref()
        .unwrap();
    assert_eq!(layout.size, Size::new(80.0, 2.0));
    assert_eq!(rect_count(&solid_app), 1);

    let (vertical_app, vertical_root) = layout_root(
        Divider::vertical().length(70.0).thickness(3.0).into_view(),
        Constraints::loose(20.0, 120.0),
    );
    let layout = vertical_app
        .tree
        .get(vertical_root)
        .unwrap()
        .layout
        .as_ref()
        .unwrap();
    assert_eq!(layout.size, Size::new(3.0, 70.0));

    let (dashed_app, _) = layout_root(
        Divider::horizontal()
            .variant(DividerVariant::Dashed)
            .length(80.0)
            .into_view(),
        Constraints::loose(120.0, 20.0),
    );
    assert!(rect_count(&dashed_app) > 1);

    let (dotted_app, _) = layout_root(
        Divider::horizontal()
            .variant(DividerVariant::Dotted)
            .thickness(2.0)
            .length(24.0)
            .into_view(),
        Constraints::loose(120.0, 20.0),
    );
    assert!(rect_count(&dotted_app) > 1);
}

#[test]
fn visual_primitives_do_not_take_focus() {
    let samples = [
        Avatar::new("Alex Rivera").into_view(),
        StatusIndicator::new(StatusTone::Danger).into_view(),
        Divider::horizontal().into_view(),
    ];

    for view in samples {
        let (app, _) = layout_root(view, Constraints::loose(100.0, 40.0));
        let mut router = InputRouter::default();
        router.route_event(
            &app.tree,
            RuntimeHandle::new(),
            &pointer_button(4.0, 4.0, true),
        );
        assert_eq!(router.focused(), None);
    }
}

fn layout_root(
    root_view: View<()>,
    constraints: Constraints,
) -> (Runtime<()>, ailloli_ui_core::ElementId) {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root_id = app.reconcile(root_view);
    let mut text_system = TextSystem::new();
    app.layout(constraints, Scale::new(1.0), &mut text_system);
    (app, root_id)
}

fn paint_cmds(app: &Runtime<()>) -> Vec<DrawCmd> {
    let mut text_system = TextSystem::new();
    app.paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter().cloned())
        .collect()
}

fn rect_count(app: &Runtime<()>) -> usize {
    paint_cmds(app)
        .iter()
        .filter(|cmd| matches!(cmd, DrawCmd::Rect(_)))
        .count()
}

fn leaf(size: Size) -> View<()> {
    View::leaf(Leaf {
        size,
        color: Some(Color::rgb(5, 6, 7)),
    })
}

fn pointer_button(x: f32, y: f32, pressed: bool) -> Event {
    Event::Pointer(PointerEvent::Button {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        pressed,
        modifiers: Modifiers::default(),
    })
}
