//! Cached frame construction, hit testing, caret geometry, and scroll metrics.

use ailloli_ui_core::scroll::{
    ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometry, ScrollbarGeometrySpec,
};
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

/// UI-agnostic editor engine for cached layout, frame production, and hit tests.
///
/// One engine retains unbounded per-paragraph layout/metrics maps, the last
/// complete frame, and a wrapping frame counter. It is not synchronized for
/// concurrent use and should normally be owned by one editor view.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::EditorEngine;
/// let mut engine = EditorEngine::new();
/// engine.clear_caches();
/// ```
#[derive(Debug, Clone, Default)]
pub struct EditorEngine {
    /// Prepared paragraph layouts keyed by geometry-affecting inputs.
    layout_cache: LayoutCache,
    /// Paragraph dimensions and aggregate revision metadata.
    metrics_cache: ParagraphMetricsCache,
    /// Most recently returned frame, including a cloned set of layout handles.
    last_frame: Option<EditorFrame>,
    /// Wrapping counter incremented before each completed frame.
    frame_id: u64,
}

/// Optional code-editor layers supplied to the shared frame pipeline.
struct FrameExtras<'a> {
    /// Code theme and whether line numbers should be painted.
    code_theme: Option<(CodeTheme, bool)>,
    /// Optional editor-owned scrollbar configuration.
    scrollbars: Option<EditorScrollbarConfig>,
    /// Search, diagnostic, and active-diagnostic paint inputs.
    code_decorations: Option<(
        &'a crate::code::SearchState,
        &'a [crate::code::Diagnostic],
        Option<usize>,
    )>,
    /// Optional source-buffer syntax tokens.
    syntax_tokens: Option<&'a [crate::code::SyntaxToken]>,
    /// Optional fold ranges used for hiding and gutter paint.
    fold_regions: Option<&'a [crate::code::FoldRegion]>,
}

/// Constructs frame extras for a generic editor.
impl<'a> FrameExtras<'a> {
    /// Returns an extras bundle with every code layer disabled.
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

/// Produces frames and queries their retained geometry.
impl EditorEngine {
    /// Creates an empty engine with frame counter zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::EditorEngine;
    /// let engine = EditorEngine::new();
    /// assert_eq!(format!("{engine:?}").contains("frame_id: 0"), true);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears paragraph layout and metric caches.
    ///
    /// The retained last frame, wrapping frame ID, and the caller-owned
    /// [`TextSystem`] cache are not cleared.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::EditorEngine;
    /// let mut engine = EditorEngine::new();
    /// engine.clear_caches();
    /// ```
    pub fn clear_caches(&mut self) {
        self.layout_cache.clear();
        self.metrics_cache.clear();
    }

    /// Builds a generic editor frame at blink time zero.
    ///
    /// A focused caret is therefore in its visible phase. The returned frame is
    /// cloned into the engine as its only cached frame, and its ID increments
    /// with wrapping arithmetic.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_editor::{EditorEngine, EditorSession};
    /// use ailloli_ui_text::{TextBuffer, TextSystem};
    /// let session = EditorSession::new(TextBuffer::from_string("hello"));
    /// let bounds = Rect::new(0.0, 0.0, 100.0, 50.0);
    /// let frame = EditorEngine::new().frame(&session, bounds, true, &mut TextSystem::new());
    /// assert_eq!(frame.viewport.bounds, bounds);
    /// assert_eq!(frame.debug_metrics.frame_id, 1);
    /// ```
    pub fn frame(
        &mut self,
        session: &EditorSession,
        bounds: Rect,
        focused: bool,
        text_system: &mut TextSystem,
    ) -> EditorFrame {
        self.frame_at(session, bounds, focused, 0, text_system)
    }

    /// Builds a generic frame at an explicit millisecond blink time.
    ///
    /// `frame_time_ms` controls only caret blink phase. Layout timing is measured
    /// independently in microseconds and saturates at [`u64::MAX`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_editor::{EditorEngine, EditorPaintItem, EditorSession};
    /// use ailloli_ui_text::{TextBuffer, TextSystem};
    /// let session = EditorSession::new(TextBuffer::from_string("x"));
    /// let frame = EditorEngine::new().frame_at(&session, Rect::new(0.0, 0.0, 80.0, 40.0), true, 500, &mut TextSystem::new());
    /// assert!(!frame.paint_items.iter().any(|item| matches!(item, EditorPaintItem::Caret { .. })));
    /// ```
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

