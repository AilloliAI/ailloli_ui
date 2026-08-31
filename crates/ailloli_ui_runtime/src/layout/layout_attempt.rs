//! Transactional staging for one retained layout traversal.

use std::cell::{Cell, RefCell};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

use ailloli_ui_core::ElementId;

use crate::component::reactive::{MountGeneration, ReactiveDependencyUpdate, ReactiveReadSet};
use crate::element::element_node::LayoutCacheKey;
use crate::layout::{LayoutPass, LayoutResult};

/// Maximum number of transaction workspaces retained by one UI thread.
const LAYOUT_ATTEMPT_POOL_LIMIT: usize = 4;
/// Largest element-sized transaction buffer retained after one traversal.
const RETAINED_LAYOUT_ENTRY_LIMIT: usize = 131_072;
/// Largest diagnostic/scratch buffer retained after one traversal.
const RETAINED_LAYOUT_SCRATCH_LIMIT: usize = RETAINED_LAYOUT_ENTRY_LIMIT * 2;

thread_local! {
    /// Bounded retained workspaces used by sequential outer layout attempts.
    static LAYOUT_ATTEMPT_POOL: RefCell<Vec<LayoutAttemptBuffers>> = const {
        RefCell::new(Vec::new())
    };
    /// Physical capacity growth attributable to layout transaction bookkeeping.
    static LAYOUT_STAGING_ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

/// Returns physical allocation events caused by layout transaction bookkeeping.
///
/// This counter intentionally excludes allocations owned by widget results,
/// text artifacts, runtime callback construction, and the reactive observation
/// collector (which has its own counter). It increments when a retained
/// transaction buffer or map must grow on the current UI thread.
#[doc(hidden)]
pub fn layout_staging_allocation_count() -> u64 {
    LAYOUT_STAGING_ALLOCATIONS.with(Cell::get)
}

/// Records one capacity growth owned directly by transactional layout staging.
fn record_layout_staging_allocation() {
    let _ = LAYOUT_STAGING_ALLOCATIONS.try_with(|allocations| {
        allocations.set(allocations.get().saturating_add(1));
    });
}

/// Pushes into a retained vector while counting only physical capacity growth.
fn push_staged<T>(values: &mut Vec<T>, value: T) {
    if values.len() == values.capacity() {
        record_layout_staging_allocation();
    }
    values.push(value);
}

/// Inserts a new retained-map key while counting physical capacity growth.
fn insert_staged<K: Eq + std::hash::Hash, V>(values: &mut HashMap<K, V>, key: K, value: V) {
    debug_assert!(!values.contains_key(&key));
    if values.len() == values.capacity() {
        record_layout_staging_allocation();
    }
    values.insert(key, value);
}

/// Process-unique identity of one outer transactional layout attempt.
///
/// This cross-crate token lets widgets associate deferred, geometry-derived
/// state with the exact authoritative attempt that produced it. Its numeric
/// representation is intentionally private and is not a cache epoch.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutAttemptToken(NonZeroU64);

impl LayoutAttemptToken {
    /// Allocates one identity without wrapping or silently reusing a token.
    fn allocate() -> Self {
        static NEXT_LAYOUT_ATTEMPT_ID: AtomicU64 = AtomicU64::new(0);

        let mut current = NEXT_LAYOUT_ATTEMPT_ID.load(Ordering::Relaxed);
        loop {
            let next = current
                .checked_add(1)
                .expect("layout attempt identity exhausted");
            match NEXT_LAYOUT_ATTEMPT_ID.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Self(
                        NonZeroU64::new(next).expect("layout attempt identity must be nonzero"),
                    );
                }
                Err(observed) => current = observed,
            }
        }
    }
}

/// One pass-specific cache write staged by a layout attempt.
pub(crate) struct StagedLayout {
    /// Retained element that produced the result.
    pub(crate) element_id: ElementId,
    /// Exact payload generation that produced this staged result.
    pub(crate) mount_generation: MountGeneration,
    /// Result kept outside authoritative tree state until validation succeeds.
    pub(crate) result: LayoutResult,
    /// Exact inputs associated with `result`.
    pub(crate) cache_key: LayoutCacheKey,
    /// Direct reactive reads for this pass, plus adopted measures at finalize.
    pub(crate) dependencies: ReactiveReadSet,
    /// Innermost explicit branch, whose complete ancestor chain must be adopted.
    pub(crate) measure_branch: Option<u64>,
    /// Whether the final staged result came from a real authoritative callback.
    pub(crate) callback_executed: bool,
}

