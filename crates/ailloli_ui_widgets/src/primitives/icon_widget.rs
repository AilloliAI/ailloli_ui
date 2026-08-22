//! Retained square icon widget for font glyphs and embedded SVG sources.

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
/// The defaults are a 16-logical-pixel square, white tint, interactive tinting,
/// and zero rotation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::IconId;
/// use ailloli_ui_widgets::primitives::Icon;
/// let icon = Icon::new(IconId::Close).size(20.0);
/// let _ = icon;
/// ```
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
    /// Creates a default icon from any [`IconId`]-compatible source.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::primitives::Icon;
    /// let icon = Icon::new(IconId::Check);
    /// let _ = icon;
    /// ```
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

    /// Creates a Lucide font-glyph icon.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::primitives::Icon;
    /// let icon = Icon::lucide(lucide_icons::Icon::Plus);
    /// let _ = icon;
    /// ```
    pub fn lucide(icon: LucideIcon) -> Self {
        Self::new(IconId::Lucide(icon))
    }

    /// Creates a Devicon font glyph from its Unicode scalar.
    ///
    /// The glyph is not checked against the bundled font at construction.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::primitives::Icon;
    /// let icon = Icon::devicon('\u{e7a8}');
    /// let _ = icon;
    /// ```
    pub fn devicon(ch: char) -> Self {
        Self::new(IconId::Devicon(ch))
    }

    /// Creates an SVG icon borrowing compile-time-static bytes.
    ///
    /// Bytes are retained and validated only by a renderer/cache when used.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::primitives::Icon;
    /// let icon = Icon::svg_static(b"<svg/>");
    /// let _ = icon;
    /// ```
    pub fn svg_static(bytes: &'static [u8]) -> Self {
        Self::new(IconId::Svg(SvgSource::Static(bytes)))
    }

    /// Creates an SVG icon from shared owned bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use ailloli_ui_widgets::primitives::Icon;
    /// let icon = Icon::svg_bytes(Arc::<[u8]>::from(&b"<svg/>"[..]));
    /// let _ = icon;
    /// ```
    pub fn svg_bytes(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self::new(IconId::Svg(SvgSource::Owned(bytes.into())))
    }

    /// Creates an SVG icon from shared UTF-8 markup.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use ailloli_ui_widgets::primitives::Icon;
    /// let icon = Icon::svg_str(Arc::<str>::from("<svg/>"));
    /// let _ = icon;
    /// ```
    pub fn svg_str(s: impl Into<Arc<str>>) -> Self {
        Self::new(IconId::Svg(SvgSource::Str(s.into())))
    }

    /// Replaces the render tint.
    ///
    /// Use [`Color::WHITE`] to preserve a full-color SVG.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, IconId};
    /// use ailloli_ui_widgets::primitives::Icon;
    /// let icon = Icon::new(IconId::Close).tint(Color::rgba(255, 0, 0, 1.0));
    /// let _ = icon;
    /// ```
    pub fn tint(mut self, color: Color) -> Self {
        self.tint = color;
        self
    }

    /// Sets the square extent in logical pixels, clamped to at least zero.
    ///
    /// Non-finite values follow [`f32::max`]; a `NaN` therefore resolves to
    /// zero while positive infinity remains unbounded for later constraints.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::primitives::Icon;
    /// let icon = Icon::new(IconId::Close).size(-4.0);
    /// let _ = icon;
    /// ```
    pub fn size(mut self, px: f32) -> Self {
        self.size = px.max(0.0);
        self
    }

    /// Enables or disables automatic hover/pressed tint adjustments.
    ///
    /// Disable this for full-color brand artwork.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::primitives::Icon;
    /// let icon = Icon::new(IconId::Close).interactive_tint(false);
    /// let _ = icon;
    /// ```
    pub fn interactive_tint(mut self, enabled: bool) -> Self {
        self.interactive_tint = enabled;
        self
    }

    /// Sets clockwise renderer rotation in radians.
    ///
    /// `NaN` and either infinity are replaced with zero; finite angles are not
    /// normalized and may exceed one full turn.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::primitives::Icon;
    /// let icon = Icon::new(IconId::Close).rotation_rad(std::f32::consts::FRAC_PI_2);
    /// let _ = icon;
    /// ```
    pub fn rotation_rad(mut self, radians: f32) -> Self {
        self.rotation_rad = if radians.is_finite() { radians } else { 0.0 };
        self
    }
}

/// Converts an icon identifier with the same defaults as [`Icon::new`].
impl From<IconId> for Icon {
    fn from(source: IconId) -> Self {
        Self::new(source)
    }
}

/// Frozen render state used after the declarative builder becomes a view.
struct IconWidget {
    layout: LayoutStyle,
    source: IconId,
    tint: Color,
    size: f32,
    interactive_tint: bool,
    rotation_rad: f32,
}

/// Implements square layout and interaction-aware tint painting.
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

/// Converts the builder into a leaf retained widget.
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
