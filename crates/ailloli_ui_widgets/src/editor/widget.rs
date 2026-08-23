//! Retained runtime implementation of the public generic editor.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use crate::layout::layout_ext::apply_layout_size;
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent, WheelDelta};
use ailloli_ui_core::event::{Event, ImeEvent};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::ScrollBehavior;
use ailloli_ui_core::style::LayoutStyle;
use ailloli_ui_core::Offset;
use ailloli_ui_editor::{
    EditorClickZone, EditorConfig, EditorEngine, EditorLanguage, EditorSession,
};
use ailloli_ui_runtime::component::{ComponentNode, Context, Signal, View, Widget};
use ailloli_ui_runtime::input::{ActivationPolicy, EventCtx, FocusPolicy, InputRole};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_text::TextSelection;
use ailloli_ui_text::{TextBuffer, TextEditAction, TextInputMode, TextKeymap};

use super::adapter::paint_editor_frame;

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
        View::leaf(EditorWidget {
            layout: self.layout,
            buffer: self.buffer.clone(),
            session,
            engine: Rc::new(RefCell::new(EditorEngine::new())),
            config: self.config,
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
}

/// Implements generic editor layout, painting, keyboard/IME, wheel, and selection.
impl<A: 'static> Widget<A> for EditorWidget {
    fn debug_name(&self) -> &'static str {
        "Editor"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic = Size::new(constraints.max_w.clamp(0.0, 320.0), 180.0);
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        self.sync_session_from_props();

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
            Event::Pointer(PointerEvent::Wheel { delta, .. }) => {
                let style = self.config.style;
                let mut session = self.sync_session_from_props();
                let axes =
                    ailloli_ui_editor::input::scroll::axes_for_wrap_mode(session.config.wrap_mode);
                let behavior =
                    ScrollBehavior::new(axes).with_line_px(style.line_height.max(1.0) * 3.0);
                let metrics = self.engine.borrow().scroll_metrics_cached(&session, bounds);
                let scroll_delta = match delta {
                    WheelDelta::LineDelta { .. } | WheelDelta::PixelDelta { .. } => {
                        behavior.wheel_delta(*delta)
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
            }) if bounds.contains(pos.x, pos.y) => {
                let mut session = self.sync_session_from_props();
                let byte = self
                    .engine
                    .borrow()
                    .hit_test_cached(&session, bounds, *pos)
                    .byte;
                if *pressed {
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
                } else {
                    session.end_pointer_selection();
                }
                self.session.set(session);
                ctx.request_repaint();
            }
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                let mut session = self.sync_session_from_props();
                if let Some(anchor) = session.edit.drag_anchor {
                    let byte = self
                        .engine
                        .borrow()
                        .hit_test_cached(&session, bounds, *pos)
                        .byte;
                    session.update_pointer_selection(anchor, byte);
                    self.session.set(session);
                    ctx.request_repaint();
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
        let mut changed = session.set_config(self.config);
        changed |= session.replace_buffer_if_changed(self.buffer.read());
        if changed {
            self.session.set(session.clone());
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
        let outcome = session.apply_edit_action(action);
        if let Some(text) = outcome.clipboard_write {
            let _ = ctx.write_clipboard_text(&text);
        }
        if outcome.text_changed {
            self.buffer.set(session.buffer.clone());
        }
        if outcome.state_changed || outcome.text_changed {
            self.session.set(session);
            ctx.request_repaint();
        }
    }
}
