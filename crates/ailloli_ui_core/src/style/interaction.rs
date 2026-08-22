//! Widget interaction flags and precedence-based state style selection.

/// Pointer/focus/disabled flags for a widget at paint or input time.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::InteractionState;
/// assert!(!InteractionState::normal().pressed);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InteractionState {
    /// `true` while the pointer hover region contains the active pointer.
    pub hovered: bool,
    /// `true` while the widget owns an active press gesture.
    pub pressed: bool,
    /// `true` while the widget owns keyboard focus.
    pub focused: bool,
    /// `true` when interaction should be disabled.
    pub disabled: bool,
}

impl InteractionState {
    /// Default interactive state (not hovered, pressed, or focused).
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::InteractionState;
    /// assert_eq!(InteractionState::normal(), InteractionState::default());
    /// ```
    pub fn normal() -> Self {
        Self::default()
    }

    /// Disabled widget (other flags cleared).
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::InteractionState;
    /// assert!(InteractionState::disabled().disabled);
    /// ```
    pub fn disabled() -> Self {
        Self {
            disabled: true,
            ..Self::default()
        }
    }
}

/// Style map keyed by interaction state (hover, press, focus, disabled).
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::{InteractionState, StateStyle};
/// let styles = StateStyle { normal: 0, hovered: Some(1), pressed: None, focused: None, disabled: None };
/// assert_eq!(styles.resolve(InteractionState { hovered: true, ..InteractionState::default() }), 1);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct StateStyle<T> {
    /// Required fallback style when no applicable override exists.
    pub normal: T,
    /// Optional hover override.
    pub hovered: Option<T>,
    /// Optional active-press override.
    pub pressed: Option<T>,
    /// Optional keyboard-focus override.
    pub focused: Option<T>,
    /// Optional disabled override.
    pub disabled: Option<T>,
}

impl<T: Clone> StateStyle<T> {
    /// Clones the highest-priority available override for `state`.
    ///
    /// Precedence is disabled, pressed, hovered, focused, then normal. A true
    /// flag with a `None` override falls through to the next true flag rather
    /// than immediately selecting normal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{InteractionState, StateStyle};
    /// let styles = StateStyle { normal: "normal", hovered: Some("hover"), pressed: Some("press"), focused: None, disabled: None };
    /// let state = InteractionState { hovered: true, pressed: true, ..InteractionState::default() };
    /// assert_eq!(styles.resolve(state), "press");
    /// ```
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