/// Deferred work diagnostic emitted only when its attempt commits.
pub(crate) enum LayoutAttemptEvent {
    /// One pass-specific cache hit.
    CacheHit(ElementId),
    /// One pass-specific cache miss followed by a layout callback.
    CacheMiss(ElementId),
    /// One real layout callback execution.
    Layout(ElementId),
}

/// Validated output of one successful staging overlay.
pub(crate) struct FinishedLayoutAttempt {
    /// Exact outer attempt whose writes passed validation.
    pub(crate) token: LayoutAttemptToken,
    /// Reusable transaction workspace retained until publication completes.
    buffers: Option<LayoutAttemptBuffers>,
}

impl FinishedLayoutAttempt {
    /// Borrows every pass-specific cache and geometry write.
    pub(crate) fn entries(&self) -> &[StagedLayout] {
        &self.buffers().entries
    }

    /// Borrows each authoritative dependency union.
    pub(crate) fn authoritative_dependencies(
        &self,
    ) -> &[(ElementId, MountGeneration, ReactiveReadSet)] {
        &self.buffers().authoritative_dependencies
    }

    /// Drains deferred diagnostics in traversal order without releasing capacity.
    pub(crate) fn drain_events(&mut self) -> std::vec::Drain<'_, LayoutAttemptEvent> {
        self.buffers_mut().events.drain(..)
    }

    /// Drains staged writes in traversal order without releasing capacity.
    pub(crate) fn drain_entries(&mut self) -> std::vec::Drain<'_, StagedLayout> {
        self.buffers_mut().entries.drain(..)
    }

    /// Rebuilds the generation-failure set without allocating after warmup.
    pub(crate) fn collect_stale_elements(
        &mut self,
        mut generation_is_current: impl FnMut(ElementId, MountGeneration) -> bool,
    ) {
        self.buffers_mut().stale_elements.clear();
        for index in 0..self.entries().len() {
            let entry = &self.entries()[index];
            let element_id = entry.element_id;
            let mount_generation = entry.mount_generation;
            if !generation_is_current(element_id, mount_generation) {
                let stale = &mut self.buffers_mut().stale_elements;
                push_staged(stale, element_id);
            }
        }
    }

    /// Mutably borrows generation failures for sorting and retry scheduling.
    pub(crate) fn stale_elements_mut(&mut self) -> &mut Vec<ElementId> {
        &mut self.buffers_mut().stale_elements
    }

    /// Rebuilds the atomic-publication batch without allocating after warmup.
    pub(crate) fn rebuild_dependency_updates(
        &mut self,
        mut build: impl FnMut(
            ElementId,
            MountGeneration,
            ReactiveReadSet,
        ) -> Option<ReactiveDependencyUpdate>,
    ) {
        self.buffers_mut().dependency_updates.clear();
        for index in 0..self.authoritative_dependencies().len() {
            let (element_id, mount_generation, dependencies) =
                &self.authoritative_dependencies()[index];
            if let Some(update) = build(*element_id, *mount_generation, dependencies.clone()) {
                let updates = &mut self.buffers_mut().dependency_updates;
                push_staged(updates, update);
            }
        }
    }

    /// Borrows the complete atomic-publication batch.
    pub(crate) fn dependency_updates(&self) -> &[ReactiveDependencyUpdate] {
        &self.buffers().dependency_updates
    }

    /// Copies rejected batch consumers into generation-failure scratch storage.
    pub(crate) fn collect_update_elements_as_stale(&mut self) {
        self.buffers_mut().stale_elements.clear();
        for index in 0..self.dependency_updates().len() {
            let element_id = self.dependency_updates()[index].element_id;
            let stale = &mut self.buffers_mut().stale_elements;
            push_staged(stale, element_id);
        }
    }

    /// Borrows the retained workspace after successful validation.
    fn buffers(&self) -> &LayoutAttemptBuffers {
        self.buffers
            .as_ref()
            .expect("finished layout attempt must retain its buffers")
    }

    /// Mutably borrows the retained workspace after successful validation.
    fn buffers_mut(&mut self) -> &mut LayoutAttemptBuffers {
        self.buffers
            .as_mut()
            .expect("finished layout attempt must retain its buffers")
    }
}

