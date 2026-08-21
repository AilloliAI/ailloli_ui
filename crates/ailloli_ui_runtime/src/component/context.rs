use crate::app::runtime_handle::RuntimeHandle;
use crate::app::{Invalidation, UiServiceRegistration};
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
        self.signal_with_invalidation(initial, Invalidation::Build)
    }

    pub fn signal_with<T: 'static>(&mut self, initial: impl FnOnce() -> T) -> Signal<T> {
        self.signal_with_invalidation_factory(initial, Invalidation::Build)
    }

    /// Creates a signal whose writes request the declared retained work.
    ///
    /// `Build` matches the historical [`Self::signal`] behavior. Widget-local
    /// state that does not alter the declarative subtree can opt into `Layout`
    /// or `Paint` without rebuilding sibling components.
    pub fn signal_with_invalidation<T: 'static>(
        &mut self,
        initial: T,
        invalidation: Invalidation,
    ) -> Signal<T> {
        self.signal_with_invalidation_factory(|| initial, invalidation)
    }

    pub fn signal_with_invalidation_factory<T: 'static>(
        &mut self,
        initial: impl FnOnce() -> T,
        invalidation: Invalidation,
    ) -> Signal<T> {
        let slot_index = self.next_signal_slot;
        self.next_signal_slot += 1;

        let runtime = self.runtime.clone();
        let element_id = self.element_id;

        let invalidate = Rc::new(move || {
            runtime.invalidate(element_id, invalidation);
        });

        let element_tree_id = self.runtime.element_tree_id();
        self.runtime
            .inner
            .borrow_mut()
            .states
            .borrow_mut()
            .signal_scoped_with(
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

    pub fn invalidate(&self, invalidation: Invalidation) {
        self.runtime.invalidate(self.element_id, invalidation);
    }

    pub fn request_repaint(&self) {
        self.invalidate(Invalidation::Paint);
    }

    pub fn request_layout(&self) {
        self.invalidate(Invalidation::Layout);
    }

    pub fn request_build(&self) {
        self.invalidate(Invalidation::Build);
    }

    /// Weak callback suitable for retained model subscriptions. The callback
    /// never keeps the runtime or its widget tree alive.
    pub fn invalidation_target(&self, invalidation: Invalidation) -> Rc<dyn Fn()> {
        self.runtime.weak_invalidator(self.element_id, invalidation)
    }

    pub fn request_repaint_after(&self, delay: Duration) {
        self.runtime.request_repaint_after(self.element_id, delay);
    }

    pub fn request_layout_after(&self, delay: Duration) {
        self.runtime.request_layout_after(self.element_id, delay);
    }

    pub fn request_build_after(&self, delay: Duration) {
        self.runtime.request_build_after(self.element_id, delay);
    }

    pub fn runtime(&self) -> RuntimeHandle<A> {
        self.runtime.clone()
    }

    pub fn register_ui_service(&self, service: &Rc<dyn Fn() -> bool>) -> UiServiceRegistration<A> {
        self.runtime.register_ui_service(service)
    }
}
