use crate::app::runtime_handle::RuntimeHandle;
use crate::component::signal::Signal;
use crate::component::state::State;
use ailloli_ui_core::ids::ElementId;
use std::rc::Rc;
use std::time::Duration;

pub struct Context<A> {
    element_id: ElementId,
    runtime: RuntimeHandle<A>,
    next_signal_slot: usize,
}

impl<A: 'static> Context<A> {
    pub fn new(element_id: ElementId, runtime: RuntimeHandle<A>) -> Self {
        Self {
            element_id,
            runtime,
            next_signal_slot: 0,
        }
    }

    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    pub fn signal<T: 'static>(&mut self, initial: T) -> Signal<T> {
        let slot_index = self.next_signal_slot;
        self.next_signal_slot += 1;

        let runtime = self.runtime.clone();
        let element_id = self.element_id;

        let invalidate = Rc::new(move || {
            runtime.mark_dirty(element_id);
        });

        let element_tree_id = self.runtime.element_tree_id();
        self.runtime
            .inner
            .borrow_mut()
            .states
            .borrow_mut()
            .signal_scoped(
                element_tree_id,
                self.element_id,
                slot_index,
                initial,
                invalidate,
            )
    }

    pub fn state<T: 'static>(&mut self, initial: T) -> State<T> {
        State::from_signal(self.signal(initial))
    }

    pub fn dispatch(&self, action: A) {
        self.runtime.dispatch(action);
    }

    pub fn request_repaint_after(&self, delay: Duration) {
        self.runtime.request_repaint_after(self.element_id, delay);
    }

    pub fn runtime(&self) -> RuntimeHandle<A> {
        self.runtime.clone()
    }
}
