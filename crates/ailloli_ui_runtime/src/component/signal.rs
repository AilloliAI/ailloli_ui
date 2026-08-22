//! Single-threaded reactive values and lazy derived computations.

use std::cell::{Cell, RefCell};
use std::fmt::Display;
use std::rc::Rc;

/// Mutable reactive cell; updates call `invalidate` to schedule redraw.
///
/// Clones alias the same [`RefCell`], invalidation callback, and revision cell.
/// The `Rc`-based handle is neither `Send` nor `Sync`; use it on the owning UI
/// thread. Mutations and callbacks run synchronously. A callback panic occurs
/// after the value and revision have changed, while a mutation-closure panic can
/// leave a partially changed value without advancing revision or invalidating.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// let signal = Signal::new(Rc::new(RefCell::new(1)), Rc::new(|| {}));
/// let alias = signal.clone();
/// alias.set(2);
/// assert_eq!(signal.read(), 2);
/// assert_eq!(signal.revision(), 1);
/// ```
pub struct Signal<T> {
    /// Aliased mutable value.
    value: Rc<RefCell<T>>,
    /// Synchronous callback invoked after successful mutations.
    invalidate: Rc<dyn Fn()>,
    /// Shared nonzero-after-first-change wrapping revision.
    revision: Rc<Cell<u64>>,
}

/// Implements the `Clone` contract for `Signal<T>`.
impl<T> Clone for Signal<T> {
    /// Produces the clone required by the standard cloning contract.
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            invalidate: self.invalidate.clone(),
            revision: self.revision.clone(),
        }
    }
}

/// Provides the operations defined for `Signal<T>`.
impl<T> Signal<T> {
    /// Creates a signal around caller-owned storage and an invalidation callback.
    ///
    /// The initial revision is zero. Multiple calls using the same `value` do
    /// not share revisions unless the resulting [`Signal`] itself is cloned.
    /// The callback may capture UI scheduling state and is invoked synchronously
    /// after every successful mutation.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::{Cell, RefCell}, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// let calls = Rc::new(Cell::new(0));
    /// let observed = calls.clone();
    /// let signal = Signal::new(Rc::new(RefCell::new("a")), Rc::new(move || observed.set(observed.get() + 1)));
    /// signal.set("b");
    /// assert_eq!(calls.get(), 1);
    /// ```
    pub fn new(value: Rc<RefCell<T>>, invalidate: Rc<dyn Fn()>) -> Self {
        Self {
            value,
            invalidate,
            revision: Rc::new(Cell::new(0)),
        }
    }

    /// Creates a signal with an explicitly shared revision cell.
    ///
    /// This crate-internal constructor lets state slots recreate handles that
    /// observe one revision. Public callers obtain the same sharing behavior by
    /// cloning a signal.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// let signal = Signal::new(Rc::new(RefCell::new(0)), Rc::new(|| {}));
    /// let alias = signal.clone();
    /// signal.set(1);
    /// assert_eq!(alias.revision(), 1);
    /// ```
    pub(crate) fn with_revision(
        value: Rc<RefCell<T>>,
        invalidate: Rc<dyn Fn()>,
        revision: Rc<Cell<u64>>,
    ) -> Self {
        Self {
            value,
            invalidate,
            revision,
        }
    }

    /// Replaces the value, advances revision, then invokes invalidation.
    ///
    /// Equality is not checked, so setting an equal value still invalidates.
    /// Revisions advance from zero to one and wrap from `u64::MAX` back to one;
    /// compare them for inequality, not ordering.
    ///
    /// # Panics
    ///
    /// Panics on a conflicting `RefCell` borrow or when the invalidation callback
    /// panics. In the callback case, the new value and revision remain committed.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// let signal = Signal::new(Rc::new(RefCell::new(3)), Rc::new(|| {}));
    /// signal.set(3);
    /// assert_eq!(signal.read(), 3);
    /// assert_eq!(signal.revision(), 1);
    /// ```
    pub fn set(&self, value: T) {
        *self.value.borrow_mut() = value;
        self.bump_revision();
        (self.invalidate)();
    }

    /// Mutates the value in place, advances revision, then invalidates.
    ///
    /// The closure is called exactly once under a mutable `RefCell` borrow.
    /// This method does not detect whether the closure actually changed `T`.
    ///
    /// # Panics
    ///
    /// Panics on conflicting borrowing. If `f` panics, its partial mutation is
    /// retained but revision and invalidation are skipped. An invalidation panic
    /// propagates after mutation and revision commit.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// let signal = Signal::new(Rc::new(RefCell::new(vec![1])), Rc::new(|| {}));
    /// signal.update(|items| items.push(2));
    /// assert_eq!(signal.read(), vec![1, 2]);
    /// ```
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        f(&mut self.value.borrow_mut());
        self.bump_revision();
        (self.invalidate)();
    }

    /// Returns the shared mutation revision without borrowing the value.
    ///
    /// Zero means no successful mutating method has completed. The revision is
    /// nonzero thereafter but can repeat after `u64` wraparound.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// let signal = Signal::new(Rc::new(RefCell::new(0)), Rc::new(|| {}));
    /// assert_eq!(signal.revision(), 0);
    /// signal.set(1);
    /// assert_eq!(signal.revision(), 1);
    /// ```
    pub fn revision(&self) -> u64 {
        self.revision.get()
    }

    /// Advances the revision while reserving zero as the pristine sentinel.
    fn bump_revision(&self) {
        self.revision
            .set(self.revision.get().wrapping_add(1).max(1));
    }
}

