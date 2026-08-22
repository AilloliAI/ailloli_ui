//! Bounded invalidation provenance and retained-work counters.

use std::collections::{HashMap, VecDeque};

use ailloli_ui_core::ElementId;

use crate::popup::ElementTreeId;

use super::Invalidation;

/// Maximum number of newest invalidation provenance records retained.
///
/// Aggregate counters continue saturating after old records are discarded.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::INVALIDATION_PROVENANCE_CAPACITY;
/// assert_eq!(INVALIDATION_PROVENANCE_CAPACITY, 256);
/// ```
pub const INVALIDATION_PROVENANCE_CAPACITY: usize = 256;

/// Origin of a retained-work request. This is diagnostic metadata only; it
/// never changes invalidation semantics.
/// The enum is non-exhaustive so downstream matches require a wildcard arm.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::InvalidationSource;
/// assert_ne!(InvalidationSource::Event, InvalidationSource::Timer);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvalidationSource {
    /// Direct request from runtime orchestration.
    Runtime,
    /// Request made through a component build context.
    Context,
    /// Request made while routing an input event.
    Event,
    /// Reactive signal mutation.
    Signal,
    /// Delayed or scheduled request.
    Timer,
    /// External retained model notification.
    Model,
    /// Presentation host request.
    Host,
    /// Legacy compatibility API.
    Compatibility,
}

/// One bounded provenance record for an invalidation request.
///
/// Records are immutable snapshots. Sequence values start at one and saturate
/// at `u64::MAX`; they can therefore cease being unique only after saturation.
/// `was_coalesced` says an element already had pending work, not that the new
/// request had the same strength.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId;
/// use ailloli_ui_runtime::app::{Invalidation, InvalidationSource, RuntimeHandle};
/// let handle = RuntimeHandle::<()>::new();
/// handle.invalidate_from(ElementId(7), Invalidation::Layout, InvalidationSource::Event);
/// let snapshot = handle.invalidation_diagnostics();
/// assert_eq!(snapshot.records.len(), 1);
/// assert_eq!(snapshot.records[0].element_id(), ElementId(7));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidationRecord {
    /// Saturating request sequence.
    sequence: u64,
    /// Namespace of the retained tree receiving the request.
    element_tree_id: ElementTreeId,
    /// Tree-local target element.
    element_id: ElementId,
    /// Requested work strength before coalescing.
    invalidation: Invalidation,
    /// Diagnostic origin.
    source: InvalidationSource,
    /// Whether this tree/element already had pending work.
    coalesced: bool,
}

/// Provides the operations defined for InvalidationRecord.
impl InvalidationRecord {
    /// Returns the one-based saturating request sequence.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, RuntimeHandle};
    /// let handle = RuntimeHandle::<()>::new();
    /// handle.invalidate(ElementId(1), Invalidation::Paint);
    /// assert_eq!(handle.invalidation_diagnostics().records[0].sequence(), 1);
    /// ```
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Returns the retained-tree namespace that received the request.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, RuntimeHandle};
    /// let handle = RuntimeHandle::<()>::new();
    /// handle.invalidate(ElementId(1), Invalidation::Paint);
    /// assert_eq!(handle.invalidation_diagnostics().records[0].element_tree_id(), handle.element_tree_id());
    /// ```
    pub const fn element_tree_id(&self) -> ElementTreeId {
        self.element_tree_id
    }

    /// Returns the tree-local target element ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, RuntimeHandle};
    /// let handle = RuntimeHandle::<()>::new();
    /// handle.invalidate(ElementId(9), Invalidation::Paint);
    /// assert_eq!(handle.invalidation_diagnostics().records[0].element_id(), ElementId(9));
    /// ```
    pub const fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Returns the requested retained-work strength before coalescing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, RuntimeHandle};
    /// let handle = RuntimeHandle::<()>::new();
    /// handle.invalidate(ElementId(1), Invalidation::Build);
    /// assert_eq!(handle.invalidation_diagnostics().records[0].invalidation(), Invalidation::Build);
    /// ```
    pub const fn invalidation(&self) -> Invalidation {
        self.invalidation
    }

    /// Returns the diagnostic origin supplied by the caller.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, InvalidationSource, RuntimeHandle};
    /// let handle = RuntimeHandle::<()>::new();
    /// handle.invalidate_from(ElementId(1), Invalidation::Paint, InvalidationSource::Model);
    /// assert_eq!(handle.invalidation_diagnostics().records[0].source(), InvalidationSource::Model);
    /// ```
    pub const fn source(&self) -> InvalidationSource {
        self.source
    }

    /// Returns whether the target already had a pending invalidation.
    ///
    /// The first request is false; subsequent requests before pending work is
    /// taken are true even when they strengthen the retained-work level.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, RuntimeHandle};
    /// let handle = RuntimeHandle::<()>::new();
    /// handle.invalidate(ElementId(1), Invalidation::Paint);
    /// handle.invalidate(ElementId(1), Invalidation::Build);
    /// let records = handle.invalidation_diagnostics().records;
    /// assert!(!records[0].was_coalesced());
    /// assert!(records[1].was_coalesced());
    /// ```
    pub const fn was_coalesced(&self) -> bool {
        self.coalesced
    }
}

