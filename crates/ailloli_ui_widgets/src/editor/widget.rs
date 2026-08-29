//! Retained runtime implementation of the public generic editor.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use crate::layout::layout_ext::apply_layout_size;
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent, WheelDelta};
use ailloli_ui_core::event::{Event, ImeEvent};
use ailloli_ui_core::geometry::{Constraints, Point, Rect, Size};
use ailloli_ui_core::scroll::{ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollState};
use ailloli_ui_core::style::LayoutStyle;
use ailloli_ui_core::Offset;
use ailloli_ui_editor::{
    EditorClickZone, EditorConfig, EditorEngine, EditorFrame, EditorLanguage, EditorPaintItem,
    EditorSession,
};
use ailloli_ui_runtime::component::{ComponentNode, Context, Signal, View, Widget};
use ailloli_ui_runtime::input::{ActivationPolicy, EventCtx, FocusPolicy, InputRole};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::Invalidation;
use ailloli_ui_text::TextSelection;
use ailloli_ui_text::{TextBuffer, TextEditAction, TextInputMode, TextKeymap};

use super::adapter::paint_editor_frame;

/// Explicit reasons for moving the viewport after the caret changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CaretScrollIntent {
    /// Editing and IME changes keep extra space below the caret.
    FollowEditing,
    /// Keyboard or programmatic navigation reveals only the caret itself.
    RevealNavigation,
}

/// Builder snapshot that creates the initial generic editor session.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::{IntoView, State, View};
/// use ailloli_ui_text::TextBuffer;
/// use ailloli_ui_widgets::editor::Editor;
/// let view: View<()> = Editor::new(State::new(TextBuffer::new())).into_view();
/// let _ = view;
/// ```
pub(crate) struct EditorComponent {
    /// Outer logical sizing policy.
    pub(crate) layout: LayoutStyle,
    /// Caller-owned text buffer synchronized in both directions.
    pub(crate) buffer: Signal<TextBuffer>,
    /// Editing and rendering configuration.
    pub(crate) config: EditorConfig,
    /// Optional initial UTF-8 byte anchor/caret pair, clamped to buffer length.
    pub(crate) initial_selection: Option<(usize, usize)>,
    /// Visible-line safety margin maintained below the revealed caret.
    pub(crate) caret_follow_margin_lines: f32,
}

/// Builds a session, clamps initial selection, and allocates the editor engine.
impl<A: 'static> ComponentNode<A> for EditorComponent {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let mut initial_session = EditorSession::with_config(self.buffer.read(), self.config);
        if let Some((anchor, caret)) = self.initial_selection {
            let len = initial_session.buffer.len_bytes();
            initial_session.edit.selection = Some(TextSelection {
                anchor: anchor.min(len),
                caret: caret.min(len),
            });
            initial_session.edit.caret_byte = caret.min(len);
        }
        let session = context.signal(initial_session);
        let caret_scroll_intent = context.signal_with_invalidation(
            self.initial_selection
                .is_some()
                .then_some(CaretScrollIntent::RevealNavigation),
            Invalidation::Paint,
        );
        View::leaf(EditorWidget {
            layout: self.layout,
            buffer: self.buffer.clone(),
            session,
            engine: Rc::new(RefCell::new(EditorEngine::new())),
            config: self.config,
            caret_follow_margin_lines: self.caret_follow_margin_lines,
            caret_scroll_intent,
        })
    }
}

