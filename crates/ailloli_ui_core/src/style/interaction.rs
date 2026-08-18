/// Pointer/focus/disabled flags for a widget at paint or input time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InteractionState {
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
    pub disabled: bool,
}

impl InteractionState {
    /// Default interactive state (not hovered, pressed, or focused).
    pub fn normal() -> Self {
        Self::default()
    }

    /// Disabled widget (other flags cleared).
    pub fn disabled() -> Self {
        Self {
            disabled: true,
            ..Self::default()
        }
    }
}

/// Style map keyed by interaction state (hover, press, focus, disabled).
#[derive(Clone, Debug, PartialEq)]
pub struct StateStyle<T> {
    pub normal: T,
    pub hovered: Option<T>,
    pub pressed: Option<T>,
    pub focused: Option<T>,
    pub disabled: Option<T>,
}

impl<T: Clone> StateStyle<T> {
    /// Picks the best matching style for `state` (disabled > pressed > hovered > focused > normal).
    pub fn resolve(&self, state: InteractionState) -> T {
        if state.disabled {
            if let Some(value) = &self.disabled {
                return value.clone();
            }
        }

        if state.pressed {
            if let Some(value) = &self.pressed {
                return value.clone();
            }
        }

        if state.hovered {
            if let Some(value) = &self.hovered {
                return value.clone();
            }
        }

        if state.focused {
            if let Some(value) = &self.focused {
                return value.clone();
            }
        }

        self.normal.clone()
    }
}
