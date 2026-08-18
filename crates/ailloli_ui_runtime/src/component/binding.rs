use super::{Memo, Signal, State};

/// Bindable widget prop: static value, [`Signal`], or [`Memo`].
///
/// Unifies `disabled(true)`, `disabled(signal)`, and `disabled(memo)` without separate DSL methods.
pub enum Binding<T> {
    Static(T),
    Signal(Signal<T>),
    Memo(Memo<T>),
}

impl<T: Clone> Clone for Binding<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Static(value) => Self::Static(value.clone()),
            Self::Signal(signal) => Self::Signal(signal.clone()),
            Self::Memo(memo) => Self::Memo(memo.clone()),
        }
    }
}

impl<T> From<T> for Binding<T> {
    fn from(value: T) -> Self {
        Self::Static(value)
    }
}

impl<T> From<Signal<T>> for Binding<T> {
    fn from(value: Signal<T>) -> Self {
        Self::Signal(value)
    }
}

impl<T> From<State<T>> for Binding<T> {
    fn from(value: State<T>) -> Self {
        Self::Signal(value.into())
    }
}

impl<T> From<Memo<T>> for Binding<T> {
    fn from(value: Memo<T>) -> Self {
        Self::Memo(value)
    }
}

impl From<&str> for Binding<String> {
    fn from(value: &str) -> Self {
        Self::Static(value.to_string())
    }
}

impl From<&String> for Binding<String> {
    fn from(value: &String) -> Self {
        Self::Static(value.clone())
    }
}

impl<T: Clone> Binding<T> {
    pub fn read(&self) -> T {
        match self {
            Binding::Static(v) => v.clone(),
            Binding::Signal(s) => s.read(),
            Binding::Memo(m) => m.read(),
        }
    }
}

impl<T> Binding<T> {
    pub fn is_signal(&self) -> bool {
        matches!(self, Binding::Signal(_))
    }
}
