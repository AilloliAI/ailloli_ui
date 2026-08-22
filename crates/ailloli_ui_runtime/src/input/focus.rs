//! Keyboard-focus policy and focus-transition helpers.

use ailloli_ui_core::ElementId;

/// Whether an element may become the keyboard-focus owner.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::input::FocusPolicy;
/// assert_eq!(FocusPolicy::default(), FocusPolicy::NotFocusable);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FocusPolicy {
    /// Pointer and keyboard navigation skip the element; this is the default.
    #[default]
    NotFocusable,
    /// The input router may assign keyboard focus to the element.
    Focusable,
}

/// Controls whether a pointer gesture that only activated/focused the host may
/// also activate a widget.
///
/// Policies are resolved from the hit-tested child towards its ancestors. If
/// no widget chooses an explicit policy, the input router uses the safe
/// [`ActivationPolicy::SuppressOnFocusOnly`] root fallback.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::input::ActivationPolicy;
/// assert_eq!(ActivationPolicy::default(), ActivationPolicy::Inherit);
/// ```
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

/// Semantic keyboard-input role of a retained widget.
///
/// The role guides focus, IME, and cursor behavior; it does not itself edit
/// text or make an element focusable.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::input::InputRole;
/// assert_eq!(InputRole::default(), InputRole::None);
/// assert_ne!(InputRole::TextSingleLine, InputRole::TextMultiLine);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InputRole {
    /// No text-input semantics; this is the default.
    #[default]
    None,
    /// Single-line editable text semantics.
    TextSingleLine,
    /// Multi-line editable text semantics.
    TextMultiLine,
}

/// Cursor role requested while a pointer hovers an element.
///
/// `Inherit` delegates to an ancestor; every other value is explicit. Mapping
/// these provider-neutral roles to platform cursor assets is a host concern.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::input::HoverCursorRole;
/// assert_eq!(HoverCursorRole::default(), HoverCursorRole::Inherit);
/// assert_ne!(HoverCursorRole::Pointer, HoverCursorRole::Text);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HoverCursorRole {
    /// Delegate to the closest ancestor with an explicit role; this is default.
    #[default]
    Inherit,
    /// Platform default arrow cursor.
    Default,
    /// Link or action pointer cursor.
    Pointer,
    /// Text-selection I-beam cursor.
    Text,
    /// Horizontal resize cursor.
    ResizeX,
    /// Vertical resize cursor.
    ResizeY,
}

/// Stores the single keyboard-focus owner for one input router.
///
/// This type does not validate whether the ID exists or is focusable; routing
/// code is responsible for clearing stale IDs and dispatching focus events.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId;
/// use ailloli_ui_runtime::input::FocusManager;
/// let mut focus = FocusManager::default();
/// focus.set_focused(Some(ElementId(7)));
/// assert_eq!(focus.focused(), Some(ElementId(7)));
/// ```
#[derive(Debug, Default, Clone)]
pub struct FocusManager {
    focused: Option<ElementId>,
}

/// Provides the operations defined for FocusManager.
impl FocusManager {
    /// Returns the current focus owner, or `None` when nothing is focused.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::FocusManager;
    /// assert_eq!(FocusManager::default().focused(), None);
    /// ```
    pub fn focused(&self) -> Option<ElementId> {
        self.focused
    }

    /// Replaces the focus owner without dispatching events or invalidating paint.
    ///
    /// Passing `None` clears focus. Unknown IDs are stored unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::input::FocusManager;
    /// let mut focus = FocusManager::default();
    /// focus.set_focused(Some(ElementId(2)));
    /// focus.set_focused(None);
    /// assert_eq!(focus.focused(), None);
    /// ```
    pub fn set_focused(&mut self, id: Option<ElementId>) {
        self.focused = id;
    }
}