/// Stateful leaf synchronizing buffer/config props with input and paint.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::{IntoView, State, View};
/// use ailloli_ui_text::TextBuffer;
/// use ailloli_ui_widgets::editor::Editor;
/// let view: View<()> = Editor::new(State::new(TextBuffer::new())).into_view();
/// let _ = view;
/// ```
pub(crate) struct EditorWidget {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Caller-owned text buffer synchronized in both directions.
    buffer: Signal<TextBuffer>,
    /// Retained buffer, selection, IME, undo, and scroll state.
    session: Signal<EditorSession>,
    /// UI-local layout/paint cache shared across event and paint passes.
    engine: Rc<RefCell<EditorEngine>>,
    /// Editing and rendering configuration reconciled into the session.
    config: EditorConfig,
    /// Visible-line safety margin maintained below the revealed caret.
    caret_follow_margin_lines: f32,
    /// Explicit reason, if any, for moving the viewport during the next layout.
    caret_scroll_intent: Signal<Option<CaretScrollIntent>>,
}

/// Implements generic editor layout, painting, keyboard/IME, wheel, and selection.
impl<A: 'static> Widget<A> for EditorWidget {
    fn debug_name(&self) -> &'static str {
        "Editor"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic = Size::new(constraints.max_w.clamp(0.0, 320.0), 180.0);
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        let mut session = self.sync_session_from_props();
        if let Some(intent) = self.caret_scroll_intent.read() {
            if let Some(text_system) = ctx.text_system.as_deref_mut() {
                let bounds = Rect::new(0.0, 0.0, size.w, size.h);
                let mut frame = self
                    .engine
                    .borrow_mut()
                    .frame(&session, bounds, true, text_system);
                if caret_reveal_frame_is_usable(&session, &frame) {
                    if apply_caret_scroll_intent(
                        &mut session,
                        &frame,
                        intent,
                        self.caret_follow_margin_lines,
                    ) {
                        self.session.set(session.clone());
                        frame = self
                            .engine
                            .borrow_mut()
                            .frame(&session, bounds, true, text_system);
                        if apply_caret_scroll_intent(
                            &mut session,
                            &frame,
                            intent,
                            self.caret_follow_margin_lines,
                        ) {
                            self.session.set(session);
                        }
                    }
                    self.caret_scroll_intent.set(None);
                }
            }
        }

        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
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
        let frame = self.engine.borrow_mut().frame_at(
            &session,
            bounds,
            focused,
            frame_time_ms,
            text_system,
        );
        paint_editor_frame(ctx, &frame);
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
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
                let style = self.config.style;
                let mut session = self.sync_session_from_props();
                let axes =
                    ailloli_ui_editor::input::scroll::axes_for_wrap_mode(session.config.wrap_mode);
                let behavior =
                    ScrollBehavior::new(axes).with_line_px(style.line_height.max(1.0) * 3.0);
                let metrics = self.engine.borrow().scroll_metrics_cached(&session, bounds);
                let scroll_delta = match delta {
                    WheelDelta::LineDelta { .. } | WheelDelta::PixelDelta { .. } => {
                        behavior.wheel_delta_with_modifiers(*delta, *modifiers)
                    }
                };
                if session.scroll_by(Offset::new(scroll_delta.x, scroll_delta.y), metrics) {
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
                    let byte = self
                        .engine
                        .borrow()
                        .hit_test_cached(&session, bounds, *pos)
                        .byte;
                    let click_count = if let Some(meta) = ctx.event_meta() {
                        session.register_pointer_click_at(
                            meta.timestamp().duration(),
                            *pos,
                            byte,
                            EditorClickZone::Text,
                        )
                    } else {
                        session.register_pointer_click(
                            Instant::now(),
                            *pos,
                            byte,
                            EditorClickZone::Text,
                        )
                    };
                    match click_count {
                        1 => {
                            session.begin_pointer_selection(byte, modifiers.shift);
                        }
                        2 => {
                            session.select_word_at_byte(byte, None, EditorLanguage::PlainText);
                        }
                        _ => {
                            session.select_line_at_byte(byte);
                        }
                    }
                    self.session.set(session);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                } else if session.edit.drag_anchor.is_some() {
                    session.end_pointer_selection();
                    self.session.set(session);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                let mut session = self.sync_session_from_props();
                if let Some(anchor) = session.edit.drag_anchor {
                    let viewport = ailloli_ui_editor::EditorViewport::new(
                        bounds,
                        session.config,
                        &session.edit,
                    );
                    let byte = self
                        .engine
                        .borrow()
                        .hit_test_cached(&session, bounds, *pos)
                        .byte;
                    session.update_pointer_selection(anchor, byte);
                    let axes = ailloli_ui_editor::input::scroll::axes_for_wrap_mode(
                        session.config.wrap_mode,
                    );
                    let delta = pointer_selection_scroll_delta(
                        *pos,
                        viewport.text_rect,
                        axes,
                        session.config.style.line_height,
                    );
                    let metrics = self.engine.borrow().scroll_metrics_cached(&session, bounds);
                    session.scroll_by(delta, metrics);
                    self.session.set(session);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Cancelled { .. }) => {
                let mut session = self.sync_session_from_props();
                if session.edit.drag_anchor.is_some() {
                    session.end_pointer_selection();
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
        Some(self.engine.borrow().caret_rect_cached(&session, bounds))
    }
}

/// Reconciles external props and applies editor actions/clipboard effects.
impl EditorWidget {
    /// Reconciles external buffer/configuration props into retained session state.
    fn sync_session_from_props(&self) -> EditorSession {
        let mut session = self.session.read();
        let config_changed = session.set_config(self.config);
        let buffer_changed = session.replace_buffer_if_changed(self.buffer.read());
        let changed = config_changed || buffer_changed;
        if changed {
            self.session.set(session.clone());
            self.caret_scroll_intent
                .set(Some(CaretScrollIntent::RevealNavigation));
        }
        session
    }

    /// Applies one edit, bridges clipboard effects, and publishes buffer changes.
    fn apply_edit_action<A>(&self, ctx: &mut EventCtx<A>, action: TextEditAction) {
        let mut session = self.sync_session_from_props();
        let mut action = action;
        if matches!(action, TextEditAction::RequestPaste) {
            if let Some(text) = ctx.read_clipboard_text() {
                action = TextEditAction::Paste { text };
            }
        }
        let intent = caret_scroll_intent_for_action(&action);
        let outcome = session.apply_edit_action(action);
        if let Some(text) = outcome.clipboard_write {
            let _ = ctx.write_clipboard_text(&text);
        }
        if outcome.text_changed {
            self.buffer.set(session.buffer.clone());
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

/// Applies one explicit caret-following intent as a minimal viewport delta.
pub(super) fn apply_caret_scroll_intent(
    session: &mut EditorSession,
    frame: &EditorFrame,
    _intent: CaretScrollIntent,
    caret_follow_margin_lines: f32,
) -> bool {
    let metrics = ScrollMetrics::new(
        Size::new(frame.viewport.text_rect.w, frame.viewport.text_rect.h),
        frame.content_size,
    );
    let axes = ailloli_ui_editor::input::scroll::axes_for_wrap_mode(session.config.wrap_mode);
    let caret = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::Caret { rect, .. } => Some(*rect),
            _ => None,
        })
        .unwrap_or_else(|| approximate_caret_screen_rect(session, frame));
    let before = Offset::new(session.edit.scroll_x, session.edit.scroll_y);
    let margin = CaretRevealMargin {
        bottom: sanitize_caret_follow_margin_lines(caret_follow_margin_lines)
            * session.config.style.line_height.max(1.0),
        ..CaretRevealMargin::default()
    };
    let after = reveal_caret_offset(
        caret,
        frame.viewport.text_rect,
        before,
        metrics,
        axes,
        margin,
    );
    if after == before {
        return false;
    }
    session.edit.scroll_x = after.x;
    session.edit.scroll_y = after.y;
    true
}

/// Returns whether a layout pass can evaluate caret visibility meaningfully.
///
/// Flex layout may first measure a fill editor with a zero-height viewport and
/// then lay it out again with its allocated height. A reveal request must stay
/// pending through that measurement pass; otherwise the zero viewport would
/// turn the caret's content position into a scroll offset before the real
/// viewport exists.
pub(super) fn caret_reveal_frame_is_usable(session: &EditorSession, frame: &EditorFrame) -> bool {
    let viewport = frame.viewport.text_rect;
    if !rect_is_finite(viewport) {
        return false;
    }
    let caret = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::Caret { rect, .. } => Some(*rect),
            _ => None,
        })
        .unwrap_or_else(|| approximate_caret_screen_rect(session, frame));
    let axes = ailloli_ui_editor::input::scroll::axes_for_wrap_mode(session.config.wrap_mode);
    (!axes.horizontal || viewport.w + 0.5 >= caret.w.max(1.0))
        && (!axes.vertical || viewport.h + 0.5 >= caret.h.max(1.0))
}

