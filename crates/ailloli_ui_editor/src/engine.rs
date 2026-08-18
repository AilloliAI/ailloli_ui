use ailloli_ui_core::scroll::ScrollMetrics;
use ailloli_ui_core::{Offset, Point, Rect, Size};
use ailloli_ui_text::TextSystem;
use std::time::Instant;

use crate::code::CodeEditorSession;
use crate::input::caret::caret_rect_for_runs;
use crate::input::hit_test::{byte_at_point, zone_byte_at_point, EditorHitTest, EditorZoneHitTest};
use crate::input::ime::display_buffer_for_edit;
use crate::layout::{
    build_visible_text_layout, build_visible_text_layout_filtered, LayoutCache,
    ParagraphMetricsCache,
};
use crate::paint::active_line_painter::{active_line_index_for_caret, active_line_item_for_caret};
use crate::paint::caret_painter::{caret_item, caret_visible};
use crate::paint::code_decorations_painter::{
    diagnostic_underline_items_for_run, search_highlight_items_for_run,
};
use crate::paint::folding_painter::fold_placeholder_item_for_run;
use crate::paint::gutter_painter::{
    diagnostic_gutter_marker_items, fold_gutter_marker_items, gutter_background_item,
    line_number_items,
};
use crate::paint::selection_painter::selection_items_for_run;
use crate::paint::syntax_painter::syntax_text_items_for_run;
use crate::paint::text_painter::text_item;
use crate::{
    CodeTheme, EditorFrame, EditorFrameDebugMetrics, EditorPaintItem, EditorScrollbarConfig,
    EditorScrollbarStyle, EditorSession, EditorViewport, EditorWrapMode,
};

/// UI-agnostic editor engine: layout cache, metrics cache, frame production, hit-test.
#[derive(Debug, Clone, Default)]
pub struct EditorEngine {
    layout_cache: LayoutCache,
    metrics_cache: ParagraphMetricsCache,
    last_frame: Option<EditorFrame>,
    frame_id: u64,
}

struct FrameExtras<'a> {
    code_theme: Option<(CodeTheme, bool)>,
    scrollbars: Option<EditorScrollbarConfig>,
    code_decorations: Option<(
        &'a crate::code::SearchState,
        &'a [crate::code::Diagnostic],
        Option<usize>,
    )>,
    syntax_tokens: Option<&'a [crate::code::SyntaxToken]>,
    fold_regions: Option<&'a [crate::code::FoldRegion]>,
}

impl<'a> FrameExtras<'a> {
    fn none() -> Self {
        Self {
            code_theme: None,
            scrollbars: None,
            code_decorations: None,
            syntax_tokens: None,
            fold_regions: None,
        }
    }
}

