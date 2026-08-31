//! Mutable layout context shared while traversing retained nodes.

#[cfg(feature = "devtools")]
use ailloli_ui_core::geometry::Size;
use ailloli_ui_core::geometry::{Constraints, Rect};
use ailloli_ui_core::ids::ElementId;
use ailloli_ui_core::math::Scale;
use ailloli_ui_text::TextSystem;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::layout_attempt::{LayoutAttempt, LayoutAttemptToken};
use super::layout_engine::LayoutEngine;
#[cfg(feature = "devtools")]
use super::layout_result::LayoutDebugInfo;
use super::layout_result::LayoutResult;
use crate::component::reactive::{ReactiveDependencyBatchResult, ReactiveDependencyUpdate};

/// Erased runtime callback used to atomically publish committed layout reads.
type ReactiveLayoutPublisher =
    Rc<dyn Fn(&[ReactiveDependencyUpdate]) -> ReactiveDependencyBatchResult>;

/// Erased runtime callback used when a layout attempt observes stale input.
type ReactiveLayoutRetry = Rc<dyn Fn(ElementId)>;

/// Erased runtime callback used when a whole staging overlay is discarded.
type ReactiveLayoutAbandon = Rc<dyn Fn()>;

/// Authority of the geometry produced by the current layout traversal.
///
/// Measurement is speculative and must not commit persistent state derived
/// from the temporary geometry. Commit passes use the allocation retained by
/// the parent and may evaluate geometry-dependent effects. A measurement pass
/// is sticky: descendants cannot regain commit authority locally.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::layout::LayoutPass;
///
/// assert!(LayoutPass::Measure.is_measure());
/// assert!(LayoutPass::Commit.is_committed());
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayoutPass {
    /// Speculative intrinsic-size or allocation probe.
    Measure,
    /// Authoritative retained allocation.
    #[default]
    Commit,
}

/// Provides phase predicates and sticky descendant composition.
impl LayoutPass {
    /// Returns `true` for speculative measurement geometry.
    pub const fn is_measure(self) -> bool {
        matches!(self, Self::Measure)
    }

    /// Returns `true` for authoritative committed geometry.
    pub const fn is_committed(self) -> bool {
        matches!(self, Self::Commit)
    }

    /// Combines an ancestor pass with a locally requested child pass.
    const fn descend(self, requested: Self) -> Self {
        if self.is_measure() || requested.is_measure() {
            Self::Measure
        } else {
            Self::Commit
        }
    }
}

/// Mutable services and scoped hints available during one layout traversal.
///
/// Geometry is expressed in logical pixels. The context borrows its optional
/// text system exclusively, so it is local to the current thread and layout
/// pass rather than a shareable global service.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Scale;
/// use ailloli_ui_runtime::layout::LayoutContext;
///
/// let ctx = LayoutContext::new(Scale::new(2.0));
/// assert_eq!(ctx.scale.dpr, 2.0);
/// assert!(ctx.text_system.is_none());
/// ```
pub struct LayoutContext<'a> {
    /// Device-pixel ratio used for snapping and layout-cache identity.
    pub scale: Scale,
    /// Shared text layout engine (optional for text-free tests).
    pub text_system: Option<&'a mut TextSystem>,
    /// Optional content-local viewport propagated by a virtualizing ancestor.
    virtual_viewport: Option<VirtualViewport>,
    /// Current geometry authority, inherited monotonically by descendants.
    layout_pass: LayoutPass,
    /// Explicit speculative branches adopted by the current authoritative attempt.
    measure_branches: Rc<RefCell<MeasureBranchState>>,
    /// Innermost explicit speculative branch, if one is active.
    current_measure_branch: Option<u64>,
    /// Runtime-owned writes staged by the current outer layout call.
    layout_attempt: Option<LayoutAttempt>,
    /// Exact successful attempt scoped only while post-layout hooks execute.
    committed_layout_attempt: Option<LayoutAttemptToken>,
    /// Optional retained dependency publisher installed by `Runtime`.
    reactive_layout_publisher: Option<ReactiveLayoutPublisher>,
    /// Optional targeted retry scheduler installed by `Runtime`.
    reactive_layout_retry: Option<ReactiveLayoutRetry>,
    /// Optional internal diagnostic callback for discarded staging overlays.
    reactive_layout_abandon: Option<ReactiveLayoutAbandon>,
    #[cfg(feature = "devtools")]
    /// Latest developer-tooling layout record for each element in this context.
    pub debug_layouts: HashMap<ElementId, LayoutDebugInfo>,
}

