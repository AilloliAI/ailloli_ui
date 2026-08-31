//! Uniform wrapper for static and reactive widget properties.

use super::{Memo, Signal, State};

/// Bindable widget prop: static value, [`Signal`], or [`Memo`].
///
/// Unifies `disabled(true)`, `disabled(signal)`, and `disabled(memo)` without separate DSL methods.
/// Cloning a static binding clones `T`; cloning reactive variants aliases their
/// existing `Rc`-backed source. Reads always return an owned value and memos are
/// evaluated on every read. Signal-backed and transitively reactive memo reads
/// register the innermost retained Build/Layout/Paint consumer; static values do
/// not create a dependency. Conditional memo reads observe only sources reached
/// by the executed branch. The surrounding callback publishes that set only on
/// success and replaces its previous set atomically. Because the reactive
/// variants are `Rc`-backed, a binding is UI-thread-local and neither `Send` nor
/// `Sync`. Subscriptions belong to the mounted consumer rather than the binding:
/// unmounting removes them while independently owned bindings remain usable.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::Binding;
/// let binding: Binding<String> = "label".into();
/// assert_eq!(binding.read(), "label");
/// assert!(!binding.is_signal());
/// ```
pub enum Binding<T> {
    /// Owned non-reactive value with constant revision zero.
    Static(T),
    /// Aliased mutable signal.
    Signal(Signal<T>),
    /// Lazy derived value, possibly carrying a source revision.
    Memo(Memo<T>),
}

/// Implements the `Clone` contract for `Binding<T>`.
impl<T: Clone> Clone for Binding<T> {
    /// Produces the clone required by the standard cloning contract.
    fn clone(&self) -> Self {
        match self {
            Self::Static(value) => Self::Static(value.clone()),
            Self::Signal(signal) => Self::Signal(signal.clone()),
            Self::Memo(memo) => Self::Memo(memo.clone()),
        }
    }
}

/// Implements the `From<T>` contract for `Binding<T>`.
impl<T> From<T> for Binding<T> {
    /// Performs the documented infallible conversion.
    fn from(value: T) -> Self {
        Self::Static(value)
    }
}

/// Implements the `From<Signal<T>>` contract for `Binding<T>`.
impl<T> From<Signal<T>> for Binding<T> {
    /// Performs the documented infallible conversion.
    fn from(value: Signal<T>) -> Self {
        Self::Signal(value)
    }
}

/// Implements the `From<State<T>>` contract for `Binding<T>`.
impl<T> From<State<T>> for Binding<T> {
    /// Performs the documented infallible conversion.
    fn from(value: State<T>) -> Self {
        Self::Signal(value.into())
    }
}

/// Implements the `From<Memo<T>>` contract for `Binding<T>`.
impl<T> From<Memo<T>> for Binding<T> {
    /// Performs the documented infallible conversion.
    fn from(value: Memo<T>) -> Self {
        Self::Memo(value)
    }
}

/// Implements the `From<&str>` contract for `Binding<String>`.
impl From<&str> for Binding<String> {
    /// Performs the documented infallible conversion.
    fn from(value: &str) -> Self {
        Self::Static(value.to_string())
    }
}

/// Implements the `From<&String>` contract for `Binding<String>`.
impl From<&String> for Binding<String> {
    /// Performs the documented infallible conversion.
    fn from(value: &String) -> Self {
        Self::Static(value.clone())
    }
}

/// Provides the operations defined for `Binding<T>`.
impl<T: Clone> Binding<T> {
    /// Returns an owned current value.
    ///
    /// Static and signal variants clone `T`; a memo evaluates its stored
    /// computation. Reactive variants are observed by the innermost retained
    /// build, layout, or paint callback; reads elsewhere are passive. A failed
    /// surrounding callback does not publish its staged dependency set.
    ///
    /// # Panics
    ///
    /// Panics if cloning `T` panics, a signal value is already mutably borrowed
    /// through another alias, or the stored memo computation panics.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::{Binding, Memo};
    /// let static_value = Binding::Static(3);
    /// let derived = Binding::Memo(Memo::new(|| 4));
    /// assert_eq!((static_value.read(), derived.read()), (3, 4));
    /// ```
    pub fn read(&self) -> T {
        match self {
            Binding::Static(v) => v.clone(),
            Binding::Signal(s) => s.read(),
            Binding::Memo(m) => m.read(),
        }
    }
}

/// Provides the operations defined for `Binding<T>`.
impl<T> Binding<T> {
    /// Returns `true` only for the direct [`Binding::Signal`] variant.
    ///
    /// A memo derived from a signal still returns `false`; inspect
    /// [`Self::revision`] when the distinction of interest is reactivity.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::{Binding, Signal};
    /// let signal = Signal::new(Rc::new(RefCell::new(1)), Rc::new(|| {}));
    /// let binding: Binding<i32> = signal.into();
    /// assert!(binding.is_signal());
    /// ```
    pub fn is_signal(&self) -> bool {
        matches!(self, Binding::Signal(_))
    }

    /// Observed revision of the reactive source used by retained caches.
    ///
    /// Static bindings and opaque memos without a reactive source remain at
    /// revision zero. Memos derived from a [`Signal`] preserve its revision.
    /// Revisions reserve zero for pristine/opaque sources and wrap from
    /// `u64::MAX` to one. Signal and derived-memo revision reads observe their
    /// source unless the runtime explicitly installs an untracked admin scope.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::{Binding, Signal};
    /// let signal = Signal::new(Rc::new(RefCell::new(1)), Rc::new(|| {}));
    /// let memo_binding: Binding<i32> = signal.map(|value| value + 1).into();
    /// assert_eq!(memo_binding.revision(), 0);
    /// signal.set(2);
    /// assert_eq!(memo_binding.revision(), 1);
    /// ```
    pub fn revision(&self) -> u64 {
        match self {
            Self::Static(_) => 0,
            Self::Signal(signal) => signal.revision(),
            Self::Memo(memo) => memo.revision(),
        }
    }
}
