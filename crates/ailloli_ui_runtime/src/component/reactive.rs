//! Internal retained dependency tracking for UI-local reactive values.
//!
//! This module is public only so sibling framework crates can share the
//! provider-neutral contracts. It is deliberately hidden from generated API
//! documentation and is not re-exported by the `ailloli_ui` facade.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

use ailloli_ui_core::ElementId;

use crate::app::Invalidation;
use crate::popup::ElementTreeId;

thread_local! {
    /// Next source identity on the owning UI thread.
    static NEXT_SOURCE_ID: Cell<u64> = const { Cell::new(1) };
    /// Next dependency-graph identity on the owning UI thread.
    static NEXT_GRAPH_ID: Cell<u64> = const { Cell::new(1) };
    /// Nested observation scopes; `None` is an explicit untracked barrier.
    static OBSERVATION_STACK: RefCell<Vec<Option<Rc<ReadCollector>>>> = const { RefCell::new(Vec::new()) };
    /// Empty collectors retained across stable traversals on the UI thread.
    static READ_COLLECTOR_POOL: RefCell<Vec<Rc<ReadCollector>>> = const { RefCell::new(Vec::new()) };
    /// Physical observation-staging allocations, exposed only to internal tests.
    static REACTIVE_STAGING_ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

/// Maximum number of empty observation collectors retained by one UI thread.
const READ_COLLECTOR_POOL_LIMIT: usize = 256;

/// Maximum number of anomalous dead weak targets removed by one notification.
///
/// Normal lifecycle cleanup is eager through [`ConsumerDependencies::drop`].
/// This budget is only a fail-safe for targets made dead by abnormal teardown,
/// and prevents one mutation from turning that cleanup into an unbounded sweep.
const DEAD_SUBSCRIBER_PRUNE_BUDGET: usize = 64;

/// Stable identity of one UI-local reactive source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ReactiveSourceId(u64);

/// Stable identity of one runtime-owned reactive dependency graph.
///
/// Standalone runtimes each begin with retained tree namespace zero, so the
/// tree-local consumer identity alone cannot safely key subscriptions held by
/// a source shared between independent runtimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ReactiveDependencyGraphId(u64);

impl ReactiveDependencyGraphId {
    /// Allocates a checked UI-thread-local identity without silent reuse.
    fn next() -> Self {
        NEXT_GRAPH_ID.with(|next| {
            let id = next.get();
            next.set(
                id.checked_add(1)
                    .expect("reactive dependency graph identifier space exhausted"),
            );
            Self(id)
        })
    }
}

/// Nonzero generation of one retained payload mounted at an element ID.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MountGeneration(u64);

impl MountGeneration {
    /// Initial generation assigned to a newly-created retained element.
    pub const INITIAL: Self = Self(1);

    /// Returns the numeric generation for diagnostics and tests.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next nonzero generation without silent reuse.
    pub(crate) fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("retained mount generation space exhausted"),
        )
    }
}

/// Retained callback stage that consumed one or more reactive sources.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReactiveStage {
    /// Declarative component build.
    Build,
    /// Widget or transparent-component layout.
    Layout,
    /// Base paint and overlay paint for one element.
    Paint,
}

impl ReactiveStage {
    /// Maps a consumer stage to the minimum retained work it requires.
    pub(crate) const fn invalidation(self) -> Invalidation {
        match self {
            Self::Build => Invalidation::Build,
            Self::Layout => Invalidation::Layout,
            Self::Paint => Invalidation::Paint,
        }
    }
}

/// Exact identity of one mounted retained callback consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ReactiveConsumer {
    /// Retained-tree namespace.
    pub(crate) element_tree_id: ElementTreeId,
    /// Tree-local retained element.
    pub(crate) element_id: ElementId,
    /// Payload generation guarding stale callbacks.
    pub(crate) mount_generation: MountGeneration,
    /// Callback stage determining invalidation strength.
    pub(crate) stage: ReactiveStage,
}

/// Source-side key for one exact consumer in one independent runtime graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ReactiveSubscriptionKey {
    /// Runtime-owned graph namespace.
    graph_id: ReactiveDependencyGraphId,
    /// Mounted tree-local callback consumer.
    consumer: ReactiveConsumer,
}

impl PartialOrd for ReactiveConsumer {
    /// Delegates to the complete deterministic consumer ordering.
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ReactiveConsumer {
    /// Orders tree, element, generation, then callback stage.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (
            self.element_tree_id,
            self.element_id.0,
            self.mount_generation,
            self.stage,
        )
            .cmp(&(
                other.element_tree_id,
                other.element_id.0,
                other.mount_generation,
                other.stage,
            ))
    }
}

/// Shared source metadata retained by every clone of a signal.
pub(crate) struct ReactiveSource {
    /// Stable UI-thread-local identity.
    id: ReactiveSourceId,
    /// Shared public wrapping revision.
    revision: Cell<u64>,
    /// Callback installed when the source was first constructed.
    historical_invalidator: Rc<dyn Fn()>,
    /// Weak retained consumers keyed by their complete mounted identity.
    subscribers: RefCell<BTreeMap<ReactiveSubscriptionKey, Weak<ReactiveSubscriber>>>,
}

