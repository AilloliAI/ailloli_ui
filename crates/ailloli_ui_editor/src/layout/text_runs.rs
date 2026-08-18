use std::ops::Range;

use ailloli_ui_text::{PreparedTextLayout, TextLayoutHandle};

use crate::EditorStyle;

/// One visible logical paragraph run in viewport coordinates.
#[derive(Debug, Clone)]
pub struct EditorTextRun {
    pub index: usize,
    pub byte_range: Range<usize>,
    pub baseline_y: f32,
    pub layout: TextLayoutHandle,
}

pub fn first_layout_baseline(layout: &PreparedTextLayout) -> f32 {
    layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0)
}

pub fn layout_visual_height(layout: &PreparedTextLayout, style: EditorStyle) -> f32 {
    layout.height().max(style.line_height.max(1.0))
}

pub fn run_visual_top(run: &EditorTextRun) -> f32 {
    run.baseline_y - first_layout_baseline(&run.layout)
}

pub fn run_visual_bottom(run: &EditorTextRun, style: EditorStyle) -> f32 {
    run_visual_top(run) + layout_visual_height(&run.layout, style)
}