impl EditorEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear_caches(&mut self) {
        self.layout_cache.clear();
        self.metrics_cache.clear();
    }

    pub fn frame(
        &mut self,
        session: &EditorSession,
        bounds: Rect,
        focused: bool,
        text_system: &mut TextSystem,
    ) -> EditorFrame {
        self.frame_at(session, bounds, focused, 0, text_system)
    }

    pub fn frame_at(
        &mut self,
        session: &EditorSession,
        bounds: Rect,
        focused: bool,
        frame_time_ms: u128,
        text_system: &mut TextSystem,
    ) -> EditorFrame {
        let viewport = EditorViewport::new(bounds, session.config, &session.edit);
        self.frame_for_viewport(
            session,
            viewport,
            focused,
            frame_time_ms,
            FrameExtras::none(),
            text_system,
        )
    }

    pub fn code_frame(
        &mut self,
        session: &CodeEditorSession,
        bounds: Rect,
        focused: bool,
        text_system: &mut TextSystem,
    ) -> EditorFrame {
        self.code_frame_at(session, bounds, focused, 0, text_system)
    }

    pub fn code_frame_at(
        &mut self,
        session: &CodeEditorSession,
        bounds: Rect,
        focused: bool,
        frame_time_ms: u128,
        text_system: &mut TextSystem,
    ) -> EditorFrame {
        let viewport = EditorViewport::with_gutter(
            bounds,
            session.editor.config,
            &session.editor.edit,
            Some(session.config.gutter),
        );
        self.frame_for_viewport(
            &session.editor,
            viewport,
            focused,
            frame_time_ms,
            FrameExtras {
                code_theme: Some((session.config.theme, session.config.gutter.line_numbers)),
                scrollbars: Some(session.config.scrollbars),
                code_decorations: Some((
                    &session.search,
                    &session.diagnostics,
                    session.active_diagnostic_index,
                )),
                syntax_tokens: Some(&session.syntax_tokens),
                fold_regions: session
                    .config
                    .gutter
                    .fold_markers
                    .then_some(session.fold_regions.as_slice()),
            },
            text_system,
        )
    }

    fn frame_for_viewport(
        &mut self,
        session: &EditorSession,
        viewport: EditorViewport,
        focused: bool,
        frame_time_ms: u128,
        extras: FrameExtras<'_>,
        text_system: &mut TextSystem,
    ) -> EditorFrame {
        let display = display_buffer_for_edit(&session.buffer, &session.edit);
        // Folding can reuse logical line indices from visible runs for now:
        // hidden lines are filtered before paint/hit-test, and gutter markers
        // carry the fold region index directly. A separate VisualLineMap is
        // intentionally deferred until folded placeholders need independent
        // cursor navigation or multi-viewport mappings.
        let layout_start = Instant::now();
        let visible =
            if let Some(fold_regions) = extras.fold_regions.filter(|regions| !regions.is_empty()) {
                build_visible_text_layout_filtered(
                    &display.buffer,
                    viewport,
                    session.config.style,
                    &mut self.layout_cache,
                    &mut self.metrics_cache,
                    text_system,
                    &|line| fold_regions.iter().any(|region| region.hides_line(line)),
                )
            } else {
                build_visible_text_layout(
                    &display.buffer,
                    viewport,
                    session.config.style,
                    &mut self.layout_cache,
                    &mut self.metrics_cache,
                    text_system,
                )
            };
        let visible_layout_us = layout_start.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        self.frame_id = self.frame_id.wrapping_add(1);
        let runs = visible.runs;
        let mut paint_items = Vec::new();
        let style = session.config.style;
        let active_line_index = focused
            .then(|| active_line_index_for_caret(&runs, display.caret_byte))
            .flatten();
        paint_items.push(EditorPaintItem::Background {
            rect: viewport.bounds,
            color: style.bg,
        });
        if let Some((theme, line_numbers)) = extras.code_theme {
            if let Some(gutter) = gutter_background_item(viewport, theme) {
                paint_items.push(gutter);
            }
            if line_numbers {
                paint_items.extend(line_number_items(
                    viewport,
                    &runs,
                    style,
                    theme,
                    active_line_index,
                    text_system,
                ));
            }
            if let Some((_, diagnostics, _)) = extras.code_decorations {
                paint_items.extend(diagnostic_gutter_marker_items(
                    viewport,
                    &runs,
                    diagnostics,
                    theme,
                ));
            }
            if let Some(fold_regions) = extras.fold_regions {
                paint_items.extend(fold_gutter_marker_items(
                    viewport,
                    &runs,
                    fold_regions,
                    style,
                    theme,
                ));
            }
        }

        let content_x = viewport.text_origin_x();
        let content_y = viewport.text_origin_y();
        let mut painted_paragraphs = Vec::new();
        for run in &runs {
            if focused && Some(run.index) == active_line_index {
                if let Some((theme, _)) = extras.code_theme {
                    if let Some(item) = active_line_item_for_caret(
                        viewport,
                        &runs,
                        display.caret_byte,
                        style,
                        theme,
                    ) {
                        paint_items.push(item);
                    }
                }
            }

            if let Some(selection) = session.edit.selection {
                let (lo, hi) = selection.normalized();
                if hi > run.byte_range.start && lo < run.byte_range.end {
                    let text_len = run.layout.text().len();
                    let lo_local = lo.saturating_sub(run.byte_range.start).min(text_len);
                    let hi_local = hi.saturating_sub(run.byte_range.start).min(text_len);
                    if hi_local > lo_local {
                        paint_items.extend(selection_items_for_run(
                            content_x, content_y, run, lo_local, hi_local, style,
                        ));
                    }
                }
            }

            if let Some((search, diagnostics, active_diagnostic_index)) = extras.code_decorations {
                let theme = extras
                    .code_theme
                    .map(|(theme, _)| theme)
                    .unwrap_or_default();
                paint_items.extend(search_highlight_items_for_run(
                    content_x, content_y, run, search, style, theme,
                ));
                paint_items.extend(diagnostic_underline_items_for_run(
                    content_x,
                    content_y,
                    run,
                    diagnostics,
                    active_diagnostic_index,
                    style,
                    theme,
                ));
            }

            if let Some(tokens) = extras.syntax_tokens.filter(|tokens| !tokens.is_empty()) {
                paint_items.extend(syntax_text_items_for_run(
                    content_x,
                    content_y,
                    run,
                    tokens,
                    style,
                    extras
                        .code_theme
                        .map(|(theme, _)| theme)
                        .unwrap_or_default(),
                    text_system,
                ));
            } else {
                paint_items.push(text_item(content_x, content_y, run, style));
            }
            if let (Some(fold_regions), Some((theme, _))) = (extras.fold_regions, extras.code_theme)
            {
                if let Some(item) = fold_placeholder_item_for_run(
                    content_x,
                    content_y,
                    run,
                    fold_regions,
                    style,
                    theme,
                    text_system,
                ) {
                    paint_items.push(item);
                }
            }
            painted_paragraphs.push(run.index);

            if caret_visible(focused, frame_time_ms, style.caret_blink_ms)
                && run.byte_range.start <= display.caret_byte
                && display.caret_byte <= run.byte_range.end
            {
                let local = display
                    .caret_byte
                    .saturating_sub(run.byte_range.start)
                    .min(run.layout.text().len());
                paint_items.push(caret_item(content_x, content_y, run, local, style));
            }
        }

        if let Some(scrollbars) = extras.scrollbars.filter(|scrollbars| scrollbars.enabled) {
            paint_items.extend(scrollbar_items_for_viewport(
                viewport,
                visible.content_size,
                scrollbars,
            ));
        }

        let paint_item_count = paint_items.len();
        let glyph_upload_count = runs
            .iter()
            .map(|run| run.layout.glyphs().len())
            .sum::<usize>();
        let frame = EditorFrame {
            viewport,
            content_size: visible.content_size,
            runs,
            paint_items,
            painted_paragraphs,
            debug_metrics: EditorFrameDebugMetrics {
                frame_id: self.frame_id,
                visible_layout_us,
                paragraph_cache_hits: visible.metrics_stats.hits,
                paragraph_cache_misses: visible.metrics_stats.misses,
                syntax_parse_us: 0,
                symbol_index_us: 0,
                paint_item_count,
                glyph_upload_count,
                used_fast_path: visible.used_fast_path,
            },
        };
        self.last_frame = Some(frame.clone());
        frame
    }

    pub fn hit_test(
        &mut self,
        session: &EditorSession,
        bounds: Rect,
        pos: Point,
        text_system: &mut TextSystem,
    ) -> EditorHitTest {
        let frame = self.frame(session, bounds, false, text_system);
        byte_at_point(
            frame.viewport,
            &frame.runs,
            session.config.style,
            session.buffer.len_bytes(),
            pos,
        )
    }

    pub fn fold_region_hit_test_cached(&self, bounds: Rect, pos: Point) -> Option<usize> {
        let frame = self.last_frame_for_bounds(bounds)?;
        frame.paint_items.iter().find_map(|item| match item {
            EditorPaintItem::FoldGutterMarker {
                rect, region_index, ..
            } if rect.contains(pos.x, pos.y) => Some(*region_index),
            _ => None,
        })
    }

    pub fn hit_test_cached(
        &self,
        session: &EditorSession,
        bounds: Rect,
        pos: Point,
    ) -> EditorHitTest {
        let Some(frame) = self.last_frame_for_bounds(bounds) else {
            return EditorHitTest {
                byte: session.buffer.len_bytes(),
            };
        };
        byte_at_point(
            frame.viewport,
            &frame.runs,
            session.config.style,
            session.buffer.len_bytes(),
            pos,
        )
    }

    pub fn hit_test_zone_cached(
        &self,
        session: &EditorSession,
        bounds: Rect,
        pos: Point,
    ) -> EditorZoneHitTest {
        let Some(frame) = self.last_frame_for_bounds(bounds) else {
            return EditorZoneHitTest {
                zone: crate::input::hit_test::EditorHitZone::Outside,
                byte: session.buffer.len_bytes(),
            };
        };
        zone_byte_at_point(
            frame.viewport,
            &frame.runs,
            session.config.style,
            session.buffer.len_bytes(),
            pos,
        )
    }

    pub fn caret_rect(
        &mut self,
        session: &EditorSession,
        bounds: Rect,
        text_system: &mut TextSystem,
    ) -> Rect {
        let frame = self.frame(session, bounds, true, text_system);
        let display = display_buffer_for_edit(&session.buffer, &session.edit);
        caret_rect_for_runs(
            frame.viewport,
            &frame.runs,
            display.caret_byte,
            session.config.style,
        )
    }

    pub fn caret_rect_cached(&self, session: &EditorSession, bounds: Rect) -> Rect {
        let viewport = crate::EditorViewport::new(bounds, session.config, &session.edit);
        let Some(frame) = self.last_frame_for_bounds(bounds) else {
            return Rect::new(
                viewport.text_origin_x(),
                viewport.text_origin_y(),
                crate::input::caret::EDITOR_CARET_WIDTH,
                session.config.style.px_size as f32 + 2.0,
            );
        };
        let display = display_buffer_for_edit(&session.buffer, &session.edit);
        caret_rect_for_runs(
            frame.viewport,
            &frame.runs,
            display.caret_byte,
            session.config.style,
        )
    }

    pub fn code_caret_rect_cached(&self, session: &CodeEditorSession, bounds: Rect) -> Rect {
        let viewport = crate::EditorViewport::with_gutter(
            bounds,
            session.editor.config,
            &session.editor.edit,
            Some(session.config.gutter),
        );
        let Some(frame) = self.last_frame_for_bounds(bounds) else {
            return Rect::new(
                viewport.text_origin_x(),
                viewport.text_origin_y(),
                crate::input::caret::EDITOR_CARET_WIDTH,
                session.editor.config.style.px_size as f32 + 2.0,
            );
        };
        let display = display_buffer_for_edit(&session.editor.buffer, &session.editor.edit);
        caret_rect_for_runs(
            frame.viewport,
            &frame.runs,
            display.caret_byte,
            session.editor.config.style,
        )
    }

    pub fn scroll_metrics_cached(&self, session: &EditorSession, bounds: Rect) -> ScrollMetrics {
        let viewport = crate::EditorViewport::new(bounds, session.config, &session.edit);
        let viewport_size = Size::new(viewport.text_rect.w, viewport.text_rect.h);
        let content_size = self
            .last_frame_for_bounds(bounds)
            .map(|frame| frame.content_size)
            .unwrap_or(viewport_size);
        ScrollMetrics::new(viewport_size, content_size)
    }

    pub fn code_scroll_metrics_cached(
        &self,
        session: &CodeEditorSession,
        bounds: Rect,
    ) -> ScrollMetrics {
        let viewport = crate::EditorViewport::with_gutter(
            bounds,
            session.editor.config,
            &session.editor.edit,
            Some(session.config.gutter),
        );
        let viewport_size = Size::new(viewport.text_rect.w, viewport.text_rect.h);
        let content_size = self
            .last_frame_for_bounds(bounds)
            .map(|frame| frame.content_size)
            .unwrap_or(viewport_size);
        ScrollMetrics::new(viewport_size, content_size)
    }

    fn last_frame_for_bounds(&self, bounds: Rect) -> Option<&EditorFrame> {
        let frame = self.last_frame.as_ref()?;
        if same_rect(frame.viewport.bounds, bounds) {
            Some(frame)
        } else {
            None
        }
    }
}