impl ReactiveSource {
    /// Creates a pristine source and allocates a checked identity.
    pub(crate) fn new(historical_invalidator: Rc<dyn Fn()>) -> Rc<Self> {
        let id = NEXT_SOURCE_ID.with(|next| {
            let id = next.get();
            next.set(
                id.checked_add(1)
                    .expect("reactive source identifier space exhausted"),
            );
            ReactiveSourceId(id)
        });
        Rc::new(Self {
            id,
            revision: Cell::new(0),
            historical_invalidator,
            subscribers: RefCell::new(BTreeMap::new()),
        })
    }

    /// Returns the current public wrapping revision.
    pub(crate) fn revision(&self) -> u64 {
        self.revision.get()
    }

    /// Advances the revision while preserving zero as the pristine sentinel.
    pub(crate) fn bump_revision(&self) {
        self.revision
            .set(self.revision.get().wrapping_add(1).max(1));
    }

    /// Records a successful read in the innermost active tracking scope.
    pub(crate) fn observe(self: &Rc<Self>) {
        let revision = self.revision();
        OBSERVATION_STACK.with(|stack| {
            let collector = stack.borrow().last().cloned().flatten();
            if let Some(collector) = collector {
                insert_read(
                    &mut collector.reads.borrow_mut(),
                    ReactiveRead::new(Rc::clone(self), revision),
                );
            }
        });
    }

    /// Installs or refreshes one weak subscriber for this source.
    fn subscribe(&self, subscriber: &Rc<ReactiveSubscriber>) {
        self.subscribers
            .borrow_mut()
            .insert(subscriber.key, Rc::downgrade(subscriber));
    }

    /// Removes one exact retained consumer from this source.
    fn unsubscribe(&self, key: ReactiveSubscriptionKey) {
        self.subscribers.borrow_mut().remove(&key);
    }

    /// Notifies live retained consumers first, then the historical invalidator.
    ///
    /// Both callback classes execute after the source's subscriber borrow has
    /// ended. Dead weak targets are pruned while producing the owned snapshot.
    pub(crate) fn notify(&self) {
        let subscribers = {
            let mut retained = self.subscribers.borrow_mut();
            let mut live = BTreeMap::<
                (
                    ReactiveDependencyGraphId,
                    ElementTreeId,
                    u64,
                    MountGeneration,
                ),
                Rc<ReactiveSubscriber>,
            >::new();
            let mut dead = Vec::new();
            for (key, target) in retained.iter() {
                let Some(target) = target.upgrade() else {
                    if dead.len() < DEAD_SUBSCRIBER_PRUNE_BUDGET {
                        dead.push(*key);
                    }
                    continue;
                };
                let mounted = (
                    key.graph_id,
                    key.consumer.element_tree_id,
                    key.consumer.element_id.0,
                    key.consumer.mount_generation,
                );
                match live.entry(mounted) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(target);
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        if key.consumer.stage.invalidation()
                            > entry.get().key.consumer.stage.invalidation()
                        {
                            entry.insert(target);
                        }
                    }
                }
            }
            for key in dead {
                retained.remove(&key);
            }
            live.into_values().collect::<Vec<_>>()
        };
        for subscriber in subscribers {
            subscriber.notify();
        }
        (self.historical_invalidator)();
    }
}

/// One source and the revision successfully observed from it.
#[derive(Clone)]
struct ReactiveRead {
    /// Source retained by a staged/committed dependency set.
    source: Rc<ReactiveSource>,
    /// Revision observed by the consumer callback.
    revision: u64,
}

impl ReactiveRead {
    /// Captures one source observation.
    fn new(source: Rc<ReactiveSource>, revision: u64) -> Self {
        Self { source, revision }
    }
}

/// Immutable-by-convention set of direct reactive reads from one callback.
#[doc(hidden)]
#[derive(Default)]
pub struct ReactiveReadSet {
    /// Sorted immutable-by-convention reads shared by retained snapshots.
    collector: Option<Rc<ReadCollector>>,
}

/// Internal cumulative counters for retained reactive work.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReactiveRuntimeDiagnosticsSnapshot {
    source_observations: u64,
    dependency_publications: u64,
    subscription_renewals: u64,
    subscription_noops: u64,
    consumer_notifications: u64,
    abandoned_layout_transactions: u64,
    stale_paint_feedback: u64,
    mount_cleanup_key_probes: u64,
    live_consumers: usize,
}

impl ReactiveRuntimeDiagnosticsSnapshot {
    /// Number of distinct source reads presented for dependency publication.
    pub const fn source_observations(self) -> u64 {
        self.source_observations
    }

    /// Number of consumer dependency sets presented after successful callbacks.
    pub const fn dependency_publications(self) -> u64 {
        self.dependency_publications
    }

    /// Number of publications that changed at least one source edge.
    pub const fn subscription_renewals(self) -> u64 {
        self.subscription_renewals
    }

    /// Number of equal or already-empty publications requiring no edge churn.
    pub const fn subscription_noops(self) -> u64 {
        self.subscription_noops
    }

    /// Number of generation-checked retained consumer notifications delivered.
    pub const fn consumer_notifications(self) -> u64 {
        self.consumer_notifications
    }

    /// Number of whole layout overlays discarded after panic or supersession.
    pub const fn abandoned_layout_transactions(self) -> u64 {
        self.abandoned_layout_transactions
    }

    /// Number of unique stale paint units that requested deferred work.
    pub const fn stale_paint_feedback(self) -> u64 {
        self.stale_paint_feedback
    }

