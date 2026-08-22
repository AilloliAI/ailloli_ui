//! Visible paragraph run values and baseline-derived visual bounds.

use std::ops::Range;

use ailloli_ui_text::{PreparedTextLayout, TextLayoutHandle};

use crate::EditorStyle;

/// One visible logical paragraph run in viewport coordinates.
///
/// `byte_range` addresses the source buffer while the layout text and helper
/// offsets are run-local. `baseline_y` is relative to the text viewport before
/// the caller adds its screen-space origin.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_editor::layout::EditorTextRun;
/// use ailloli_ui_text::{TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new(
///     "line", TextStyle::new(FontId::Mono, 13, Color::WHITE),
/// ));
/// let run = EditorTextRun { index: 2, byte_range: 8..12, baseline_y: 18.0, layout };
/// assert_eq!(run.byte_range, 8..12);
/// ```
#[derive(Debug, Clone)]
pub struct EditorTextRun {
    /// Zero-based logical paragraph index in the source buffer.
    pub index: usize,
    /// Half-open UTF-8 byte range in the source, excluding a trailing newline.
    pub byte_range: Range<usize>,
    /// First shaped baseline relative to the visible text viewport.
    pub baseline_y: f32,
    /// Shared prepared layout for this paragraph's newline-trimmed text.
    pub layout: TextLayoutHandle,
}

/// Returns the first shaped line's baseline in layout-local logical pixels.
///
/// Empty layouts use the zero sentinel.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_editor::layout::first_layout_baseline;
/// use ailloli_ui_text::{TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new(
///     "x", TextStyle::new(FontId::Mono, 13, Color::WHITE),
/// ));
/// assert!(first_layout_baseline(&layout) >= 0.0);
/// ```
pub fn first_layout_baseline(layout: &PreparedTextLayout) -> f32 {
    layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0)
}

/// Returns the run height in logical pixels with an editor-line-height floor.
///
/// The configured floor is itself clamped to at least `1.0`; non-finite layout
/// or style values are not repaired beyond normal [`f32::max`] behavior.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_editor::{layout::layout_visual_height, EditorStyle};
/// use ailloli_ui_text::{TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new(
///     "x", TextStyle::new(FontId::Mono, 13, Color::WHITE),
/// ));
/// assert!(layout_visual_height(&layout, EditorStyle::default()) >= 18.0);
/// ```
pub fn layout_visual_height(layout: &PreparedTextLayout, style: EditorStyle) -> f32 {
    layout.height().max(style.line_height.max(1.0))
}

/// Returns a run's top edge relative to the text viewport.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_editor::layout::{first_layout_baseline, run_visual_top, EditorTextRun};
/// use ailloli_ui_text::{TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new(
///     "x", TextStyle::new(FontId::Mono, 13, Color::WHITE),
/// ));
/// let baseline = first_layout_baseline(&layout);
/// let run = EditorTextRun { index: 0, byte_range: 0..1, baseline_y: baseline + 7.0, layout };
/// assert_eq!(run_visual_top(&run), 7.0);
/// ```
pub fn run_visual_top(run: &EditorTextRun) -> f32 {
    run.baseline_y - first_layout_baseline(&run.layout)
}

/// Returns a run's bottom edge relative to the text viewport.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_editor::{layout::{run_visual_bottom, run_visual_top, EditorTextRun}, EditorStyle};
/// use ailloli_ui_text::{TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new(
///     "x", TextStyle::new(FontId::Mono, 13, Color::WHITE),
/// ));
/// let run = EditorTextRun { index: 0, byte_range: 0..1, baseline_y: 14.0, layout };
/// assert!(run_visual_bottom(&run, EditorStyle::default()) > run_visual_top(&run));
/// ```
pub fn run_visual_bottom(run: &EditorTextRun, style: EditorStyle) -> f32 {
    run_visual_top(run) + layout_visual_height(&run.layout, style)
}