/// Bookkeeping shared with explicit measurement tokens.
#[derive(Default)]
struct MeasureBranchState {
    /// Last allocated nonzero branch identity.
    next_id: u64,
    /// Direct parent of every branch allocated by the active attempt.
    parents: HashMap<u64, Option<u64>>,
    /// Branches explicitly accepted by their owner widget.
    adopted: HashSet<u64>,
}

/// Result of an explicit speculative layout branch.
///
/// Dropping this token abandons every measurement performed by the branch.
/// Call [`Self::adopt`] only when the result contributes to the authoritative
/// geometry being returned by the widget. Existing widgets that use
/// [`LayoutCtx::with_layout_pass`] retain their historical auto-adopt behavior.
#[doc(hidden)]
#[must_use = "dropping a measurement branch abandons its reactive dependencies"]
pub struct LayoutMeasurement<T> {
    /// Branch result, taken exactly once by `adopt`.
    value: Option<T>,
    /// Nonzero identity recorded by staged measurement entries.
    branch_id: u64,
    /// Shared acceptance set owned by the layout context.
    state: Rc<RefCell<MeasureBranchState>>,
}

impl<T> LayoutMeasurement<T> {
    /// Borrows the speculative result before deciding whether to adopt it.
    #[doc(hidden)]
    pub fn result(&self) -> &T {
        self.value
            .as_ref()
            .expect("layout measurement result was already adopted")
    }

    /// Accepts this branch and returns its result.
    ///
    /// The authoritative layout attempt will publish the branch's reactive
    /// reads only if that complete attempt also succeeds and remains current.
    #[doc(hidden)]
    pub fn adopt(mut self) -> T {
        self.state.borrow_mut().adopted.insert(self.branch_id);
        self.value
            .take()
            .expect("layout measurement result was already adopted")
    }
}

/// Viewport made available to widgets whose content is larger than the
/// visible scroll container. Coordinates are local to the content.
///
/// Values use logical pixels and are stored verbatim. In particular, negative
/// or non-finite overscan is not clamped here; consumers must define how such
/// values affect virtualization.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_runtime::layout::VirtualViewport;
///
/// let viewport = VirtualViewport::new(Rect::new(0.0, 120.0, 640.0, 480.0), 32.0);
/// assert_eq!(viewport.overscan, 32.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VirtualViewport {
    /// Visible rectangle in content-local logical pixels.
    pub rect: Rect,
    /// Extra logical pixels requested around every visible edge.
    pub overscan: f32,
}

/// Provides the operations defined for VirtualViewport.
impl VirtualViewport {
    /// Creates a viewport without validating its rectangle or overscan.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_runtime::layout::VirtualViewport;
    ///
    /// let viewport = VirtualViewport::new(Rect::new(10.0, 20.0, 30.0, 40.0), 8.0);
    /// assert_eq!(viewport.rect.y, 20.0);
    /// ```
    pub const fn new(rect: Rect, overscan: f32) -> Self {
        Self { rect, overscan }
    }
}

/// Short alias for [`LayoutContext`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Scale;
/// use ailloli_ui_runtime::layout::LayoutCtx;
///
/// let ctx = LayoutCtx::new(Scale::new(1.0));
/// assert_eq!(ctx.scale.dpr, 1.0);
/// ```
pub type LayoutCtx<'a> = LayoutContext<'a>;

/// Provides the operations defined for `LayoutContext<'a>`.
impl<'a> LayoutContext<'a> {
    /// Creates a context without a text system or virtual viewport.
    ///
    /// With `devtools`, the debug-record map also starts empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Scale;
    /// use ailloli_ui_runtime::layout::LayoutContext;
    ///
    /// let ctx = LayoutContext::new(Scale::new(1.25));
    /// assert!(ctx.virtual_viewport().is_none());
    /// ```
    pub fn new(scale: Scale) -> Self {
        Self {
            scale,
            text_system: None,
            virtual_viewport: None,
            layout_pass: LayoutPass::Commit,
            measure_branches: Rc::new(RefCell::new(MeasureBranchState::default())),
            current_measure_branch: None,
            layout_attempt: None,
            committed_layout_attempt: None,
            reactive_layout_publisher: None,
            reactive_layout_retry: None,
            reactive_layout_abandon: None,
            #[cfg(feature = "devtools")]
            debug_layouts: HashMap::new(),
        }
    }

