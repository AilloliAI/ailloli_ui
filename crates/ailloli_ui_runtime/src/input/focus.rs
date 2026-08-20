use ailloli_ui_core::ElementId;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FocusPolicy {
    #[default]
    NotFocusable,
    Focusable,
}

/// Controls whether a pointer gesture that only activated/focused the host may
/// also activate a widget.
///
/// Policies are resolved from the hit-tested child towards its ancestors. If
/// no widget chooses an explicit policy, the input router uses the safe
/// [`ActivationPolicy::SuppressOnFocusOnly`] root fallback.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ActivationPolicy {
    /// Defer the decision to the closest ancestor with an explicit policy.
    #[default]
    Inherit,
    /// Preserve focus handling but suppress action activation.
    SuppressOnFocusOnly,
    /// Deliver the gesture normally, for example to place a text caret.
    AllowOnFocusOnly,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InputRole {
    #[default]
    None,
    TextSingleLine,
    TextMultiLine,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HoverCursorRole {
    #[default]
    Inherit,
    Default,
    Pointer,
    Text,
    ResizeX,
    ResizeY,
}

#[derive(Debug, Default, Clone)]
pub struct FocusManager {
    focused: Option<ElementId>,
}

impl FocusManager {
    pub fn focused(&self) -> Option<ElementId> {
        self.focused
    }

    pub fn set_focused(&mut self, id: Option<ElementId>) {
        self.focused = id;
    }
}
