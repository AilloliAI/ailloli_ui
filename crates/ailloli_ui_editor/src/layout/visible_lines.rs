use ailloli_ui_core::{Size, TextStyle};
use ailloli_ui_text::{TextBuffer, TextSystem};

use crate::layout::layout_cache::LayoutCacheKey;
use crate::layout::{
    first_layout_baseline, layout_visual_height, EditorTextRun, FenwickTree, LayoutCache,
    ParagraphMetrics, ParagraphMetricsCache, ParagraphMetricsStats,
};
use crate::{EditorStyle, EditorViewport, EditorWrapMode};

#[derive(Debug, Clone)]
pub struct VisibleTextLayout {
    pub runs: Vec<EditorTextRun>,
    pub content_size: Size,
    pub metrics_stats: ParagraphMetricsStats,
    pub used_fast_path: bool,
}

/// Builds visible paragraph runs for the current viewport.
pub fn build_visible_text_layout(
    buffer: &TextBuffer,
    viewport: EditorViewport,
    style: EditorStyle,
    layout_cache: &mut LayoutCache,
    metrics_cache: &mut ParagraphMetricsCache,
    text_system: &mut TextSystem,
) -> VisibleTextLayout {
    build_visible_text_layout_filtered(
        buffer,
        viewport,
        style,
        layout_cache,
        metrics_cache,
        text_system,
        &|_| false,
    )
}

pub fn build_visible_text_layout_filtered(
    buffer: &TextBuffer,
    viewport: EditorViewport,
    style: EditorStyle,
    layout_cache: &mut LayoutCache,
    metrics_cache: &mut ParagraphMetricsCache,
    text_system: &mut TextSystem,
    is_hidden: &dyn Fn(usize) -> bool,
) -> VisibleTextLayout {
    if viewport.wrap_mode == EditorWrapMode::NoWrap
        && !(0..buffer.paragraphs().len()).any(is_hidden)
    {
        return build_visible_nowrap_text_layout(
            buffer,
            viewport,
            style,
            layout_cache,
            metrics_cache,
            text_system,
        );
    }

    let viewport_top = viewport.scroll_y;
    let viewport_bot = viewport_top + viewport.content_rect.h.max(0.0);
    let text_style = TextStyle::new(style.font, style.px_size, style.fg);
    let mut runs = Vec::new();
    let mut para_top = 0.0;
    let mut content_width = 0.0f32;
    let mut stats = ParagraphMetricsStats::default();
    let wrap_mode = viewport.text_wrap_mode();
    let max_width = viewport.max_text_width();

    for (p_idx, meta) in buffer.paragraphs().iter().enumerate() {
        if is_hidden(p_idx) {
            continue;
        }
        let Some(paragraph) = buffer.paragraph_text(p_idx) else {
            continue;
        };
        let trimmed = paragraph.trim_end_matches('\n').to_string();
        let key = LayoutCacheKey::new(
            p_idx,
            meta.revision,
            &trimmed,
            text_style,
            wrap_mode,
            max_width,
        );
        let layout = layout_cache.layout_paragraph(
            key,
            &trimmed,
            text_style,
            max_width,
            wrap_mode,
            text_system,
        );
        let metrics = if let Some(metrics) = metrics_cache.get(&key) {
            stats.hits += 1;
            metrics
        } else {
            stats.misses += 1;
            let metrics = ParagraphMetrics {
                width: layout.width(),
                height: layout_visual_height(&layout, style),
                line_count: layout.lines.len().max(1),
                local_version: meta.revision,
            };
            metrics_cache.insert(key, metrics);
            metrics
        };
        content_width = content_width.max(metrics.width);
        let para_bottom = para_top + metrics.height;
        if para_bottom >= viewport_top && para_top <= viewport_bot {
            runs.push(EditorTextRun {
                index: p_idx,
                byte_range: meta.byte_range.start..(meta.byte_range.start + trimmed.len()),
                baseline_y: para_top - viewport_top + first_layout_baseline(&layout),
                layout,
            });
        }
        para_top = para_bottom;
    }

    metrics_cache.update_metadata(
        buffer.paragraphs().len(),
        buffer.paragraphs().iter().map(|meta| meta.revision),
        para_top,
        content_width,
    );

    VisibleTextLayout {
        runs,
        content_size: Size::new(content_width, para_top),
        metrics_stats: stats,
        used_fast_path: false,
    }
}