    /// Creates a context borrowing `text_system` for this layout pass.
    ///
    /// The virtual viewport starts as `None`; with `devtools`, debug records
    /// start empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Scale;
    /// use ailloli_ui_runtime::layout::LayoutContext;
    /// use ailloli_ui_text::TextSystem;
    ///
    /// let mut text = TextSystem::new();
    /// let ctx = LayoutContext::with_text_system(Scale::new(1.0), &mut text);
    /// assert!(ctx.text_system.is_some());
    /// ```
    pub fn with_text_system(scale: Scale, text_system: &'a mut TextSystem) -> Self {
        Self {
            scale,
            text_system: Some(text_system),
            virtual_viewport: None,
            layout_pass: LayoutPass::Commit,
            measure_branches: Rc::new(RefCell::new(MeasureBranchState::default())),
            current_measure_branch: None,
            layout_attempt: None,
            committed_layout_attempt: None,
            reactive_layout_publisher: None,
            reactive_layout_retry: None,
            reactive_layout_abandon: None,
            #[cfg(feature = "devtools")]
            debug_layouts: HashMap::new(),
        }
    }

    /// Returns the currently scoped content viewport.
    ///
    /// `None` means widgets should lay out without a virtualization hint.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Scale};
    /// use ailloli_ui_runtime::layout::{LayoutCtx, VirtualViewport};
    ///
    /// let mut ctx = LayoutCtx::new(Scale::new(1.0));
    /// ctx.replace_virtual_viewport(Some(VirtualViewport::new(
    ///     Rect::new(0.0, 50.0, 200.0, 100.0), 12.0,
    /// )));
    /// assert_eq!(ctx.virtual_viewport().unwrap().rect.y, 50.0);
    /// ```
    pub const fn virtual_viewport(&self) -> Option<VirtualViewport> {
        self.virtual_viewport
    }

    /// Returns the authority of geometry produced by the current traversal.
    ///
    /// Widgets may always compute geometry, but must not persist effects based
    /// on it while this returns [`LayoutPass::Measure`].
    pub const fn layout_pass(&self) -> LayoutPass {
        self.layout_pass
    }

    /// Returns the exact outer attempt owning the current layout callback.
    ///
    /// During `layout`, this identifies the active staging overlay. During
    /// `layout_committed`, it identifies the validated overlay whose geometry
    /// has just been published. Outside those callbacks it returns `None`.
    #[doc(hidden)]
    pub fn layout_attempt_token(&self) -> Option<LayoutAttemptToken> {
        self.layout_attempt
            .as_ref()
            .map(LayoutAttempt::token)
            .or(self.committed_layout_attempt)
    }

