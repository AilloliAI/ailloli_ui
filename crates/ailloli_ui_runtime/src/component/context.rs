//! Component context for state, services, invalidation, and runtime access.

use crate::app::runtime_handle::RuntimeHandle;
use crate::app::{Invalidation, InvalidationSource, UiServiceRegistration};
use crate::component::signal::Signal;
use crate::component::state::State;
use ailloli_ui_core::ids::ElementId;
use std::rc::Rc;
use std::time::Duration;

/// Capabilities and hook-slot cursor for one synchronous component build.
///
/// A fresh context starts at signal slot zero. Components must call state/signal
/// constructors in stable order and with a stable type per slot across builds;
/// changing that order/type violates the type-erased [`crate::app::StateStore`]
/// invariant and can panic. The context and its runtime handle are UI-thread
/// local (`Rc`/`RefCell`), not `Send` or `Sync`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId;
/// use ailloli_ui_runtime::app::RuntimeHandle;
/// use ailloli_ui_runtime::component::Context;
/// let ctx = Context::<()>::new(ElementId(1), RuntimeHandle::new());
/// assert_eq!(ctx.element_id(), ElementId(1));
/// ```
pub struct Context<A> {
    /// Retained element currently being built.
    element_id: ElementId,
    /// UI-local runtime used for hooks, actions, and invalidation.
    runtime: RuntimeHandle<A>,
    /// Zero-based hook slot assigned to the next state lookup in this build.
    next_signal_slot: usize,
}

