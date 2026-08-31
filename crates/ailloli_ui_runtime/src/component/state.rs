//! Ergonomic state handle layered over [`Signal`].

use std::cell::RefCell;
use std::fmt::Display;
use std::rc::Rc;

use super::{Memo, Signal};

/// Public DX state handle built on top of the retained runtime `Signal`.
///
/// Clones alias one signal value, revision, historical callback, and dynamic
/// retained consumers. Like [`Signal`], this `Rc`-based type is UI-thread-local
/// and neither `Send` nor `Sync`. [`Self::new`] starts without an owner callback,
/// but every successful read during retained Build, Layout, or Paint registers
/// that exact mounted consumer. Reads outside those callbacks remain passive.
/// Successful callbacks replace their previous observations atomically, so
/// conditional reads track only the branch that committed. A failed callback
/// preserves its previous observations. Unmounting removes the consumer; the
/// state itself remains usable while another handle owns it.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::State;
/// let state = State::new(1);
/// let alias = state.clone();
/// alias.set(2);
/// assert_eq!(state.read(), 2);
/// ```
pub struct State<T> {
    /// Underlying aliased reactive cell.
    signal: Signal<T>,
}

/// Implements the `Clone` contract for `State<T>`.
impl<T> Clone for State<T> {
    /// Produces the clone required by the standard cloning contract.
    fn clone(&self) -> Self {
        Self {
            signal: self.signal.clone(),
        }
    }
}

/// Provides the operations defined for `State<T>`.
impl<T: 'static> State<T> {
    /// Creates standalone state without an initial owner callback.
    ///
    /// Mutations advance the hidden source revision and notify retained consumers
    /// established by later Build/Layout/Paint reads. Before such a read, after
    /// their unmount, or when accessed only outside retained scopes, the state
    /// schedules no frame. This differs from state created by a runtime context,
    /// which also retains its owner-provided historical invalidator for
    /// compatibility.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// let state = State::new(String::from("ready"));
    /// assert_eq!(state.read(), "ready");
    /// ```
    pub fn new(value: T) -> Self {
        Self::from_signal(Signal::new(Rc::new(RefCell::new(value)), Rc::new(|| {})))
    }

    /// Wraps an existing signal while preserving its shared invalidator/revision.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::{Signal, State};
    /// let signal = Signal::new(Rc::new(RefCell::new(1)), Rc::new(|| {}));
    /// let state = State::from_signal(signal.clone());
    /// state.set(2);
    /// assert_eq!((signal.read(), signal.revision()), (2, 1));
    /// ```
    pub fn from_signal(signal: Signal<T>) -> Self {
        Self { signal }
    }

    /// Replaces the value and notifies the underlying signal's consumers.
    ///
    /// Equality is not checked. Borrowing and callback panics follow
    /// [`Signal::set`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// let state = State::new(1);
    /// state.set(4);
    /// assert_eq!(state.read(), 4);
    /// ```
    pub fn set(&self, value: T) {
        self.signal.set(value);
    }

    /// Mutates the value in place and notifies the underlying signal's consumers.
    ///
    /// The closure is executed synchronously under a mutable `RefCell` borrow;
    /// panic behavior follows [`Signal::update`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// let state = State::new(vec![1]);
    /// state.update(|items| items.push(2));
    /// assert_eq!(state.read(), vec![1, 2]);
    /// ```
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        self.signal.update(f);
    }

    /// Consumes this handle and returns its underlying signal.
    ///
    /// Other cloned state handles continue to alias the returned signal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// let state = State::new(3);
    /// let alias = state.clone();
    /// let signal = state.into_signal();
    /// signal.set(4);
    /// assert_eq!(alias.read(), 4);
    /// ```
    pub fn into_signal(self) -> Signal<T> {
        self.signal
    }
}

/// Provides the operations defined for `State<T>`.
impl<T: Clone + 'static> State<T> {
    /// Clones, observes, and returns the current value.
    ///
    /// # Panics
    ///
    /// Panics if cloning `T` panics or the underlying signal value is already
    /// mutably borrowed through another alias.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// let state = State::new(String::from("copy"));
    /// let value: String = state.read();
    /// assert_eq!(value, "copy");
    /// ```
    pub fn read(&self) -> T {
        self.signal.read()
    }

    /// Creates a non-caching lazy memo with this state's source revision.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// let state = State::new(2);
    /// let squared = state.map(|value| value * value);
    /// assert_eq!(squared.read(), 4);
    /// state.set(3);
    /// assert_eq!(squared.read(), 9);
    /// ```
    pub fn map<U: 'static>(&self, f: impl Fn(T) -> U + 'static) -> Memo<U> {
        self.signal.map(f)
    }

    /// Creates a lazy string memo using a custom formatter.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// let state = State::new(12);
    /// assert_eq!(state.to_text_with(|value| format!("{value}%")).read(), "12%");
    /// ```
    pub fn to_text_with<F>(&self, format: F) -> Memo<String>
    where
        F: Fn(T) -> String + 'static,
    {
        self.signal.to_text_with(format)
    }
}

impl<T> State<T>
where
    T: Clone + Display + 'static,
{
    /// Creates a lazy string memo using [`ToString::to_string`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// assert_eq!(State::new(12).to_text().read(), "12");
    /// ```
    pub fn to_text(&self) -> Memo<String> {
        self.signal.to_text()
    }
}

/// Implements the `From<State<T>>` contract for `Signal<T>`.
impl<T> From<State<T>> for Signal<T> {
    /// Performs the documented infallible conversion.
    fn from(value: State<T>) -> Self {
        value.signal
    }
}
