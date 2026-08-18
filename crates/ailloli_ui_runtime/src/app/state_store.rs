use crate::component::signal::Signal;
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

#[derive(Default)]
pub struct StateStore {
    values: HashMap<StateSlot, Box<dyn Any>>,
}

impl StateStore {
    pub fn signal<T: 'static>(
        &mut self,
        element_id: ElementId,
        slot_index: usize,
        initial: T,
        invalidate: Rc<dyn Fn()>,
    ) -> Signal<T> {
        let slot = StateSlot {
            element_id,
            slot_index,
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

    pub fn remove_element(&mut self, element_id: ElementId) {
        self.values.retain(|slot, _| slot.element_id != element_id);
    }
}