fn same_rect(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() < 0.5
        && (a.y - b.y).abs() < 0.5
        && (a.w - b.w).abs() < 0.5
        && (a.h - b.h).abs() < 0.5
}

fn scrollbar_items_for_viewport(
    viewport: EditorViewport,
    content_size: Size,
    config: EditorScrollbarConfig,
) -> Vec<EditorPaintItem> {
    let metrics = ScrollMetrics::new(
        Size::new(viewport.text_rect.w, viewport.text_rect.h),
        content_size,
    );
    let max_offset = metrics.max_offset();
    let show_vertical = max_offset.y > 0.5;
    let show_horizontal = viewport.wrap_mode == EditorWrapMode::NoWrap && max_offset.x > 0.5;
    let mut items = Vec::new();

    if show_vertical {
        let bottom_reserve = if show_horizontal {
            config.style.thickness + config.style.inset
        } else {
            0.0
        };
        if let Some((track_rect, thumb_rect)) = vertical_scrollbar_rects(
            viewport.text_rect,
            metrics,
            Offset::new(viewport.scroll_x, viewport.scroll_y),
            config.style,
            max_offset.y,
            bottom_reserve,
        ) {
            items.push(scrollbar_item(track_rect, thumb_rect, config.style));
        }
    }

    if show_horizontal {
        let right_reserve = if show_vertical {
            config.style.thickness + config.style.inset
        } else {
            0.0
        };
        if let Some((track_rect, thumb_rect)) = horizontal_scrollbar_rects(
            viewport.text_rect,
            metrics,
            Offset::new(viewport.scroll_x, viewport.scroll_y),
            config.style,
            max_offset.x,
            right_reserve,
        ) {
            items.push(scrollbar_item(track_rect, thumb_rect, config.style));
        }
    }

    items
}

