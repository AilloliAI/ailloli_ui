use std::sync::Arc;

use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_core::{Color, IconId, SvgSource};
use ailloli_ui_runtime::component::{IntoView, View, Widget};
use ailloli_ui_runtime::input::EventCtx;
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawCmd, DrawImage};
use lucide_icons::Icon as LucideIcon;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};

/// Declarative icon widget (Lucide, Devicon, or custom SVG).
///
/// For font glyphs, `tint` replaces the mask color.
/// For full-color SVGs, use `Color::WHITE` to preserve native colors.
pub struct Icon {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    source: IconId,
    tint: Color,
    size: f32,
    interactive_tint: bool,
    rotation_rad: f32,
}

crate::impl_layout_builders_unit!(Icon);

impl Icon {
    pub fn new(source: impl Into<IconId>) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            source: source.into(),
            tint: Color::WHITE,
            size: 16.0,
            interactive_tint: true,
            rotation_rad: 0.0,
        }
    }

    pub fn lucide(icon: LucideIcon) -> Self {
        Self::new(IconId::Lucide(icon))
    }

    pub fn devicon(ch: char) -> Self {
        Self::new(IconId::Devicon(ch))
    }

    pub fn svg_static(bytes: &'static [u8]) -> Self {
        Self::new(IconId::Svg(SvgSource::Static(bytes)))
    }

    pub fn svg_bytes(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self::new(IconId::Svg(SvgSource::Owned(bytes.into())))
    }

    pub fn svg_str(s: impl Into<Arc<str>>) -> Self {
        Self::new(IconId::Svg(SvgSource::Str(s.into())))
    }

    pub fn tint(mut self, color: Color) -> Self {
        self.tint = color;
        self
    }

    pub fn size(mut self, px: f32) -> Self {
        self.size = px.max(0.0);
        self
    }

    /// Enables or disables automatic hover/pressed tint adjustments.
    ///
    /// Disable this for full-color brand artwork.
    pub fn interactive_tint(mut self, enabled: bool) -> Self {
        self.interactive_tint = enabled;
        self
    }

    pub fn rotation_rad(mut self, radians: f32) -> Self {
        self.rotation_rad = if radians.is_finite() { radians } else { 0.0 };
        self
    }
}

impl From<IconId> for Icon {
    fn from(source: IconId) -> Self {
        Self::new(source)
    }
}

struct IconWidget {
    layout: LayoutStyle,
    source: IconId,
    tint: Color,
    size: f32,
    interactive_tint: bool,
    rotation_rad: f32,
}

impl<A: 'static> Widget<A> for IconWidget {
    fn debug_name(&self) -> &'static str {
        "Icon"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic = Size::new(self.size, self.size);
        let size = apply_layout_size(intrinsic, self.layout, constraints);
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
        let i = ctx.interaction();
        let mut tint = self.tint;
        if self.interactive_tint && i.hovered {
            tint = Color::f32(
                tint.r + (1.0 - tint.r) * 0.14,
                tint.g + (1.0 - tint.g) * 0.14,
                tint.b + (1.0 - tint.b) * 0.14,
                tint.a,
            );
        }
        if self.interactive_tint && i.pressed {
            tint = Color::f32(
                (tint.r * 0.88).max(0.0),
                (tint.g * 0.88).max(0.0),
                (tint.b * 0.88).max(0.0),
                tint.a,
            );
        }
        ctx.push(DrawCmd::Image(DrawImage {
            rect: bounds,
            icon: self.source.clone(),
            tint,
            rotation_rad: self.rotation_rad,
        }));
    }

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }
}

impl<A: 'static> IntoView<A> for Icon {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(IconWidget {
                layout: self.layout,
                source: self.source,
                tint: self.tint,
                size: self.size,
                interactive_tint: self.interactive_tint,
                rotation_rad: self.rotation_rad,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}