    /// Number of exact Build/Layout/Paint keys probed during mount cleanup.
    pub const fn mount_cleanup_key_probes(self) -> u64 {
        self.mount_cleanup_key_probes
    }

    /// Number of exact mounted Build/Layout/Paint consumers currently retained.
    pub const fn live_consumers(self) -> usize {
        self.live_consumers
    }
}

/// Mutable runtime-owned form of [`ReactiveRuntimeDiagnosticsSnapshot`].
#[derive(Default)]
pub(crate) struct ReactiveRuntimeDiagnostics {
    source_observations: u64,
    dependency_publications: u64,
    subscription_renewals: u64,
    subscription_noops: u64,
    consumer_notifications: u64,
    abandoned_layout_transactions: u64,
    stale_paint_feedback: u64,
    mount_cleanup_key_probes: u64,
}

impl ReactiveRuntimeDiagnostics {
    /// Records one successful dependency-set publication attempt.
    pub(crate) fn publication(&mut self, sources: usize, renewed: bool) {
        self.source_observations = self.source_observations.saturating_add(sources as u64);
        self.dependency_publications = self.dependency_publications.saturating_add(1);
        if renewed {
            self.subscription_renewals = self.subscription_renewals.saturating_add(1);
        } else {
            self.subscription_noops = self.subscription_noops.saturating_add(1);
        }
    }

    /// Records one live retained notification before it enters dirty coalescing.
    pub(crate) fn consumer_notification(&mut self) {
        self.consumer_notifications = self.consumer_notifications.saturating_add(1);
    }

    /// Records a complete discarded layout overlay.
    pub(crate) fn abandoned_layout_transaction(&mut self) {
        self.abandoned_layout_transactions = self.abandoned_layout_transactions.saturating_add(1);
    }

    /// Records unique stale paint units from one completed traversal.
    pub(crate) fn stale_paint_feedback(&mut self, count: usize) {
        self.stale_paint_feedback = self.stale_paint_feedback.saturating_add(count as u64);
    }

    /// Records fixed-key probes used to remove one exact mounted generation.
    pub(crate) fn mount_cleanup_key_probes(&mut self, count: usize) {
        self.mount_cleanup_key_probes = self.mount_cleanup_key_probes.saturating_add(count as u64);
    }

    /// Produces an owned snapshot without resetting counters.
    pub(crate) const fn snapshot(
        &self,
        live_consumers: usize,
    ) -> ReactiveRuntimeDiagnosticsSnapshot {
        ReactiveRuntimeDiagnosticsSnapshot {
            source_observations: self.source_observations,
            dependency_publications: self.dependency_publications,
            subscription_renewals: self.subscription_renewals,
            subscription_noops: self.subscription_noops,
            consumer_notifications: self.consumer_notifications,
            abandoned_layout_transactions: self.abandoned_layout_transactions,
            stale_paint_feedback: self.stale_paint_feedback,
            mount_cleanup_key_probes: self.mount_cleanup_key_probes,
            live_consumers,
        }
    }
}

/// One staged dependency replacement used by a runtime-owned atomic batch.
#[doc(hidden)]
#[derive(Clone)]
pub struct ReactiveDependencyUpdate {
    /// Tree-local element receiving the dependency set.
    pub(crate) element_id: ElementId,
    /// Exact retained payload generation observed by the callback.
    pub(crate) mount_generation: MountGeneration,
    /// Callback stage determining future invalidation strength.
    pub(crate) stage: ReactiveStage,
    /// Direct source reads published after callback success.
    pub(crate) reads: ReactiveReadSet,
}

/// Verdict returned after an atomic dependency-publication batch.
///
/// An accepted batch may be a semantic no-op when every consumer already
/// retains the same source membership. `Stale` is reserved for a batch whose
/// complete set of mount generations no longer matches the runtime.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactiveDependencyBatchResult {
    /// Every generation was current and the complete batch was applied.
    Accepted {
        /// Whether at least one consumer changed its source membership.
        renewed: bool,
    },
    /// At least one consumer generation was stale; no update was applied.
    Stale,
}

impl ReactiveDependencyBatchResult {
    /// Returns whether the complete batch was accepted by the runtime.
    pub const fn is_accepted(self) -> bool {
        matches!(self, Self::Accepted { .. })
    }

    /// Returns the membership-change verdict for an accepted batch.
    pub const fn renewed(self) -> Option<bool> {
        match self {
            Self::Accepted { renewed } => Some(renewed),
            Self::Stale => None,
        }
    }
}

impl ReactiveDependencyUpdate {
    /// Creates one staged replacement without mutating the runtime graph.
    pub fn new(
        element_id: ElementId,
        mount_generation: MountGeneration,
        stage: ReactiveStage,
        reads: ReactiveReadSet,
    ) -> Self {
        Self {
            element_id,
            mount_generation,
            stage,
            reads,
        }
    }
}

impl ReactiveReadSet {
    /// Returns true when no reactive source was observed.
    pub fn is_empty(&self) -> bool {
        self.collector
            .as_ref()
            .is_none_or(|collector| collector.reads.borrow().is_empty())
    }

    /// Returns the number of distinct directly observed sources.
    pub fn len(&self) -> usize {
        self.collector
            .as_ref()
            .map_or(0, |collector| collector.reads.borrow().len())
    }

