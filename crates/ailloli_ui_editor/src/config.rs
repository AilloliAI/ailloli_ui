use crate::style::EditorStyle;

/// Line wrapping strategy for editor layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EditorWrapMode {
    /// Text editor mode: soft-wrap by word, with emergency breaks for long runs.
    #[default]
    SoftWrap,
    /// Code editor mode: no visual wrap; horizontal scrolling clips overflowing text.
    NoWrap,
}

/// Engine configuration shared by layout, input, and paint model generation.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EditorConfig {
    pub style: EditorStyle,
    pub wrap_mode: EditorWrapMode,
}
