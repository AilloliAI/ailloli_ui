//! Widget-instance identity.

/// Opaque numeric ID assigned to a widget instance for input routing and paint.
///
/// Zero has no special meaning at this type layer.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::WidgetId;
/// assert_eq!(WidgetId(9).0, 9);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WidgetId(pub u64);