    /// Merges another direct-read set while preserving the first observed revision.
    ///
    /// Keeping the earlier snapshot ensures that observing the same source on
    /// both sides of a mid-attempt mutation makes [`Self::is_current`] fail.
    pub fn merge(&mut self, other: &Self) {
        let Some(other_collector) = &other.collector else {
            return;
        };
        if self
            .collector
            .as_ref()
            .is_some_and(|collector| Rc::ptr_eq(collector, other_collector))
        {
            return;
        }
        if self.collector.is_none() {
            self.collector = Some(Rc::clone(other_collector));
            return;
        }

        self.make_collector_unique();
        let target = self
            .collector
            .as_ref()
            .expect("nonempty read set must retain a collector");
        let other_reads = other_collector.reads.borrow();
        let mut target_reads = target.reads.borrow_mut();
        for read in other_reads.iter() {
            insert_read(&mut target_reads, read.clone());
        }
    }

    /// Returns whether both sets retain exactly the same source identities.
    pub fn same_sources(&self, other: &Self) -> bool {
        match (&self.collector, &other.collector) {
            (None, None) => true,
            (Some(left), Some(right)) if Rc::ptr_eq(left, right) => true,
            (Some(left), Some(right)) => left
                .reads
                .borrow()
                .iter()
                .map(|read| read.source.id)
                .eq(right.reads.borrow().iter().map(|read| read.source.id)),
            (Some(left), None) => left.reads.borrow().is_empty(),
            (None, Some(right)) => right.reads.borrow().is_empty(),
        }
    }

    /// Reinjects these reads into the innermost currently active scope.
    ///
    /// This is used when retained work reuses a cache entry without calling the
    /// original consumer callback again.
    pub fn adopt_into_current(&self) {
        let Some(source_collector) = &self.collector else {
            return;
        };
        OBSERVATION_STACK.with(|stack| {
            let collector = stack.borrow().last().cloned().flatten();
            if let Some(collector) = collector {
                let mut target = collector.reads.borrow_mut();
                for read in source_collector.reads.borrow().iter() {
                    insert_read(&mut target, read.clone());
                }
            }
        });
    }

    /// Returns whether every source still has its captured revision.
    pub fn is_current(&self) -> bool {
        self.collector.as_ref().is_none_or(|collector| {
            collector
                .reads
                .borrow()
                .iter()
                .all(|read| read.source.revision() == read.revision)
        })
    }

    /// Clones retained sources into a deterministic identity map.
    fn sources(&self) -> BTreeMap<ReactiveSourceId, Rc<ReactiveSource>> {
        self.collector
            .as_ref()
            .map_or_else(BTreeMap::new, |collector| {
                collector
                    .reads
                    .borrow()
                    .iter()
                    .map(|read| (read.source.id, Rc::clone(&read.source)))
                    .collect()
            })
    }

    /// Compares this sorted snapshot with a graph-owned source map.
    fn same_source_map(&self, sources: &BTreeMap<ReactiveSourceId, Rc<ReactiveSource>>) -> bool {
        self.collector.as_ref().map_or_else(
            || sources.is_empty(),
            |collector| {
                collector
                    .reads
                    .borrow()
                    .iter()
                    .map(|read| read.source.id)
                    .eq(sources.keys().copied())
            },
        )
    }

    /// Detaches a shared snapshot into a reusable collector before mutation.
    fn make_collector_unique(&mut self) {
        let Some(current) = &self.collector else {
            return;
        };
        if Rc::strong_count(current) == 1 {
            return;
        }

        let replacement = take_read_collector();
        {
            let mut replacement_reads = replacement.reads.borrow_mut();
            for read in current.reads.borrow().iter().cloned() {
                insert_read(&mut replacement_reads, read);
            }
        }
        self.collector = Some(replacement);
    }
}

impl Clone for ReactiveReadSet {
    /// Shares the immutable snapshot without cloning its source vector.
    fn clone(&self) -> Self {
        Self {
            collector: self.collector.clone(),
        }
    }
}

impl Drop for ReactiveReadSet {
    /// Returns an exclusively-owned collector to the bounded UI-thread pool.
    fn drop(&mut self) {
        if let Some(collector) = self.collector.take() {
            recycle_read_collector(collector);
        }
    }
}

/// Collector shared between one stack entry and its RAII owner or snapshot.
#[derive(Default)]
struct ReadCollector {
    /// First successful read per source, sorted by monotone source identity.
    reads: RefCell<Vec<ReactiveRead>>,
}

/// Inserts one first-revision-wins read while preserving deterministic order.
fn insert_read(reads: &mut Vec<ReactiveRead>, read: ReactiveRead) {
    match reads.binary_search_by_key(&read.source.id, |existing| existing.source.id) {
        Ok(_) => {}
        Err(index) => {
            if reads.len() == reads.capacity() {
                record_reactive_staging_allocation();
            }
            reads.insert(index, read);
        }
    }
}

/// Records one heap growth controlled directly by reactive observation staging.
fn record_reactive_staging_allocation() {
    let _ = REACTIVE_STAGING_ALLOCATIONS.try_with(|allocations| {
        allocations.set(allocations.get().saturating_add(1));
    });
}

/// Pushes one tracked scope or barrier while accounting for stack growth.
fn push_observation_scope(entry: Option<Rc<ReadCollector>>) {
    OBSERVATION_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        if stack.len() == stack.capacity() {
            record_reactive_staging_allocation();
        }
        stack.push(entry);
    });
}