/// Provides the operations defined for `Context<A>`.
impl<A: 'static> Context<A> {
    /// Creates a build context for `element_id` at hook slot zero.
    ///
    /// The ID is stored verbatim and need not currently exist in a tree. The
    /// runtime clone determines the element-tree namespace used for state and
    /// invalidation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::component::Context;
    /// let ctx = Context::<()>::new(ElementId(5), RuntimeHandle::new());
    /// assert_eq!(ctx.element_id(), ElementId(5));
    /// ```
    pub fn new(element_id: ElementId, runtime: RuntimeHandle<A>) -> Self {
        Self {
            element_id,
            runtime,
            next_signal_slot: 0,
        }
    }

    /// Returns the retained component element targeted by this context.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::component::Context;
    /// assert_eq!(Context::<()>::new(ElementId(9), RuntimeHandle::new()).element_id(), ElementId(9));
    /// ```
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Returns the signal in the next hook slot, creating it from `initial` once.
    ///
    /// Writes request build invalidation. On later builds, an existing slot
    /// keeps its current value and ignores `initial`. Calls advance the slot
    /// cursor, so order and type must remain stable.
    ///
    /// # Panics
    ///
    /// Panics if an existing slot contains a different Rust type or if internal
    /// `RefCell` borrows conflict. Slot-index overflow follows build overflow
    /// settings and is practically unreachable.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::component::Context;
    /// let runtime = RuntimeHandle::<()>::new();
    /// let mut ctx = Context::new(ElementId(1), runtime.clone());
    /// let value = ctx.signal(3_u8);
    /// value.set(4);
    /// assert_eq!(value.read(), 4);
    /// assert!(runtime.frame_work_plan().needs_build());
    /// ```
    pub fn signal<T: 'static>(&mut self, initial: T) -> Signal<T> {
        self.signal_with_invalidation(initial, Invalidation::Build)
    }

    /// Lazily initializes the signal in the next build hook slot.
    ///
    /// The factory runs only when that tree/element/slot has no stored signal;
    /// existing state skips it. Writes request build invalidation.
    ///
    /// # Panics
    ///
    /// Has the same stable-order/type and borrow requirements as [`Self::signal`].
    /// Factory panics propagate without installing a value.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::cell::Cell;
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::component::Context;
    /// let calls = Cell::new(0);
    /// let mut ctx = Context::<()>::new(ElementId(1), RuntimeHandle::new());
    /// let signal = ctx.signal_with(|| { calls.set(calls.get() + 1); 7 });
    /// assert_eq!(signal.read(), 7);
    /// assert_eq!(calls.get(), 1);
    /// ```
    pub fn signal_with<T: 'static>(&mut self, initial: impl FnOnce() -> T) -> Signal<T> {
        self.signal_with_invalidation_factory(initial, Invalidation::Build)
    }

    /// Creates a signal whose writes request the declared retained work.
    ///
    /// `Build` matches the historical [`Self::signal`] behavior. Widget-local
    /// state that does not alter the declarative subtree can opt into `Layout`
    /// or `Paint` without rebuilding sibling components.
    ///
    /// Existing state ignores `initial`, but the returned signal uses the
    /// invalidator installed when the slot was originally created. Reusing a
    /// slot with a different invalidation level does not replace that callback.
    ///
    /// # Panics
    ///
    /// Has the same stable-order/type and borrow requirements as [`Self::signal`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, RuntimeHandle};
    /// use ailloli_ui_runtime::component::Context;
    /// let runtime = RuntimeHandle::<()>::new();
    /// let mut ctx = Context::new(ElementId(2), runtime.clone());
    /// ctx.signal_with_invalidation(false, Invalidation::Paint).set(true);
    /// let plan = runtime.frame_work_plan();
    /// assert!(plan.needs_paint() && !plan.needs_layout());
    /// ```
    pub fn signal_with_invalidation<T: 'static>(
        &mut self,
        initial: T,
        invalidation: Invalidation,
    ) -> Signal<T> {
        self.signal_with_invalidation_factory(|| initial, invalidation)
    }

    /// Lazily initializes a signal with a chosen write-invalidation level.
    ///
    /// The next slot index is reserved before the factory runs. Existing slots
    /// do not invoke `initial` and retain their originally installed invalidator.
    ///
    /// # Panics
    ///
    /// Panics on a stored type mismatch, conflicting interior borrow, or
    /// propagated factory panic. Hook-order changes can therefore fail during
    /// component reconciliation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, RuntimeHandle};
    /// use ailloli_ui_runtime::component::Context;
    /// let runtime = RuntimeHandle::<()>::new();
    /// let mut ctx = Context::new(ElementId(2), runtime.clone());
    /// let signal = ctx.signal_with_invalidation_factory(|| String::from("ready"), Invalidation::Layout);
    /// signal.set(String::from("changed"));
    /// assert!(runtime.frame_work_plan().needs_layout());
    /// ```
    pub fn signal_with_invalidation_factory<T: 'static>(
        &mut self,
        initial: impl FnOnce() -> T,
        invalidation: Invalidation,
    ) -> Signal<T> {
        let slot_index = self.next_signal_slot;
        self.next_signal_slot += 1;

        let element_id = self.element_id;
        let invalidate = self
            .runtime
            .weak_signal_invalidator(element_id, invalidation);

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

    /// Returns [`State`] backed by the next build-invalidating signal slot.
    ///
    /// # Panics
    ///
    /// Has the same stable-order/type and borrow requirements as [`Self::signal`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::component::Context;
    /// let mut ctx = Context::<()>::new(ElementId(1), RuntimeHandle::new());
    /// let state = ctx.state(10);
    /// state.update(|value| *value += 2);
    /// assert_eq!(state.read(), 12);
    /// ```
    pub fn state<T: 'static>(&mut self, initial: T) -> State<T> {
        State::from_signal(self.signal(initial))
    }

    /// Queues one application action in FIFO order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::component::Context;
    /// let runtime = RuntimeHandle::<u8>::new();
    /// Context::new(ElementId(1), runtime.clone()).dispatch(6);
    /// assert_eq!(runtime.take_actions(), vec![6]);
    /// ```
    pub fn dispatch(&self, action: A) {
        self.runtime.dispatch(action);
    }

    /// Marks this component element with explicit context-origin invalidation.
    ///
    /// Repeated requests coalesce to the strongest work level and diagnostics
    /// preserve the request provenance.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, RuntimeHandle};
    /// use ailloli_ui_runtime::component::Context;
    /// let runtime = RuntimeHandle::<()>::new();
    /// Context::new(ElementId(4), runtime.clone()).invalidate(Invalidation::Layout);
    /// assert!(runtime.frame_work_plan().needs_layout());
    /// ```
    pub fn invalidate(&self, invalidation: Invalidation) {
        self.runtime
            .invalidate_from(self.element_id, invalidation, InvalidationSource::Context);
    }

    /// Requests paint work for this component element.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::component::Context;
    /// let runtime = RuntimeHandle::<()>::new();
    /// Context::new(ElementId(1), runtime.clone()).request_repaint();
    /// assert!(runtime.frame_work_plan().needs_paint());
    /// ```
    pub fn request_repaint(&self) {
        self.invalidate(Invalidation::Paint);
    }

    /// Requests layout and paint work for this component element.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::component::Context;
    /// let runtime = RuntimeHandle::<()>::new();
    /// Context::new(ElementId(1), runtime.clone()).request_layout();
    /// assert!(runtime.frame_work_plan().needs_layout());
    /// ```
    pub fn request_layout(&self) {
        self.invalidate(Invalidation::Layout);
    }

    /// Requests build, layout, and paint work for this component element.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::component::Context;
    /// let runtime = RuntimeHandle::<()>::new();
    /// Context::new(ElementId(1), runtime.clone()).request_build();
    /// assert!(runtime.frame_work_plan().needs_build());
    /// ```
    pub fn request_build(&self) {
        self.invalidate(Invalidation::Build);
    }

    /// Weak callback suitable for retained model subscriptions. The callback
    /// never keeps the runtime or its widget tree alive.
    ///
    /// While the runtime exists, invocation records `InvalidationSource::Model`
    /// through the handle's weak invalidator. After all strong handles are
    /// dropped, invocation is a harmless no-op.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, RuntimeHandle};
    /// use ailloli_ui_runtime::component::Context;
    /// let runtime = RuntimeHandle::<()>::new();
    /// let callback = Context::new(ElementId(3), runtime.clone()).invalidation_target(Invalidation::Paint);
    /// callback();
    /// assert_eq!(runtime.take_dirty_elements(), vec![ElementId(3)]);
    /// ```
    pub fn invalidation_target(&self, invalidation: Invalidation) -> Rc<dyn Fn()> {
        self.runtime.weak_invalidator(self.element_id, invalidation)
    }

    /// Schedules paint invalidation no earlier than `delay` from now.
    ///
    /// Timers are tree-scoped and the runtime deduplicates compatible requests
    /// by earliest due instant.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::component::Context;
    /// let runtime = RuntimeHandle::<()>::new();
    /// Context::new(ElementId(1), runtime.clone()).request_repaint_after(Duration::from_millis(1));
    /// assert!(runtime.next_scheduled_repaint_due().is_some());
    /// ```
    pub fn request_repaint_after(&self, delay: Duration) {
        self.runtime.request_repaint_after(self.element_id, delay);
    }

    /// Schedules layout invalidation no earlier than `delay` from now.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::component::Context;
    /// let runtime = RuntimeHandle::<()>::new();
    /// Context::new(ElementId(1), runtime.clone()).request_layout_after(Duration::from_millis(1));
    /// assert!(runtime.next_scheduled_repaint_due().is_some());
    /// ```
    pub fn request_layout_after(&self, delay: Duration) {
        self.runtime.request_layout_after(self.element_id, delay);
    }

    /// Schedules build invalidation no earlier than `delay` from now.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::component::Context;
    /// let runtime = RuntimeHandle::<()>::new();
    /// Context::new(ElementId(1), runtime.clone()).request_build_after(Duration::from_millis(1));
    /// assert!(runtime.next_scheduled_repaint_due().is_some());
    /// ```
    pub fn request_build_after(&self, delay: Duration) {
        self.runtime.request_build_after(self.element_id, delay);
    }

    /// Clones the runtime handle while preserving this tree namespace.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::component::Context;
    /// let ctx = Context::<()>::new(ElementId(1), RuntimeHandle::new());
    /// assert_eq!(ctx.runtime().element_tree_id(), ctx.runtime().element_tree_id());
    /// ```
    pub fn runtime(&self) -> RuntimeHandle<A> {
        self.runtime.clone()
    }

    /// Registers a polled UI service and returns its lifetime guard.
    ///
    /// The callback is invoked by [`RuntimeHandle::service_ui_sources`] and
    /// returns whether it changed component-visible state. A `true` result
    /// requests `Build` for this exact component owner and mount generation;
    /// `false` queues no retained work. Dropping the registration removes only
    /// that tree-scoped service. Callback panics propagate from servicing.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::rc::Rc;
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::component::Context;
    /// let runtime = RuntimeHandle::<()>::new();
    /// let service: Rc<dyn Fn() -> bool> = Rc::new(|| true);
    /// let registration = Context::new(ElementId(1), runtime.clone()).register_ui_service(&service);
    /// assert!(runtime.service_ui_sources());
    /// drop(registration);
    /// assert!(!runtime.service_ui_sources());
    /// ```
    pub fn register_ui_service(&self, service: &Rc<dyn Fn() -> bool>) -> UiServiceRegistration<A> {
        self.runtime
            .register_owned_ui_service(self.element_id, service)
    }
}
