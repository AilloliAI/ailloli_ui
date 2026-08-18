use ailloli_ui_core::{Color, FontId};

/// Visual metrics and colors used by the editor engine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorStyle {
    pub bg: Color,
    pub fg: Color,
    pub caret: Color,
    pub selection_bg: Color,
    pub caret_blink_ms: i64,
    pub font: FontId,
    pub px_size: u16,
    pub line_height: f32,
    pub pad: f32,
}

impl Default for EditorStyle {
    fn default() -> Self {
        Self {
            bg: Color::hex("#0f1117").expect("hex"),
            fg: Color::hex("#ebebf0").expect("hex"),
            caret: Color::hex("#f2f2f7").expect("hex"),
            selection_bg: Color::hex("#2563eb59").expect("hex"),
            caret_blink_ms: 500,
            font: FontId::Mono,
            px_size: 13,
            line_height: 18.0,
            pad: 10.0,
        }
    }
}

/// Visual style used by editor-owned scrollbars.
///
/// This mirrors the public `ScrollView` scrollbar defaults without depending on
/// `ailloli_ui_widgets`, because the editor engine is UI-adapter agnostic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorScrollbarStyle {
    pub track_color: Color,
    pub thumb_color: Color,
    pub thickness: f32,
    pub min_thumb_len: f32,
    pub inset: f32,
    pub radius: f32,
}

impl Default for EditorScrollbarStyle {
    fn default() -> Self {
        Self {
            track_color: Color::rgba(148, 163, 184, 0.16),
            thumb_color: Color::rgba(148, 163, 184, 0.56),
            thickness: 6.0,
            min_thumb_len: 24.0,
            inset: 3.0,
            radius: 3.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorScrollbarConfig {
    pub enabled: bool,
    pub style: EditorScrollbarStyle,
}

impl Default for EditorScrollbarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            style: EditorScrollbarStyle::default(),
        }
    }
}
