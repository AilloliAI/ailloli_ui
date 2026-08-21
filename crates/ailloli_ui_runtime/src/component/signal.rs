// Example Usage
// let input = context.signal(String::new());
// let can_send = input.map(|v| !v.trim().is_empty());
//
// Then:
//
//  Button::with_label("Send")
//    .disabled_signal(can_send.map(|v| !v))

use std::cell::{Cell, RefCell};
use std::fmt::Display;
use std::rc::Rc;

/// Mutable reactive cell; updates call `invalidate` to schedule redraw.
pub struct Signal<T> {
    value: Rc<RefCell<T>>,
    invalidate: Rc<dyn Fn()>,
    revision: Rc<Cell<u64>>,
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            invalidate: self.invalidate.clone(),
            revision: self.revision.clone(),
        }
    }
}

impl<T> Signal<T> {
    pub fn new(value: Rc<RefCell<T>>, invalidate: Rc<dyn Fn()>) -> Self {
        Self {
            value,
            invalidate,
            revision: Rc::new(Cell::new(0)),
        }
    }

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

    pub fn set(&self, value: T) {
        *self.value.borrow_mut() = value;
        self.bump_revision();
        (self.invalidate)();
    }

    pub fn update(&self, f: impl FnOnce(&mut T)) {
        f(&mut self.value.borrow_mut());
        self.bump_revision();
        (self.invalidate)();
    }

    pub fn revision(&self) -> u64 {
        self.revision.get()
    }

    fn bump_revision(&self) {
        self.revision
            .set(self.revision.get().wrapping_add(1).max(1));
    }
}

impl<T: PartialEq> Signal<T> {
    /// Replaces a small comparable value only when it actually changed.
    ///
    /// This avoids needless invalidations for scalar/domain revision signals;
    /// it is not intended as a substitute for retained models or for comparing
    /// complete trees.
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

impl<T: Clone> Signal<T> {
    pub fn read(&self) -> T {
        self.value.borrow().clone()
    }

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
    pub fn to_text(&self) -> Memo<String> {
        self.map(|value| value.to_string())
    }
}

impl<T> Signal<T>
where
    T: Clone + 'static,
{
    pub fn to_text_with<F>(&self, format: F) -> Memo<String>
    where
        F: Fn(T) -> String + 'static,
    {
        self.map(format)
    }
}

impl<T: Default> Signal<T> {
    pub fn take(&self) -> T {
        let mut value = self.value.borrow_mut();
        let old = std::mem::take(&mut *value);
        self.bump_revision();
        (self.invalidate)();
        old
    }
}

/// Derived read-only value computed from signals or other memos.
pub struct Memo<T> {
    read_fn: Rc<dyn Fn() -> T>,
    revision_fn: Rc<dyn Fn() -> u64>,
}

impl<T> Clone for Memo<T> {
    fn clone(&self) -> Self {
        Self {
            read_fn: self.read_fn.clone(),
            revision_fn: self.revision_fn.clone(),
        }
    }
}

impl<T> Memo<T> {
    pub fn new(read_fn: impl Fn() -> T + 'static) -> Self {
        Self {
            read_fn: Rc::new(read_fn),
            revision_fn: Rc::new(|| 0),
        }
    }

    fn with_revision(
        read_fn: impl Fn() -> T + 'static,
        revision_fn: impl Fn() -> u64 + 'static,
    ) -> Self {
        Self {
            read_fn: Rc::new(read_fn),
            revision_fn: Rc::new(revision_fn),
        }
    }

    pub fn read(&self) -> T {
        (self.read_fn)()
    }

    pub fn revision(&self) -> u64 {
        (self.revision_fn)()
    }

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
    pub fn to_text(&self) -> Memo<String> {
        self.map(|value| value.to_string())
    }
}

impl<T> Memo<T>
where
    T: 'static,
{
    pub fn to_text_with<F>(&self, format: F) -> Memo<String>
    where
        F: Fn(T) -> String + 'static,
    {
        self.map(format)
    }
}
