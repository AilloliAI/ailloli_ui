/// Focus change notification dispatched to widgets when runtime focus changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusEvent {
    pub focused: bool,
}

impl FocusEvent {
    pub fn new(focused: bool) -> Self {
        Self { focused }
    }
}