/// Snapshot of requested work. The provenance ring is bounded even during a
/// long-running chat or terminal session.
///
/// All aggregate counters saturate at `u64::MAX`. `records` contains at most
/// [`INVALIDATION_PROVENANCE_CAPACITY`] newest requests in chronological order;
/// `dropped_provenance_records` counts evictions, not lost aggregate requests.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::InvalidationDiagnosticsSnapshot;
/// let snapshot = InvalidationDiagnosticsSnapshot::default();
/// assert_eq!(snapshot.requests, 0);
/// assert!(snapshot.records.is_empty());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InvalidationDiagnosticsSnapshot {
    /// Total recorded requests, saturating at `u64::MAX`.
    pub requests: u64,
    /// Requests whose tree/element already had pending work.
    pub coalesced_requests: u64,
    /// Paint-strength requests before coalescing.
    pub paint_requests: u64,
    /// Layout-strength requests before coalescing.
    pub layout_requests: u64,
    /// Build-strength requests before coalescing.
    pub build_requests: u64,
    /// Number of oldest provenance entries evicted from the bounded ring.
    pub dropped_provenance_records: u64,
    /// Newest bounded provenance entries in request order.
    pub records: Vec<InvalidationRecord>,
}

#[derive(Default)]
/// Mutable bounded invalidation counters owned by [`super::RuntimeInner`].
///
/// # Examples
///
/// Public callers inspect its immutable snapshot through a runtime handle:
///
/// ```
/// use ailloli_ui_runtime::app::RuntimeHandle;
/// let handle = RuntimeHandle::<()>::new();
/// assert_eq!(handle.invalidation_diagnostics().requests, 0);
/// ```
pub(crate) struct InvalidationDiagnostics {
    /// Next saturating sequence seed.
    next_sequence: u64,
    /// Aggregate counters; its records field is populated only during snapshot.
    snapshot: InvalidationDiagnosticsSnapshot,
    /// Bounded oldest-to-newest provenance ring.
    records: VecDeque<InvalidationRecord>,
}

/// Provides the operations defined for InvalidationDiagnostics.
impl InvalidationDiagnostics {
    /// Saturating-increments aggregate counters and appends one bounded record.
    ///
    /// At capacity, the oldest record is removed before insertion and the
    /// dropped counter advances. Sequence and all counters saturate rather than
    /// wrap.
    ///
    /// # Examples
    ///
    /// The public runtime path invokes this recorder:
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, RuntimeHandle};
    /// let handle = RuntimeHandle::<()>::new();
    /// handle.invalidate(ElementId(1), Invalidation::Layout);
    /// assert_eq!(handle.invalidation_diagnostics().layout_requests, 1);
    /// ```
    pub(crate) fn record(
        &mut self,
        element_tree_id: ElementTreeId,
        element_id: ElementId,
        invalidation: Invalidation,
        source: InvalidationSource,
        coalesced: bool,
    ) {
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.snapshot.requests = self.snapshot.requests.saturating_add(1);
        self.snapshot.coalesced_requests = self
            .snapshot
            .coalesced_requests
            .saturating_add(u64::from(coalesced));
        match invalidation {
            Invalidation::Paint => {
                self.snapshot.paint_requests = self.snapshot.paint_requests.saturating_add(1)
            }
            Invalidation::Layout => {
                self.snapshot.layout_requests = self.snapshot.layout_requests.saturating_add(1)
            }
            Invalidation::Build => {
                self.snapshot.build_requests = self.snapshot.build_requests.saturating_add(1)
            }
        }
        if self.records.len() == INVALIDATION_PROVENANCE_CAPACITY {
            self.records.pop_front();
            self.snapshot.dropped_provenance_records =
                self.snapshot.dropped_provenance_records.saturating_add(1);
        }
        self.records.push_back(InvalidationRecord {
            sequence: self.next_sequence,
            element_tree_id,
            element_id,
            invalidation,
            source,
            coalesced,
        });
    }

    /// Clones aggregate counters and materializes the current provenance ring.
    ///
    /// Snapshotting is O(number of retained records), at most 256, and does not
    /// clear or otherwise mutate diagnostics.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let handle = RuntimeHandle::<()>::new();
    /// let a = handle.invalidation_diagnostics();
    /// let b = handle.invalidation_diagnostics();
    /// assert_eq!(a, b);
    /// ```
    pub(crate) fn snapshot(&self) -> InvalidationDiagnosticsSnapshot {
        let mut snapshot = self.snapshot.clone();
        snapshot.records = self.records.iter().copied().collect();
        snapshot
    }
}

