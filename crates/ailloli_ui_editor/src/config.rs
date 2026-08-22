//! Generic editor wrapping and visual configuration.

use crate::style::EditorStyle;

/// Line wrapping strategy for editor layout.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::EditorWrapMode;
/// assert_eq!(EditorWrapMode::default(), EditorWrapMode::SoftWrap);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EditorWrapMode {
    /// Text editor mode: soft-wrap by word, with emergency breaks for long runs.
    #[default]
    SoftWrap,
    /// Code editor mode: no visual wrap; horizontal scrolling clips overflowing text.
    NoWrap,
}

/// Engine configuration shared by layout, input, and paint model generation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{EditorConfig, EditorWrapMode};
/// let config = EditorConfig::default();
/// assert_eq!(config.wrap_mode, EditorWrapMode::SoftWrap);
/// assert_eq!(config.style.line_height, 18.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EditorConfig {
    /// Logical-pixel metrics and semantic colors.
    pub style: EditorStyle,
    /// Soft-wrap or horizontal-scroll layout policy.
    pub wrap_mode: EditorWrapMode,
}