/// Maps edit commands to an explicit viewport-following policy.
pub(super) fn caret_scroll_intent_for_action(action: &TextEditAction) -> Option<CaretScrollIntent> {
    match action {
        TextEditAction::InsertText { .. }
        | TextEditAction::ImePreedit { .. }
        | TextEditAction::ImeCommit { .. }
        | TextEditAction::ImeEnd
        | TextEditAction::DeleteBackward
        | TextEditAction::DeleteForward
        | TextEditAction::Cut
        | TextEditAction::Paste { .. }
        | TextEditAction::RequestPaste
        | TextEditAction::Undo
        | TextEditAction::Redo => Some(CaretScrollIntent::FollowEditing),
        TextEditAction::Move { .. }
        | TextEditAction::SelectAll
        | TextEditAction::SetSelection { .. } => Some(CaretScrollIntent::RevealNavigation),
        TextEditAction::PointerCaret { .. } | TextEditAction::Copy => None,
    }
}

/// Produces event-driven auto-scroll only after a drag leaves the text viewport.
pub(super) fn pointer_selection_scroll_delta(
    pos: Point,
    text_rect: Rect,
    axes: ScrollAxes,
    line_px: f32,
) -> Offset {
    if !pos.x.is_finite()
        || !pos.y.is_finite()
        || !text_rect.x.is_finite()
        || !text_rect.y.is_finite()
        || !text_rect.w.is_finite()
        || !text_rect.h.is_finite()
    {
        return Offset::default();
    }
    let step = if line_px.is_finite() {
        line_px.max(1.0)
    } else {
        1.0
    };
    let axis_delta = |value: f32, start: f32, extent: f32| {
        let end = start + extent.max(0.0);
        if value <= start {
            -step.max(start - value).min(extent.max(step))
        } else if value >= end {
            step.max(value - end).min(extent.max(step))
        } else {
            0.0
        }
    };
    Offset::new(
        if axes.horizontal {
            axis_delta(pos.x, text_rect.x, text_rect.w)
        } else {
            0.0
        },
        if axes.vertical {
            axis_delta(pos.y, text_rect.y, text_rect.h)
        } else {
            0.0
        },
    )
}

