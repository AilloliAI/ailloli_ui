// Example Usage
// let input = context.signal(String::new());
// let can_send = input.map(|v| !v.trim().is_empty());
//
// Then:
//
//  Button::with_label("Send")
//    .disabled_signal(can_send.map(|v| !v))

use std::cell::RefCell;
use std::fmt::Display;
use std::rc::Rc;

/// Mutable reactive cell; updates call `invalidate` to schedule redraw.
pub struct Signal<T> {
    value: Rc<RefCell<T>>,
    invalidate: Rc<dyn Fn()>,
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            invalidate: self.invalidate.clone(),
        }
    }
}

impl<T> Signal<T> {
    pub fn new(value: Rc<RefCell<T>>, invalidate: Rc<dyn Fn()>) -> Self {
        Self { value, invalidate }
    }

    pub fn set(&self, value: T) {
        *self.value.borrow_mut() = value;
        (self.invalidate)();
    }

    pub fn update(&self, f: impl FnOnce(&mut T)) {
        f(&mut self.value.borrow_mut());
        (self.invalidate)();
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

        Memo::new(move || {
            let value = signal.read();
            f(value)
        })
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
        (self.invalidate)();
        old
    }
}

/// Derived read-only value computed from signals or other memos.
pub struct Memo<T> {
    read_fn: Rc<dyn Fn() -> T>,
}

impl<T> Clone for Memo<T> {
    fn clone(&self) -> Self {
        Self {
            read_fn: self.read_fn.clone(),
        }
    }
}

impl<T> Memo<T> {
    pub fn new(read_fn: impl Fn() -> T + 'static) -> Self {
        Self {
            read_fn: Rc::new(read_fn),
        }
    }

    pub fn read(&self) -> T {
        (self.read_fn)()
    }

    pub fn map<U: 'static>(&self, f: impl Fn(T) -> U + 'static) -> Memo<U>
    where
        T: 'static,
    {
        let memo = self.clone();

        Memo::new(move || {
            let value = memo.read();
            f(value)
        })
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