fn build_visible_nowrap_text_layout(
    buffer: &TextBuffer,
    viewport: EditorViewport,
    style: EditorStyle,
    layout_cache: &mut LayoutCache,
    metrics_cache: &mut ParagraphMetricsCache,
    text_system: &mut TextSystem,
) -> VisibleTextLayout {
    let text_style = TextStyle::new(style.font, style.px_size, style.fg);
    let line_height = style.line_height.max(1.0);
    let paragraph_count = buffer.paragraphs().len();
    let viewport_top = viewport.scroll_y.max(0.0);
    let viewport_bot = viewport_top + viewport.content_rect.h.max(0.0);
    let heights = vec![line_height; paragraph_count];
    let height_index = FenwickTree::from_values(&heights);
    let first = height_index.lower_bound(viewport_top).min(paragraph_count);
    let last = (height_index.lower_bound(viewport_bot) + 2).min(paragraph_count);
    let mut runs = Vec::new();
    let max_width = viewport.max_text_width();
    let wrap_mode = viewport.text_wrap_mode();
    let mut stats = ParagraphMetricsStats::default();

    // In NoWrap mode horizontal scroll metrics must account for the whole
    // document, including the longest line when it sits outside the viewport,
    // without shaping every paragraph and losing the large-file fast path.
    let mut content_width = 0.0f32;
    for p_idx in 0..paragraph_count {
        if let Some(paragraph) = buffer.paragraph_text(p_idx) {
            content_width = content_width.max(estimate_nowrap_width(
                paragraph.trim_end_matches('\n'),
                style,
            ));
        }
    }

    for p_idx in first.min(paragraph_count)..last {
        let meta = &buffer.paragraphs()[p_idx];
        let Some(paragraph) = buffer.paragraph_text(p_idx) else {
            continue;
        };
        let trimmed = paragraph.trim_end_matches('\n').to_string();
        let key = LayoutCacheKey::new(
            p_idx,
            meta.revision,
            &trimmed,
            text_style,
            wrap_mode,
            max_width,
        );
        let layout = layout_cache.layout_paragraph(
            key,
            &trimmed,
            text_style,
            max_width,
            wrap_mode,
            text_system,
        );
        let metrics = if let Some(metrics) = metrics_cache.get(&key) {
            stats.hits += 1;
            metrics
        } else {
            stats.misses += 1;
            let metrics = ParagraphMetrics {
                width: layout.width(),
                height: layout_visual_height(&layout, style),
                line_count: 1,
                local_version: meta.revision,
            };
            metrics_cache.insert(key, metrics);
            metrics
        };
        content_width = content_width.max(metrics.width);
        runs.push(EditorTextRun {
            index: p_idx,
            byte_range: meta.byte_range.start..(meta.byte_range.start + trimmed.len()),
            baseline_y: p_idx as f32 * line_height - viewport_top + first_layout_baseline(&layout),
            layout,
        });
    }
    metrics_cache.update_metadata(
        paragraph_count,
        buffer.paragraphs().iter().map(|meta| meta.revision),
        paragraph_count as f32 * line_height,
        content_width,
    );

    VisibleTextLayout {
        runs,
        content_size: Size::new(content_width, paragraph_count as f32 * line_height),
        metrics_stats: stats,
        used_fast_path: true,
    }
}

fn estimate_nowrap_width(text: &str, style: EditorStyle) -> f32 {
    text.chars().count() as f32 * style.px_size as f32 * 0.62
}
