use ailloli_ui_core::{Color, Theme};

use crate::{EditorConfig, EditorScrollbarConfig, EditorWrapMode};

/// Code-editor specific configuration layered over the generic editor config.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CodeEditorConfig {
    pub editor: EditorConfig,
    pub theme: CodeTheme,
    pub gutter: GutterConfig,
    pub scrollbars: EditorScrollbarConfig,
    pub features: CodeEditorFeatureFlags,
}

impl Default for CodeEditorConfig {
    fn default() -> Self {
        let theme = CodeTheme::default();
        let mut editor = EditorConfig {
            wrap_mode: EditorWrapMode::NoWrap,
            ..EditorConfig::default()
        };
        editor.style.bg = theme.background;
        editor.style.fg = theme.foreground;
        Self {
            editor,
            theme,
            gutter: GutterConfig::default(),
            scrollbars: EditorScrollbarConfig::default(),
            features: CodeEditorFeatureFlags::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeEditorFeatureFlags {
    pub search: bool,
    pub diagnostics: bool,
    pub folding: bool,
    pub symbols: bool,
    pub semantic_backends: bool,
}

impl Default for CodeEditorFeatureFlags {
    fn default() -> Self {
        Self {
            search: true,
            diagnostics: true,
            folding: true,
            symbols: true,
            semantic_backends: false,
        }
    }
}

/// Gutter controls for code-oriented editor frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GutterConfig {
    pub enabled: bool,
    pub width: f32,
    pub line_numbers: bool,
    pub fold_markers: bool,
}

impl Default for GutterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            width: 48.0,
            line_numbers: true,
            fold_markers: true,
        }
    }
}

/// Minimal visual theme for the CodeEditor MVP.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CodeTheme {
    pub background: Color,
    pub foreground: Color,
    pub gutter_bg: Color,
    pub line_number: Color,
    pub active_line_number: Color,
    pub active_line_bg: Color,
    pub active_line_ring: Color,
    pub search_match_bg: Color,
    pub search_active_match_bg: Color,
    pub diagnostic_error: Color,
    pub diagnostic_warning: Color,
    pub diagnostic_info: Color,
    pub diagnostic_hint: Color,
    pub diagnostic_active_bg: Color,
    pub fold_marker: Color,
    pub fold_marker_active: Color,
    pub fold_guide: Color,
    pub syntax_keyword: Color,
    pub syntax_type: Color,
    pub syntax_function: Color,
    pub syntax_string: Color,
    pub syntax_number: Color,
    pub syntax_comment: Color,
    pub syntax_operator: Color,
    pub syntax_punctuation: Color,
    pub syntax_identifier: Color,
}

impl CodeTheme {
    pub fn dark() -> Self {
        Self::default()
    }

    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        Self {
            background: palette.background,
            foreground: palette.text,
            gutter_bg: palette.surface,
            line_number: palette.text_muted,
            active_line_number: palette.text,
            active_line_bg: Color::rgba(255, 255, 255, 0.05),
            active_line_ring: palette.focus.with_alpha(0.32),
            search_match_bg: palette.warning.with_alpha(0.28),
            search_active_match_bg: palette.accent.with_alpha(0.48),
            diagnostic_error: palette.danger,
            diagnostic_warning: palette.warning,
            diagnostic_info: palette.info,
            diagnostic_hint: palette.text_muted,
            diagnostic_active_bg: palette.danger.with_alpha(0.12),
            fold_marker: palette.warning,
            fold_marker_active: palette.accent,
            fold_guide: palette.warning.with_alpha(0.36),
            syntax_keyword: Color::rgb(197, 134, 192),
            syntax_type: palette.info,
            syntax_function: Color::rgb(220, 220, 170),
            syntax_string: Color::rgb(206, 145, 120),
            syntax_number: Color::rgb(181, 206, 168),
            syntax_comment: Color::rgb(106, 153, 85),
            syntax_operator: palette.text,
            syntax_punctuation: palette.text_muted,
            syntax_identifier: palette.text,
        }
    }
}

impl Default for CodeTheme {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}
