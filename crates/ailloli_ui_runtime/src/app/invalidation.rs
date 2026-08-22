//! Retained-work strengths and host-visible aggregate frame plans.

use std::cmp::Ordering;

/// Smallest unit of retained UI work requested for an element.
///
/// The ordering is intentional: several requests for the same element are
/// coalesced to the strongest level (`Paint < Layout < Build`). Hosts only see
/// the aggregate [`FrameWorkPlan`]; exact element roots remain runtime-local.
/// The enum is non-exhaustive so downstream matches require a wildcard arm.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::Invalidation;
/// assert!(Invalidation::Paint < Invalidation::Layout);
/// assert!(Invalidation::Layout < Invalidation::Build);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Invalidation {
    /// Repaint from already reconciled and laid-out retained state.
    Paint,
    /// Recompute layout, then paint. Ancestors may be affected; siblings are
    /// not invalidated unless their own inputs or constraints change.
    Layout,
    /// Rebuild the owning component, then layout and paint its affected path.
    Build,
}

/// Provides the operations defined for Invalidation.
impl Invalidation {
    /// Returns the stable coalescing rank: paint 0, layout 1, build 2.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::Invalidation;
    /// assert_eq!(Invalidation::Paint.rank(), 0);
    /// assert_eq!(Invalidation::Layout.rank(), 1);
    /// assert_eq!(Invalidation::Build.rank(), 2);
    /// ```
    pub const fn rank(self) -> u8 {
        match self {
            Self::Paint => 0,
            Self::Layout => 1,
            Self::Build => 2,
        }
    }

    /// Coalesces two requests without losing the stronger unit of work.
    ///
    /// The operation is commutative, associative, and idempotent for the current
    /// variants; it returns whichever value has the greater [`Self::rank`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::Invalidation;
    /// assert_eq!(Invalidation::Paint.merge(Invalidation::Build), Invalidation::Build);
    /// assert_eq!(Invalidation::Layout.merge(Invalidation::Paint), Invalidation::Layout);
    /// ```
    pub const fn merge(self, other: Self) -> Self {
        if self.rank() >= other.rank() {
            self
        } else {
            other
        }
    }

    /// Returns `true` only for component-build invalidation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::Invalidation;
    /// assert!(Invalidation::Build.needs_build());
    /// assert!(!Invalidation::Layout.needs_build());
    /// ```
    pub const fn needs_build(self) -> bool {
        matches!(self, Self::Build)
    }

    /// Returns `true` for layout and build invalidations.
    ///
    /// Build implies the layout pass that follows reconciliation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::Invalidation;
    /// assert!(!Invalidation::Paint.needs_layout());
    /// assert!(Invalidation::Layout.needs_layout());
    /// assert!(Invalidation::Build.needs_layout());
    /// ```
    pub const fn needs_layout(self) -> bool {
        matches!(self, Self::Layout | Self::Build)
    }

    /// Returns `true` for every current invalidation strength.
    ///
    /// Any retained-work request ultimately requires painting a presented frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::Invalidation;
    /// assert!(Invalidation::Paint.needs_paint());
    /// assert!(Invalidation::Build.needs_paint());
    /// ```
    pub const fn needs_paint(self) -> bool {
        true
    }
}

/// Implements the Ord contract for Invalidation.
impl Ord for Invalidation {
    /// Compares two values using the documented total order.
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }
}

/// Implements the PartialOrd contract for Invalidation.
impl PartialOrd for Invalidation {
    /// Delegates partial comparison to the total order.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Provider-neutral aggregate consumed by a presentation host.
///
/// Element identifiers and propagation roots deliberately do not cross this
/// boundary. The runtime retains those details for incremental reconciliation
/// and layout.
///
/// The fields are intentionally private: hosts can ask which frame stages are
/// necessary but cannot infer or manipulate retained element roots.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{FrameWorkPlan, Invalidation};
/// let plan = FrameWorkPlan::from_invalidation(Invalidation::Layout);
/// assert!(!plan.needs_build());
/// assert!(plan.needs_layout() && plan.needs_paint());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameWorkPlan {
    needs_build: bool,
    needs_layout: bool,
    needs_paint: bool,
}

