//! Platform-window chrome actions produced by input routing.

/// Edge or corner used for an interactive native-window resize.
///
/// The compass directions are logical screen directions; they do not change in
/// right-to-left interfaces.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::input::ResizeEdge;
/// assert_ne!(ResizeEdge::N, ResizeEdge::SE);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    /// North/top edge.
    N,
    /// South/bottom edge.
    S,
    /// East/right edge.
    E,
    /// West/left edge.
    W,
    /// North-east/top-right corner.
    NE,
    /// North-west/top-left corner.
    NW,
    /// South-east/bottom-right corner.
    SE,
    /// South-west/bottom-left corner.
    SW,
}

/// Cursor role requested from a native window backend.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::input::{CursorStyle, ResizeEdge};
/// assert_eq!(CursorStyle::Resize(ResizeEdge::E), CursorStyle::Resize(ResizeEdge::E));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorStyle {
    /// Backend default pointer cursor.
    Default,
    /// Resize cursor corresponding to one edge or corner.
    Resize(ResizeEdge),
}

/// Native-window chrome operation requested by a routed UI event.
///
/// Values are host intents only. The runtime does not move, resize, or mutate a
/// platform window until an adapter consumes the action.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::input::{ChromeAction, ResizeEdge};
/// let action = ChromeAction::StartWindowResize { edge: ResizeEdge::SW };
/// assert_eq!(action, ChromeAction::StartWindowResize { edge: ResizeEdge::SW });
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChromeAction {
    /// Begin an operating-system window drag using the initiating pointer event.
    StartWindowDrag,
    /// Begin interactive native resize from `edge`.
    StartWindowResize {
        /// Native window edge or corner that follows the pointer.
        edge: ResizeEdge,
    },
    /// Change the native cursor until another request supersedes it.
    SetCursor {
        /// Cursor role the native host should display.
        cursor: CursorStyle,
    },
}
