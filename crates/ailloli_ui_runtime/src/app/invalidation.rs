use std::cmp::Ordering;

/// Smallest unit of retained UI work requested for an element.
///
/// The ordering is intentional: several requests for the same element are
/// coalesced to the strongest level (`Paint < Layout < Build`). Hosts only see
/// the aggregate [`FrameWorkPlan`]; exact element roots remain runtime-local.
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

impl Invalidation {
    pub const fn rank(self) -> u8 {
        match self {
            Self::Paint => 0,
            Self::Layout => 1,
            Self::Build => 2,
        }
    }

    /// Coalesces two requests without losing the stronger unit of work.
    pub const fn merge(self, other: Self) -> Self {
        if self.rank() >= other.rank() {
            self
        } else {
            other
        }
    }

    pub const fn needs_build(self) -> bool {
        matches!(self, Self::Build)
    }

    pub const fn needs_layout(self) -> bool {
        matches!(self, Self::Layout | Self::Build)
    }

    pub const fn needs_paint(self) -> bool {
        true
    }
}

impl Ord for Invalidation {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }
}

impl PartialOrd for Invalidation {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Provider-neutral aggregate consumed by a presentation host.
///
/// Element identifiers and propagation roots deliberately do not cross this
/// boundary. The runtime retains those details for incremental reconciliation
/// and layout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameWorkPlan {
    needs_build: bool,
    needs_layout: bool,
    needs_paint: bool,
}

impl FrameWorkPlan {
    pub const fn none() -> Self {
        Self {
            needs_build: false,
            needs_layout: false,
            needs_paint: false,
        }
    }

    pub const fn from_invalidation(invalidation: Invalidation) -> Self {
        Self {
            needs_build: invalidation.needs_build(),
            needs_layout: invalidation.needs_layout(),
            needs_paint: invalidation.needs_paint(),
        }
    }

    pub const fn needs_build(self) -> bool {
        self.needs_build
    }

    pub const fn needs_layout(self) -> bool {
        self.needs_layout
    }

    pub const fn needs_paint(self) -> bool {
        self.needs_paint
    }

    pub const fn is_empty(self) -> bool {
        !self.needs_paint
    }

    pub const fn merge(self, other: Self) -> Self {
        Self {
            needs_build: self.needs_build || other.needs_build,
            needs_layout: self.needs_layout || other.needs_layout,
            needs_paint: self.needs_paint || other.needs_paint,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
