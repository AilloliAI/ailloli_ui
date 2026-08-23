//! Declarative single-child box model with background, border, shadow, and clip.

use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{ClipShape, Constraints, Rect, Size};
use ailloli_ui_core::style::{
    Background, Border, BoxShadow, BoxStyle, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius,
};
use ailloli_ui_core::{Color, EdgeInsets, Offset, Theme};
use ailloli_ui_runtime::component::{IntoView, View, Widget};
use ailloli_ui_runtime::input::EventCtx;
use ailloli_ui_runtime::layout::LayoutEngine;
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawBoxShadow, DrawCmd, DrawRRect, DrawRect};

use super::layout_ext::finish_view_sized;

/// Insets a rectangle and floors resulting dimensions at zero.
fn deflate_rect(mut rect: Rect, by: EdgeInsets) -> Rect {
    rect.x += by.left;
    rect.y += by.top;
    rect.w = (rect.w - by.left - by.right).max(0.0);
    rect.h = (rect.h - by.top - by.bottom).max(0.0);
    rect
}

/// Subtracts insets independently from every constraint bound, floored at zero.
fn deflate_constraints(c: Constraints, by: EdgeInsets) -> Constraints {
    Constraints {
        min_w: (c.min_w - by.horizontal()).max(0.0),
        max_w: (c.max_w - by.horizontal()).max(0.0),
        min_h: (c.min_h - by.vertical()).max(0.0),
        max_h: (c.max_h - by.vertical()).max(0.0),
    }
}

/// Adds corresponding inset edges without overflow or finiteness checks.
fn add_insets(a: EdgeInsets, b: EdgeInsets) -> EdgeInsets {
    EdgeInsets::new(
        a.left + b.left,
        a.top + b.top,
        a.right + b.right,
        a.bottom + b.bottom,
    )
}

/// Inflates a size by horizontal and vertical inset totals.
fn add_insets_to_size(size: Size, insets: EdgeInsets) -> Size {
    Size::new(size.w + insets.horizontal(), size.h + insets.vertical())
}

/// Multiplies alpha and clamps the result to the normalized color range.
fn apply_opacity(mut c: Color, opacity: f32) -> Color {
    c.a = (c.a * opacity).clamp(0.0, 1.0);
    c
}

/// Applies container opacity independently to all four border colors.
fn apply_border_opacity(mut border: Border, opacity: f32) -> Border {
    border.colors.left = apply_opacity(border.colors.left, opacity);
    border.colors.top = apply_opacity(border.colors.top, opacity);
    border.colors.right = apply_opacity(border.colors.right, opacity);
    border.colors.bottom = apply_opacity(border.colors.bottom, opacity);
    border
}

/// Applies container opacity to a shadow color.
fn apply_shadow_opacity(mut shadow: BoxShadow, opacity: f32) -> BoxShadow {
    shadow.color = apply_opacity(shadow.color, opacity);
    shadow
}

/// Returns the last shadow, first inserting the medium default when empty.
fn ensure_shadow(shadows: &mut Vec<BoxShadow>) -> &mut BoxShadow {
    if shadows.is_empty() {
        shadows.push(BoxShadow::md());
    }
    shadows.last_mut().expect("shadow inserted when empty")
}

/// Returns the smallest axis-aligned rectangle containing both inputs.
fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = a.right().max(b.right());
    let y1 = a.bottom().max(b.bottom());
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Declarative box with background, border, shadows, and optional child clip.
///
/// The default is transparent, borderless, shadowless, unclipped, automatically
/// sized, and has no child. Margin surrounds the border box; border and padding
/// surround the content. Only one child is retained.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Color;
/// use ailloli_ui_widgets::{layout::Container, text::Text};
/// let card: Container<()> = Container::new().background(Color::BLACK).padding(8.0).child(Text::new("Body"));
/// let _ = card;
/// ```
pub struct Container<A = ()> {
    /// Outer logical sizing policy.
    pub(crate) layout: LayoutStyle,
    /// Parent-flex participation metadata.
    pub(crate) flex_item: FlexItemStyle,
    /// Background, border, radius, padding, and shadow styling.
    box_style: BoxStyle,
    /// Whether child paint and hit testing are clipped to the content shape.
    clip_children: bool,
    /// Whether this clip establishes the native window-root boundary.
    window_root_clip: bool,
    /// Optional sole retained child.
    child: Option<View<A>>,
}

