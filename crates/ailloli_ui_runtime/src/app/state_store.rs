use crate::component::signal::Signal;
use crate::popup::ElementTreeId;
use ailloli_ui_core::ids::ElementId;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StateSlot {
    pub element_id: ElementId,
    pub slot_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ScopedStateSlot {
    element_tree_id: ElementTreeId,
    slot: StateSlot,
}

#[derive(Default)]
pub struct StateStore {
    values: HashMap<ScopedStateSlot, Box<dyn Any>>,
}

impl StateStore {
    /// Returns a signal stored in the legacy root-tree namespace (`tree 0`).
    ///
    /// Retained runtimes use [`Self::signal_scoped`] so equal `ElementId`
    /// values from independent element trees cannot alias one another.
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
    pub fn signal_scoped<T: 'static>(
        &mut self,
        element_tree_id: ElementTreeId,
        element_id: ElementId,
        slot_index: usize,
        initial: T,
        invalidate: Rc<dyn Fn()>,
    ) -> Signal<T> {
        let slot = ScopedStateSlot {
            element_tree_id,
            slot: StateSlot {
                element_id,
                slot_index,
            },
        };

        self.values
            .entry(slot)
            .or_insert_with(|| Box::new(Rc::new(RefCell::new(initial))) as Box<dyn Any>);

        let value = self
            .values
            .get(&slot)
            .expect("state slot must exist")
            .downcast_ref::<Rc<RefCell<T>>>()
            .expect("state slot type mismatch")
            .clone();

        Signal::new(value, invalidate)
    }

    /// Removes an element from the legacy root-tree namespace (`tree 0`).
    pub fn remove_element(&mut self, element_id: ElementId) {
        self.remove_element_scoped(ElementTreeId::new(0), element_id);
    }

    /// Removes all state slots belonging to `element_id` in one tree only.
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
    pub(crate) fn remove_tree_scoped(&mut self, element_tree_id: ElementTreeId) {
        self.values
            .retain(|slot, _| slot.element_tree_id != element_tree_id);
    }
}