/// Normalizes a public builder value without making non-finite input contagious.
pub(super) fn sanitize_caret_follow_margin_lines(lines: f32) -> f32 {
    if lines.is_finite() {
        lines.max(0.0)
    } else {
        3.0
    }
}

/// Insets defining the region in which a caret requires no viewport movement.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct CaretRevealMargin {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

/// Returns the minimally adjusted, clamped scroll offset for a screen-space caret.
fn reveal_caret_offset(
    caret: Rect,
    viewport: Rect,
    current: Offset,
    metrics: ScrollMetrics,
    axes: ScrollAxes,
    margin: CaretRevealMargin,
) -> Offset {
    if !rect_is_finite(caret) || !rect_is_finite(viewport) {
        return current;
    }

    let (safe_left, safe_right) =
        safe_axis_bounds(viewport.x, viewport.w, caret.w, margin.left, margin.right);
    let (safe_top, safe_bottom) =
        safe_axis_bounds(viewport.y, viewport.h, caret.h, margin.top, margin.bottom);
    let delta_x = if caret.x < safe_left {
        caret.x - safe_left
    } else if caret.right() > safe_right {
        caret.right() - safe_right
    } else {
        0.0
    };
    let delta_y = if caret.y < safe_top {
        caret.y - safe_top
    } else if caret.bottom() > safe_bottom {
        caret.bottom() - safe_bottom
    } else {
        0.0
    };
    let delta = axes.filter_offset(Offset::new(delta_x, delta_y));
    if delta == Offset::default() {
        return current;
    }
    ScrollState::with_offset(current)
        .scroll_by(delta, metrics, axes)
        .after
}

