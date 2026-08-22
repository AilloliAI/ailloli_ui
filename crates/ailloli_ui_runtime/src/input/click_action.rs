//! Type-erased click callbacks dispatched through an event context.

use std::rc::Rc;

use super::EventCtx;

/// Shared single-threaded click callback.
type ClickHandler<A> = Rc<dyn Fn(&mut EventCtx<A>)>;

/// Lazily constructs a dispatched action every time it is resolved.
///
/// Clones share the same `Rc` factory. Neither the factory nor this wrapper is
/// `Send` or `Sync`; it belongs to the UI thread. Factory panics propagate.
///
/// # Examples
///
/// ```
/// use std::cell::Cell;
/// use std::rc::Rc;
/// use ailloli_ui_runtime::input::DeferredAction;
/// let calls = Rc::new(Cell::new(0));
/// let seen = calls.clone();
/// let action = DeferredAction::new(move || { seen.set(seen.get() + 1); seen.get() });
/// assert_eq!((action.resolve(), action.resolve()), (1, 2));
/// ```
pub struct DeferredAction<A> {
    factory: Rc<dyn Fn() -> A>,
}

/// Implements the `Clone` contract for `DeferredAction<A>`.
impl<A> Clone for DeferredAction<A> {
    /// Produces the clone required by the standard cloning contract.
    fn clone(&self) -> Self {
        Self {
            factory: self.factory.clone(),
        }
    }
}

/// Provides the operations defined for `DeferredAction<A>`.
impl<A> DeferredAction<A> {
    /// Wraps a UI-thread factory without invoking it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::DeferredAction;
    /// let action = DeferredAction::new(|| String::from("save"));
    /// assert_eq!(action.resolve(), "save");
    /// ```
    pub fn new(factory: impl Fn() -> A + 'static) -> Self {
        Self {
            factory: Rc::new(factory),
        }
    }

    /// Invokes the factory and returns its newly produced action.
    ///
    /// No result is cached; repeated calls can return different values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::DeferredAction;
    /// let action = DeferredAction::new(|| 42);
    /// assert_eq!(action.resolve(), 42);
    /// ```
    pub fn resolve(&self) -> A {
        (self.factory)()
    }
}

/// Executable UI-thread callback for one click-like activation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId;
/// use ailloli_ui_runtime::app::RuntimeHandle;
/// use ailloli_ui_runtime::input::{ClickAction, EventCtx};
/// let runtime = RuntimeHandle::<u8>::new();
/// let mut ctx = EventCtx::new(runtime.clone(), ElementId(1));
/// ClickAction::handler(|ctx| ctx.dispatch(7)).run(&mut ctx);
/// assert_eq!(runtime.take_actions(), vec![7]);
/// ```
pub struct ClickAction<A> {
    handler: ClickHandler<A>,
}

/// Provides the operations defined for `ClickAction<A>`.
impl<A> ClickAction<A> {
    /// Wraps a UI-thread event-context callback.
    ///
    /// The callback is retained by `Rc`; captured resources live until every
    /// action clone/internal reference is dropped. Callback panics propagate.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::{ClickAction, EventCtx};
    /// let runtime = RuntimeHandle::<&'static str>::new();
    /// let mut ctx = EventCtx::new(runtime.clone(), ElementId(1));
    /// let action = ClickAction::handler(|ctx| ctx.dispatch("open"));
    /// action.run(&mut ctx);
    /// assert_eq!(runtime.take_actions(), vec!["open"]);
    /// ```
    pub fn handler(handler: impl Fn(&mut EventCtx<A>) + 'static) -> Self {
        Self {
            handler: Rc::new(handler),
        }
    }

    /// Runs the stored callback synchronously with `ctx`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::{ClickAction, EventCtx};
    /// let runtime = RuntimeHandle::<i32>::new();
    /// let mut ctx = EventCtx::new(runtime.clone(), ElementId(9));
    /// ClickAction::handler(|ctx| ctx.dispatch(3)).run(&mut ctx);
    /// assert_eq!(runtime.take_actions(), vec![3]);
    /// ```
    pub fn run(&self, ctx: &mut EventCtx<A>) {
        (self.handler)(ctx);
    }
}

/// Converts static, deferred, or callback values into a [`ClickAction`].
///
/// A plain action requires `Clone` and dispatches a fresh clone on every run.
/// [`DeferredAction`] instead invokes its factory each time.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId;
/// use ailloli_ui_runtime::app::RuntimeHandle;
/// use ailloli_ui_runtime::input::{EventCtx, IntoClickAction};
/// let runtime = RuntimeHandle::<String>::new();
/// let mut ctx = EventCtx::new(runtime.clone(), ElementId(1));
/// String::from("save").into_click_action().run(&mut ctx);
/// assert_eq!(runtime.take_actions(), vec![String::from("save")]);
/// ```
pub trait IntoClickAction<A> {
    /// Consumes `self` and returns its executable click callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::{ClickAction, IntoClickAction};
    /// let action: ClickAction<()> = ClickAction::handler(|_| {}).into_click_action();
    /// # let _ = action;
    /// ```
    fn into_click_action(self) -> ClickAction<A>;
}

/// Implements the `IntoClickAction<A>` contract for `ClickAction<A>`.
impl<A> IntoClickAction<A> for ClickAction<A> {
    /// Implements the into_click_action helper used by this module.
    fn into_click_action(self) -> ClickAction<A> {
        self
    }
}

/// Implements the `IntoClickAction<A>` contract for `A`.
impl<A: Clone + 'static> IntoClickAction<A> for A {
    /// Implements the into_click_action helper used by this module.
    fn into_click_action(self) -> ClickAction<A> {
        ClickAction::handler(move |ctx| ctx.dispatch(self.clone()))
    }
}

/// Implements the `IntoClickAction<A>` contract for `DeferredAction<A>`.
impl<A: 'static> IntoClickAction<A> for DeferredAction<A> {
    /// Implements the into_click_action helper used by this module.
    fn into_click_action(self) -> ClickAction<A> {
        ClickAction::handler(move |ctx| ctx.dispatch(self.resolve()))
    }
}
