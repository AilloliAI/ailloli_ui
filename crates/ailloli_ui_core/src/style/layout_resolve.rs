use crate::geometry::{Constraints, Size};
use crate::style::Length;

use super::LayoutStyle;

/// Resolved explicit sizes from [`LayoutStyle`] (fill / px / percent).
///
/// `None` on an axis means intrinsic sizing (children sum, text measure, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ResolvedLayout {
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub min_width: Option<f32>,
    pub max_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_height: Option<f32>,
}

impl ResolvedLayout {
    /// Combines intrinsic size with explicit axes and min/max bounds under `parent`.
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

fn resolve_bound(length: Length, available: f32) -> Option<f32> {
    length.resolve(available)
}

impl LayoutStyle {
    /// Resolves width/height/min/max against parent constraints.
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

    /// Constraints for children (inner area after this node's margin/padding).
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

/// Applies `LayoutStyle` to an intrinsic size under parent constraints.
pub fn resolve_widget_size(intrinsic: Size, layout: LayoutStyle, parent: Constraints) -> Size {
    let resolved = layout.resolve(parent);
    let (w, h) = resolved.size(intrinsic.w, intrinsic.h, parent);
    Size::new(w, h)
}