fn scrollbar_item(
    track_rect: Rect,
    thumb_rect: Rect,
    style: EditorScrollbarStyle,
) -> EditorPaintItem {
    EditorPaintItem::Scrollbar {
        track_rect,
        thumb_rect,
        track_color: style.track_color,
        thumb_color: style.thumb_color,
        radius: style.radius,
    }
}

fn vertical_scrollbar_rects(
    bounds: Rect,
    metrics: ScrollMetrics,
    offset: Offset,
    style: EditorScrollbarStyle,
    max_offset_y: f32,
    bottom_reserve: f32,
) -> Option<(Rect, Rect)> {
    let track_h = bounds.h - style.inset * 2.0 - bottom_reserve;
    if track_h <= style.thickness || metrics.content.h <= 0.0 {
        return None;
    }
    let track = Rect::new(
        bounds.right() - style.inset - style.thickness,
        bounds.y + style.inset,
        style.thickness,
        track_h,
    );
    let ratio = (metrics.viewport.h / metrics.content.h).clamp(0.0, 1.0);
    let thumb_h = (track.h * ratio)
        .max(style.min_thumb_len.min(track.h))
        .min(track.h);
    let travel = (track.h - thumb_h).max(0.0);
    let progress = if max_offset_y > 0.0 {
        (offset.y / max_offset_y).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb = Rect::new(track.x, track.y + travel * progress, track.w, thumb_h);
    Some((track, thumb))
}

fn horizontal_scrollbar_rects(
    bounds: Rect,
    metrics: ScrollMetrics,
    offset: Offset,
    style: EditorScrollbarStyle,
    max_offset_x: f32,
    right_reserve: f32,
) -> Option<(Rect, Rect)> {
    let track_w = bounds.w - style.inset * 2.0 - right_reserve;
    if track_w <= style.thickness || metrics.content.w <= 0.0 {
        return None;
    }
    let track = Rect::new(
        bounds.x + style.inset,
        bounds.bottom() - style.inset - style.thickness,
        track_w,
        style.thickness,
    );
    let ratio = (metrics.viewport.w / metrics.content.w).clamp(0.0, 1.0);
    let thumb_w = (track.w * ratio)
        .max(style.min_thumb_len.min(track.w))
        .min(track.w);
    let travel = (track.w - thumb_w).max(0.0);
    let progress = if max_offset_x > 0.0 {
        (offset.x / max_offset_x).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb = Rect::new(track.x + travel * progress, track.y, thumb_w, track.h);
    Some((track, thumb))
}
