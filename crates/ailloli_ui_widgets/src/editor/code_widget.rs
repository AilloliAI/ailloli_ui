//! Retained runtime implementation of the public document-aware code editor.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use crate::layout::layout_ext::apply_layout_size;
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent, WheelDelta};
use ailloli_ui_core::event::{Event, ImeEvent};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::ScrollBehavior;
use ailloli_ui_core::style::LayoutStyle;
use ailloli_ui_core::{Offset, ScrollbarAxis};
use ailloli_ui_editor::{
    code_scrollbar_geometries, resolve_document_language, CodeEditorConfig, CodeEditorSession,
    CodeFileSummary, Diagnostic, Document, EditorClickZone, EditorEngine, EditorHitZone,
    EditorLanguage, EditorPaintItem, EditorViewport, FoldRegion, SearchQuery,
};
use ailloli_ui_runtime::component::{ComponentNode, Context, Signal, View, Widget};
use ailloli_ui_runtime::input::{ActivationPolicy, EventCtx, FocusPolicy, InputRole};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::Invalidation;
use ailloli_ui_text::{TextEditAction, TextInputMode, TextKeymap, TextSelection};

use super::adapter::paint_editor_frame;
use super::code_builder::DocumentChangeHandler;
use super::widget::{
    apply_caret_scroll_intent, caret_scroll_intent_for_action, evaluate_caret_scroll_intent,
    pointer_selection_scroll_delta, CaretScrollEvaluation, CaretScrollIntent,
};
use crate::scrollbar::{thumb_color_for_state, ScrollbarInteraction};

/// Builder snapshot that creates initial code-editor session and engine state.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{Document, DocumentId};
/// use ailloli_ui_runtime::component::{IntoView, State, View};
/// use ailloli_ui_text::TextBuffer;
/// use ailloli_ui_widgets::editor::CodeEditor;
/// let view: View<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).into_view();
/// let _ = view;
/// ```
pub(crate) struct CodeEditorComponent<A> {
    /// Outer logical sizing policy.
    pub(crate) layout: LayoutStyle,
    /// Caller-owned document state synchronized into the editor session.
    pub(crate) document: Signal<Document>,
    /// Editing, rendering, and feature configuration.
    pub(crate) config: CodeEditorConfig,
    /// Optional language override applied to the document.
    pub(crate) language: Option<EditorLanguage>,
    /// Optional initial horizontal/vertical offsets in logical pixels.
    pub(crate) initial_scroll: Option<(f32, f32)>,
    /// Optional initial search query.
    pub(crate) search_query: Option<SearchQuery>,
    /// Optional active match index within the search result list.
    pub(crate) search_active_match: Option<usize>,
    /// Diagnostics initially projected over the document.
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// Optional active diagnostic index.
    pub(crate) active_diagnostic: Option<usize>,
    /// Optional caller-provided fold regions, replacing derived regions.
    pub(crate) fold_regions: Option<Vec<FoldRegion>>,
    /// Optional precomputed symbol summary retained for feature consumers.
    pub(crate) symbol_summary: Option<CodeFileSummary>,
    /// Optional initial UTF-8 byte anchor/caret pair, clamped to document length.
    pub(crate) initial_selection: Option<(usize, usize)>,
    /// Visible-line safety margin maintained below the revealed caret.
    pub(crate) caret_follow_margin_lines: f32,
    /// Optional callback invoked after edits synchronize the document.
    pub(crate) on_document_change: Option<DocumentChangeHandler<A>>,
}