/// Permanent per-element counters for retained work actually executed.
///
/// Every field saturates at `u64::MAX`. Counters remain associated with an
/// element until its retained-tree entry is removed; they count runtime work,
/// not invalidation requests.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::ElementWorkCounters;
/// let counters = ElementWorkCounters::default();
/// assert_eq!(counters.builds, 0);
/// assert_eq!(counters.layout_cache_hits, 0);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ElementWorkCounters {
    /// Component build/reconciliation executions.
    pub builds: u64,
    /// Widget layout executions.
    pub layouts: u64,
    /// Widget paint executions.
    pub paints: u64,
    /// Element hit-test visits.
    pub hit_tests: u64,
    /// Retained layout cache hits.
    pub layout_cache_hits: u64,
    /// Retained layout cache misses.
    pub layout_cache_misses: u64,
    /// Layout invalidations propagated through the tree.
    pub layout_propagations: u64,
    /// Post-layout geometry commits.
    pub layout_commits: u64,
}

/// Provides the operations defined for ElementWorkCounters.
impl ElementWorkCounters {
    /// Saturating-adds every category from `other`.
    fn add_assign(&mut self, other: Self) {
        self.builds = self.builds.saturating_add(other.builds);
        self.layouts = self.layouts.saturating_add(other.layouts);
        self.paints = self.paints.saturating_add(other.paints);
        self.hit_tests = self.hit_tests.saturating_add(other.hit_tests);
        self.layout_cache_hits = self
            .layout_cache_hits
            .saturating_add(other.layout_cache_hits);
        self.layout_cache_misses = self
            .layout_cache_misses
            .saturating_add(other.layout_cache_misses);
        self.layout_propagations = self
            .layout_propagations
            .saturating_add(other.layout_propagations);
        self.layout_commits = self.layout_commits.saturating_add(other.layout_commits);
    }
}

/// Snapshot for one retained element tree.
///
/// `totals` is computed by saturating-summing every value in `elements` at
/// snapshot time. The map contains only elements for which work has been
/// recorded and uses tree-local [`ElementId`] keys.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::ElementTreeDiagnosticsSnapshot;
/// let snapshot = ElementTreeDiagnosticsSnapshot::default();
/// assert_eq!(snapshot.totals, Default::default());
/// assert!(snapshot.elements.is_empty());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ElementTreeDiagnosticsSnapshot {
    /// Saturating sum across all retained element counters.
    pub totals: ElementWorkCounters,
    /// Per-element counters for entries with recorded work.
    pub elements: HashMap<ElementId, ElementWorkCounters>,
}

#[derive(Default)]
/// Interior-mutable per-tree work-counter store.
///
/// # Examples
///
/// Public callers obtain snapshots from an [`crate::element::ElementTree`]:
///
/// ```
/// use ailloli_ui_runtime::element::ElementTree;
/// let tree = ElementTree::<()>::new();
/// assert!(tree.diagnostics().elements.is_empty());
/// ```
pub(crate) struct ElementTreeDiagnostics {
    /// Sparse tree-local counters behind runtime-internal interior mutability.
    counters: std::cell::RefCell<HashMap<ElementId, ElementWorkCounters>>,
}