/// Resolves safe leading/trailing edges while retaining room for the caret.
fn safe_axis_bounds(
    origin: f32,
    extent: f32,
    caret_extent: f32,
    leading: f32,
    trailing: f32,
) -> (f32, f32) {
    let extent = extent.max(0.0);
    let caret_extent = caret_extent.max(0.0);
    let available = (extent - caret_extent).max(0.0);
    let leading = finite_non_negative(leading).min(available);
    let trailing = finite_non_negative(trailing).min(available - leading);
    (origin + leading, origin + extent - trailing)
}

/// Returns zero for negative or non-finite margin components.
fn finite_non_negative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

/// Rejects geometry that cannot produce a deterministic incremental delta.
fn rect_is_finite(rect: Rect) -> bool {
    rect.x.is_finite() && rect.y.is_finite() && rect.w.is_finite() && rect.h.is_finite()
}

/// Provides screen-space geometry when the virtualized frame has not shaped the caret line.
fn approximate_caret_screen_rect(session: &EditorSession, frame: &EditorFrame) -> Rect {
    let paragraph = session
        .buffer
        .paragraphs()
        .iter()
        .position(|meta| session.edit.caret_byte <= meta.byte_range.end)
        .unwrap_or_else(|| session.buffer.paragraphs().len().saturating_sub(1));
    Rect::new(
        frame.viewport.text_rect.x,
        frame.viewport.text_rect.y + paragraph as f32 * session.config.style.line_height.max(1.0)
            - frame.viewport.scroll_y,
        1.0,
        session
            .config
            .style
            .line_height
            .max(1.0)
            .min(frame.content_size.h.max(1.0)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailloli_ui_text::TextMovement;

    #[test]
    fn caret_scroll_intents_are_chosen_by_input_context() {
        assert_eq!(
            caret_scroll_intent_for_action(&TextEditAction::InsertText { text: "x".into() }),
            Some(CaretScrollIntent::FollowEditing)
        );
        assert_eq!(
            caret_scroll_intent_for_action(&TextEditAction::Move {
                movement: TextMovement::LineDown,
                extend: false,
            }),
            Some(CaretScrollIntent::RevealNavigation)
        );
        assert_eq!(
            caret_scroll_intent_for_action(&TextEditAction::PointerCaret {
                byte: 4,
                extend: false,
            }),
            None
        );
    }

    #[test]
    fn pointer_selection_scrolls_only_outside_enabled_viewport_axes() {
        let viewport = Rect::new(10.0, 20.0, 100.0, 80.0);
        assert_eq!(
            pointer_selection_scroll_delta(
                Point::new(60.0, 60.0),
                viewport,
                ScrollAxes::BOTH,
                18.0,
            ),
            Offset::default()
        );
        assert_eq!(
            pointer_selection_scroll_delta(
                Point::new(121.0, 111.0),
                viewport,
                ScrollAxes::BOTH,
                18.0,
            ),
            Offset::new(18.0, 18.0)
        );
        assert_eq!(
            pointer_selection_scroll_delta(
                Point::new(viewport.right(), viewport.bottom()),
                viewport,
                ScrollAxes::BOTH,
                18.0,
            ),
            Offset::new(18.0, 18.0)
        );
        assert_eq!(
            pointer_selection_scroll_delta(
                Point::new(0.0, 0.0),
                viewport,
                ScrollAxes::VERTICAL,
                18.0,
            ),
            Offset::new(0.0, -20.0)
        );
    }

    #[test]
    fn caret_follow_margin_rejects_invalid_builder_values() {
        assert_eq!(sanitize_caret_follow_margin_lines(5.0), 5.0);
        assert_eq!(sanitize_caret_follow_margin_lines(-2.0), 0.0);
        assert_eq!(sanitize_caret_follow_margin_lines(f32::NAN), 3.0);
    }

    #[test]
    fn caret_reveal_waits_for_the_allocated_viewport_after_flex_measurement() {
        let session = EditorSession::new(TextBuffer::from_string("first\nsecond"));
        let mut engine = EditorEngine::new();
        let mut text_system = ailloli_ui_text::TextSystem::new();
        let measured = engine.frame(
            &session,
            Rect::new(0.0, 0.0, 200.0, 0.0),
            true,
            &mut text_system,
        );
        let allocated = engine.frame(
            &session,
            Rect::new(0.0, 0.0, 200.0, 180.0),
            true,
            &mut text_system,
        );

        assert!(!caret_reveal_frame_is_usable(&session, &measured));
        assert!(caret_reveal_frame_is_usable(&session, &allocated));
    }

    #[test]
    fn visible_caret_reveal_is_idempotent() {
        let viewport = Rect::new(10.0, 20.0, 100.0, 100.0);
        let metrics = ScrollMetrics::new(Size::new(100.0, 100.0), Size::new(500.0, 500.0));
        let current = Offset::new(40.0, 60.0);
        let margin = CaretRevealMargin {
            bottom: 30.0,
            ..CaretRevealMargin::default()
        };
        assert_eq!(
            reveal_caret_offset(
                Rect::new(50.0, 55.0, 1.0, 18.0),
                viewport,
                current,
                metrics,
                ScrollAxes::BOTH,
                margin,
            ),
            current
        );
    }

    #[test]
    fn caret_reveal_adds_only_the_boundary_overflow() {
        let viewport = Rect::new(10.0, 20.0, 100.0, 100.0);
        let metrics = ScrollMetrics::new(Size::new(100.0, 100.0), Size::new(500.0, 500.0));
        let current = Offset::new(40.0, 60.0);
        let margin = CaretRevealMargin {
            bottom: 30.0,
            ..CaretRevealMargin::default()
        };
        let first = reveal_caret_offset(
            Rect::new(50.0, 80.0, 1.0, 18.0),
            viewport,
            current,
            metrics,
            ScrollAxes::BOTH,
            margin,
        );
        assert_eq!(first, Offset::new(40.0, 68.0));
        // The caret moves up by the same screen-space delta after the viewport
        // advances, so repeating reveal with refreshed geometry is idempotent.
        assert_eq!(
            reveal_caret_offset(
                Rect::new(50.0, 72.0, 1.0, 18.0),
                viewport,
                first,
                metrics,
                ScrollAxes::BOTH,
                margin,
            ),
            first
        );
    }

    #[test]
    fn caret_reveal_is_minimal_upward_and_horizontally() {
        let viewport = Rect::new(10.0, 20.0, 100.0, 100.0);
        let metrics = ScrollMetrics::new(Size::new(100.0, 100.0), Size::new(500.0, 500.0));
        let current = Offset::new(40.0, 60.0);
        assert_eq!(
            reveal_caret_offset(
                Rect::new(4.0, 15.0, 1.0, 18.0),
                viewport,
                current,
                metrics,
                ScrollAxes::BOTH,
                CaretRevealMargin::default(),
            ),
            Offset::new(34.0, 55.0)
        );
    }
}