/// Builds the first session, clamps initial selection, and refreshes enabled models.
impl<A: 'static> ComponentNode<A> for CodeEditorComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let document = document_with_language(self.document.read(), self.language);
        let mut initial_session = CodeEditorSession::new(document, self.config);
        if let Some((x, y)) = self.initial_scroll {
            initial_session.editor.edit.scroll_x = x;
            initial_session.editor.edit.scroll_y = y;
        }
        if let Some((anchor, caret)) = self.initial_selection {
            let len = initial_session.editor.buffer.len_bytes();
            initial_session.editor.edit.selection = Some(TextSelection {
                anchor: anchor.min(len),
                caret: caret.min(len),
            });
            initial_session.editor.edit.caret_byte = caret.min(len);
        }
        if self.config.features.search {
            apply_search_props(
                &mut initial_session,
                self.search_query.clone(),
                self.search_active_match,
            );
        }
        if self.config.features.diagnostics {
            initial_session.set_diagnostics(self.diagnostics.clone());
            initial_session.set_active_diagnostic_index(self.active_diagnostic);
        }
        initial_session.refresh_syntax_tokens();
        if self.config.features.folding {
            apply_fold_regions_prop(&mut initial_session, self.fold_regions.clone());
        }
        let session = context.signal(initial_session);
        let caret_scroll_intent = context.signal_with_invalidation(
            (self.initial_selection.is_some() && self.initial_scroll.is_none())
                .then_some(CaretScrollIntent::RevealNavigation),
            Invalidation::Paint,
        );
        let scrollbar_interaction =
            context.signal_with_invalidation(ScrollbarInteraction::default(), Invalidation::Paint);
        View::leaf(CodeEditorWidget {
            layout: self.layout,
            document: self.document.clone(),
            session,
            engine: Rc::new(RefCell::new(EditorEngine::new())),
            config: self.config,
            language: self.language,
            initial_scroll: self.initial_scroll,
            search_query: self.search_query.clone(),
            search_active_match: self.search_active_match,
            diagnostics: self.diagnostics.clone(),
            active_diagnostic: self.active_diagnostic,
            fold_regions: self.fold_regions.clone(),
            symbol_summary: self.symbol_summary.clone(),
            on_document_change: self.on_document_change.clone(),
            caret_follow_margin_lines: self.caret_follow_margin_lines,
            caret_scroll_intent,
            scrollbar_interaction,
        })
    }
}

/// Stateful leaf that synchronizes props, input, document edits, and painting.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{Document, DocumentId};
/// use ailloli_ui_runtime::component::{IntoView, State, View};
/// use ailloli_ui_text::TextBuffer;
/// use ailloli_ui_widgets::editor::CodeEditor;
/// let view: View<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).into_view();
/// let _ = view;
/// ```
pub(crate) struct CodeEditorWidget<A> {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Caller-owned document state synchronized in both directions.
    document: Signal<Document>,
    /// Retained editing, search, diagnostic, and folding session.
    session: Signal<CodeEditorSession>,
    /// UI-local layout/paint cache shared across event and paint passes.
    engine: Rc<RefCell<EditorEngine>>,
    /// Editing, rendering, and feature configuration.
    config: CodeEditorConfig,
    /// Optional language override applied during prop reconciliation.
    language: Option<EditorLanguage>,
    /// Optional initial scroll offsets used when a new document is installed.
    initial_scroll: Option<(f32, f32)>,
    /// Optional search query reconciled into the session.
    search_query: Option<SearchQuery>,
    /// Optional active search match index.
    search_active_match: Option<usize>,
    /// Diagnostic set reconciled into the session.
    diagnostics: Vec<Diagnostic>,
    /// Optional active diagnostic index.
    active_diagnostic: Option<usize>,
    /// Optional caller-provided fold regions.
    fold_regions: Option<Vec<FoldRegion>>,
    /// Optional precomputed symbol summary retained for enabled symbol tooling.
    symbol_summary: Option<CodeFileSummary>,
    /// Optional callback invoked after a document-changing edit.
    on_document_change: Option<DocumentChangeHandler<A>>,
    /// Visible-line safety margin maintained below the revealed caret.
    caret_follow_margin_lines: f32,
    /// Explicit reason, if any, for moving the viewport during the next layout.
    caret_scroll_intent: Signal<Option<CaretScrollIntent>>,
    /// Retained hover and captured scrollbar gesture.
    scrollbar_interaction: Signal<ScrollbarInteraction>,
}