/// Provides the operations defined for FrameWorkPlan.
impl FrameWorkPlan {
    /// Returns a plan with no build, layout, or paint work.
    ///
    /// This equals [`Default::default`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::FrameWorkPlan;
    /// assert_eq!(FrameWorkPlan::none(), FrameWorkPlan::default());
    /// assert!(FrameWorkPlan::none().is_empty());
    /// ```
    pub const fn none() -> Self {
        Self {
            needs_build: false,
            needs_layout: false,
            needs_paint: false,
        }
    }

    /// Converts one invalidation strength into its implied frame stages.
    ///
    /// Paint maps to paint only; layout maps to layout plus paint; build maps to
    /// all three stages.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{FrameWorkPlan, Invalidation};
    /// let plan = FrameWorkPlan::from_invalidation(Invalidation::Build);
    /// assert!(plan.needs_build() && plan.needs_layout() && plan.needs_paint());
    /// ```
    pub const fn from_invalidation(invalidation: Invalidation) -> Self {
        Self {
            needs_build: invalidation.needs_build(),
            needs_layout: invalidation.needs_layout(),
            needs_paint: invalidation.needs_paint(),
        }
    }

    /// Returns whether component reconciliation/build work is required.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{FrameWorkPlan, Invalidation};
    /// assert!(FrameWorkPlan::from_invalidation(Invalidation::Build).needs_build());
    /// assert!(!FrameWorkPlan::from_invalidation(Invalidation::Layout).needs_build());
    /// ```
    pub const fn needs_build(self) -> bool {
        self.needs_build
    }

    /// Returns whether retained layout work is required.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{FrameWorkPlan, Invalidation};
    /// assert!(FrameWorkPlan::from_invalidation(Invalidation::Layout).needs_layout());
    /// assert!(!FrameWorkPlan::from_invalidation(Invalidation::Paint).needs_layout());
    /// ```
    pub const fn needs_layout(self) -> bool {
        self.needs_layout
    }

    /// Returns whether scene painting is required.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{FrameWorkPlan, Invalidation};
    /// assert!(FrameWorkPlan::from_invalidation(Invalidation::Paint).needs_paint());
    /// assert!(!FrameWorkPlan::none().needs_paint());
    /// ```
    pub const fn needs_paint(self) -> bool {
        self.needs_paint
    }

    /// Returns `true` exactly when painting is not required.
    ///
    /// Public constructors preserve the invariant that build/layout imply paint,
    /// so this is equivalent to all stages being false.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{FrameWorkPlan, Invalidation};
    /// assert!(FrameWorkPlan::none().is_empty());
    /// assert!(!FrameWorkPlan::from_invalidation(Invalidation::Paint).is_empty());
    /// ```
    pub const fn is_empty(self) -> bool {
        !self.needs_paint
    }

    /// Combines two plans by OR-ing each stage flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{FrameWorkPlan, Invalidation};
    /// let plan = FrameWorkPlan::none().merge(FrameWorkPlan::from_invalidation(Invalidation::Layout));
    /// assert!(!plan.needs_build());
    /// assert!(plan.needs_layout() && plan.needs_paint());
    /// ```
    pub const fn merge(self, other: Self) -> Self {
        Self {
            needs_build: self.needs_build || other.needs_build,
            needs_layout: self.needs_layout || other.needs_layout,
            needs_paint: self.needs_paint || other.needs_paint,
        }
    }
}

#[cfg(test)]
/// Tests implementation details.
mod tests {
    use super::*;

    #[test]
    /// Verifies that invalidations form the locked work order.
    fn invalidations_form_the_locked_work_order() {
        assert!(Invalidation::Paint < Invalidation::Layout);
        assert!(Invalidation::Layout < Invalidation::Build);
        assert_eq!(
            Invalidation::Paint.merge(Invalidation::Build),
            Invalidation::Build
        );
        assert_eq!(
            Invalidation::Layout.merge(Invalidation::Paint),
            Invalidation::Layout
        );
    }

    #[test]
    /// Verifies that frame plan exposes only aggregate work.
    fn frame_plan_exposes_only_aggregate_work() {
        assert_eq!(
            FrameWorkPlan::from_invalidation(Invalidation::Paint),
            FrameWorkPlan {
                needs_build: false,
                needs_layout: false,
                needs_paint: true,
            }
        );
        assert_eq!(
            FrameWorkPlan::from_invalidation(Invalidation::Build),
            FrameWorkPlan {
                needs_build: true,
                needs_layout: true,
                needs_paint: true,
            }
        );
    }
}