    /// Builds a code-editor frame at blink time zero.
    ///
    /// The engine consumes the session's current tokens, folds, search results,
    /// and diagnostics without refreshing them. It adds the configured gutter,
    /// code layers, and enabled scrollbars to the generic paint model.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, EditorEngine};
    /// use ailloli_ui_text::{TextBuffer, TextSystem};
    /// let session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("code")), CodeEditorConfig::default());
    /// let frame = EditorEngine::new().code_frame(&session, Rect::new(0.0, 0.0, 140.0, 60.0), false, &mut TextSystem::new());
    /// assert!(frame.viewport.gutter_rect.is_some());
    /// ```
    pub fn code_frame(
        &mut self,
        session: &CodeEditorSession,
        bounds: Rect,
        focused: bool,
        text_system: &mut TextSystem,
    ) -> EditorFrame {
        self.code_frame_at(session, bounds, focused, 0, text_system)
    }

    /// Builds a code-editor frame at an explicit millisecond blink time.
    ///
    /// Gutter fold markers control whether folded lines are filtered at all;
    /// other [`crate::CodeEditorFeatureFlags`] are descriptive configuration and
    /// are not consulted by this frame builder.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, EditorEngine};
    /// use ailloli_ui_text::{TextBuffer, TextSystem};
    /// let session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("x")), CodeEditorConfig::default());
    /// let frame = EditorEngine::new().code_frame_at(&session, Rect::new(0.0, 0.0, 140.0, 60.0), true, 0, &mut TextSystem::new());
    /// assert_eq!(frame.debug_metrics.frame_id, 1);
    /// ```
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

    /// Runs the common layout and ordered paint-layer pipeline.
    ///
    /// IME preedit text is projected into a temporary display buffer. Collapsed
    /// folds remove hidden logical lines before hit testing and paint. The frame
    /// always begins with a background item; subsequent gutter, active line,
    /// selection, decorations, text, fold labels, carets, and scrollbars follow
    /// deterministic layer order.
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

    /// Builds a fresh unfocused frame and hit-tests a screen point.
    ///
    /// The new frame replaces `last_frame`. The returned byte is clamped to the
    /// source buffer and follows shaped caret geometry; points outside visible
    /// runs fall back according to [`crate::input::hit_test::byte_at_point`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Point, Rect};
    /// use ailloli_ui_editor::{EditorEngine, EditorSession};
    /// use ailloli_ui_text::{TextBuffer, TextSystem};
    /// let session = EditorSession::new(TextBuffer::from_string("abc"));
    /// let hit = EditorEngine::new().hit_test(&session, Rect::new(0.0, 0.0, 100.0, 50.0), Point::new(12.0, 12.0), &mut TextSystem::new());
    /// assert!(hit.byte <= 3);
    /// ```
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

    /// Hit-tests fold markers in the retained frame.
    ///
    /// A cached frame is eligible when every bounds component differs by less
    /// than `0.5` logical pixel. The first marker rectangle containing the point
    /// returns its fold-region slice index. No frame, mismatched bounds, or no
    /// marker returns `None`; session identity is not part of the cache key.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Point, Rect};
    /// use ailloli_ui_editor::EditorEngine;
    /// assert_eq!(EditorEngine::new().fold_region_hit_test_cached(Rect::new(0.0, 0.0, 10.0, 10.0), Point::new(1.0, 1.0)), None);
    /// ```
    pub fn fold_region_hit_test_cached(&self, bounds: Rect, pos: Point) -> Option<usize> {
        let frame = self.last_frame_for_bounds(bounds)?;
        frame.paint_items.iter().find_map(|item| match item {
            EditorPaintItem::FoldGutterMarker {
                rect, region_index, ..
            } if rect.contains(pos.x, pos.y) => Some(*region_index),
            _ => None,
        })
    }

    /// Hit-tests text using the retained frame when bounds approximately match.
    ///
    /// Cache matching ignores session identity and configuration. If no eligible
    /// frame exists, the fallback byte is exactly `session.buffer.len_bytes()`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Point, Rect};
    /// use ailloli_ui_editor::{EditorEngine, EditorSession};
    /// use ailloli_ui_text::TextBuffer;
    /// let session = EditorSession::new(TextBuffer::from_string("abc"));
    /// let hit = EditorEngine::new().hit_test_cached(&session, Rect::new(0.0, 0.0, 100.0, 50.0), Point::new(0.0, 0.0));
    /// assert_eq!(hit.byte, 3);
    /// ```
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

