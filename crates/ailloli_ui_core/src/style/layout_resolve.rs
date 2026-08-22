//! Resolution of declarative lengths against parent layout constraints.

use crate::geometry::{Constraints, Size};
use crate::style::Length;

use super::LayoutStyle;

/// Resolved explicit sizes from [`LayoutStyle`] (fill / px / percent).
///
/// `None` on an axis means intrinsic sizing (children sum, text measure, etc.).
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Constraints, Size};
/// use ailloli_ui_core::style::{LayoutStyle, resolve_widget_size};
/// let size = resolve_widget_size(Size::new(20.0, 10.0), LayoutStyle::new().width(40.0), Constraints::loose(100.0, 100.0));
/// assert_eq!(size, Size::new(40.0, 10.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ResolvedLayout {
    /// Explicit width in logical pixels, or `None` for intrinsic width.
    pub width: Option<f32>,
    /// Explicit height in logical pixels, or `None` for intrinsic height.
    pub height: Option<f32>,
    /// Resolved minimum width, or `None` when no minimum was specified.
    pub min_width: Option<f32>,
    /// Resolved maximum width, or `None` when no maximum was specified.
    pub max_width: Option<f32>,
    /// Resolved minimum height, or `None` when no minimum was specified.
    pub min_height: Option<f32>,
    /// Resolved maximum height, or `None` when no maximum was specified.
    pub max_height: Option<f32>,
}

impl ResolvedLayout {
    /// Combines intrinsic size with explicit axes and min/max bounds under `parent`.
    ///
    /// Per-widget minimums are applied before maximums. An explicit axis is
    /// clamped to the ordered parent interval; an intrinsic axis is capped only
    /// at the parent maximum and is not expanded to the parent minimum.
    ///
    /// # Panics
    ///
    /// May panic when an explicit axis is clamped against parent bounds that
    /// remain NaN after ordering.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Constraints;
    /// use ailloli_ui_core::style::ResolvedLayout;
    /// let resolved = ResolvedLayout { width: Some(40.0), ..ResolvedLayout::default() };
    /// assert_eq!(resolved.size(10.0, 20.0, Constraints::loose(100.0, 100.0)), (40.0, 20.0));
    /// ```
    pub fn size(self, intrinsic_w: f32, intrinsic_h: f32, parent: Constraints) -> (f32, f32) {
        let mut w = self.width.unwrap_or(intrinsic_w);
        let mut h = self.height.unwrap_or(intrinsic_h);

        if let Some(min_w) = self.min_width {
            w = w.max(min_w);
        }
        if let Some(max_w) = self.max_width {
            w = w.min(max_w);
        }
        if let Some(min_h) = self.min_height {
            h = h.max(min_h);
        }
        if let Some(max_h) = self.max_height {
            h = h.min(max_h);
        }

        // Auto axes: cap at parent max only (do not expand to parent min).
        let parent_w_min = parent.min_w.min(parent.max_w);
        let parent_w_max = parent.max_w.max(parent.min_w);
        let parent_h_min = parent.min_h.min(parent.max_h);
        let parent_h_max = parent.max_h.max(parent.min_h);

        if self.width.is_none() {
            w = w.min(parent_w_max);
        } else {
            w = w.clamp(parent_w_min, parent_w_max);
        }
        if self.height.is_none() {
            h = h.min(parent_h_max);
        } else {
            h = h.clamp(parent_h_min, parent_h_max);
        }

        (w, h)
    }
}

/// Resolves one declarative bound against parent availability.
fn resolve_bound(length: Length, available: f32) -> Option<f32> {
    length.resolve(available)
}

impl LayoutStyle {
    /// Resolves width, height, and min/max lengths against parent maximums.
    ///
    /// Every percentage and fill value uses the corresponding stored parent
    /// maximum. Margin and padding do not participate in this operation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Constraints;
    /// use ailloli_ui_core::style::LayoutStyle;
    /// assert_eq!(LayoutStyle::new().width(50.0).resolve(Constraints::loose(100.0, 80.0)).width, Some(50.0));
    /// ```
    pub fn resolve(self, parent: Constraints) -> ResolvedLayout {
        ResolvedLayout {
            width: resolve_bound(self.width, parent.max_w),
            height: resolve_bound(self.height, parent.max_h),
            min_width: resolve_bound(self.min_width, parent.max_w),
            max_width: resolve_bound(self.max_width, parent.max_w),
            min_height: resolve_bound(self.min_height, parent.max_h),
            max_height: resolve_bound(self.max_height, parent.max_h),
        }
    }

    /// Derives normalized constraints for children from an already inner box.
    ///
    /// The caller is responsible for removing margin/padding before passing
    /// `inner`. Explicit dimensions narrow both bounds; min/max declarations
    /// then tighten their matching side. The final intervals are reordered if
    /// the declarations cross.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Constraints;
    /// use ailloli_ui_core::style::LayoutStyle;
    /// let children = LayoutStyle::new().width(40.0).constraints_for_children(Constraints::loose(100.0, 80.0));
    /// assert_eq!((children.min_w, children.max_w), (40.0, 40.0));
    /// ```
    pub fn constraints_for_children(self, inner: Constraints) -> Constraints {
        let resolved = self.resolve(inner);
        let mut max_w = inner.max_w;
        let mut max_h = inner.max_h;
        let mut min_w = inner.min_w;
        let mut min_h = inner.min_h;

        if let Some(w) = resolved.width {
            max_w = w.min(max_w);
            min_w = w.max(min_w);
        }
        if let Some(h) = resolved.height {
            max_h = h.min(max_h);
            min_h = h.max(min_h);
        }
        if let Some(v) = resolved.min_width {
            min_w = v.max(min_w);
        }
        if let Some(v) = resolved.max_width {
            max_w = v.min(max_w);
        }
        if let Some(v) = resolved.min_height {
            min_h = v.max(min_h);
        }
        if let Some(v) = resolved.max_height {
            max_h = v.min(max_h);
        }

        Constraints {
            min_w,
            max_w,
            min_h,
            max_h,
        }
        .normalized()
    }
}

/// Applies a [`LayoutStyle`] to an intrinsic size under parent constraints.
///
/// The returned [`Size`] uses logical pixels and follows
/// [`ResolvedLayout::size`] for explicit versus intrinsic parent-min behavior.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Constraints, Size};
/// use ailloli_ui_core::style::{LayoutStyle, resolve_widget_size};
/// assert_eq!(resolve_widget_size(Size::new(20.0, 10.0), LayoutStyle::new().height(30.0), Constraints::loose(100.0, 100.0)), Size::new(20.0, 30.0));
/// ```
pub fn resolve_widget_size(intrinsic: Size, layout: LayoutStyle, parent: Constraints) -> Size {
    let resolved = layout.resolve(parent);
    let (w, h) = resolved.size(intrinsic.w, intrinsic.h, parent);
    Size::new(w, h)
}