impl Drop for FinishedLayoutAttempt {
    /// Returns all transaction capacity to the bounded UI-thread pool.
    fn drop(&mut self) {
        if let Some(buffers) = self.buffers.take() {
            recycle_layout_attempt_buffers(buffers);
        }
    }
}

/// Direct dependencies contributed by one speculative invocation.
struct MeasureContribution {
    /// Element whose authoritative dependencies may adopt the reads.
    element_id: ElementId,
    /// Exact payload generation that performed the speculative read.
    mount_generation: MountGeneration,
    /// Innermost explicit branch, whose complete ancestor chain must be adopted.
    branch: Option<u64>,
    /// Direct reads observed or restored from a measurement cache hit.
    dependencies: ReactiveReadSet,
}

/// Capacity-bearing storage reused across sequential outer layout attempts.
#[derive(Default)]
struct LayoutAttemptBuffers {
    /// Latest staged cache entry for each element and pass.
    entries: Vec<StagedLayout>,
    /// Stable vector positions used to replace repeated pass results in place.
    entry_indexes: HashMap<(ElementId, LayoutPass), (MountGeneration, usize)>,
    /// Every speculative read set that may contribute to a later commit.
    measure_contributions: Vec<MeasureContribution>,
    /// Diagnostics kept transactional with the runtime-owned layout writes.
    events: Vec<LayoutAttemptEvent>,
    /// Scratch union of adopted measurement and commit dependencies.
    measured_by_element: HashMap<(ElementId, MountGeneration), ReactiveReadSet>,
    /// Deterministic first-observation order for authoritative dependency sets.
    dependency_order: Vec<(ElementId, MountGeneration)>,
    /// Per-element union contributing to an outer authoritative traversal.
    authoritative_dependencies: Vec<(ElementId, MountGeneration, ReactiveReadSet)>,
    /// Atomic graph update batch assembled immediately before publication.
    dependency_updates: Vec<ReactiveDependencyUpdate>,
    /// Generation failures used by fail-closed retry scheduling.
    stale_elements: Vec<ElementId>,
}

impl LayoutAttemptBuffers {
    /// Drops attempt-owned values while retaining every backing allocation.
    fn clear(&mut self) {
        self.entries.clear();
        self.entry_indexes.clear();
        self.measure_contributions.clear();
        self.events.clear();
        self.measured_by_element.clear();
        self.dependency_order.clear();
        self.authoritative_dependencies.clear();
        self.dependency_updates.clear();
        self.stale_elements.clear();
    }

    /// Returns whether this workspace stays inside the explicit pool budget.
    fn is_retainable(&self) -> bool {
        self.entries.capacity() <= RETAINED_LAYOUT_ENTRY_LIMIT
            && self.entry_indexes.capacity() <= RETAINED_LAYOUT_SCRATCH_LIMIT
            && self.measure_contributions.capacity() <= RETAINED_LAYOUT_ENTRY_LIMIT
            && self.events.capacity() <= RETAINED_LAYOUT_SCRATCH_LIMIT
            && self.measured_by_element.capacity() <= RETAINED_LAYOUT_SCRATCH_LIMIT
            && self.dependency_order.capacity() <= RETAINED_LAYOUT_ENTRY_LIMIT
            && self.authoritative_dependencies.capacity() <= RETAINED_LAYOUT_ENTRY_LIMIT
            && self.dependency_updates.capacity() <= RETAINED_LAYOUT_ENTRY_LIMIT
            && self.stale_elements.capacity() <= RETAINED_LAYOUT_ENTRY_LIMIT
    }
}