/// Acquires one empty reusable collector and counts physical allocations.
fn take_read_collector() -> Rc<ReadCollector> {
    READ_COLLECTOR_POOL.with(|pool| {
        pool.borrow_mut().pop().unwrap_or_else(|| {
            record_reactive_staging_allocation();
            Rc::new(ReadCollector::default())
        })
    })
}

/// Recycles one exclusively-owned collector without losing vector capacity.
fn recycle_read_collector(collector: Rc<ReadCollector>) {
    if Rc::strong_count(&collector) != 1 {
        return;
    }
    collector.reads.borrow_mut().clear();
    let _ = READ_COLLECTOR_POOL.try_with(|pool| {
        let mut pool = pool.borrow_mut();
        if pool.len() < READ_COLLECTOR_POOL_LIMIT {
            if pool.len() == pool.capacity() {
                record_reactive_staging_allocation();
            }
            pool.push(collector);
        }
    });
}

/// Returns physical observation-staging allocations on the current UI thread.
#[doc(hidden)]
pub fn reactive_scope_allocation_count() -> u64 {
    REACTIVE_STAGING_ALLOCATIONS.with(Cell::get)
}

/// RAII scope collecting direct reactive reads on the owning UI thread.
#[doc(hidden)]
#[must_use = "dropping an unfinished reactive scope abandons its observations"]
pub struct ReactiveReadScope {
    /// Collector registered at the top of the thread-local stack.
    collector: Option<Rc<ReadCollector>>,
    /// Whether the matching stack entry still needs to be removed.
    active: bool,
}

impl ReactiveReadScope {
    /// Starts a nested scope; only this innermost scope receives later reads.
    pub fn new() -> Self {
        let collector = take_read_collector();
        push_observation_scope(Some(Rc::clone(&collector)));
        Self {
            collector: Some(collector),
            active: true,
        }
    }

    /// Closes this scope and returns its successfully observed sources.
    ///
    /// A scope must be finished in LIFO order. During unwinding, [`Drop`]
    /// restores the parent scope and abandons the partial set instead.
    pub fn finish(mut self) -> ReactiveReadSet {
        assert!(
            self.detach_from_stack(),
            "reactive observation scopes must close in LIFO order"
        );
        ReactiveReadSet {
            collector: self.collector.take(),
        }
    }

    /// Removes this scope and reports whether it was the active stack entry.
    ///
    /// The uncommon non-LIFO path removes the exact stale entry before the
    /// caller reports misuse. That prevents a second panic from this guard's
    /// destructor while the first panic is already unwinding.
    fn detach_from_stack(&mut self) -> bool {
        if !self.active {
            return true;
        }
        let was_top = OBSERVATION_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            let collector = self
                .collector
                .as_ref()
                .expect("active reactive scope must retain its collector");
            let position = stack.iter().rposition(|entry| {
                entry
                    .as_ref()
                    .is_some_and(|entry| Rc::ptr_eq(entry, collector))
            });
            let Some(position) = position else {
                return false;
            };
            let was_top = position + 1 == stack.len();
            stack.remove(position);
            was_top
        });
        self.active = false;
        was_top
    }
}

