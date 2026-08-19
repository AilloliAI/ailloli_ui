use ailloli_ui_core::ElementId;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FocusPolicy {
    #[default]
    NotFocusable,
    Focusable,
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