/// Acquires one empty workspace from the bounded current-thread pool.
fn take_layout_attempt_buffers() -> LayoutAttemptBuffers {
    LAYOUT_ATTEMPT_POOL.with(|pool| pool.borrow_mut().pop().unwrap_or_default())
}

/// Recycles one workspace without retaining an unbounded number of trees.
fn recycle_layout_attempt_buffers(mut buffers: LayoutAttemptBuffers) {
    buffers.clear();
    if !buffers.is_retainable() {
        return;
    }
    let _ = LAYOUT_ATTEMPT_POOL.try_with(|pool| {
        let mut pool = pool.borrow_mut();
        if pool.len() < LAYOUT_ATTEMPT_POOL_LIMIT {
            if pool.len() == pool.capacity() {
                record_layout_staging_allocation();
            }
            pool.push(buffers);
        }
    });
}

/// Runtime-owned overlay for one outer layout call.
///
/// Cache and authoritative geometry writes are accumulated here and applied to
/// the retained tree only after every adopted dependency snapshot is current.
pub(crate) struct LayoutAttempt {
    /// Exact identity shared by every staged entry in this outer attempt.
    token: LayoutAttemptToken,
    /// Whether one element identity was reused for another payload mid-attempt.
    generation_conflict: bool,
    /// Reusable capacity-bearing storage owned until finish or abandonment.
    buffers: Option<LayoutAttemptBuffers>,
}

impl LayoutAttempt {
    /// Starts a new overlay with a checked, never-reused identity.
    pub(crate) fn new() -> Self {
        Self {
            token: LayoutAttemptToken::allocate(),
            generation_conflict: false,
            buffers: Some(take_layout_attempt_buffers()),
        }
    }

    /// Returns the identity widgets use for their attempt-local staging.
    pub(crate) const fn token(&self) -> LayoutAttemptToken {
        self.token
    }

    /// Returns an exact staged cache hit before consulting retained state.
    pub(crate) fn cached(
        &self,
        element_id: ElementId,
        pass: LayoutPass,
        mount_generation: MountGeneration,
        cache_key: LayoutCacheKey,
    ) -> Option<(LayoutResult, ReactiveReadSet)> {
        let buffers = self.buffers();
        let (staged_generation, index) = buffers.entry_indexes.get(&(element_id, pass))?;
        let entry = buffers.entries.get(*index)?;
        (*staged_generation == mount_generation && entry.cache_key == cache_key)
            .then(|| (entry.result.clone(), entry.dependencies.clone()))
    }

    /// Stages the latest pass result and records every measurement contribution.
    pub(crate) fn stage(&mut self, entry: StagedLayout) {
        let StagedLayout {
            element_id,
            mount_generation,
            cache_key,
            ref dependencies,
            measure_branch,
            ..
        } = entry;
        let generation_conflict =
            [LayoutPass::Measure, LayoutPass::Commit]
                .into_iter()
                .any(|pass| {
                    self.buffers()
                        .entry_indexes
                        .get(&(element_id, pass))
                        .is_some_and(|(staged_generation, _)| {
                            *staged_generation != mount_generation
                        })
                });
        self.generation_conflict |= generation_conflict;
        if cache_key.layout_pass.is_measure() {
            push_staged(
                &mut self.buffers_mut().measure_contributions,
                MeasureContribution {
                    element_id,
                    mount_generation,
                    branch: measure_branch,
                    dependencies: dependencies.clone(),
                },
            );
        }

        if let Some((_, index)) = self
            .buffers()
            .entry_indexes
            .get(&(element_id, cache_key.layout_pass))
            .copied()
        {
            // Only the final invocation is authoritative. In particular, a
            // retained cache hit that supersedes an earlier callback must not
            // make that callback's attempt-local widget state publish later.
            self.buffers_mut().entries[index] = entry;
        } else {
            let buffers = self.buffers_mut();
            let index = buffers.entries.len();
            push_staged(&mut buffers.entries, entry);
            insert_staged(
                &mut buffers.entry_indexes,
                (element_id, cache_key.layout_pass),
                (mount_generation, index),
            );
        }
    }

    /// Records a diagnostic that becomes visible only after attempt validation.
    pub(crate) fn record(&mut self, event: LayoutAttemptEvent) {
        push_staged(&mut self.buffers_mut().events, event);
    }

