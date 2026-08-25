//! Code-editor feature, gutter, scrollbar, and semantic color configuration.

use ailloli_ui_core::{Color, Theme};

use crate::{EditorConfig, EditorScrollbarConfig, EditorWrapMode};

/// Code-editor specific configuration layered over the generic editor config.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{CodeEditorConfig, EditorWrapMode};
/// let config = CodeEditorConfig::default();
/// assert_eq!(config.editor.wrap_mode, EditorWrapMode::NoWrap);
/// assert!(config.gutter.enabled && config.scrollbars.enabled);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CodeEditorConfig {
    /// Generic editor metrics and no-wrap policy.
    pub editor: EditorConfig,
    /// Code-specific semantic colors.
    pub theme: CodeTheme,
    /// Left gutter visibility and geometry.
    pub gutter: GutterConfig,
    /// Editor-owned scrollbar configuration.
    pub scrollbars: EditorScrollbarConfig,
    /// Optional code feature toggles.
    pub features: CodeEditorFeatureFlags,
}

/// Selects no-wrap layout, dark theme colors, gutter, scrollbars, and features.
impl Default for CodeEditorConfig {
    /// Synchronizes generic editor background/foreground with the code theme.
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

/// Feature switches for code-specific processing and paint layers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::CodeEditorFeatureFlags;
/// let flags = CodeEditorFeatureFlags::default();
/// assert!(flags.search && flags.diagnostics && flags.folding && flags.symbols);
/// assert!(!flags.semantic_backends);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeEditorFeatureFlags {
    /// Enables search match computation and paint.
    pub search: bool,
    /// Enables diagnostic hit testing and paint.
    pub diagnostics: bool,
    /// Enables fold-region processing and gutter markers.
    pub folding: bool,
    /// Enables local symbol indexing.
    pub symbols: bool,
    /// Enables opt-in LSP/SCIP enrichment owned by the caller.
    pub semantic_backends: bool,
}

/// Enables local deterministic features while leaving semantic backends off.
impl Default for CodeEditorFeatureFlags {
    /// Returns search/diagnostics/folding/symbols true and semantic false.
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
///
/// Width is logical pixels. Viewport construction treats disabled, zero, or
/// negative width as no gutter and clamps positive width to content width.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::GutterConfig;
/// let gutter = GutterConfig::default();
/// assert_eq!(gutter.width, 48.0);
/// assert!(gutter.enabled && gutter.line_numbers && gutter.fold_markers);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GutterConfig {
    /// Whether any gutter space is reserved.
    pub enabled: bool,
    /// Requested gutter width in logical pixels.
    pub width: f32,
    /// Whether line-number items are emitted.
    pub line_numbers: bool,
    /// Whether fold marker/guide items are emitted.
    pub fold_markers: bool,
}

/// Supplies an enabled 48-logical-pixel gutter with numbers and folds.
impl Default for GutterConfig {
    /// Returns the documented gutter defaults.
    fn default() -> Self {
        Self {
            enabled: true,
            width: 48.0,
            line_numbers: true,
            fold_markers: true,
        }
    }
}

/// Minimal visual theme for the CodeEditor.
///
/// Colors are semantic paint inputs and do not affect shaping/cache geometry.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::CodeTheme;
/// assert_eq!(CodeTheme::dark(), CodeTheme::default());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CodeTheme {
    /// Main code surface.
    pub background: Color,
    /// Default source text.
    pub foreground: Color,
    /// Gutter surface.
    pub gutter_bg: Color,
    /// Inactive line numbers.
    pub line_number: Color,
    /// Active line number.
    pub active_line_number: Color,
    /// Active line fill.
    pub active_line_bg: Color,
    /// Active line focus ring.
    pub active_line_ring: Color,
    /// Inactive search match fill.
    pub search_match_bg: Color,
    /// Active search match fill.
    pub search_active_match_bg: Color,
    /// Error diagnostic marker/underline.
    pub diagnostic_error: Color,
    /// Warning diagnostic marker/underline.
    pub diagnostic_warning: Color,
    /// Information diagnostic marker/underline.
    pub diagnostic_info: Color,
    /// Hint diagnostic marker/underline.
    pub diagnostic_hint: Color,
    /// Active diagnostic line fill.
    pub diagnostic_active_bg: Color,
    /// Inactive fold marker.
    pub fold_marker: Color,
    /// Active fold marker.
    pub fold_marker_active: Color,
    /// Fold guide line.
    pub fold_guide: Color,
    /// Syntax keyword.
    pub syntax_keyword: Color,
    /// Syntax type.
    pub syntax_type: Color,
    /// Syntax function.
    pub syntax_function: Color,
    /// Syntax string.
    pub syntax_string: Color,
    /// Syntax numeric literal.
    pub syntax_number: Color,
    /// Syntax comment.
    pub syntax_comment: Color,
    /// Syntax operator.
    pub syntax_operator: Color,
    /// Syntax punctuation.
    pub syntax_punctuation: Color,
    /// Syntax identifier/default token.
    pub syntax_identifier: Color,
}

/// Code-theme constructors.
impl CodeTheme {
    /// Returns the framework default dark-derived theme.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::CodeTheme;
    /// assert_eq!(CodeTheme::dark(), CodeTheme::default());
    /// ```
    pub fn dark() -> Self {
        Self::default()
    }

    /// Maps framework palette roles plus fixed syntax accents into code colors.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_editor::CodeTheme;
    /// let base = Theme::default();
    /// let code = CodeTheme::from_theme(base);
    /// assert_eq!(code.background, base.palette().background);
    /// assert_eq!(code.foreground, base.palette().text);
    /// ```
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

/// Derives the code palette from [`Theme::default`].
impl Default for CodeTheme {
    /// Returns the framework-default themed palette.
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}