/// Implements editor layout, paint, keyboard/IME, wheel, selection, and fold input.
impl<A: 'static> Widget<A> for CodeEditorWidget<A> {
    fn debug_name(&self) -> &'static str {
        "CodeEditor"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic = Size::new(constraints.max_w.clamp(0.0, 420.0), 220.0);
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        let bounds = Rect::new(0.0, 0.0, size.w, size.h);
        let mut session = self.sync_session_from_props();
        let mut geometries = Vec::new();
        let layout_pass = ctx.layout_pass();
        if let Some(text_system) = ctx.text_system.as_deref_mut() {
            let mut frame =
                self.engine
                    .borrow_mut()
                    .code_frame(&session, bounds, true, text_system);
            if let Some(intent) = self.caret_scroll_intent.read() {
                match evaluate_caret_scroll_intent(
                    layout_pass,
                    &mut session.editor,
                    &frame,
                    intent,
                    self.caret_follow_margin_lines,
                ) {
                    CaretScrollEvaluation::Deferred => {}
                    CaretScrollEvaluation::Evaluated {
                        scroll_changed: true,
                    } => {
                        self.session.set(session.clone());
                        frame = self.engine.borrow_mut().code_frame(
                            &session,
                            bounds,
                            true,
                            text_system,
                        );
                        if apply_caret_scroll_intent(
                            &mut session.editor,
                            &frame,
                            intent,
                            self.caret_follow_margin_lines,
                        ) {
                            self.session.set(session.clone());
                            frame = self.engine.borrow_mut().code_frame(
                                &session,
                                bounds,
                                true,
                                text_system,
                            );
                        }
                        self.caret_scroll_intent.set(None);
                    }
                    CaretScrollEvaluation::Evaluated {
                        scroll_changed: false,
                    } => self.caret_scroll_intent.set(None),
                }
            }
            geometries = code_scrollbar_geometries(
                frame.viewport,
                frame.content_size,
                self.config.scrollbars,
            );
        }
        if layout_pass.is_committed() {
            let mut interaction = self.scrollbar_interaction.read();
            if interaction.reconcile(&geometries) {
                self.scrollbar_interaction.set(interaction);
            }
        }

        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: geometries
                .iter()
                .map(|geometry| geometry.hit_track)
                .collect(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let focused = ctx.is_focused();
        let frame_time_ms = ctx.frame_time_ms();
        let Some(text_system) = ctx.text_system.as_deref_mut() else {
            return;
        };
        let session = self.sync_session_from_props();
        let mut frame = self.engine.borrow_mut().code_frame_at(
            &session,
            bounds,
            focused,
            frame_time_ms,
            text_system,
        );
        let geometries =
            code_scrollbar_geometries(frame.viewport, frame.content_size, self.config.scrollbars);
        let interaction = self.scrollbar_interaction.read();
        for geometry in geometries {
            let visual = interaction.visual_state(geometry.axis, ctx.is_hovered());
            for item in &mut frame.paint_items {
                if let EditorPaintItem::Scrollbar {
                    track_rect,
                    thumb_color,
                    ..
                } = item
                {
                    if *track_rect == geometry.track {
                        *thumb_color = thumb_color_for_state(*thumb_color, visual);
                    }
                }
            }
        }
        paint_editor_frame(ctx, &frame);
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if matches!(event, Event::Pointer(_)) {
            let mut session = self.sync_session_from_props();
            let metrics = self
                .engine
                .borrow()
                .code_scroll_metrics_cached(&session, bounds);
            let viewport = EditorViewport::with_gutter(
                bounds,
                session.editor.config,
                &session.editor.edit,
                Some(session.config.gutter),
            );
            let geometries =
                code_scrollbar_geometries(viewport, metrics.content, session.config.scrollbars);
            let current = Offset::new(session.editor.edit.scroll_x, session.editor.edit.scroll_y);
            let mut interaction = self.scrollbar_interaction.read();
            let response = interaction.handle_event(event, &geometries, current);
            if response.state_changed {
                self.scrollbar_interaction.set(interaction);
            }
            if let Some((axis, target)) = response.scroll_to {
                let delta = match axis {
                    ScrollbarAxis::Horizontal => {
                        Offset::new(target - session.editor.edit.scroll_x, 0.0)
                    }
                    ScrollbarAxis::Vertical => {
                        Offset::new(0.0, target - session.editor.edit.scroll_y)
                    }
                };
                if session.editor.scroll_by(delta, metrics) {
                    self.session.set(session);
                }
            }
            if response.repaint {
                ctx.request_repaint();
            }
            if response.consumed {
                ctx.stop_propagation();
                return;
            }
        }

        match event {
            Event::Keyboard(key) => {
                if let Some(action) = TextKeymap::new(TextInputMode::MultiLine).action_for_key(key)
                {
                    self.apply_edit_action(ctx, action);
                }
            }
            Event::Ime(ImeEvent::Preedit { preedit, .. }) => {
                self.apply_edit_action(
                    ctx,
                    TextEditAction::ImePreedit {
                        preedit: preedit.clone(),
                    },
                );
            }
            Event::Ime(ImeEvent::Commit { text }) => {
                self.apply_edit_action(ctx, TextEditAction::ImeCommit { text: text.clone() });
            }
            Event::Ime(ImeEvent::End | ImeEvent::Disabled) => {
                self.apply_edit_action(ctx, TextEditAction::ImeEnd);
            }
            Event::Pointer(PointerEvent::Wheel {
                delta, modifiers, ..
            }) => {
                let style = self.config.editor.style;
                let mut session = self.sync_session_from_props();
                let axes = ailloli_ui_editor::input::scroll::axes_for_wrap_mode(
                    session.editor.config.wrap_mode,
                );
                let behavior =
                    ScrollBehavior::new(axes).with_line_px(style.line_height.max(1.0) * 3.0);
                let metrics = self
                    .engine
                    .borrow()
                    .code_scroll_metrics_cached(&session, bounds);
                let scroll_delta = match delta {
                    WheelDelta::LineDelta { .. } | WheelDelta::PixelDelta { .. } => {
                        behavior.wheel_delta_with_modifiers(*delta, *modifiers)
                    }
                };
                if session
                    .editor
                    .scroll_by(Offset::new(scroll_delta.x, scroll_delta.y), metrics)
                {
                    self.session.set(session);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed,
                modifiers,
            }) => {
                let mut session = self.sync_session_from_props();
                if *pressed {
                    if !bounds.contains(pos.x, pos.y) {
                        return;
                    }
                    if let Some(region_index) = self
                        .engine
                        .borrow()
                        .fold_region_hit_test_cached(bounds, *pos)
                    {
                        if session.toggle_fold_region(region_index) {
                            self.session.set(session);
                            self.caret_scroll_intent
                                .set(Some(CaretScrollIntent::RevealNavigation));
                            ctx.request_layout();
                            ctx.stop_propagation();
                            return;
                        }
                    }
                    let hit =
                        self.engine
                            .borrow()
                            .hit_test_zone_cached(&session.editor, bounds, *pos);
                    match hit.zone {
                        EditorHitZone::Text => {
                            let click_count = if let Some(meta) = ctx.event_meta() {
                                session.editor.register_pointer_click_at(
                                    meta.timestamp().duration(),
                                    *pos,
                                    hit.byte,
                                    EditorClickZone::Text,
                                )
                            } else {
                                session.editor.register_pointer_click(
                                    Instant::now(),
                                    *pos,
                                    hit.byte,
                                    EditorClickZone::Text,
                                )
                            };
                            match click_count {
                                1 => {
                                    session
                                        .editor
                                        .begin_pointer_selection(hit.byte, modifiers.shift);
                                }
                                2 => {
                                    session.select_word_at_byte(hit.byte);
                                }
                                _ => {
                                    session.select_line_at_byte(hit.byte);
                                }
                            }
                        }
                        EditorHitZone::Gutter => {
                            let click_count = if let Some(meta) = ctx.event_meta() {
                                session.editor.register_pointer_click_at(
                                    meta.timestamp().duration(),
                                    *pos,
                                    hit.byte,
                                    EditorClickZone::Gutter,
                                )
                            } else {
                                session.editor.register_pointer_click(
                                    Instant::now(),
                                    *pos,
                                    hit.byte,
                                    EditorClickZone::Gutter,
                                )
                            };
                            if click_count >= 2 {
                                session.select_line_at_byte(hit.byte);
                            } else {
                                session
                                    .editor
                                    .begin_pointer_selection(hit.byte, modifiers.shift);
                            }
                        }
                        EditorHitZone::Outside => {}
                    }
                    self.session.set(session);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                } else if session.editor.edit.drag_anchor.is_some() {
                    session.editor.end_pointer_selection();
                    self.session.set(session);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                let mut session = self.sync_session_from_props();
                if let Some(anchor) = session.editor.edit.drag_anchor {
                    let viewport = EditorViewport::with_gutter(
                        bounds,
                        session.editor.config,
                        &session.editor.edit,
                        Some(session.config.gutter),
                    );
                    let byte = self
                        .engine
                        .borrow()
                        .hit_test_cached(&session.editor, bounds, *pos)
                        .byte;
                    session.editor.update_pointer_selection(anchor, byte);
                    let axes = ailloli_ui_editor::input::scroll::axes_for_wrap_mode(
                        session.editor.config.wrap_mode,
                    );
                    let delta = pointer_selection_scroll_delta(
                        *pos,
                        viewport.text_rect,
                        axes,
                        session.editor.config.style.line_height,
                    );
                    let metrics = self
                        .engine
                        .borrow()
                        .code_scroll_metrics_cached(&session, bounds);
                    session.editor.scroll_by(delta, metrics);
                    self.session.set(session);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Cancelled { .. }) => {
                let mut session = self.sync_session_from_props();
                if session.editor.edit.drag_anchor.is_some() {
                    session.editor.end_pointer_selection();
                    self.session.set(session);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::Focusable
    }

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::AllowOnFocusOnly
    }

    fn input_role(&self) -> InputRole {
        InputRole::TextMultiLine
    }

    fn ime_cursor_rect(&self, bounds: Rect, _layout: &LayoutResult) -> Option<Rect> {
        let session = self.sync_session_from_props();
        Some(
            self.engine
                .borrow()
                .code_caret_rect_cached(&session, bounds),
        )
    }
}

/// Prop reconciliation and edit-to-document synchronization helpers.
impl<A: 'static> CodeEditorWidget<A> {
    /// Reconciles external document/configuration props into retained session state.
    fn sync_session_from_props(&self) -> CodeEditorSession {
        let mut session = self.session.read();
        let document = document_with_language(self.document.read(), self.language);
        let mut changed = false;
        let mut reveal_navigation = false;
        if self.config.features.symbols {
            let _ = self
                .symbol_summary
                .as_ref()
                .map(|summary| summary.symbols.len());
        }

        if session.config != self.config {
            session.config = self.config;
            let editor_changed = session.editor.set_config(self.config.editor);
            changed |= editor_changed;
            reveal_navigation |= editor_changed;
        }
        if session.replace_document_if_changed(document) {
            changed = true;
            if let Some((x, y)) = self.initial_scroll {
                session.editor.edit.scroll_x = x;
                session.editor.edit.scroll_y = y;
            }
            session.refresh_syntax_tokens();
            apply_fold_regions_prop(&mut session, self.fold_regions.clone());
            reveal_navigation = self.initial_scroll.is_none();
        }
        let before_search = session.search.clone();
        if session.config.features.search {
            apply_search_props(
                &mut session,
                self.search_query.clone(),
                self.search_active_match,
            );
        } else {
            session.clear_search();
        }
        changed |= session.search != before_search;
        if session.config.features.diagnostics {
            if session.diagnostics != self.diagnostics
                || session.active_diagnostic_index != self.active_diagnostic
            {
                session.set_diagnostics(self.diagnostics.clone());
                session.set_active_diagnostic_index(self.active_diagnostic);
                changed = true;
            }
        } else if !session.diagnostics.is_empty() || session.active_diagnostic_index.is_some() {
            session.set_diagnostics(Vec::new());
            session.set_active_diagnostic_index(None);
            changed = true;
        }
        let before_folds = session.fold_regions.clone();
        if session.config.features.folding {
            apply_fold_regions_prop(&mut session, self.fold_regions.clone());
        } else {
            session.set_fold_regions(Vec::new());
        }
        changed |= session.fold_regions != before_folds;

        if changed {
            self.session.set(session.clone());
            if reveal_navigation {
                self.caret_scroll_intent
                    .set(Some(CaretScrollIntent::RevealNavigation));
            }
        }
        session
    }

    /// Applies one edit, bridges clipboard effects, and publishes document changes.
    fn apply_edit_action(&self, ctx: &mut EventCtx<A>, action: TextEditAction) {
        let mut session = self.sync_session_from_props();
        let mut action = action;
        if matches!(action, TextEditAction::RequestPaste) {
            if let Some(text) = ctx.read_clipboard_text() {
                action = TextEditAction::Paste { text };
            }
        }
        let intent = caret_scroll_intent_for_action(&action);
        let outcome = session.editor.apply_edit_action(action);
        if let Some(text) = outcome.clipboard_write {
            let _ = ctx.write_clipboard_text(&text);
        }
        if outcome.text_changed && session.sync_document_from_editor() {
            session.refresh_syntax_tokens();
            apply_fold_regions_prop(&mut session, self.fold_regions.clone());
            session.refresh_search();
            let document = session.document.clone();
            self.document.set(document.clone());
            if let Some(handler) = &self.on_document_change {
                handler(ctx, document);
            }
        }
        if outcome.state_changed || outcome.text_changed {
            self.session.set(session);
            if let Some(intent) = intent {
                self.caret_scroll_intent.set(Some(intent));
                ctx.request_layout();
            } else {
                ctx.request_repaint();
            }
        }
    }
}

/// Resolves a language override/path hint into a cloned document value.
fn document_with_language(mut document: Document, language: Option<EditorLanguage>) -> Document {
    document.language = resolve_document_language(&document, language);
    document
}

/// Applies a query/active index or clears retained search state.
fn apply_search_props(
    session: &mut CodeEditorSession,
    query: Option<SearchQuery>,
    active_match: Option<usize>,
) {
    if let Some(query) = query {
        session.set_search_query(query);
        session.set_search_active_index(active_match.or(session.search.active_index));
    } else {
        session.clear_search();
    }
}

/// Applies external fold geometry or refreshes language-derived regions.
fn apply_fold_regions_prop(session: &mut CodeEditorSession, fold_regions: Option<Vec<FoldRegion>>) {
    if let Some(fold_regions) = fold_regions {
        if !same_fold_region_shape(&session.fold_regions, &fold_regions) {
            session.set_fold_regions(fold_regions);
        }
    } else {
        session.refresh_fold_regions();
    }
}

/// Compares fold identity and line geometry while ignoring collapsed state.
fn same_fold_region_shape(current: &[FoldRegion], next: &[FoldRegion]) -> bool {
    current.len() == next.len()
        && current
            .iter()
            .zip(next)
            .all(|(a, b)| a.id == b.id && a.start_line == b.start_line && a.end_line == b.end_line)
}