    /// Hit-tests gutter/text/outside zone using an approximately matching frame.
    ///
    /// Without one, returns [`crate::EditorHitZone::Outside`] at the source-buffer
    /// end. Cache matching ignores session identity and configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Point, Rect};
    /// use ailloli_ui_editor::{EditorEngine, EditorHitZone, EditorSession};
    /// use ailloli_ui_text::TextBuffer;
    /// let session = EditorSession::new(TextBuffer::from_string("abc"));
    /// let hit = EditorEngine::new().hit_test_zone_cached(&session, Rect::new(0.0, 0.0, 100.0, 50.0), Point::new(0.0, 0.0));
    /// assert_eq!((hit.zone, hit.byte), (EditorHitZone::Outside, 3));
    /// ```
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

    /// Builds a fresh focused frame and returns display-buffer caret geometry.
    ///
    /// IME preedit projection is included. The new frame replaces `last_frame`;
    /// the returned rectangle is in screen-space logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_editor::{EditorEngine, EditorSession};
    /// use ailloli_ui_text::{TextBuffer, TextSystem};
    /// let session = EditorSession::new(TextBuffer::from_string("abc"));
    /// let caret = EditorEngine::new().caret_rect(&session, Rect::new(0.0, 0.0, 100.0, 50.0), &mut TextSystem::new());
    /// assert_eq!(caret.w, 1.0);
    /// assert!(caret.h > 0.0);
    /// ```
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

    /// Returns generic caret geometry from an approximately matching frame.
    ///
    /// Without a frame, returns a one-logical-pixel caret at the text origin with
    /// height `px_size + 2`. Cache matching ignores session identity/configuration
    /// beyond the supplied bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_editor::{EditorEngine, EditorSession};
    /// use ailloli_ui_text::TextBuffer;
    /// let session = EditorSession::new(TextBuffer::new());
    /// let caret = EditorEngine::new().caret_rect_cached(&session, Rect::new(0.0, 0.0, 100.0, 50.0));
    /// assert_eq!((caret.x, caret.y, caret.w, caret.h), (10.0, 10.0, 1.0, 15.0));
    /// ```
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

    /// Returns code-editor caret geometry from an approximately matching frame.
    ///
    /// The no-frame fallback accounts for the configured gutter and otherwise
    /// uses the generic one-pixel, `px_size + 2` caret. Cached frame matching uses
    /// only approximate bounds, not session identity.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, EditorEngine};
    /// use ailloli_ui_text::TextBuffer;
    /// let session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::new()), CodeEditorConfig::default());
    /// let caret = EditorEngine::new().code_caret_rect_cached(&session, Rect::new(0.0, 0.0, 100.0, 50.0));
    /// assert_eq!((caret.x, caret.y, caret.w, caret.h), (58.0, 10.0, 1.0, 15.0));
    /// ```
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

    /// Returns generic text-viewport/content scroll metrics from the cached frame.
    ///
    /// With no approximately matching frame, content equals viewport and the
    /// maximum offset is zero. The viewport excludes editor padding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Offset, Rect, Size};
    /// use ailloli_ui_editor::{EditorEngine, EditorSession};
    /// use ailloli_ui_text::TextBuffer;
    /// let session = EditorSession::new(TextBuffer::new());
    /// let metrics = EditorEngine::new().scroll_metrics_cached(&session, Rect::new(0.0, 0.0, 100.0, 50.0));
    /// assert_eq!(metrics.viewport, Size::new(80.0, 30.0));
    /// assert_eq!(metrics.max_offset(), Offset::new(0.0, 0.0));
    /// ```
    pub fn scroll_metrics_cached(&self, session: &EditorSession, bounds: Rect) -> ScrollMetrics {
        let viewport = crate::EditorViewport::new(bounds, session.config, &session.edit);
        let viewport_size = Size::new(viewport.text_rect.w, viewport.text_rect.h);
        let content_size = self
            .last_frame_for_bounds(bounds)
            .map(|frame| frame.content_size)
            .unwrap_or(viewport_size);
        ScrollMetrics::new(viewport_size, content_size)
    }

