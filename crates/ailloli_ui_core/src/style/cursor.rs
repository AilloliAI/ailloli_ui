//! Platform cursor hints selected by interactive widget regions.

/// Platform cursor hint for hover regions.
///
/// Values cover automatic/default, text, pointer, grab/grabbing, and horizontal
/// or vertical resize cursors.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::CursorStyle;
/// assert_eq!(CursorStyle::default(), CursorStyle::Auto);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CursorStyle {
    /// Let the host choose from context; this is the default.
    #[default]
    Auto,
    /// Platform default arrow cursor.
    Default,
    /// Text insertion cursor.
    Text,
    /// Pointing-hand cursor for links and activatable regions.
    Pointer,
    /// Open-hand cursor for a draggable region.
    Grab,
    /// Closed-hand cursor while a drag is active.
    Grabbing,
    /// Horizontal resize cursor.
    ResizeX,
    /// Vertical resize cursor.
    ResizeY,
}
