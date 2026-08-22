//! Non-interactive horizontal and vertical separators.

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_core::{Color, Theme};
use ailloli_ui_runtime::component::{IntoView, View, Widget};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawCmd, DrawRect};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Main axis of a [`Divider`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::DividerOrientation;
/// assert_eq!(DividerOrientation::default(), DividerOrientation::Horizontal);
/// ```
pub enum DividerOrientation {
    /// Length runs left to right and thickness runs top to bottom.
    #[default]
    Horizontal,
    /// Length runs top to bottom and thickness runs left to right.
    Vertical,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Stroke pattern used by a [`Divider`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::DividerVariant;
/// assert_eq!(DividerVariant::default(), DividerVariant::Solid);
/// ```
pub enum DividerVariant {
    /// One rectangle filling the resolved bounds.
    #[default]
    Solid,
    /// Rectangular segments using [`DividerStyle::dash`] and `gap`.
    Dashed,
    /// Short segments whose length equals the resolved thickness.
    Dotted,
}

#[derive(Clone, Debug, PartialEq)]
/// Resolved color and logical-pixel metrics for a [`Divider`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::DividerStyle;
/// let style = DividerStyle::from_theme(Theme::dark());
/// assert_eq!(style.thickness, 1.0);
/// assert_eq!(style.length, 160.0);
/// ```
pub struct DividerStyle {
    /// Segment fill color.
    pub color: Color,
    /// Cross-axis thickness in logical pixels.
    pub thickness: f32,
    /// Main-axis intrinsic length in logical pixels.
    pub length: f32,
    /// Dashed segment length in logical pixels.
    pub dash: f32,
    /// Space between dashed or dotted segments in logical pixels.
    pub gap: f32,
}

impl Default for DividerStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl DividerStyle {
    /// Resolves the default separator color from `theme`.
    ///
    /// Geometry remains fixed at `1 × 160` logical pixels, with 10-pixel
    /// dashes and 6-pixel gaps.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::DividerStyle;
    /// let style = DividerStyle::from_theme(Theme::dark());
    /// assert_eq!(style.dash, 10.0);
    /// assert_eq!(style.gap, 6.0);
    /// ```
    pub fn from_theme(theme: Theme) -> Self {
        Self {
            color: theme.palette().border,
            thickness: 1.0,
            length: 160.0,
            dash: 10.0,
            gap: 6.0,
        }
    }
}

/// A decorative separator that does not receive focus or events.
///
/// Explicit layout width and height may override the style's intrinsic length
/// and thickness. Segments are clipped by construction to the resolved bounds.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::Divider;
/// let divider = Divider::horizontal();
/// let _ = divider;
/// ```
pub struct Divider {
    /// Layout configuration used to resolve intrinsic geometry.
    pub(crate) layout: LayoutStyle,
    /// Flex-item behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Main-axis direction.
    orientation: DividerOrientation,
    /// Stroke pattern.
    variant: DividerVariant,
    /// Resolved color and metrics.
    style: DividerStyle,
}

crate::impl_layout_builders_unit!(Divider);

impl Divider {
    /// Creates a solid horizontal divider using the default style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Divider;
    /// let divider = Divider::horizontal();
    /// let _ = divider;
    /// ```
    pub fn horizontal() -> Self {
        Self::new(DividerOrientation::Horizontal)
    }

    /// Creates a solid vertical divider using the default style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Divider;
    /// let divider = Divider::vertical();
    /// let _ = divider;
    /// ```
    pub fn vertical() -> Self {
        Self::new(DividerOrientation::Vertical)
    }

    /// Selects the solid, dashed, or dotted paint pattern.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Divider, DividerVariant};
    /// let divider = Divider::horizontal().variant(DividerVariant::Dashed);
    /// let _ = divider;
    /// ```
    pub fn variant(mut self, variant: DividerVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Sets cross-axis thickness in logical pixels, clamped to at least zero.
    ///
    /// `NaN` is treated as zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Divider;
    /// let divider = Divider::horizontal().thickness(2.0);
    /// let _ = divider;
    /// ```
    pub fn thickness(mut self, value: f32) -> Self {
        self.style.thickness = value.max(0.0);
        self
    }

    /// Sets intrinsic main-axis length in logical pixels, clamped to zero.
    ///
    /// Explicit layout dimensions may override this intrinsic value. `NaN` is
    /// treated as zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Divider;
    /// let divider = Divider::vertical().length(240.0);
    /// let _ = divider;
    /// ```
    pub fn length(mut self, value: f32) -> Self {
        self.style.length = value.max(0.0);
        self
    }

    /// Replaces the segment fill color.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::controls::Divider;
    /// let divider = Divider::horizontal().color(Color::WHITE);
    /// let _ = divider;
    /// ```
    pub fn color(mut self, color: Color) -> Self {
        self.style.color = color;
        self
    }

    /// Replaces the complete divider style without pre-clamping its values.
    ///
    /// Layout clamps negative `thickness` and `length` to zero. Dashed/dotted
    /// painting clamps gaps and segment lengths to at least one logical pixel.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{Divider, DividerStyle};
    /// let style = DividerStyle::from_theme(Theme::dark());
    /// let divider = Divider::horizontal().divider_style(style);
    /// let _ = divider;
    /// ```
    pub fn divider_style(mut self, style: DividerStyle) -> Self {
        self.style = style;
        self
    }

    /// Creates a divider in `orientation` with the default solid style.
    fn new(orientation: DividerOrientation) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            orientation,
            variant: DividerVariant::Solid,
            style: DividerStyle::default(),
        }
    }
}

/// Retained leaf widget holding the divider's resolved geometry and style.
struct DividerWidget {
    /// Layout copied from the builder.
    layout: LayoutStyle,
    /// Orientation copied from the builder.
    orientation: DividerOrientation,
    /// Pattern copied from the builder.
    variant: DividerVariant,
    /// Style copied from the builder.
    style: DividerStyle,
}

impl<A: 'static> Widget<A> for DividerWidget {
    fn debug_name(&self) -> &'static str {
        "Divider"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let thickness = self.style.thickness.max(0.0);
        let length = self.style.length.max(0.0);
        let intrinsic = match self.orientation {
            DividerOrientation::Horizontal => Size::new(length, thickness),
            DividerOrientation::Vertical => Size::new(thickness, length),
        };
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
        match self.variant {
            DividerVariant::Solid => {
                if bounds.w > 0.0 && bounds.h > 0.0 {
                    ctx.push(DrawCmd::Rect(DrawRect {
                        rect: bounds,
                        color: self.style.color,
                    }));
                }
            }
            DividerVariant::Dashed => paint_segments(
                ctx,
                bounds,
                self.orientation,
                self.style.dash.max(self.style.thickness).max(1.0),
                self.style.gap.max(1.0),
                self.style.color,
            ),
            DividerVariant::Dotted => paint_segments(
                ctx,
                bounds,
                self.orientation,
                self.style.thickness.max(1.0),
                self.style.gap.max(1.0),
                self.style.color,
            ),
        }
    }

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }
}

impl<A: 'static> IntoView<A> for Divider {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(DividerWidget {
                layout: self.layout,
                orientation: self.orientation,
                variant: self.variant,
                style: self.style,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Paints non-overlapping rectangular segments along the main axis.
///
/// The final segment is shortened to the remaining length. Callers guarantee
/// positive `segment_len` and `gap`, which makes the loop progress.
fn paint_segments(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    orientation: DividerOrientation,
    segment_len: f32,
    gap: f32,
    color: Color,
) {
    let main_len = match orientation {
        DividerOrientation::Horizontal => bounds.w,
        DividerOrientation::Vertical => bounds.h,
    };
    if main_len <= 0.0 {
        return;
    }

    let mut cursor = 0.0;
    while cursor < main_len {
        let len = segment_len.min(main_len - cursor);
        let rect = match orientation {
            DividerOrientation::Horizontal => Rect::new(bounds.x + cursor, bounds.y, len, bounds.h),
            DividerOrientation::Vertical => Rect::new(bounds.x, bounds.y + cursor, bounds.w, len),
        };
        if rect.w > 0.0 && rect.h > 0.0 {
            ctx.push(DrawCmd::Rect(DrawRect { rect, color }));
        }
        cursor += segment_len + gap;
    }
}