/// Provides the operations defined for ElementTreeDiagnostics.
impl ElementTreeDiagnostics {
    /// Creates a counter entry if needed and applies one synchronous update.
    ///
    /// # Panics
    ///
    /// Panics on a conflicting borrow of the internal [`std::cell::RefCell`].
    fn update(&self, element_id: ElementId, update: impl FnOnce(&mut ElementWorkCounters)) {
        update(self.counters.borrow_mut().entry(element_id).or_default());
    }

    /// Saturating-increments the element's build counter.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// let tree = ElementTree::<()>::new();
    /// assert_eq!(tree.diagnostics().totals.builds, 0);
    /// ```
    pub(crate) fn build(&self, id: ElementId) {
        self.update(id, |value| value.builds = value.builds.saturating_add(1));
    }

    /// Saturating-increments the element's layout counter.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// assert_eq!(ElementTree::<()>::new().diagnostics().totals.layouts, 0);
    /// ```
    pub(crate) fn layout(&self, id: ElementId) {
        self.update(id, |value| value.layouts = value.layouts.saturating_add(1));
    }

    /// Saturating-increments the element's paint counter.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// assert_eq!(ElementTree::<()>::new().diagnostics().totals.paints, 0);
    /// ```
    pub(crate) fn paint(&self, id: ElementId) {
        self.update(id, |value| value.paints = value.paints.saturating_add(1));
    }

    /// Saturating-increments the element's hit-test counter.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// assert_eq!(ElementTree::<()>::new().diagnostics().totals.hit_tests, 0);
    /// ```
    pub(crate) fn hit_test(&self, id: ElementId) {
        self.update(id, |value| {
            value.hit_tests = value.hit_tests.saturating_add(1)
        });
    }

    /// Saturating-increments the element's layout-cache-hit counter.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// assert_eq!(ElementTree::<()>::new().diagnostics().totals.layout_cache_hits, 0);
    /// ```
    pub(crate) fn layout_cache_hit(&self, id: ElementId) {
        self.update(id, |value| {
            value.layout_cache_hits = value.layout_cache_hits.saturating_add(1)
        });
    }

    /// Saturating-increments the element's layout-cache-miss counter.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// assert_eq!(ElementTree::<()>::new().diagnostics().totals.layout_cache_misses, 0);
    /// ```
    pub(crate) fn layout_cache_miss(&self, id: ElementId) {
        self.update(id, |value| {
            value.layout_cache_misses = value.layout_cache_misses.saturating_add(1)
        });
    }

    /// Saturating-increments the element's layout-propagation counter.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// assert_eq!(ElementTree::<()>::new().diagnostics().totals.layout_propagations, 0);
    /// ```
    pub(crate) fn layout_propagation(&self, id: ElementId) {
        self.update(id, |value| {
            value.layout_propagations = value.layout_propagations.saturating_add(1)
        });
    }

    /// Saturating-increments the element's layout-commit counter.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// assert_eq!(ElementTree::<()>::new().diagnostics().totals.layout_commits, 0);
    /// ```
    pub(crate) fn layout_commit(&self, id: ElementId) {
        self.update(id, |value| {
            value.layout_commits = value.layout_commits.saturating_add(1)
        });
    }

    /// Removes all counters for one element, returning no missing-ID signal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// let tree = ElementTree::<()>::new();
    /// assert!(tree.diagnostics().elements.is_empty());
    /// ```
    pub(crate) fn remove(&self, id: ElementId) {
        self.counters.borrow_mut().remove(&id);
    }

    /// Clones sparse counters and computes saturating totals.
    ///
    /// The operation is O(number of elements with recorded work) and leaves the
    /// live counters unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// let tree = ElementTree::<()>::new();
    /// assert_eq!(tree.diagnostics(), tree.diagnostics());
    /// ```
    pub(crate) fn snapshot(&self) -> ElementTreeDiagnosticsSnapshot {
        let elements = self.counters.borrow().clone();
        let mut totals = ElementWorkCounters::default();
        for counters in elements.values().copied() {
            totals.add_assign(counters);
        }
        ElementTreeDiagnosticsSnapshot { totals, elements }
    }
}