/// Provides the operations defined for `Signal<T>`.
impl<T: PartialEq> Signal<T> {
    /// Replaces a small comparable value only when it actually changed.
    ///
    /// This avoids needless invalidations for scalar/domain revision signals;
    /// it is not intended as a substitute for retained models or for comparing
    /// complete trees.
    ///
    /// A `false` result performs no invalidation and does not advance revision.
    /// A `true` result commits the value before synchronously invalidating.
    ///
    /// # Panics
    ///
    /// Panics on a conflicting `RefCell` borrow, a panicking equality operation,
    /// or a panicking invalidation callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// let signal = Signal::new(Rc::new(RefCell::new(4)), Rc::new(|| {}));
    /// assert!(!signal.set_if_changed(4));
    /// assert!(signal.set_if_changed(5));
    /// assert_eq!((signal.read(), signal.revision()), (5, 1));
    /// ```
    pub fn set_if_changed(&self, value: T) -> bool {
        let mut current = self.value.borrow_mut();
        if *current == value {
            return false;
        }
        *current = value;
        drop(current);
        self.bump_revision();
        (self.invalidate)();
        true
    }
}

/// Provides the operations defined for `Signal<T>`.
impl<T: Clone> Signal<T> {
    /// Clones and returns the current value.
    ///
    /// # Panics
    ///
    /// Panics when the value is already mutably borrowed through another alias.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// let signal = Signal::new(Rc::new(RefCell::new(String::from("hello"))), Rc::new(|| {}));
    /// let copy = signal.read();
    /// assert_eq!(copy, "hello");
    /// ```
    pub fn read(&self) -> T {
        self.value.borrow().clone()
    }

    /// Creates a lazy derived value whose revision mirrors this signal.
    ///
    /// No value is cached: every [`Memo::read`] clones the current `T` and calls
    /// `f`. The mapping itself does not register an additional invalidation
    /// callback; consumers observe the originating signal's revision.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// let signal = Signal::new(Rc::new(RefCell::new(2)), Rc::new(|| {}));
    /// let doubled = signal.map(|value| value * 2);
    /// assert_eq!(doubled.read(), 4);
    /// signal.set(3);
    /// assert_eq!((doubled.read(), doubled.revision()), (6, 1));
    /// ```
    pub fn map<U: 'static>(&self, f: impl Fn(T) -> U + 'static) -> Memo<U>
    where
        T: 'static,
    {
        let signal = self.clone();
        let revision_signal = self.clone();

        Memo::with_revision(
            move || {
                let value = signal.read();
                f(value)
            },
            move || revision_signal.revision(),
        )
    }
}

impl<T> Signal<T>
where
    T: Clone + Display + 'static,
{
    /// Creates a lazy string memo using [`ToString::to_string`].
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// let signal = Signal::new(Rc::new(RefCell::new(42)), Rc::new(|| {}));
    /// assert_eq!(signal.to_text().read(), "42");
    /// ```
    pub fn to_text(&self) -> Memo<String> {
        self.map(|value| value.to_string())
    }
}

impl<T> Signal<T>
where
    T: Clone + 'static,
{
    /// Creates a lazy string memo using a custom owned-value formatter.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// let signal = Signal::new(Rc::new(RefCell::new(3)), Rc::new(|| {}));
    /// assert_eq!(signal.to_text_with(|value| format!("#{value}")).read(), "#3");
    /// ```
    pub fn to_text_with<F>(&self, format: F) -> Memo<String>
    where
        F: Fn(T) -> String + 'static,
    {
        self.map(format)
    }
}

/// Provides the operations defined for `Signal<T>`.
impl<T: Default> Signal<T> {
    /// Replaces the current value with [`Default`] and returns the old value.
    ///
    /// Revision and invalidation always occur, even when the old value already
    /// equaled its default.
    ///
    /// # Panics
    ///
    /// Panics on conflicting borrowing or a panicking invalidation callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// let signal = Signal::new(Rc::new(RefCell::new(String::from("take"))), Rc::new(|| {}));
    /// assert_eq!(signal.take(), "take");
    /// assert_eq!(signal.take(), "");
    /// assert_eq!(signal.revision(), 2);
    /// ```
    pub fn take(&self) -> T {
        let mut value = self.value.borrow_mut();
        let old = std::mem::take(&mut *value);
        self.bump_revision();
        (self.invalidate)();
        old
    }
}