    /// Returns code text-viewport/content metrics from the cached frame.
    ///
    /// The viewport excludes both padding and the configured gutter. With no
    /// approximately matching frame, content equals viewport.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size};
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, EditorEngine};
    /// use ailloli_ui_text::TextBuffer;
    /// let session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::new()), CodeEditorConfig::default());
    /// let metrics = EditorEngine::new().code_scroll_metrics_cached(&session, Rect::new(0.0, 0.0, 100.0, 50.0));
    /// assert_eq!(metrics.viewport, Size::new(32.0, 30.0));
    /// assert_eq!(metrics.content, metrics.viewport);
    /// ```
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

    /// Returns the retained frame when bounds components differ by under 0.5.
    fn last_frame_for_bounds(&self, bounds: Rect) -> Option<&EditorFrame> {
        let frame = self.last_frame.as_ref()?;
        if same_rect(frame.viewport.bounds, bounds) {
            Some(frame)
        } else {
            None
        }
    }
}

/// Compares rectangle components with a strict half-logical-pixel tolerance.
fn same_rect(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() < 0.5
        && (a.y - b.y).abs() < 0.5
        && (a.w - b.w).abs() < 0.5
        && (a.h - b.h).abs() < 0.5
}

/// Resolves the code editor's visible scrollbar geometry.
///
/// The returned rectangles are the single source of truth used by adapters for
/// paint and pointer interaction. Horizontal geometry is omitted while wrapping
/// is enabled, and either axis is omitted when it has no usable overflow.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Rect, Size, ScrollbarAxis};
/// use ailloli_ui_editor::{code_scrollbar_geometries, CodeEditorConfig, EditorViewport};
/// use ailloli_ui_text::TextEditState;
/// let config = CodeEditorConfig::default();
/// let viewport = EditorViewport::with_gutter(
///     Rect::new(0.0, 0.0, 240.0, 120.0),
///     config.editor,
///     &TextEditState::new(),
///     Some(config.gutter),
/// );
/// let bars = code_scrollbar_geometries(viewport, Size::new(600.0, 400.0), config.scrollbars);
/// assert!(bars.iter().any(|bar| bar.axis == ScrollbarAxis::Horizontal));
/// assert!(bars.iter().any(|bar| bar.axis == ScrollbarAxis::Vertical));
/// ```
pub fn code_scrollbar_geometries(
    viewport: EditorViewport,
    content_size: Size,
    config: EditorScrollbarConfig,
) -> Vec<ScrollbarGeometry> {
    if !config.enabled {
        return Vec::new();
    }
    let metrics = ScrollMetrics::new(
        Size::new(viewport.text_rect.w, viewport.text_rect.h),
        content_size,
    );
    let max_offset = metrics.max_offset();
    let show_vertical = max_offset.y > 0.5;
    let show_horizontal = viewport.wrap_mode == EditorWrapMode::NoWrap && max_offset.x > 0.5;
    let state = ScrollState::with_offset(Offset::new(viewport.scroll_x, viewport.scroll_y));
    let style = config.style;
    let mut geometries = Vec::with_capacity(2);

    if show_vertical {
        let bottom_reserve = if show_horizontal {
            style.thickness + style.inset
        } else {
            0.0
        };
        if let Some(geometry) =
            ScrollbarGeometrySpec::new(ScrollbarAxis::Vertical, viewport.text_rect, metrics, state)
                .with_paint_metrics(style.thickness, style.min_thumb_len, style.inset)
                .with_end_reserve(bottom_reserve)
                .resolve()
        {
            geometries.push(geometry);
        }
    }

    if show_horizontal {
        let right_reserve = if show_vertical {
            style.thickness + style.inset
        } else {
            0.0
        };
        if let Some(geometry) = ScrollbarGeometrySpec::new(
            ScrollbarAxis::Horizontal,
            viewport.text_rect,
            metrics,
            state,
        )
        .with_paint_metrics(style.thickness, style.min_thumb_len, style.inset)
        .with_end_reserve(right_reserve)
        .resolve()
        {
            geometries.push(geometry);
        }
    }

    geometries
}

/// Builds vertical then horizontal scrollbar items for overflowing axes.
fn scrollbar_items_for_viewport(
    viewport: EditorViewport,
    content_size: Size,
    config: EditorScrollbarConfig,
) -> Vec<EditorPaintItem> {
    code_scrollbar_geometries(viewport, content_size, config)
        .into_iter()
        .map(|geometry| scrollbar_item(geometry.track, geometry.thumb, config.style))
        .collect()
}

/// Converts resolved scrollbar geometry into a neutral paint item.
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