    /// Runs a descendant traversal under a sticky requested layout pass.
    ///
    /// Requesting [`LayoutPass::Commit`] beneath an existing measurement keeps
    /// the descendant in [`LayoutPass::Measure`]. The previous pass is restored
    /// after `layout` returns.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Scale;
    /// use ailloli_ui_runtime::layout::{LayoutCtx, LayoutPass};
    ///
    /// let mut ctx = LayoutCtx::new(Scale::new(1.0));
    /// ctx.with_layout_pass(LayoutPass::Measure, |ctx| {
    ///     assert!(ctx.layout_pass().is_measure());
    ///     ctx.with_layout_pass(LayoutPass::Commit, |ctx| {
    ///         assert!(ctx.layout_pass().is_measure());
    ///     });
    /// });
    /// assert!(ctx.layout_pass().is_committed());
    /// ```
    pub fn with_layout_pass<R>(
        &mut self,
        requested: LayoutPass,
        layout: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = self.layout_pass;
        self.layout_pass = previous.descend(requested);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| layout(self)));
        self.layout_pass = previous;
        match result {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Runs an explicitly adoptable speculative measurement branch.
    ///
    /// The returned token abandons the branch by default. This hidden
    /// cross-crate API lets layout widgets distinguish probes that contribute
    /// to their final allocation from alternatives that were only evaluated.
    #[doc(hidden)]
    pub fn measure_branch<R>(
        &mut self,
        layout: impl FnOnce(&mut Self) -> R,
    ) -> LayoutMeasurement<R> {
        let parent_branch = self.current_measure_branch;
        let branch_id = {
            let mut state = self.measure_branches.borrow_mut();
            state.next_id = state
                .next_id
                .checked_add(1)
                .expect("layout measurement branch identity exhausted");
            let branch_id = state.next_id;
            state.parents.insert(branch_id, parent_branch);
            branch_id
        };
        let previous_pass = self.layout_pass;
        self.layout_pass = previous_pass.descend(LayoutPass::Measure);
        self.current_measure_branch = Some(branch_id);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| layout(self)));
        self.current_measure_branch = parent_branch;
        self.layout_pass = previous_pass;
        match result {
            Ok(value) => LayoutMeasurement {
                value: Some(value),
                branch_id,
                state: self.measure_branches.clone(),
            },
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Returns the explicit branch currently owning speculative work.
    pub(crate) const fn current_measure_branch(&self) -> Option<u64> {
        self.current_measure_branch
    }

    /// Returns whether a branch and every enclosing explicit branch were accepted.
    pub(crate) fn measure_branch_is_adopted(&self, branch_id: u64) -> bool {
        let state = self.measure_branches.borrow();
        let mut branch = Some(branch_id);
        while let Some(branch_id) = branch {
            if !state.adopted.contains(&branch_id) {
                return false;
            }
            let Some(parent) = state.parents.get(&branch_id) else {
                return false;
            };
            branch = *parent;
        }
        true
    }

    /// Clears adoption records after the outer authoritative attempt finishes.
    pub(crate) fn clear_measure_branch_adoptions(&mut self) {
        let mut state = self.measure_branches.borrow_mut();
        state.adopted.clear();
        state.parents.clear();
    }

    /// Returns whether an outer layout call currently owns the staging overlay.
    pub(crate) const fn has_layout_attempt(&self) -> bool {
        self.layout_attempt.is_some()
    }

    /// Starts an empty staging overlay for one outer layout call.
    pub(crate) fn begin_layout_attempt(&mut self) {
        assert!(
            self.layout_attempt.is_none(),
            "nested layout attempts must reuse the active overlay"
        );
        self.clear_measure_branch_adoptions();
        self.layout_attempt = Some(LayoutAttempt::new());
    }

    /// Borrows the active staging overlay.
    pub(crate) fn layout_attempt(&self) -> &LayoutAttempt {
        self.layout_attempt
            .as_ref()
            .expect("layout attempt is not active")
    }

    /// Mutably borrows the active staging overlay.
    pub(crate) fn layout_attempt_mut(&mut self) -> &mut LayoutAttempt {
        self.layout_attempt
            .as_mut()
            .expect("layout attempt is not active")
    }

    /// Removes the active overlay for validation, commit, or abandonment.
    pub(crate) fn take_layout_attempt(&mut self) -> LayoutAttempt {
        self.layout_attempt
            .take()
            .expect("layout attempt is not active")
    }

    /// Scopes one validated attempt identity to a post-layout hook.
    pub(crate) fn with_committed_layout_attempt<R>(
        &mut self,
        token: Option<LayoutAttemptToken>,
        callback: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = std::mem::replace(&mut self.committed_layout_attempt, token);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(self)));
        self.committed_layout_attempt = previous;
        match result {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Installs runtime-owned dependency publication and retry callbacks.
    pub(crate) fn set_reactive_layout_callbacks(
        &mut self,
        publisher: ReactiveLayoutPublisher,
        retry: ReactiveLayoutRetry,
    ) {
        self.reactive_layout_publisher = Some(publisher);
        self.reactive_layout_retry = Some(retry);
    }

    /// Installs the runtime-owned discarded-attempt diagnostic callback.
    pub(crate) fn set_reactive_layout_abandon_callback(&mut self, abandon: ReactiveLayoutAbandon) {
        self.reactive_layout_abandon = Some(abandon);
    }

    /// Publishes a prevalidated batch when this context belongs to a runtime.
    pub(crate) fn publish_reactive_layout(
        &self,
        updates: &[ReactiveDependencyUpdate],
    ) -> ReactiveDependencyBatchResult {
        if let Some(publisher) = &self.reactive_layout_publisher {
            publisher(updates)
        } else {
            ReactiveDependencyBatchResult::Accepted { renewed: false }
        }
    }

    /// Schedules another targeted layout after a superseded attempt.
    pub(crate) fn request_reactive_layout_retry(&self, element_id: ElementId) {
        if let Some(retry) = &self.reactive_layout_retry {
            retry(element_id);
        }
    }

    /// Records one outer staging overlay discarded before publication.
    pub(crate) fn record_reactive_layout_abandon(&self) {
        if let Some(abandon) = &self.reactive_layout_abandon {
            abandon();
        }
    }

    /// Replaces the current content viewport and returns the previous value.
    /// Layout containers use this to scope the hint to their child traversal.
    /// Passing `None` clears the hint. The caller should restore the returned
    /// value after laying out a scoped child.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Scale};
    /// use ailloli_ui_runtime::layout::{LayoutCtx, VirtualViewport};
    ///
    /// let mut ctx = LayoutCtx::new(Scale::new(1.0));
    /// let viewport = VirtualViewport::new(Rect::new(0.0, 0.0, 80.0, 60.0), 4.0);
    /// assert!(ctx.replace_virtual_viewport(Some(viewport)).is_none());
    /// assert_eq!(ctx.replace_virtual_viewport(None), Some(viewport));
    /// ```
    pub fn replace_virtual_viewport(
        &mut self,
        viewport: Option<VirtualViewport>,
    ) -> Option<VirtualViewport> {
        std::mem::replace(&mut self.virtual_viewport, viewport)
    }

    #[cfg(feature = "devtools")]
    /// Records the latest layout and returns the stored debug snapshot.
    ///
    /// The first call for an element fixes `constraints_in`. Every call sets
    /// `constraints_final` to `Some(constraints)` and replaces `layout_size`.
    /// The map grows by unique element ID for the lifetime of this context.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, ElementId, Scale, Size};
    /// use ailloli_ui_runtime::layout::LayoutCtx;
    ///
    /// let mut ctx = LayoutCtx::new(Scale::new(1.0));
    /// let info = ctx.record_debug_layout(
    ///     ElementId(7), Constraints::tight(20.0, 10.0), Size::new(20.0, 10.0),
    /// );
    /// assert_eq!(info.layout_size, Size::new(20.0, 10.0));
    /// assert!(info.constraints_final.is_some());
    /// ```
    pub fn record_debug_layout(
        &mut self,
        element_id: ElementId,
        constraints: Constraints,
        size: Size,
    ) -> LayoutDebugInfo {
        let entry = self
            .debug_layouts
            .entry(element_id)
            .or_insert_with(|| LayoutDebugInfo {
                constraints_in: constraints,
                constraints_final: None,
                layout_size: size,
            });
        entry.constraints_final = Some(constraints);
        entry.layout_size = size;
        entry.clone()
    }
}

