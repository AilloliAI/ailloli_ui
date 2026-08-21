use std::collections::{HashMap, VecDeque};

use ailloli_ui_core::ElementId;

use crate::popup::ElementTreeId;

use super::Invalidation;

pub const INVALIDATION_PROVENANCE_CAPACITY: usize = 256;

/// Origin of a retained-work request. This is diagnostic metadata only; it
/// never changes invalidation semantics.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvalidationSource {
    Runtime,
    Context,
    Event,
    Signal,
    Timer,
    Model,
    Host,
    Compatibility,
}

/// One bounded provenance record for an invalidation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidationRecord {
    sequence: u64,
    element_tree_id: ElementTreeId,
    element_id: ElementId,
    invalidation: Invalidation,
    source: InvalidationSource,
    coalesced: bool,
}

impl InvalidationRecord {
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub const fn element_tree_id(&self) -> ElementTreeId {
        self.element_tree_id
    }

    pub const fn element_id(&self) -> ElementId {
        self.element_id
    }

    pub const fn invalidation(&self) -> Invalidation {
        self.invalidation
    }

    pub const fn source(&self) -> InvalidationSource {
        self.source
    }

    pub const fn was_coalesced(&self) -> bool {
        self.coalesced
    }
}

/// Snapshot of requested work. The provenance ring is bounded even during a
/// long-running chat or terminal session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InvalidationDiagnosticsSnapshot {
    pub requests: u64,
    pub coalesced_requests: u64,
    pub paint_requests: u64,
    pub layout_requests: u64,
    pub build_requests: u64,
    pub dropped_provenance_records: u64,
    pub records: Vec<InvalidationRecord>,
}

#[derive(Default)]
pub(crate) struct InvalidationDiagnostics {
    next_sequence: u64,
    snapshot: InvalidationDiagnosticsSnapshot,
    records: VecDeque<InvalidationRecord>,
}

impl InvalidationDiagnostics {
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

    pub(crate) fn snapshot(&self) -> InvalidationDiagnosticsSnapshot {
        let mut snapshot = self.snapshot.clone();
        snapshot.records = self.records.iter().copied().collect();
        snapshot
    }
}

/// Permanent per-element counters for retained work actually executed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ElementWorkCounters {
    pub builds: u64,
    pub layouts: u64,
    pub paints: u64,
    pub hit_tests: u64,
    pub layout_cache_hits: u64,
    pub layout_cache_misses: u64,
    pub layout_propagations: u64,
    pub layout_commits: u64,
}

impl ElementWorkCounters {
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ElementTreeDiagnosticsSnapshot {
    pub totals: ElementWorkCounters,
    pub elements: HashMap<ElementId, ElementWorkCounters>,
}

#[derive(Default)]
pub(crate) struct ElementTreeDiagnostics {
    counters: std::cell::RefCell<HashMap<ElementId, ElementWorkCounters>>,
}

impl ElementTreeDiagnostics {
    fn update(&self, element_id: ElementId, update: impl FnOnce(&mut ElementWorkCounters)) {
        update(self.counters.borrow_mut().entry(element_id).or_default());
    }

    pub(crate) fn build(&self, id: ElementId) {
        self.update(id, |value| value.builds = value.builds.saturating_add(1));
    }

    pub(crate) fn layout(&self, id: ElementId) {
        self.update(id, |value| value.layouts = value.layouts.saturating_add(1));
    }

    pub(crate) fn paint(&self, id: ElementId) {
        self.update(id, |value| value.paints = value.paints.saturating_add(1));
    }

    pub(crate) fn hit_test(&self, id: ElementId) {
        self.update(id, |value| {
            value.hit_tests = value.hit_tests.saturating_add(1)
        });
    }

    pub(crate) fn layout_cache_hit(&self, id: ElementId) {
        self.update(id, |value| {
            value.layout_cache_hits = value.layout_cache_hits.saturating_add(1)
        });
    }

    pub(crate) fn layout_cache_miss(&self, id: ElementId) {
        self.update(id, |value| {
            value.layout_cache_misses = value.layout_cache_misses.saturating_add(1)
        });
    }

    pub(crate) fn layout_propagation(&self, id: ElementId) {
        self.update(id, |value| {
            value.layout_propagations = value.layout_propagations.saturating_add(1)
        });
    }

    pub(crate) fn layout_commit(&self, id: ElementId) {
        self.update(id, |value| {
            value.layout_commits = value.layout_commits.saturating_add(1)
        });
    }

    pub(crate) fn remove(&self, id: ElementId) {
        self.counters.borrow_mut().remove(&id);
    }

    pub(crate) fn snapshot(&self) -> ElementTreeDiagnosticsSnapshot {
        let elements = self.counters.borrow().clone();
        let mut totals = ElementWorkCounters::default();
        for counters in elements.values().copied() {
            totals.add_assign(counters);
        }
        ElementTreeDiagnosticsSnapshot { totals, elements }
    }
}
