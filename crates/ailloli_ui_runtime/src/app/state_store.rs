//! Type-erased, tree-scoped retained component state slots.

use crate::component::signal::Signal;
use crate::popup::ElementTreeId;
use ailloli_ui_core::ids::ElementId;
use std::any::Any;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Public legacy slot identity within the root tree namespace.
///
/// Slot indices are caller-assigned `usize` values with no reserved sentinel.
/// The same element and index must always be requested with the same concrete
/// value type while mounted.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId;
/// use ailloli_ui_runtime::app::StateSlot;
/// let slot = StateSlot { element_id: ElementId(3), slot_index: 1 };
/// assert_eq!(slot.slot_index, 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StateSlot {
    /// Tree-local retained element identity.
    pub element_id: ElementId,
    /// Positional state-hook slot within that element.
    pub slot_index: usize,
}

/// Complete private key adding a retained-tree namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ScopedStateSlot {
    /// Retained-tree namespace preventing cross-runtime aliasing.
    element_tree_id: ElementTreeId,
    /// Element and positional slot within the tree.
    slot: StateSlot,
}

/// Type-erased retained signal storage keyed by tree, element, and slot index.
///
/// The store has no automatic capacity limit; reconciliation must remove
/// unmounted element/tree slots. Stored values and returned signals are
/// `Rc<RefCell<_>>` based, making this a single-UI-thread facility. The first
/// concrete type used for a mounted key remains mandatory until removal.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::StateStore;
/// let _store = StateStore::default();
/// ```
#[derive(Default)]
pub struct StateStore {
    /// Sparse type-erased `(value, revision)` tuples.
    values: HashMap<ScopedStateSlot, Box<dyn Any>>,
}