    /// Validates and returns the writes accepted by a successful outer call.
    ///
    /// Explicitly abandoned measurements are removed. Accepted measurements
    /// are merged into the matching committed element while preserving the
    /// first revision observed for a source during the complete attempt.
    pub(crate) fn finish(
        mut self,
        branch_chain_is_adopted: impl Fn(u64) -> bool,
    ) -> Result<FinishedLayoutAttempt, ()> {
        if self.generation_conflict {
            return Err(());
        }

        let buffers = self
            .buffers
            .as_mut()
            .expect("active layout attempt must retain its buffers");
        buffers.measured_by_element.clear();
        buffers.dependency_order.clear();
        buffers.authoritative_dependencies.clear();
        for contribution in buffers.measure_contributions.drain(..) {
            let accepted = contribution.branch.is_none_or(&branch_chain_is_adopted);
            if accepted {
                let consumer = (contribution.element_id, contribution.mount_generation);
                let map_will_grow =
                    buffers.measured_by_element.len() == buffers.measured_by_element.capacity();
                let dependencies = match buffers.measured_by_element.entry(consumer) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => {
                        push_staged(&mut buffers.dependency_order, consumer);
                        if map_will_grow {
                            record_layout_staging_allocation();
                        }
                        entry.insert(ReactiveReadSet::default())
                    }
                };
                dependencies.merge(&contribution.dependencies);
            }
        }

        buffers.entries.retain(|entry| {
            !entry.cache_key.layout_pass.is_measure()
                || entry.measure_branch.is_none_or(&branch_chain_is_adopted)
        });
        for entry in &mut buffers.entries {
            if entry.cache_key.layout_pass.is_committed() {
                let consumer = (entry.element_id, entry.mount_generation);
                if let Some(measured) = buffers.measured_by_element.get(&consumer) {
                    let mut combined = measured.clone();
                    combined.merge(&entry.dependencies);
                    entry.dependencies = combined;
                }
                let map_will_grow =
                    buffers.measured_by_element.len() == buffers.measured_by_element.capacity();
                match buffers.measured_by_element.entry(consumer) {
                    Entry::Occupied(mut occupied) => {
                        occupied.insert(entry.dependencies.clone());
                    }
                    Entry::Vacant(vacant) => {
                        push_staged(&mut buffers.dependency_order, consumer);
                        if map_will_grow {
                            record_layout_staging_allocation();
                        }
                        vacant.insert(entry.dependencies.clone());
                    }
                }
            }
        }

        for index in 0..buffers.dependency_order.len() {
            let (element_id, mount_generation) = buffers.dependency_order[index];
            if let Some(dependencies) = buffers
                .measured_by_element
                .remove(&(element_id, mount_generation))
            {
                push_staged(
                    &mut buffers.authoritative_dependencies,
                    (element_id, mount_generation, dependencies),
                );
            }
        }
        if buffers
            .entries
            .iter()
            .all(|entry| entry.dependencies.is_current())
            && buffers
                .authoritative_dependencies
                .iter()
                .all(|(_, _, dependencies)| dependencies.is_current())
        {
            let buffers = self
                .buffers
                .take()
                .expect("validated layout attempt must retain its buffers");
            Ok(FinishedLayoutAttempt {
                token: self.token,
                buffers: Some(buffers),
            })
        } else {
            Err(())
        }
    }

    /// Borrows the active capacity-bearing workspace.
    fn buffers(&self) -> &LayoutAttemptBuffers {
        self.buffers
            .as_ref()
            .expect("active layout attempt must retain its buffers")
    }

    /// Mutably borrows the active capacity-bearing workspace.
    fn buffers_mut(&mut self) -> &mut LayoutAttemptBuffers {
        self.buffers
            .as_mut()
            .expect("active layout attempt must retain its buffers")
    }
}

impl Drop for LayoutAttempt {
    /// Recycles abandoned or failed transaction storage without publishing it.
    fn drop(&mut self) {
        if let Some(buffers) = self.buffers.take() {
            recycle_layout_attempt_buffers(buffers);
        }
    }
}
