//! Single-child rectangular clipping wrapper.

use ailloli_ui_core::geometry::{ClipShape, Constraints, Rect, Size};
use ailloli_ui_core::Offset;
use ailloli_ui_runtime::component::{IntoView, View, Widget};
use ailloli_ui_runtime::layout::LayoutEngine;
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;

/// Clips one child to this wrapper's resolved logical-pixel bounds.
///
/// The child uses the incoming constraints. With no child, the wrapper resolves
/// constrained zero size. The clip is not a window-root clip.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::{layout::ClipRect, text::Text};
/// let clip: ClipRect<()> = ClipRect::new().child(Text::new("visible portion"));
/// let _ = clip;
/// ```
pub struct ClipRect<A = ()> {
    child: Option<View<A>>,
}

/// Creates the same empty clip as [`ClipRect::new`].
impl<A: 'static> Default for ClipRect<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> ClipRect<A> {
    /// Creates an empty rectangular clip wrapper.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::ClipRect;
    /// let clip: ClipRect<()> = ClipRect::new();
    /// let _ = clip;
    /// ```
    pub fn new() -> Self {
        Self { child: None }
    }

    /// Sets the single clipped child, replacing any previous child.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::ClipRect, text::Text};
    /// let clip: ClipRect<()> = ClipRect::new().child(Text::new("child"));
    /// let _ = clip;
    /// ```
    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.child = Some(child.into_view());
        self
    }
}

/// Stateless retained rectangular clip implementation.
struct ClipRectWidget;

/// Resolves child size and publishes an exact rectangular clip shape.
impl<A: 'static> Widget<A> for ClipRectWidget {
    fn debug_name(&self) -> &'static str {
        "ClipRect"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let mut child_layouts = Vec::new();
        let mut size = constraints.constrain(Size::default());

        if let Some(child) = children.first_mut() {
            let r = child.layout(engine, ctx, constraints);
            size = constraints.constrain(r.size);
            child_layouts.push(ChildLayout {
                offset: Offset::default(),
                size: r.size,
                paint_bounds: Rect::new(0.0, 0.0, r.size.w, r.size.h),
                visual_bounds: r.visual_bounds,
            });
        }

        let bounds = Rect::new(0.0, 0.0, size.w, size.h);
        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds: bounds,
            visual_bounds: bounds,
            overlay_hit_bounds: Vec::new(),
            clip: Some(ClipShape::Rect(bounds)),
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

/// Converts the builder into a retained node containing zero or one child.
impl<A: 'static> IntoView<A> for ClipRect<A> {
    fn into_view(self) -> View<A> {
        let mut children = Vec::new();
        if let Some(child) = self.child {
            children.push(child);
        }

        View::node(ClipRectWidget, children)
    }
}
