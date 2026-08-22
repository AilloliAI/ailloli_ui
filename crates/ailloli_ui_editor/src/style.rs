//! Generic editor and scrollbar visual defaults.

use ailloli_ui_core::{Color, FontId};

/// Visual metrics and colors used by the editor engine.
///
/// Geometry is in logical pixels. `caret_blink_ms <= 0` disables blinking and
/// keeps the caret visible; callers should provide finite non-negative geometry.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::FontId;
/// use ailloli_ui_editor::EditorStyle;
/// let style = EditorStyle::default();
/// assert_eq!(style.font, FontId::Mono);
/// assert_eq!((style.px_size, style.line_height, style.pad), (13, 18.0, 10.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorStyle {
    /// Editor background color.
    pub bg: Color,
    /// Default foreground text color.
    pub fg: Color,
    /// Caret color.
    pub caret: Color,
    /// Selection highlight color.
    pub selection_bg: Color,
    /// Blink half-period in milliseconds; non-positive means always visible.
    pub caret_blink_ms: i64,
    /// Font family identifier passed to text layout.
    pub font: FontId,
    /// Font size in logical pixels.
    pub px_size: u16,
    /// Minimum visual line height in logical pixels.
    pub line_height: f32,
    /// Uniform content inset in logical pixels.
    pub pad: f32,
}

/// Supplies the dark neutral editor palette and fixed logical metrics.
impl Default for EditorStyle {
    /// Returns 13 px mono text, 18 px lines, 10 px padding, and 500 ms blink.
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
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::EditorScrollbarStyle;
/// let style = EditorScrollbarStyle::default();
/// assert_eq!((style.thickness, style.min_thumb_len, style.inset, style.radius), (6.0, 24.0, 3.0, 3.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorScrollbarStyle {
    /// Track color.
    pub track_color: Color,
    /// Scroll thumb color.
    pub thumb_color: Color,
    /// Overlay bar thickness in logical pixels.
    pub thickness: f32,
    /// Minimum thumb length in logical pixels.
    pub min_thumb_len: f32,
    /// Distance from viewport edges in logical pixels.
    pub inset: f32,
    /// Thumb/track corner radius in logical pixels.
    pub radius: f32,
}

/// Supplies conservative overlay-scrollbar defaults.
impl Default for EditorScrollbarStyle {
    /// Returns a 6 px bar with 24 px minimum thumb and 3 px inset/radius.
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

/// Enables or disables editor-owned scrollbars and selects their style.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::EditorScrollbarConfig;
/// let config = EditorScrollbarConfig::default();
/// assert!(config.enabled);
/// assert_eq!(config.style.thickness, 6.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorScrollbarConfig {
    /// Whether the adapter should paint editor-owned scrollbars.
    pub enabled: bool,
    /// Visual scrollbar metrics and colors.
    pub style: EditorScrollbarStyle,
}

/// Enables scrollbars with [`EditorScrollbarStyle::default`].
impl Default for EditorScrollbarConfig {
    /// Returns enabled scrollbars.
    fn default() -> Self {
        Self {
            enabled: true,
            style: EditorScrollbarStyle::default(),
        }
    }
}