/// Derived read-only value computed from signals or other memos.
///
/// A memo stores functions, not a cached value. Every [`Self::read`] evaluates
/// its closure again. Clones share the same closures through [`Rc`] and are
/// neither `Send` nor `Sync`. Revision reporting is advisory: [`Self::new`]
/// always reports zero, while memos derived through [`Signal::map`] propagate
/// their source revision.
///
/// # Examples
///
/// ```
/// use std::{cell::Cell, rc::Rc};
/// use ailloli_ui_runtime::component::Memo;
/// let calls = Rc::new(Cell::new(0));
/// let observed = calls.clone();
/// let memo = Memo::new(move || { observed.set(observed.get() + 1); 7 });
/// assert_eq!((memo.read(), memo.read()), (7, 7));
/// assert_eq!(calls.get(), 2);
/// ```
pub struct Memo<T> {
    /// Lazy computation invoked on every read.
    read_fn: Rc<dyn Fn() -> T>,
    /// Advisory source revision computation.
    revision_fn: Rc<dyn Fn() -> u64>,
}

/// Implements the `Clone` contract for `Memo<T>`.
impl<T> Clone for Memo<T> {
    /// Produces the clone required by the standard cloning contract.
    fn clone(&self) -> Self {
        Self {
            read_fn: self.read_fn.clone(),
            revision_fn: self.revision_fn.clone(),
        }
    }
}

/// Provides the operations defined for `Memo<T>`.
impl<T> Memo<T> {
    /// Creates an opaque lazy computation with constant revision zero.
    ///
    /// `read_fn` is not evaluated during construction and its outputs are not
    /// cached. If it observes mutable external state, values can change while
    /// [`Self::revision`] remains zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// let memo = Memo::new(|| 21 * 2);
    /// assert_eq!(memo.read(), 42);
    /// assert_eq!(memo.revision(), 0);
    /// ```
    pub fn new(read_fn: impl Fn() -> T + 'static) -> Self {
        Self {
            read_fn: Rc::new(read_fn),
            revision_fn: Rc::new(|| 0),
        }
    }

    /// Creates a memo with separate lazy value and revision functions.
    fn with_revision(
        read_fn: impl Fn() -> T + 'static,
        revision_fn: impl Fn() -> u64 + 'static,
    ) -> Self {
        Self {
            read_fn: Rc::new(read_fn),
            revision_fn: Rc::new(revision_fn),
        }
    }

    /// Evaluates and returns the current derived value.
    ///
    /// # Panics
    ///
    /// Any panic from the stored computation propagates to the caller.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// let memo = Memo::new(|| String::from("fresh"));
    /// assert_eq!(memo.read(), "fresh");
    /// ```
    pub fn read(&self) -> T {
        (self.read_fn)()
    }

    /// Evaluates the memo's advisory revision function.
    ///
    /// A standalone memo returns zero. Derived signal memos return the source's
    /// nonzero-after-change wrapping revision. This call does not read the value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// let memo = Memo::new(|| 1);
    /// assert_eq!(memo.revision(), 0);
    /// ```
    pub fn revision(&self) -> u64 {
        (self.revision_fn)()
    }

    /// Lazily maps this memo while preserving its revision function.
    ///
    /// Neither the source nor result is cached; each result read first evaluates
    /// the source and then `f`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// let source = Memo::new(|| 5);
    /// let label = source.map(|value| format!("value={value}"));
    /// assert_eq!(label.read(), "value=5");
    /// assert_eq!(label.revision(), 0);
    /// ```
    pub fn map<U: 'static>(&self, f: impl Fn(T) -> U + 'static) -> Memo<U>
    where
        T: 'static,
    {
        let memo = self.clone();
        let revision_memo = self.clone();

        Memo::with_revision(
            move || {
                let value = memo.read();
                f(value)
            },
            move || revision_memo.revision(),
        )
    }
}

impl<T> Memo<T>
where
    T: Display + 'static,
{
    /// Lazily formats each source value through [`ToString::to_string`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// assert_eq!(Memo::new(|| 8).to_text().read(), "8");
    /// ```
    pub fn to_text(&self) -> Memo<String> {
        self.map(|value| value.to_string())
    }
}

impl<T> Memo<T>
where
    T: 'static,
{
    /// Lazily formats each source value with `format`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// let text = Memo::new(|| 8).to_text_with(|value| format!("{value}px"));
    /// assert_eq!(text.read(), "8px");
    /// ```
    pub fn to_text_with<F>(&self, format: F) -> Memo<String>
    where
        F: Fn(T) -> String + 'static,
    {
        self.map(format)
    }
}