impl Default for ReactiveReadScope {
    /// Starts an empty observation scope.
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ReactiveReadScope {
    /// Restores the parent observation scope while abandoning unfinished reads.
    fn drop(&mut self) {
        let _ = self.detach_from_stack();
        if let Some(collector) = self.collector.take() {
            recycle_read_collector(collector);
        }
    }
}

/// Runs a closure behind an untracked barrier, restoring the parent on unwind.
#[doc(hidden)]
pub fn with_untracked_reads<R>(read: impl FnOnce() -> R) -> R {
    struct Barrier;

    impl Drop for Barrier {
        fn drop(&mut self) {
            OBSERVATION_STACK.with(|stack| {
                let entry = stack
                    .borrow_mut()
                    .pop()
                    .expect("untracked reactive barrier stack underflow");
                assert!(
                    entry.is_none(),
                    "untracked reactive barrier closed out of order"
                );
            });
        }
    }

    push_observation_scope(None);
    let barrier = Barrier;
    let result = read();
    drop(barrier);
    result
}

/// Strong runtime-owned notification target shared by all sources of a consumer.
pub(crate) struct ReactiveSubscriber {
    /// Complete runtime-graph and mounted-consumer source-map key.
    key: ReactiveSubscriptionKey,
    /// Weak-runtime callback; does not retain the runtime graph.
    notify: Rc<dyn Fn()>,
}

impl ReactiveSubscriber {
    /// Invokes the retained notification target outside source borrows.
    fn notify(&self) {
        (self.notify)();
    }
}

/// Runtime-owned source edges for one exact retained consumer.
struct ConsumerDependencies {
    /// Strong target making source-side weak subscriptions live.
    subscriber: Rc<ReactiveSubscriber>,
    /// Strong sources used for explicit delta unsubscribe and cleanup.
    sources: BTreeMap<ReactiveSourceId, Rc<ReactiveSource>>,
}

impl Drop for ConsumerDependencies {
    /// Eagerly removes every source-side weak edge owned by this inverse guard.
    fn drop(&mut self) {
        for source in self.sources.values() {
            source.unsubscribe(self.subscriber.key);
        }
    }
}

/// Exact retained dependency graph shared by all trees of one runtime handle.
pub(crate) struct ReactiveDependencyGraph {
    /// Runtime-unique namespace used by every source-side subscription key.
    id: ReactiveDependencyGraphId,
    /// Consumer identity to its current atomic source set.
    consumers: BTreeMap<ReactiveConsumer, ConsumerDependencies>,
}

impl Default for ReactiveDependencyGraph {
    /// Creates an empty graph with a checked runtime-unique identity.
    fn default() -> Self {
        Self {
            id: ReactiveDependencyGraphId::next(),
            consumers: BTreeMap::new(),
        }
    }
}

impl ReactiveDependencyGraph {
    /// Atomically replaces one consumer's source edges after callback success.
    ///
    /// Returns `true` only when membership changed. Equal sets keep the existing
    /// subscriber and source edges untouched, avoiding per-frame renewal.
    pub(crate) fn replace(
        &mut self,
        consumer: ReactiveConsumer,
        reads: &ReactiveReadSet,
        notify: impl FnOnce() -> Rc<dyn Fn()>,
    ) -> bool {
        let subscription_key = ReactiveSubscriptionKey {
            graph_id: self.id,
            consumer,
        };
        if self
            .consumers
            .get(&consumer)
            .is_some_and(|existing| reads.same_source_map(&existing.sources))
        {
            return false;
        }

        let next_sources = reads.sources();
        if next_sources.is_empty() {
            return self.remove_consumer(consumer);
        }

        let mut existing = self.consumers.remove(&consumer);
        let subscriber = existing.as_ref().map_or_else(
            || {
                Rc::new(ReactiveSubscriber {
                    key: subscription_key,
                    notify: notify(),
                })
            },
            |dependencies| Rc::clone(&dependencies.subscriber),
        );

        if let Some(dependencies) = &existing {
            for (id, source) in &dependencies.sources {
                if !next_sources.contains_key(id) {
                    source.unsubscribe(subscription_key);
                }
            }
        }
        for (id, source) in &next_sources {
            let already_present = existing
                .as_ref()
                .is_some_and(|dependencies| dependencies.sources.contains_key(id));
            if !already_present {
                source.subscribe(&subscriber);
            }
        }

        if let Some(dependencies) = existing.as_mut() {
            dependencies.sources = next_sources;
            self.consumers.insert(consumer, existing.unwrap());
        } else {
            self.consumers.insert(
                consumer,
                ConsumerDependencies {
                    subscriber,
                    sources: next_sources,
                },
            );
        }
        true
    }

    /// Removes one exact consumer and all of its source edges.
    pub(crate) fn remove_consumer(&mut self, consumer: ReactiveConsumer) -> bool {
        self.consumers.remove(&consumer).is_some()
    }

    /// Removes the three exact stage keys owned by one mounted generation.
    ///
    /// The returned count is the number of deterministic map probes performed,
    /// including absent stages. Normal subtree teardown therefore grows with
    /// the number of removed mounts instead of rescanning all live consumers.
    pub(crate) fn remove_mount(
        &mut self,
        element_tree_id: ElementTreeId,
        element_id: ElementId,
        mount_generation: MountGeneration,
    ) -> usize {
        let stages = [
            ReactiveStage::Build,
            ReactiveStage::Layout,
            ReactiveStage::Paint,
        ];
        for stage in stages {
            self.remove_consumer(ReactiveConsumer {
                element_tree_id,
                element_id,
                mount_generation,
                stage,
            });
        }
        stages.len()
    }

    /// Removes all graph edges belonging to one retained-tree namespace.
    pub(crate) fn remove_tree(&mut self, element_tree_id: ElementTreeId) {
        let consumers = self
            .consumers
            .keys()
            .filter(|consumer| consumer.element_tree_id == element_tree_id)
            .copied()
            .collect::<Vec<_>>();
        for consumer in consumers {
            self.remove_consumer(consumer);
        }
    }