/// Handle used by widgets to lay out one retained direct child.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId;
/// use ailloli_ui_runtime::layout::LayoutChild;
///
/// let child = LayoutChild { element_id: ElementId(3) };
/// assert_eq!(child.element_id, ElementId(3));
/// ```
pub struct LayoutChild {
    /// Retained-tree identity delegated to [`LayoutEngine`].
    pub element_id: ElementId,
}

/// Provides the operations defined for LayoutChild.
impl LayoutChild {
    /// Lays out this child with the supplied logical-pixel constraints.
    ///
    /// A stale or unknown `element_id` produces [`LayoutResult::zero`] and no
    /// diagnostic cache event. Other behavior, including widget panics, is
    /// delegated to [`LayoutEngine::layout_element`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, ElementId, Scale, Size};
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutEngine};
    ///
    /// let mut tree = ElementTree::<()>::new();
    /// let mut engine = LayoutEngine::new(&mut tree);
    /// let mut ctx = LayoutCtx::new(Scale::new(1.0));
    /// let mut child = LayoutChild { element_id: ElementId(99) };
    /// assert_eq!(
    ///     child.layout(&mut engine, &mut ctx, Constraints::tight(10.0, 20.0)).size,
    ///     Size::default(),
    /// );
    /// ```
    pub fn layout<A: 'static>(
        &mut self,
        engine: &mut LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        engine.layout_element(ctx, self.element_id, constraints)
    }
}