crate::impl_layout_builders!(Container);

/// Creates the same empty transparent box as [`Container::new`].
impl<A: 'static> Default for Container<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Container<A> {
    /// Creates an empty transparent box with automatic layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::Container;
    /// let container: Container<()> = Container::new();
    /// let _ = container;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            box_style: BoxStyle::default(),
            clip_children: false,
            window_root_clip: false,
            child: None,
        }
    }

    /// Creates a theme surface with a one-pixel border and large radius.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::layout::Container;
    /// let surface: Container<()> = Container::surface(Theme::dark());
    /// let _ = surface;
    /// ```
    pub fn surface(theme: Theme) -> Self {
        let palette = theme.palette();
        Self::new()
            .background(palette.surface)
            .border(1.0, palette.border)
            .radius(theme.radius().lg)
    }

    /// Creates an elevated themed surface with the medium theme shadow.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::layout::Container;
    /// let panel: Container<()> = Container::panel(Theme::dark());
    /// let _ = panel;
    /// ```
    pub fn panel(theme: Theme) -> Self {
        let palette = theme.palette();
        Self::surface(theme)
            .background(palette.surface_elevated)
            .shadow(theme.shadows().md)
    }

    /// Replaces the background with one solid color.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().background(Color::BLACK);
    /// let _ = box_;
    /// ```
    pub fn background(mut self, color: Color) -> Self {
        self.box_style.background = Background::color(color);
        self
    }

    /// Sets a uniform logical-pixel corner radius without clamping.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().radius(8.0);
    /// let _ = box_;
    /// ```
    pub fn radius(mut self, value: f32) -> Self {
        self.box_style.radius = Radius::uniform(value);
        self
    }

    /// Replaces all four border widths and colors.
    ///
    /// Width normalization follows [`Border::new`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().border(1.0, Color::WHITE);
    /// let _ = box_;
    /// ```
    pub fn border(mut self, width: f32, color: Color) -> Self {
        self.box_style.border = Border::new(width, color);
        self
    }

    /// Replaces all four widths while preserving edge colors.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().border_width(2.0);
    /// let _ = box_;
    /// ```
    pub fn border_width(mut self, width: f32) -> Self {
        self.box_style.border = self.box_style.border.with_width(width);
        self
    }

    /// Replaces all four colors while preserving edge widths.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().border_color(Color::WHITE);
    /// let _ = box_;
    /// ```
    pub fn border_color(mut self, color: Color) -> Self {
        self.box_style.border = self.box_style.border.with_color(color);
        self
    }

    /// Replaces only the left border width and color.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().border_left(2.0, Color::WHITE);
    /// let _ = box_;
    /// ```
    pub fn border_left(mut self, width: f32, color: Color) -> Self {
        self.box_style.border = self.box_style.border.with_left(width, color);
        self
    }

    /// Replaces only the top border width and color.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().border_top(2.0, Color::WHITE);
    /// let _ = box_;
    /// ```
    pub fn border_top(mut self, width: f32, color: Color) -> Self {
        self.box_style.border = self.box_style.border.with_top(width, color);
        self
    }

    /// Replaces only the right border width and color.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().border_right(2.0, Color::WHITE);
    /// let _ = box_;
    /// ```
    pub fn border_right(mut self, width: f32, color: Color) -> Self {
        self.box_style.border = self.box_style.border.with_right(width, color);
        self
    }

    /// Replaces only the bottom border width and color.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().border_bottom(2.0, Color::WHITE);
    /// let _ = box_;
    /// ```
    pub fn border_bottom(mut self, width: f32, color: Color) -> Self {
        self.box_style.border = self.box_style.border.with_bottom(width, color);
        self
    }

    /// Appends one shadow; later shadows paint after earlier shadows.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::BoxShadow;
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().shadow(BoxShadow::md());
    /// let _ = box_;
    /// ```
    pub fn shadow(mut self, shadow: BoxShadow) -> Self {
        self.box_style.shadows.push(shadow);
        self
    }

    /// Removes every configured shadow.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::BoxShadow;
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().shadow(BoxShadow::md()).shadow_none();
    /// let _ = box_;
    /// ```
    pub fn shadow_none(mut self) -> Self {
        self.box_style.shadows.clear();
        self
    }

    /// Replaces the last shadow color, inserting a medium shadow if absent.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().shadow_color(Color::BLACK);
    /// let _ = box_;
    /// ```
    pub fn shadow_color(mut self, color: Color) -> Self {
        ensure_shadow(&mut self.box_style.shadows).color = color;
        self
    }

    /// Replaces the last shadow blur radius in logical pixels.
    ///
    /// Negative and `NaN` values resolve to zero; an absent shadow first inserts
    /// the medium default.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().shadow_blur(12.0);
    /// let _ = box_;
    /// ```
    pub fn shadow_blur(mut self, value: f32) -> Self {
        ensure_shadow(&mut self.box_style.shadows).blur_radius = value.max(0.0);
        self
    }

    /// Replaces the last shadow offset in logical pixels.
    ///
    /// Values are stored without finiteness checks; an absent shadow first
    /// inserts the medium default.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().shadow_offset(0.0, 4.0);
    /// let _ = box_;
    /// ```
    pub fn shadow_offset(mut self, x: f32, y: f32) -> Self {
        ensure_shadow(&mut self.box_style.shadows).offset = Offset::new(x, y);
        self
    }

    /// Replaces the last shadow spread in logical pixels.
    ///
    /// Negative and `NaN` values resolve to zero; an absent shadow first inserts
    /// the medium default.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().shadow_spread(2.0);
    /// let _ = box_;
    /// ```
    pub fn shadow_spread(mut self, value: f32) -> Self {
        ensure_shadow(&mut self.box_style.shadows).spread = value.max(0.0);
        self
    }

    /// Clips children to the inner rect (rounded when `radius > 0`).
    ///
    /// The clip excludes margin, border, and padding. It is disabled by default.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::Container;
    /// let box_: Container<()> = Container::new().radius(8.0).clip_children(true);
    /// let _ = box_;
    /// ```
    pub fn clip_children(mut self, value: bool) -> Self {
        self.clip_children = value;
        self
    }

    /// Marks this container as the window root clip (`Window::radius`).
    /// Enables window-root stencil heuristics on the GPU path.
    ///
    /// This marker is independent of `clip_children`; callers are responsible
    /// for applying it only to the actual root clip.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::Container;
    /// let root: Container<()> = Container::new().clip_children(true).window_root_clip(true);
    /// let _ = root;
    /// ```
    pub fn window_root_clip(mut self, value: bool) -> Self {
        self.window_root_clip = value;
        self
    }

    /// Sets the single content child, replacing any previous child.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::Container, text::Text};
    /// let box_: Container<()> = Container::new().child(Text::new("content"));
    /// let _ = box_;
    /// ```
    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.child = Some(child.into_view());
        self
    }
}