    /// Returns the number of exact retained consumers for diagnostics/tests.
    pub(crate) fn len(&self) -> usize {
        self.consumers.len()
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    use super::*;

    #[test]
    fn nested_scopes_attribute_reads_only_to_the_innermost_scope() {
        let outer_source = ReactiveSource::new(Rc::new(|| {}));
        let inner_source = ReactiveSource::new(Rc::new(|| {}));
        let outer = ReactiveReadScope::new();
        outer_source.observe();
        let inner = ReactiveReadScope::new();
        inner_source.observe();
        let inner = inner.finish();
        let outer = outer.finish();

        assert_eq!((outer.len(), inner.len()), (1, 1));
    }

    #[test]
    fn untracked_barriers_restore_the_parent_scope_after_unwind() {
        let source = ReactiveSource::new(Rc::new(|| {}));
        let scope = ReactiveReadScope::new();
        let panic = catch_unwind(AssertUnwindSafe(|| {
            with_untracked_reads(|| {
                source.observe();
                panic!("expected test panic");
            });
        }));
        assert!(panic.is_err());
        source.observe();
        assert_eq!(scope.finish().len(), 1);
    }

    #[test]
    fn repeated_source_sets_do_not_renew_graph_membership() {
        let source = ReactiveSource::new(Rc::new(|| {}));
        let first_scope = ReactiveReadScope::new();
        source.observe();
        let first = first_scope.finish();
        source.bump_revision();
        let second_scope = ReactiveReadScope::new();
        source.observe();
        let second = second_scope.finish();
        assert!(first.same_sources(&second));

        let consumer = ReactiveConsumer {
            element_tree_id: ElementTreeId::new(4),
            element_id: ElementId(7),
            mount_generation: MountGeneration::INITIAL,
            stage: ReactiveStage::Layout,
        };
        let mut graph = ReactiveDependencyGraph::default();
        let subscriber_factories = Cell::new(0_u64);
        assert!(graph.replace(consumer, &first, || {
            subscriber_factories.set(subscriber_factories.get() + 1);
            Rc::new(|| {})
        }));
        assert!(!graph.replace(consumer, &second, || {
            subscriber_factories.set(subscriber_factories.get() + 1);
            Rc::new(|| {})
        }));
        assert_eq!(graph.len(), 1);
        assert_eq!(subscriber_factories.get(), 1);
    }

    #[test]
    fn mount_cleanup_probes_three_exact_keys_per_element() {
        const MOUNT_COUNT: u64 = 512;

        let source = ReactiveSource::new(Rc::new(|| {}));
        let scope = ReactiveReadScope::new();
        source.observe();
        let reads = scope.finish();
        let tree = ElementTreeId::new(11);
        let other_tree = ElementTreeId::new(12);
        let next_generation = MountGeneration::INITIAL.next();
        let stages = [
            ReactiveStage::Build,
            ReactiveStage::Layout,
            ReactiveStage::Paint,
        ];
        let mut graph = ReactiveDependencyGraph::default();

        for index in 0..MOUNT_COUNT {
            for stage in stages {
                assert!(graph.replace(
                    ReactiveConsumer {
                        element_tree_id: tree,
                        element_id: ElementId(index + 1),
                        mount_generation: MountGeneration::INITIAL,
                        stage,
                    },
                    &reads,
                    || Rc::new(|| {}),
                ));
            }
        }
        for (element_tree_id, mount_generation) in [
            (tree, next_generation),
            (other_tree, MountGeneration::INITIAL),
        ] {
            for stage in stages {
                assert!(graph.replace(
                    ReactiveConsumer {
                        element_tree_id,
                        element_id: ElementId(1),
                        mount_generation,
                        stage,
                    },
                    &reads,
                    || Rc::new(|| {}),
                ));
            }
        }

        let mut diagnostics = ReactiveRuntimeDiagnostics::default();
        for index in 0..MOUNT_COUNT {
            let probes = graph.remove_mount(tree, ElementId(index + 1), MountGeneration::INITIAL);
            diagnostics.mount_cleanup_key_probes(probes);
        }

        assert_eq!(graph.len(), stages.len() * 2);
        assert_eq!(
            diagnostics.snapshot(graph.len()).mount_cleanup_key_probes(),
            MOUNT_COUNT * stages.len() as u64,
        );
        graph.remove_tree(tree);
        assert_eq!(graph.len(), stages.len());
        graph.remove_tree(other_tree);
        assert_eq!(graph.len(), 0);
    }

    #[test]
    fn merging_reads_preserves_mutation_during_attempt_as_stale() {
        let source = ReactiveSource::new(Rc::new(|| {}));
        let before_scope = ReactiveReadScope::new();
        source.observe();
        let mut merged = before_scope.finish();
        source.bump_revision();
        let after_scope = ReactiveReadScope::new();
        source.observe();
        let after = after_scope.finish();

        merged.merge(&after);
        assert!(merged.same_sources(&after));
        assert!(!merged.is_current());
        assert!(after.is_current());
    }

    #[test]
    fn warmed_scopes_reuse_staging_storage() {
        let source = ReactiveSource::new(Rc::new(|| {}));
        let collect_once = || {
            let scope = ReactiveReadScope::new();
            source.observe();
            let reads = scope.finish();
            assert_eq!(reads.len(), 1);
        };

        collect_once();
        let allocations = reactive_scope_allocation_count();
        for _ in 0..100 {
            collect_once();
        }

        assert_eq!(reactive_scope_allocation_count(), allocations);
    }

    #[test]
    fn warmed_nested_and_abandoned_scopes_reuse_storage_and_restore_the_parent() {
        let outer_source = ReactiveSource::new(Rc::new(|| {}));
        let inner_source = ReactiveSource::new(Rc::new(|| {}));
        let collect_nested = || {
            let outer = ReactiveReadScope::new();
            outer_source.observe();
            let panic = catch_unwind(AssertUnwindSafe(|| {
                let _inner = ReactiveReadScope::new();
                inner_source.observe();
                panic!("expected nested-scope panic");
            }));
            assert!(panic.is_err());
            outer_source.observe();
            assert_eq!(outer.finish().len(), 1);
        };

        collect_nested();
        let allocations = reactive_scope_allocation_count();
        for _ in 0..20 {
            collect_nested();
        }

        assert_eq!(reactive_scope_allocation_count(), allocations);
    }

    #[test]
    fn copy_on_write_merge_keeps_shared_snapshots_immutable() {
        let first_source = ReactiveSource::new(Rc::new(|| {}));
        let second_source = ReactiveSource::new(Rc::new(|| {}));

        let first_scope = ReactiveReadScope::new();
        first_source.observe();
        let first = first_scope.finish();
        let mut merged = first.clone();

        let second_scope = ReactiveReadScope::new();
        second_source.observe();
        let second = second_scope.finish();
        merged.merge(&second);

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(merged.len(), 2);
        second_source.bump_revision();
        assert!(first.is_current());
        assert!(!second.is_current());
        assert!(!merged.is_current());
    }

    #[test]
    fn non_lifo_finish_panics_once_without_corrupting_the_remaining_scope() {
        let outer_source = ReactiveSource::new(Rc::new(|| {}));
        let inner_source = ReactiveSource::new(Rc::new(|| {}));
        let outer = ReactiveReadScope::new();
        outer_source.observe();
        let inner = ReactiveReadScope::new();
        inner_source.observe();

        let panic = catch_unwind(AssertUnwindSafe(|| drop(outer.finish())));
        assert!(panic.is_err());
        assert_eq!(inner.finish().len(), 1);
    }

    #[test]
    fn one_source_notification_keeps_only_the_strongest_stage_per_mounted_consumer() {
        let source = ReactiveSource::new(Rc::new(|| {}));
        let graph_id = ReactiveDependencyGraphId::next();
        let layout_calls = Rc::new(Cell::new(0_u64));
        let paint_calls = Rc::new(Cell::new(0_u64));
        let layout_seen = layout_calls.clone();
        let paint_seen = paint_calls.clone();
        let layout = Rc::new(ReactiveSubscriber {
            key: ReactiveSubscriptionKey {
                graph_id,
                consumer: ReactiveConsumer {
                    element_tree_id: ElementTreeId::new(1),
                    element_id: ElementId(7),
                    mount_generation: MountGeneration::INITIAL,
                    stage: ReactiveStage::Layout,
                },
            },
            notify: Rc::new(move || layout_seen.set(layout_seen.get() + 1)),
        });
        let paint = Rc::new(ReactiveSubscriber {
            key: ReactiveSubscriptionKey {
                graph_id,
                consumer: ReactiveConsumer {
                    element_tree_id: ElementTreeId::new(1),
                    element_id: ElementId(7),
                    mount_generation: MountGeneration::INITIAL,
                    stage: ReactiveStage::Paint,
                },
            },
            notify: Rc::new(move || paint_seen.set(paint_seen.get() + 1)),
        });
        source.subscribe(&paint);
        source.subscribe(&layout);

        source.notify();

        assert_eq!(layout_calls.get(), 1);
        assert_eq!(paint_calls.get(), 0);
    }

    #[test]
    fn inverse_guard_drop_eagerly_removes_source_side_weak_edges() {
        let source = ReactiveSource::new(Rc::new(|| {}));
        let scope = ReactiveReadScope::new();
        source.observe();
        let reads = scope.finish();
        let consumer = ReactiveConsumer {
            element_tree_id: ElementTreeId::new(2),
            element_id: ElementId(9),
            mount_generation: MountGeneration::INITIAL,
            stage: ReactiveStage::Build,
        };

        {
            let mut graph = ReactiveDependencyGraph::default();
            assert!(graph.replace(consumer, &reads, || Rc::new(|| {})));
            assert_eq!(source.subscribers.borrow().len(), 1);
        }

        assert!(source.subscribers.borrow().is_empty());
    }

    #[test]
    fn weak_source_side_edges_do_not_form_an_rc_cycle() {
        let source = ReactiveSource::new(Rc::new(|| {}));
        let weak_source = Rc::downgrade(&source);
        let scope = ReactiveReadScope::new();
        source.observe();
        let reads = scope.finish();
        let consumer = ReactiveConsumer {
            element_tree_id: ElementTreeId::new(2),
            element_id: ElementId(10),
            mount_generation: MountGeneration::INITIAL,
            stage: ReactiveStage::Build,
        };
        let mut graph = ReactiveDependencyGraph::default();
        assert!(graph.replace(consumer, &reads, || Rc::new(|| {})));

        drop(source);
        drop(reads);
        assert!(
            weak_source.upgrade().is_some(),
            "the inverse guard owns the source"
        );
        drop(graph);

        assert!(
            weak_source.upgrade().is_none(),
            "a weak source-side target must not retain the graph/source cycle"
        );
    }

    #[test]
    fn anomalous_dead_target_pruning_respects_the_per_notification_budget() {
        let source = ReactiveSource::new(Rc::new(|| {}));
        let graph_id = ReactiveDependencyGraphId::next();
        let dead_count = DEAD_SUBSCRIBER_PRUNE_BUDGET + 5;
        for index in 0..dead_count {
            let subscriber = Rc::new(ReactiveSubscriber {
                key: ReactiveSubscriptionKey {
                    graph_id,
                    consumer: ReactiveConsumer {
                        element_tree_id: ElementTreeId::new(3),
                        element_id: ElementId(index as u64 + 1),
                        mount_generation: MountGeneration::INITIAL,
                        stage: ReactiveStage::Paint,
                    },
                },
                notify: Rc::new(|| {}),
            });
            source.subscribe(&subscriber);
        }
        assert_eq!(source.subscribers.borrow().len(), dead_count);

        source.notify();
        assert_eq!(
            source.subscribers.borrow().len(),
            dead_count - DEAD_SUBSCRIBER_PRUNE_BUDGET,
        );
        source.notify();
        assert!(source.subscribers.borrow().is_empty());
    }
}