/// Provides the operations defined for StateStore.
impl StateStore {
    /// Returns a signal stored in the legacy root-tree namespace (`tree 0`).
    ///
    /// Retained runtimes use [`Self::signal_scoped`] so equal `ElementId`
    /// values from independent element trees cannot alias one another.
    ///
    /// `initial` is evaluated by the caller before this method; it is discarded
    /// if the slot already exists. Returned handles share value/revision, while
    /// each captures the `invalidate` callback supplied on that particular call.
    ///
    /// # Panics
    ///
    /// Panics if this mounted tree-0/element/index key was first created with a
    /// different concrete `T`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::rc::Rc;
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::StateStore;
    /// let mut store = StateStore::default();
    /// let first = store.signal(ElementId(1), 0, 10, Rc::new(|| {}));
    /// first.set(11);
    /// let reused = store.signal(ElementId(1), 0, 99, Rc::new(|| {}));
    /// assert_eq!((reused.read(), reused.revision()), (11, 1));
    /// ```
    pub fn signal<T: 'static>(
        &mut self,
        element_id: ElementId,
        slot_index: usize,
        initial: T,
        invalidate: Rc<dyn Fn()>,
    ) -> Signal<T> {
        self.signal_scoped(
            ElementTreeId::new(0),
            element_id,
            slot_index,
            initial,
            invalidate,
        )
    }

    /// Returns a signal isolated to one retained element tree.
    ///
    /// Equal element IDs and slot indices in different `element_tree_id`
    /// namespaces do not alias. As with [`Self::signal`], `initial` is eagerly
    /// constructed before the method sees whether the slot exists.
    ///
    /// # Panics
    ///
    /// Panics when the existing scoped key contains a different concrete type.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::rc::Rc;
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::{app::StateStore, popup::ElementTreeId};
    /// let mut store = StateStore::default();
    /// let a = store.signal_scoped(ElementTreeId::new(1), ElementId(1), 0, 10, Rc::new(|| {}));
    /// let b = store.signal_scoped(ElementTreeId::new(2), ElementId(1), 0, 20, Rc::new(|| {}));
    /// a.set(11);
    /// assert_eq!((a.read(), b.read()), (11, 20));
    /// ```
    pub fn signal_scoped<T: 'static>(
        &mut self,
        element_tree_id: ElementTreeId,
        element_id: ElementId,
        slot_index: usize,
        initial: T,
        invalidate: Rc<dyn Fn()>,
    ) -> Signal<T> {
        self.signal_scoped_with(
            element_tree_id,
            element_id,
            slot_index,
            || initial,
            invalidate,
        )
    }

    /// Lazily initializes a retained signal slot. The factory is never
    /// evaluated again while the same typed slot remains mounted.
    ///
    /// Every returned handle shares the slot value and revision but owns the
    /// invalidation callback passed in that call. The store retains its `Rc`s
    /// until removal; outstanding handles keep old state alive after removal,
    /// while a later lookup creates a fresh independent slot.
    ///
    /// # Panics
    ///
    /// Panics when an existing scoped key contains a different concrete `T`.
    /// A panic from `initial` propagates and leaves the entry vacant.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::Cell, rc::Rc};
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::{app::StateStore, popup::ElementTreeId};
    /// let mut store = StateStore::default();
    /// let calls = Rc::new(Cell::new(0));
    /// let counted = calls.clone();
    /// let first = store.signal_scoped_with(ElementTreeId::new(1), ElementId(1), 0, move || { counted.set(counted.get() + 1); 7 }, Rc::new(|| {}));
    /// let second = store.signal_scoped_with(ElementTreeId::new(1), ElementId(1), 0, || 99, Rc::new(|| {}));
    /// assert_eq!((first.read(), second.read(), calls.get()), (7, 7, 1));
    /// ```
    pub fn signal_scoped_with<T: 'static>(
        &mut self,
        element_tree_id: ElementTreeId,
        element_id: ElementId,
        slot_index: usize,
        initial: impl FnOnce() -> T,
        invalidate: Rc<dyn Fn()>,
    ) -> Signal<T> {
        let slot = ScopedStateSlot {
            element_tree_id,
            slot: StateSlot {
                element_id,
                slot_index,
            },
        };

        self.values.entry(slot).or_insert_with(|| {
            Box::new((Rc::new(RefCell::new(initial())), Rc::new(Cell::new(0_u64)))) as Box<dyn Any>
        });

        let (value, revision) = self
            .values
            .get(&slot)
            .expect("state slot must exist")
            .downcast_ref::<(Rc<RefCell<T>>, Rc<Cell<u64>>)>()
            .expect("state slot type mismatch");

        Signal::with_revision(value.clone(), invalidate, revision.clone())
    }

    /// Removes an element from the legacy root-tree namespace (`tree 0`).
    ///
    /// Every slot index and concrete type for the element is removed. Missing
    /// elements are a no-op. Existing `Signal` handles continue to own and mutate
    /// their detached values; the next lookup creates fresh state.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::rc::Rc;
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::StateStore;
    /// let mut store = StateStore::default();
    /// let old = store.signal(ElementId(1), 0, 4, Rc::new(|| {}));
    /// store.remove_element(ElementId(1));
    /// let fresh = store.signal(ElementId(1), 0, 9, Rc::new(|| {}));
    /// assert_eq!((old.read(), fresh.read()), (4, 9));
    /// ```
    pub fn remove_element(&mut self, element_id: ElementId) {
        self.remove_element_scoped(ElementTreeId::new(0), element_id);
    }

    /// Removes all state slots belonging to `element_id` in one tree only.
    ///
    /// This scans the full sparse store in O(total slots). Other trees—even with
    /// the same element ID—remain intact.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::rc::Rc;
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::{app::StateStore, popup::ElementTreeId};
    /// let mut store = StateStore::default();
    /// let other = store.signal_scoped(ElementTreeId::new(2), ElementId(1), 0, 20, Rc::new(|| {}));
    /// let _ = store.signal_scoped(ElementTreeId::new(1), ElementId(1), 0, 10, Rc::new(|| {}));
    /// store.remove_element_scoped(ElementTreeId::new(1), ElementId(1));
    /// assert_eq!(other.read(), 20);
    /// ```
    pub fn remove_element_scoped(&mut self, element_tree_id: ElementTreeId, element_id: ElementId) {
        self.values.retain(|slot, _| {
            slot.element_tree_id != element_tree_id || slot.slot.element_id != element_id
        });
    }

    /// Removes every retained state slot belonging to one element-tree
    /// namespace.
    ///
    /// This is intentionally tree-scoped: several retained runtimes can share
    /// the same store while reusing identical [`ElementId`] values.
    /// The operation scans all slots and leaves outstanding signal handles alive.
    ///
    /// # Examples
    ///
    /// Public callers can perform the narrower element-scoped removal:
    ///
    /// ```
    /// use std::rc::Rc;
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::{app::StateStore, popup::ElementTreeId};
    /// let mut store = StateStore::default();
    /// let old = store.signal_scoped(ElementTreeId::new(1), ElementId(1), 0, 3, Rc::new(|| {}));
    /// store.remove_element_scoped(ElementTreeId::new(1), ElementId(1));
    /// assert_eq!(old.read(), 3);
    /// ```
    pub(crate) fn remove_tree_scoped(&mut self, element_tree_id: ElementTreeId) {
        self.values
            .retain(|slot, _| slot.element_tree_id != element_tree_id);
    }
}