/// Frozen box/layout state installed in the retained tree.
struct ContainerWidget {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Background, border, radius, padding, and shadow styling.
    style: BoxStyle,
    /// Whether child paint and hit testing are clipped to the content shape.
    clip_children: bool,
    /// Whether this clip establishes the native window-root boundary.
    window_root_clip: bool,
}

/// Implements box-model layout, clip publication, and layered surface painting.
impl<A: 'static> Widget<A> for ContainerWidget {
    fn debug_name(&self) -> &'static str {
        "Container"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let m = self.layout.margin;
        let p = self.layout.padding;
        let b = self.style.border.layout_widths();
        let content_insets = add_insets(b, p);

        let outer_constraints = deflate_constraints(constraints, m);

        let slot_tight = outer_constraints.min_w == outer_constraints.max_w
            && outer_constraints.min_h == outer_constraints.max_h;

        let inner = if slot_tight {
            deflate_constraints(outer_constraints, content_insets)
        } else {
            let inner = self.layout.constraints_for_children(outer_constraints);
            deflate_constraints(inner, content_insets)
        };

        let mut child_layouts = Vec::new();
        let mut child_size = Size::default();

        if let Some(child) = children.first_mut() {
            let r = child.layout(engine, ctx, inner);
            child_size = r.size;
            child_layouts.push(ChildLayout {
                offset: Offset::new(m.left + b.left + p.left, m.top + b.top + p.top),
                size: r.size,
                paint_bounds: Rect::new(0.0, 0.0, r.size.w, r.size.h),
                visual_bounds: r.visual_bounds,
            });
        }

        let border_box = add_insets_to_size(child_size, content_insets);
        let outer_box = add_insets_to_size(border_box, m);

        let size = {
            let resolved = self.layout.resolve(constraints);
            let (mut w, mut h) = resolved.size(outer_box.w, outer_box.h, constraints);
            if slot_tight && !self.layout.width.is_auto() {
                w = outer_constraints.max_w;
            }
            if slot_tight && !self.layout.height.is_auto() {
                h = outer_constraints.max_h;
            }
            Size::new(w, h)
        };

        let content_bounds = deflate_rect(
            Rect::new(
                m.left,
                m.top,
                (size.w - m.horizontal()).max(0.0),
                (size.h - m.vertical()).max(0.0),
            ),
            content_insets,
        );
        let inner_radius = self.style.border.inner_radius(self.style.radius);
        let clip_radius = (inner_radius.tl - p.left.max(p.top)).max(0.0);
        let clip = if self.clip_children {
            if clip_radius > 0.0 {
                Some(ClipShape::RoundRect {
                    rect: content_bounds,
                    radius: clip_radius,
                })
            } else {
                Some(ClipShape::Rect(content_bounds))
            }
        } else {
            None
        };

        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        let border_bounds = deflate_rect(paint_bounds, m);
        let visual_bounds = union_rect(paint_bounds, self.style.visual_bounds(border_bounds));

        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds,
            visual_bounds,
            overlay_hit_bounds: Vec::new(),
            clip,
            is_window_root_clip: self.window_root_clip,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let border_bounds = deflate_rect(bounds, self.layout.margin);
        let opacity = self.style.opacity.0;

        for shadow in self.style.shadows.iter().copied().filter(|s| !s.inset) {
            let shadow = apply_shadow_opacity(shadow, opacity);
            if shadow.color.a > 0.0 {
                ctx.push(DrawCmd::BoxShadow(DrawBoxShadow {
                    rect: border_bounds,
                    radius: self.style.radius,
                    shadow,
                }));
            }
        }

        if let Background::Color(bg) = self.style.background {
            let bg = apply_opacity(bg, opacity);
            if self.style.radius != Radius::zero() {
                ctx.push(DrawCmd::RRect(DrawRRect {
                    rect: border_bounds,
                    radius: self.style.radius.tl,
                    color: bg,
                }));
            } else {
                ctx.push(DrawCmd::Rect(DrawRect {
                    rect: border_bounds,
                    color: bg,
                }));
            }
        }
    }

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let border = apply_border_opacity(self.style.border, self.style.opacity.0);
        if border.is_visible() {
            ctx.push(DrawCmd::Border(DrawBorder {
                rect: deflate_rect(bounds, self.layout.margin),
                radius: self.style.radius,
                border,
            }));
        }
    }

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }
}

/// Converts the builder into one retained box node and optional child.
impl<A: 'static> IntoView<A> for Container<A> {
    fn into_view(self) -> View<A> {
        let widget = ContainerWidget {
            layout: self.layout,
            style: self.box_style,
            clip_children: self.clip_children,
            window_root_clip: self.window_root_clip,
        };

        let mut children = Vec::new();
        if let Some(child) = self.child {
            children.push(child);
        }
        finish_view_sized(
            View::node(widget, children),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}
