use std::cell::RefCell;
use std::fmt::Display;
use std::rc::Rc;

use super::{Memo, Signal};

/// Public DX state handle built on top of the retained runtime `Signal`.
pub struct State<T> {
    signal: Signal<T>,
}

impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        Self {
            signal: self.signal.clone(),
        }
    }
}

impl<T: 'static> State<T> {
    pub fn new(value: T) -> Self {
        Self::from_signal(Signal::new(Rc::new(RefCell::new(value)), Rc::new(|| {})))
    }

    pub fn from_signal(signal: Signal<T>) -> Self {
        Self { signal }
    }

    pub fn set(&self, value: T) {
        self.signal.set(value);
    }

    pub fn update(&self, f: impl FnOnce(&mut T)) {
        self.signal.update(f);
    }

    pub fn into_signal(self) -> Signal<T> {
        self.signal
    }
}

impl<T: Clone + 'static> State<T> {
    pub fn read(&self) -> T {
        self.signal.read()
    }

    pub fn map<U: 'static>(&self, f: impl Fn(T) -> U + 'static) -> Memo<U> {
        self.signal.map(f)
    }

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
    pub fn to_text(&self) -> Memo<String> {
        self.signal.to_text()
    }
}

impl<T> From<State<T>> for Signal<T> {
    fn from(value: State<T>) -> Self {
        value.signal
    }
}
