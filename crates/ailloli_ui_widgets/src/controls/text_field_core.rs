//! Shared retained editing, layout, scrolling, and IME machinery for text fields.
//!
//! These helpers are crate-visible so controls can share one editing contract;
//! consumers use [`super::text_input::TextInput`] rather than calling them.

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, ImeEvent, ImePreedit};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::{ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollState};
use ailloli_ui_core::style::LayoutStyle;
use ailloli_ui_core::{Offset, TextStyle};
use ailloli_ui_runtime::component::Signal;
use ailloli_ui_runtime::input::{EventCtx, Selection};
use ailloli_ui_runtime::layout::{LayoutArtifact, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawCmd, DrawRect, DrawText};
use ailloli_ui_text::{
    TextBuffer, TextEditAction, TextEditState, TextInputMode, TextKeymap, TextLayoutHandle,
    TextLayoutParams, WrapMode,
};

use super::text_input::TextInputStyle;
use crate::layout::layout_ext::apply_layout_size;

#[derive(Debug, Clone, Copy, Default)]
/// Event-routing policy supplied by a public text control.
///
/// `consume_handled_events` defaults to `false`. Multi-line pointer and changed
/// wheel paths already consume their events directly; the option additionally
/// consumes every path the shared handler reports as handled.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TextInput;
/// let input: TextInput<()> = TextInput::new();
/// let _ = input; // public TextInput uses the default propagation policy
/// ```
pub(crate) struct TextFieldEventOptions {
    /// Whether the shared handler stops propagation after any handled event.
    pub consume_handled_events: bool,
}

/// Builds the display string and display caret for an optional IME preedit.
///
/// Without preedit, the original text is cloned and the caret is length-clamped.
/// With preedit, its text is inserted at the clamped byte offset and its
/// optional selection end determines the display caret. Offsets must be UTF-8
/// character boundaries when insertion occurs.
///
/// # Panics
///
/// Panics when non-empty preedit is inserted at a clamped caret offset that is
/// not a UTF-8 character boundary.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::ImePreedit;
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_text::TextSystem;
/// use ailloli_ui_widgets::controls::text_input::draw_text_input;
/// use ailloli_ui_widgets::controls::TextInputStyle;
/// let mut text = TextSystem::new();
/// let preedit = ImePreedit::new("é");
/// let commands = draw_text_input(
///     Rect::new(0.0, 0.0, 160.0, 36.0), "caf", 3, None, Some(&preedit),
///     true, 0, TextInputStyle::default(), &mut text,
/// );
/// assert!(!commands.is_empty());
/// ```
pub(crate) fn display_text_for_edit(
    text: &str,
    caret_byte: usize,
    preedit: Option<&ImePreedit>,
) -> (String, usize) {
    match preedit {
        None => (text.to_string(), caret_byte.min(text.len())),
        Some(preedit) => {
            let mut display = text.to_string();
            let at = caret_byte.min(display.len());
            if !preedit.text.is_empty() {
                display.insert_str(at, &preedit.text);
            }
            let caret = preedit
                .selection
                .map(|(_, end)| at + end.min(preedit.text.len()))
                .unwrap_or(at + preedit.text.len())
                .min(display.len());
            (display, caret)
        }
    }
}

/// Insets bounds by horizontal and vertical style padding.
///
/// Negative remaining width or height is clamped to zero; negative padding is
/// otherwise accepted and expands the returned rectangle.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_widgets::controls::TextInputStyle;
/// let style = TextInputStyle::default();
/// let bounds = Rect::new(0.0, 0.0, 100.0, 40.0);
/// assert!(bounds.w - style.pad_x * 2.0 <= bounds.w);
/// ```
pub(crate) fn text_input_content_rect(bounds: Rect, style: TextInputStyle) -> Rect {
    Rect::new(
        bounds.x + style.pad_x,
        bounds.y + style.pad_y,
        (bounds.w - style.pad_x * 2.0).max(0.0),
        (bounds.h - style.pad_y * 2.0).max(0.0),
    )
}

#[allow(clippy::too_many_arguments)]
/// Measures single-line content and returns an optional reusable text artifact.
///
/// The public value takes precedence over a stale buffer. Empty display text
/// measures the placeholder, or one space when no placeholder exists. Width
/// resolves from layout against constraints; height follows text plus padding.
/// Without a text system, height falls back to `1.2 × font_px + 2 × pad_y`.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new(String::new())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value).placeholder("Search");
/// let _ = input; // single-line layout measures the placeholder while empty
/// ```
pub(crate) fn layout_single_line_text(
    ctx: &mut LayoutCtx<'_>,
    constraints: Constraints,
    layout: LayoutStyle,
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
    edit: &Signal<TextEditState>,
    placeholder: Option<String>,
    style: TextInputStyle,
) -> (Size, Option<TextLayoutHandle>) {
    let buffer = read_display_buffer(value, buffer);
    let value = buffer.as_str();
    let edit = edit.read();
    let (display, _) = display_text_for_edit(
        &value,
        edit.caret_byte.min(value.len()),
        edit.preedit.as_ref(),
    );
    let sample = if display.is_empty() {
        placeholder.unwrap_or_else(|| " ".to_string())
    } else {
        display
    };
    let text_layout = ctx.text_system.as_deref_mut().map(|ts| {
        ts.layout_cached(TextLayoutParams {
            text: &sample,
            style: style.text,
            max_width: Some(constraints.max_w.max(0.0)),
            wrap_mode: WrapMode::NoWrap,
        })
    });
    let height = if let Some(laid) = text_layout.as_ref() {
        laid.metrics.height + style.pad_y * 2.0
    } else {
        style.text.px_size as f32 * 1.2 + style.pad_y * 2.0
    };
    let max_w = layout
        .width
        .resolve(constraints.max_w)
        .unwrap_or(constraints.max_w);
    let intrinsic = Size::new(max_w, height);
    (
        apply_layout_size(intrinsic, layout, constraints),
        text_layout,
    )
}

#[allow(clippy::too_many_arguments)]
/// Paints clipped single-line text, selection, and blinking caret.
///
/// A matching layout artifact is reused; otherwise text is reshaped during
/// paint when a text system exists. Horizontal scroll shifts the baseline.
/// Placeholder color is used only when value and preedit are empty. Selection
/// is hidden during preedit, and a non-positive blink cadence hides the caret.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_text::TextSystem;
/// use ailloli_ui_widgets::controls::text_input::draw_text_input;
/// use ailloli_ui_widgets::controls::TextInputStyle;
/// let mut text = TextSystem::new();
/// let commands = draw_text_input(
///     Rect::new(0.0, 0.0, 180.0, 36.0), "abc", 3, None, None,
///     true, 0, TextInputStyle::default(), &mut text,
/// );
/// assert!(commands.len() >= 3);
/// ```
pub(crate) fn paint_single_line_text(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    layout: &LayoutResult,
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
    edit: &Signal<TextEditState>,
    placeholder: Option<String>,
    style: TextInputStyle,
    focused: bool,
) {
    let buffer = read_display_buffer(value, buffer);
    let value = buffer.as_str();
    let is_empty = value.is_empty();
    let edit_state = edit.read();
    let (display, caret_in_display) = display_text_for_edit(
        &value,
        edit_state.caret_byte.min(value.len()),
        edit_state.preedit.as_ref(),
    );
    let text = if is_empty && display.is_empty() {
        placeholder.unwrap_or_default()
    } else {
        display
    };
    let text_color = if is_empty && edit_state.preedit.is_none() {
        style.placeholder
    } else {
        style.text.color
    };
    let style = TextInputStyle {
        text: TextStyle {
            color: text_color,
            ..style.text
        },
        ..style
    };

    let layout_handle = match layout.artifact.as_ref() {
        Some(LayoutArtifact::Text(layout)) if layout.text() == text => layout.clone(),
        _ => {
            let Some(ts) = ctx.text_system.as_deref_mut() else {
                return;
            };
            ts.layout_cached(TextLayoutParams {
                text: &text,
                style: style.text,
                max_width: Some(bounds.w.max(0.0)),
                wrap_mode: WrapMode::NoWrap,
            })
        }
    };

    let content_rect = text_input_content_rect(bounds, style);
    let baseline_x = content_rect.x - edit_state.scroll_x;
    let baseline_y = bounds.y + style.pad_y + style.text.px_size as f32;
    let px = style.text.px_size as f32;
    let y_top = (baseline_y - px).round();
    let frame_time_ms = ctx.frame_time_ms() as i64;

    ctx.with_clip(content_rect, |ctx| {
        if edit_state.preedit.is_none() {
            if let Some(sel) = edit_state.selection {
                if !sel.is_collapsed() {
                    let (lo, hi) = sel.normalized();
                    let lo = lo.min(text.len());
                    let hi = hi.min(text.len());
                    if hi > lo {
                        let x0 = (baseline_x + layout_handle.caret_x_at(lo)).round();
                        let x1 = (baseline_x + layout_handle.caret_x_at(hi)).round();
                        ctx.push(DrawCmd::Rect(DrawRect {
                            rect: Rect::new(x0, y_top, (x1 - x0).max(1.0), px + 2.0),
                            color: style.selection_bg,
                        }));
                    }
                }
            }
        }

        ctx.push(DrawCmd::Text(DrawText {
            pos: [baseline_x, baseline_y],
            color: style.text.color,
            decoration: ailloli_ui_core::TextDecoration::None,
            layout: layout_handle.clone(),
        }));

        if focused && style.caret_blink_ms > 0 {
            let on = ((frame_time_ms / style.caret_blink_ms) % 2) == 0;
            if on {
                let caret_x = (baseline_x + layout_handle.caret_x_at(caret_in_display)).round();
                ctx.push(DrawCmd::Rect(DrawRect {
                    rect: Rect::new(caret_x, y_top, style.caret_w, px + 2.0),
                    color: style.caret,
                }));
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
/// Measures wrapped multi-line content and returns a reusable text artifact.
///
/// Text wraps by word or, when needed, anywhere within the padded content
/// width. Empty display text measures the placeholder or one space. Width is
/// layout-resolved; height grows from wrapped content plus vertical padding.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("one two three".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value).multiline();
/// let _ = input;
/// ```
pub(crate) fn layout_multi_line_text(
    ctx: &mut LayoutCtx<'_>,
    constraints: Constraints,
    layout: LayoutStyle,
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
    edit: &Signal<TextEditState>,
    placeholder: Option<String>,
    style: TextInputStyle,
) -> (Size, Option<TextLayoutHandle>) {
    let buffer = read_display_buffer(value, buffer);
    let value = buffer.as_str();
    let edit = edit.read();
    let (display, _) = display_text_for_edit(
        &value,
        edit.caret_byte.min(value.len()),
        edit.preedit.as_ref(),
    );
    let sample = if display.is_empty() {
        placeholder.unwrap_or_else(|| " ".to_string())
    } else {
        display
    };
    let max_w = layout
        .width
        .resolve(constraints.max_w)
        .unwrap_or(constraints.max_w);
    let content_w = (max_w - style.pad_x * 2.0).max(0.0);
    let text_layout = ctx.text_system.as_deref_mut().map(|ts| {
        ts.layout_cached(TextLayoutParams {
            text: &sample,
            style: style.text,
            max_width: Some(content_w),
            wrap_mode: WrapMode::WordOrAnywhere,
        })
    });
    let height = text_layout
        .as_ref()
        .map(|laid| laid.height())
        .unwrap_or_else(|| style.text.px_size as f32 * 1.2)
        + style.pad_y * 2.0;
    let intrinsic = Size::new(max_w, height);
    (
        apply_layout_size(intrinsic, layout, constraints),
        text_layout,
    )
}

#[allow(clippy::too_many_arguments)]
/// Paints clipped wrapped text, per-line selection, caret, and scrollbar.
///
/// Both scroll axes offset the text. A vertical scrollbar is emitted only when
/// content exceeds the viewport by more than one logical pixel; its thumb ratio
/// is clamped to `0.08..=1.0` and its height has an 18-pixel floor.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("first\nsecond".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value).multiline();
/// let _ = input; // retained painting clips wrapped text to padded bounds
/// ```
pub(crate) fn paint_multi_line_text(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    layout: &LayoutResult,
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
    edit: &Signal<TextEditState>,
    placeholder: Option<String>,
    style: TextInputStyle,
    focused: bool,
) {
    let buffer = read_display_buffer(value, buffer);
    let value = buffer.as_str();
    let is_empty = value.is_empty();
    let edit_state = edit.read();
    let (display, caret_in_display) = display_text_for_edit(
        &value,
        edit_state.caret_byte.min(value.len()),
        edit_state.preedit.as_ref(),
    );
    let text = if is_empty && display.is_empty() {
        placeholder.unwrap_or_default()
    } else {
        display
    };
    let text_color = if is_empty && edit_state.preedit.is_none() {
        style.placeholder
    } else {
        style.text.color
    };
    let style = TextInputStyle {
        text: TextStyle {
            color: text_color,
            ..style.text
        },
        ..style
    };

    let content_rect = text_input_content_rect(bounds, style);
    let layout_handle = match layout.artifact.as_ref() {
        Some(LayoutArtifact::Text(layout)) if layout.text() == text => layout.clone(),
        _ => {
            let Some(ts) = ctx.text_system.as_deref_mut() else {
                return;
            };
            ts.layout_cached(TextLayoutParams {
                text: &text,
                style: style.text,
                max_width: Some(content_rect.w.max(0.0)),
                wrap_mode: WrapMode::WordOrAnywhere,
            })
        }
    };

    let origin_x = content_rect.x - edit_state.scroll_x;
    let first_baseline = layout_handle
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(style.text.px_size as f32);
    let baseline_y = content_rect.y - edit_state.scroll_y + first_baseline;
    let frame_time_ms = ctx.frame_time_ms() as i64;

    ctx.with_clip(content_rect, |ctx| {
        if edit_state.preedit.is_none() {
            if let Some(sel) = edit_state.selection {
                if !sel.is_collapsed() {
                    paint_multi_line_selection(
                        ctx,
                        &layout_handle,
                        sel.normalized(),
                        content_rect,
                        edit_state.scroll_x,
                        edit_state.scroll_y,
                        style,
                    );
                }
            }
        }

        ctx.push(DrawCmd::Text(DrawText {
            pos: [origin_x, baseline_y],
            color: style.text.color,
            decoration: ailloli_ui_core::TextDecoration::None,
            layout: layout_handle.clone(),
        }));

        if focused && style.caret_blink_ms > 0 {
            let on = ((frame_time_ms / style.caret_blink_ms) % 2) == 0;
            if on {
                let caret = layout_handle.caret_rect_at(caret_in_display, style.caret_w);
                ctx.push(DrawCmd::Rect(DrawRect {
                    rect: Rect::new(
                        (content_rect.x - edit_state.scroll_x + caret.x).round(),
                        (content_rect.y - edit_state.scroll_y + caret.y).round(),
                        style.caret_w,
                        caret.h.max(style.text.px_size as f32),
                    ),
                    color: style.caret,
                }));
            }
        }

        paint_multi_line_scrollbar(
            ctx,
            content_rect,
            layout_handle.height(),
            edit_state.scroll_y,
            style,
        );
    });
}

/// Paints the selected intersection of every shaped line.
///
/// Byte bounds are clamped to layout text; each non-empty intersection has at
/// least one logical pixel of width.
fn paint_multi_line_selection(
    ctx: &mut PaintCtx<'_>,
    layout: &TextLayoutHandle,
    (lo, hi): (usize, usize),
    content_rect: Rect,
    scroll_x: f32,
    scroll_y: f32,
    style: TextInputStyle,
) {
    let lo = lo.min(layout.text().len());
    let hi = hi.min(layout.text().len());
    if hi <= lo {
        return;
    }
    for line in &layout.lines {
        let line_start = line.text_range.start.min(layout.text().len());
        let line_end = line.text_range.end.min(layout.text().len());
        let hit_start = lo.max(line_start);
        let hit_end = hi.min(line_end);
        if hit_end <= hit_start {
            continue;
        }
        let x0 = if hit_start <= line_start {
            0.0
        } else {
            layout.caret_rect_at(hit_start, 0.0).x
        };
        let x1 = if hit_end >= line_end {
            line.width
        } else {
            layout.caret_rect_at(hit_end, 0.0).x
        };
        let x_left = x0.min(x1);
        let caret = layout.caret_rect_at(line_start, 0.0);
        let height = caret.h.max(style.text.px_size as f32 + 2.0);
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(
                (content_rect.x - scroll_x + x_left).round(),
                (content_rect.y - scroll_y + caret.y).round(),
                (x1 - x0).abs().max(1.0),
                height,
            ),
            color: style.selection_bg,
        }));
    }
}

/// Paints the two-pixel vertical scrollbar thumb when content overflows.
fn paint_multi_line_scrollbar(
    ctx: &mut PaintCtx<'_>,
    content_rect: Rect,
    content_height: f32,
    scroll_y: f32,
    style: TextInputStyle,
) {
    if content_height <= content_rect.h + 1.0 || content_rect.h <= 1.0 {
        return;
    }
    let track_h = content_rect.h;
    let ratio = (content_rect.h / content_height).clamp(0.08, 1.0);
    let thumb_h = (track_h * ratio).max(18.0).min(track_h);
    let max_scroll = (content_height - content_rect.h).max(1.0);
    let y = content_rect.y + ((scroll_y / max_scroll).clamp(0.0, 1.0) * (track_h - thumb_h));
    ctx.push(DrawCmd::Rect(DrawRect {
        rect: Rect::new(content_rect.right() - 3.0, y, 2.0, thumb_h),
        color: style.border.with_alpha(0.62),
    }));
}

#[allow(clippy::too_many_arguments)]
/// Routes one single-line keyboard, IME, wheel, or pointer event.
///
/// Keyboard mapping uses [`TextInputMode::SingleLine`]. Wheel input scrolls only
/// horizontally. A left press places/extends the selection and starts a drag;
/// release or cancellation ends it. The return value is `true` exactly for a
/// handled path. Propagation is stopped only when requested by `options`.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new(String::new())), Rc::new(|| {}));
/// let input = TextInput::new().bind(value).on_change(|text| text);
/// let _ = input; // the public widget delegates events to the single-line router
/// ```
pub(crate) fn handle_single_line_text_event<A>(
    ctx: &mut EventCtx<A>,
    event: &Event,
    bounds: Rect,
    layout: &LayoutResult,
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
    edit: &Signal<TextEditState>,
    style: TextInputStyle,
    options: TextFieldEventOptions,
) -> bool {
    let handled = match event {
        Event::Keyboard(key) => {
            if let Some(action) = TextKeymap::new(TextInputMode::SingleLine).action_for_key(key) {
                apply_edit_action(ctx, value, buffer, edit, action, bounds, layout, style);
                true
            } else {
                false
            }
        }
        Event::Ime(ImeEvent::Preedit { preedit, .. }) => {
            apply_edit_action(
                ctx,
                value,
                buffer,
                edit,
                TextEditAction::ImePreedit {
                    preedit: preedit.clone(),
                },
                bounds,
                layout,
                style,
            );
            true
        }
        Event::Ime(ImeEvent::Commit { text }) => {
            apply_edit_action(
                ctx,
                value,
                buffer,
                edit,
                TextEditAction::ImeCommit { text: text.clone() },
                bounds,
                layout,
                style,
            );
            true
        }
        Event::Ime(ImeEvent::End | ImeEvent::Disabled) => {
            apply_edit_action(
                ctx,
                value,
                buffer,
                edit,
                TextEditAction::ImeEnd,
                bounds,
                layout,
                style,
            );
            true
        }
        Event::Pointer(PointerEvent::Wheel { delta, .. }) => {
            let buffer_value = read_display_buffer(value, buffer);
            let value_text = buffer_value.as_str();
            let edit_state = edit.read();
            let (display, _) = display_text_for_edit(
                &value_text,
                edit_state.caret_byte.min(value_text.len()),
                edit_state.preedit.as_ref(),
            );
            let metrics = scroll_metrics(bounds, layout, &display, style);
            let state = ScrollState::with_offset(Offset::new(edit_state.scroll_x, 0.0));
            let behavior = ScrollBehavior::new(ScrollAxes::HORIZONTAL);
            let outcome = state.scroll_by(
                behavior.wheel_delta(*delta),
                metrics,
                ScrollAxes::HORIZONTAL,
            );
            if outcome.changed {
                let mut next = edit_state;
                next.scroll_x = outcome.after.x;
                edit.set(next);
                ctx.request_repaint();
                true
            } else {
                false
            }
        }
        Event::Pointer(PointerEvent::Button {
            pos,
            button: MouseButton::Left,
            pressed,
            modifiers,
        }) if bounds.contains(pos.x, pos.y) => {
            let byte = byte_at_point(bounds, *pos, layout, value, buffer, edit, style);
            if *pressed {
                let mut edit_state = edit.read();
                edit_state.drag_anchor = Some(byte);
                let buffer_value = sync_buffer_from_value(value, buffer);
                edit_state.set_caret(&buffer_value, byte, modifiers.shift);
                edit.set(edit_state);
                ctx.request_repaint();
            } else {
                edit.update(|edit| edit.drag_anchor = None);
                ctx.request_repaint();
            }
            true
        }
        Event::Pointer(PointerEvent::Moved { pos, .. }) => {
            let mut edit_state = edit.read();
            if let Some(anchor) = edit_state.drag_anchor {
                let byte = byte_at_point(bounds, *pos, layout, value, buffer, edit, style);
                let buffer_value = sync_buffer_from_value(value, buffer);
                edit_state.selection = Some(Selection {
                    anchor,
                    caret: byte,
                });
                edit_state.caret_byte = byte.min(buffer_value.len_bytes());
                edit.set(edit_state);
                ctx.request_repaint();
                true
            } else {
                false
            }
        }
        Event::Pointer(PointerEvent::Cancelled { .. }) if edit.read().drag_anchor.is_some() => {
            edit.update(|edit| edit.drag_anchor = None);
            ctx.request_repaint();
            true
        }
        _ => false,
    };

    if handled && options.consume_handled_events {
        ctx.stop_propagation();
    }
    handled
}

#[allow(clippy::too_many_arguments)]
/// Routes one multi-line keyboard, IME, two-axis wheel, or pointer event.
///
/// Keyboard mapping uses [`TextInputMode::MultiLine`]. Changed wheel scrolling
/// and handled pointer paths always stop propagation. Keyboard/IME paths follow
/// `options`. Edits request a deferred post-layout caret reveal when the current
/// artifact cannot resolve it.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new(String::new())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value).multiline();
/// let _ = input; // newline keys and two-axis wheel input use the multi-line router
/// ```
pub(crate) fn handle_multi_line_text_event<A>(
    ctx: &mut EventCtx<A>,
    event: &Event,
    bounds: Rect,
    layout: &LayoutResult,
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
    edit: &Signal<TextEditState>,
    pending_reveal: &Signal<bool>,
    style: TextInputStyle,
    options: TextFieldEventOptions,
) -> bool {
    let handled = match event {
        Event::Keyboard(key) => {
            if let Some(action) = TextKeymap::new(TextInputMode::MultiLine).action_for_key(key) {
                apply_multi_line_edit_action(
                    ctx,
                    value,
                    buffer,
                    edit,
                    pending_reveal,
                    action,
                    bounds,
                    layout,
                    style,
                );
                true
            } else {
                false
            }
        }
        Event::Ime(ImeEvent::Preedit { preedit, .. }) => {
            apply_multi_line_edit_action(
                ctx,
                value,
                buffer,
                edit,
                pending_reveal,
                TextEditAction::ImePreedit {
                    preedit: preedit.clone(),
                },
                bounds,
                layout,
                style,
            );
            true
        }
        Event::Ime(ImeEvent::Commit { text }) => {
            apply_multi_line_edit_action(
                ctx,
                value,
                buffer,
                edit,
                pending_reveal,
                TextEditAction::ImeCommit { text: text.clone() },
                bounds,
                layout,
                style,
            );
            true
        }
        Event::Ime(ImeEvent::End | ImeEvent::Disabled) => {
            apply_multi_line_edit_action(
                ctx,
                value,
                buffer,
                edit,
                pending_reveal,
                TextEditAction::ImeEnd,
                bounds,
                layout,
                style,
            );
            true
        }
        Event::Pointer(PointerEvent::Wheel { delta, .. }) => {
            let buffer_value = read_display_buffer(value, buffer);
            let value_text = buffer_value.as_str();
            let edit_state = edit.read();
            let (display, _) = display_text_for_edit(
                &value_text,
                edit_state.caret_byte.min(value_text.len()),
                edit_state.preedit.as_ref(),
            );
            let metrics = multi_line_scroll_metrics(bounds, layout, &display, style);
            let state =
                ScrollState::with_offset(Offset::new(edit_state.scroll_x, edit_state.scroll_y));
            let behavior = ScrollBehavior::new(ScrollAxes::BOTH)
                .with_line_px((style.text.px_size as f32 * 1.4).max(1.0));
            let outcome = state.scroll_by(behavior.wheel_delta(*delta), metrics, ScrollAxes::BOTH);
            if outcome.changed {
                let mut next = edit_state;
                next.scroll_x = outcome.after.x;
                next.scroll_y = outcome.after.y;
                edit.set(next);
                ctx.request_repaint();
                ctx.stop_propagation();
                true
            } else {
                false
            }
        }
        Event::Pointer(PointerEvent::Button {
            pos,
            button: MouseButton::Left,
            pressed,
            modifiers,
        }) if bounds.contains(pos.x, pos.y) => {
            let byte = byte_at_point_multi_line(bounds, *pos, layout, value, buffer, edit, style);
            if *pressed {
                let mut edit_state = edit.read();
                edit_state.drag_anchor = Some(byte);
                let buffer_value = sync_buffer_from_value(value, buffer);
                edit_state.set_caret(&buffer_value, byte, modifiers.shift);
                edit.set(edit_state);
                ctx.request_repaint();
            } else {
                edit.update(|edit| edit.drag_anchor = None);
                ctx.request_repaint();
            }
            ctx.stop_propagation();
            true
        }
        Event::Pointer(PointerEvent::Moved { pos, .. }) => {
            let mut edit_state = edit.read();
            if let Some(anchor) = edit_state.drag_anchor {
                let byte =
                    byte_at_point_multi_line(bounds, *pos, layout, value, buffer, edit, style);
                let buffer_value = sync_buffer_from_value(value, buffer);
                edit_state.selection = Some(Selection {
                    anchor,
                    caret: byte,
                });
                edit_state.caret_byte = byte.min(buffer_value.len_bytes());
                edit.set(edit_state);
                ctx.request_repaint();
                ctx.stop_propagation();
                true
            } else {
                false
            }
        }
        Event::Pointer(PointerEvent::Cancelled { .. }) if edit.read().drag_anchor.is_some() => {
            edit.update(|edit| edit.drag_anchor = None);
            ctx.request_repaint();
            ctx.stop_propagation();
            true
        }
        _ => false,
    };

    if handled && options.consume_handled_events {
        ctx.stop_propagation();
    }
    handled
}

#[allow(clippy::too_many_arguments)]
/// Applies one single-line edit action and synchronizes all observable state.
///
/// A paste request reads the context clipboard and becomes a paste only when
/// text is available. Clipboard writes are best-effort. Changed text updates
/// buffer then public value; any text or edit-state change reveals the caret,
/// commits edit state, and requests repaint. No-op outcomes do none of these.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("a".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value.clone());
/// assert_eq!(value.read(), "a");
/// let _ = input; // accepted edit actions update this same signal
/// ```
pub(crate) fn apply_edit_action<A>(
    ctx: &mut EventCtx<A>,
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
    edit: &Signal<TextEditState>,
    action: TextEditAction,
    bounds: Rect,
    layout: &LayoutResult,
    style: TextInputStyle,
) {
    let mut value_buffer = sync_buffer_from_value(value, buffer);
    let mut edit_state = edit.read();
    let mut action = action;
    if matches!(action, TextEditAction::RequestPaste) {
        if let Some(text) = ctx.read_clipboard_text() {
            action = TextEditAction::Paste { text };
        }
    }
    let outcome = edit_state.apply(&mut value_buffer, action);
    if let Some(text) = outcome.clipboard_write {
        let _ = ctx.write_clipboard_text(&text);
    }
    let text_after = value_buffer.as_str();
    if outcome.text_changed {
        buffer.set(value_buffer);
        value.set(text_after.clone());
    }
    if outcome.state_changed || outcome.text_changed {
        reveal_caret(edit, &mut edit_state, bounds, layout, &text_after, style);
        edit.set(edit_state);
        ctx.request_repaint();
    }
}

#[allow(clippy::too_many_arguments)]
/// Applies one multi-line edit action and schedules reliable caret reveal.
///
/// Clipboard and value synchronization match [`apply_edit_action`]. If the
/// current text artifact matches, both scroll axes reveal the caret immediately;
/// otherwise `pending_reveal` is set for the post-layout callback.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("a\nb".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value).multiline();
/// let _ = input; // a stale artifact defers reveal until committed layout
/// ```
pub(crate) fn apply_multi_line_edit_action<A>(
    ctx: &mut EventCtx<A>,
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
    edit: &Signal<TextEditState>,
    pending_reveal: &Signal<bool>,
    action: TextEditAction,
    bounds: Rect,
    layout: &LayoutResult,
    style: TextInputStyle,
) {
    let mut value_buffer = sync_buffer_from_value(value, buffer);
    let mut edit_state = edit.read();
    let mut action = action;
    if matches!(action, TextEditAction::RequestPaste) {
        if let Some(text) = ctx.read_clipboard_text() {
            action = TextEditAction::Paste { text };
        }
    }
    let outcome = edit_state.apply(&mut value_buffer, action);
    if let Some(text) = outcome.clipboard_write {
        let _ = ctx.write_clipboard_text(&text);
    }
    let text_after = value_buffer.as_str();
    if outcome.text_changed {
        buffer.set(value_buffer);
        value.set(text_after.clone());
    }
    if outcome.state_changed || outcome.text_changed {
        if !reveal_caret_multi_line(&mut edit_state, bounds, layout, &text_after, style) {
            pending_reveal.set(true);
        } else if pending_reveal.read() {
            pending_reveal.set(false);
        }
        edit.set(edit_state);
        ctx.request_repaint();
    }
}

/// Maps a pointer position to a single-line UTF-8 byte offset.
///
/// Coordinates include horizontal scroll and content padding. A matching text
/// artifact performs shaped hit testing. Without one, the fallback estimates
/// `0.6 × font_px` per byte and length-clamps the result; that approximation is
/// not guaranteed to land on a Unicode character boundary.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("hello".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value);
/// let _ = input; // pointer placement uses shaped layout when available
/// ```
pub(crate) fn byte_at_point(
    bounds: Rect,
    pos: ailloli_ui_core::Point,
    layout: &LayoutResult,
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
    edit: &Signal<TextEditState>,
    style: TextInputStyle,
) -> usize {
    let buffer_value = read_display_buffer(value, buffer);
    let value_text = buffer_value.as_str();
    let edit_state = edit.read();
    let (display, _) = display_text_for_edit(
        &value_text,
        edit_state.caret_byte.min(value_text.len()),
        edit_state.preedit.as_ref(),
    );
    let content = text_input_content_rect(bounds, style);
    let local_x = (pos.x - content.x + edit_state.scroll_x).max(0.0);
    let Some(layout) = text_layout_from_artifact(layout, &display) else {
        let approx = (local_x / (style.text.px_size as f32 * 0.6).max(1.0)).round() as usize;
        return approx.min(value_text.len());
    };
    layout
        .caret_index_at_point(local_x, 0.0)
        .min(value_text.len())
}

/// Maps a pointer position to a multi-line UTF-8 byte offset.
///
/// Both scroll offsets and content padding are applied. Matching shaped layout
/// performs two-dimensional hit testing. The no-artifact fallback estimates
/// from x only and may return a non-character-boundary byte for Unicode text.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("one\ntwo".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value).multiline();
/// let _ = input;
/// ```
pub(crate) fn byte_at_point_multi_line(
    bounds: Rect,
    pos: ailloli_ui_core::Point,
    layout: &LayoutResult,
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
    edit: &Signal<TextEditState>,
    style: TextInputStyle,
) -> usize {
    let buffer_value = read_display_buffer(value, buffer);
    let value_text = buffer_value.as_str();
    let edit_state = edit.read();
    let (display, _) = display_text_for_edit(
        &value_text,
        edit_state.caret_byte.min(value_text.len()),
        edit_state.preedit.as_ref(),
    );
    let content = text_input_content_rect(bounds, style);
    let local_x = (pos.x - content.x + edit_state.scroll_x).max(0.0);
    let local_y = (pos.y - content.y + edit_state.scroll_y).max(0.0);
    let Some(layout) = text_layout_from_artifact(layout, &display) else {
        let approx = (local_x / (style.text.px_size as f32 * 0.6).max(1.0)).round() as usize;
        return approx.min(value_text.len());
    };
    layout
        .caret_index_at_point(local_x, local_y)
        .min(value_text.len())
}

/// Adjusts single-line horizontal scroll so the display caret is visible.
///
/// The current IME preedit is included in display/caret mapping. Matching layout
/// supplies shaped caret geometry; otherwise width is estimated as
/// `0.6 × font_px` per byte. The updated edit state is written to both the
/// mutable argument and its signal, even when the offset is unchanged.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("a long value".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value);
/// let _ = input; // edits reveal the caret horizontally
/// ```
pub(crate) fn reveal_caret(
    edit: &Signal<TextEditState>,
    edit_state: &mut TextEditState,
    bounds: Rect,
    layout: &LayoutResult,
    value: &str,
    style: TextInputStyle,
) {
    let (display, caret) = display_text_for_edit(
        value,
        edit_state.caret_byte.min(value.len()),
        edit_state.preedit.as_ref(),
    );
    let metrics = scroll_metrics(bounds, layout, &display, style);
    let caret_x = text_layout_from_artifact(layout, &display)
        .map(|layout| layout.caret_x_at(caret))
        .unwrap_or_else(|| caret as f32 * (style.text.px_size as f32 * 0.6).max(1.0));
    let state = ScrollState::with_offset(Offset::new(edit_state.scroll_x, 0.0));
    let outcome = state.reveal_rect(
        Rect::new(caret_x, 0.0, 1.0, style.text.px_size as f32 + 2.0),
        metrics,
        ScrollAxes::HORIZONTAL,
    );
    edit_state.scroll_x = outcome.after.x;
    edit.set(edit_state.clone());
}

/// Reveals a multi-line display caret using a matching layout artifact.
///
/// Returns `false` when no artifact contains exactly the composed display text;
/// in that case the caller should retry after layout. Returns `true` when the
/// reveal was resolved, even if neither scroll offset changed.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("first\nsecond".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value).multiline();
/// let _ = input; // reveal is deferred when the current artifact is stale
/// ```
pub(crate) fn reveal_caret_multi_line(
    edit_state: &mut TextEditState,
    bounds: Rect,
    layout: &LayoutResult,
    value: &str,
    style: TextInputStyle,
) -> bool {
    let (display, caret) = display_text_for_edit(
        value,
        edit_state.caret_byte.min(value.len()),
        edit_state.preedit.as_ref(),
    );
    let Some(layout) = text_layout_from_artifact(layout, &display) else {
        return false;
    };
    reveal_caret_multi_line_with_layout(edit_state, bounds, layout, caret, style);
    true
}

/// Re-reads current signals and attempts a multi-line caret reveal.
///
/// Returns whether a matching layout artifact was available. When available,
/// the edit signal is written back even if its scroll offsets remain equal.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("text".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value).multiline();
/// let _ = input; // committed layout retries any pending caret reveal
/// ```
pub(crate) fn reveal_caret_multi_line_from_current_layout(
    bounds: Rect,
    layout: &LayoutResult,
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
    edit: &Signal<TextEditState>,
    style: TextInputStyle,
) -> bool {
    let buffer_value = read_display_buffer(value, buffer);
    let value_text = buffer_value.as_str();
    let mut edit_state = edit.read();
    let changed = reveal_caret_multi_line(&mut edit_state, bounds, layout, &value_text, style);
    if changed {
        edit.set(edit_state);
    }
    changed
}

/// Reveals a shaped multi-line caret across both scroll axes.
///
/// The target caret rectangle is inflated by four logical pixels before reveal.
/// Returns `true` only if either stored scroll offset changed by more than
/// [`f32::EPSILON`]; it always writes the resolved offsets to `edit_state`.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("many\nlines".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value).multiline();
/// let _ = input;
/// ```
pub(crate) fn reveal_caret_multi_line_with_layout(
    edit_state: &mut TextEditState,
    bounds: Rect,
    layout: &TextLayoutHandle,
    caret: usize,
    style: TextInputStyle,
) -> bool {
    let content = text_input_content_rect(bounds, style);
    let metrics = ScrollMetrics::new(
        Size::new(content.w, content.h),
        Size::new(
            layout.width().max(content.w),
            layout.height().max(content.h),
        ),
    );
    let caret_rect = layout.caret_rect_at(caret, style.caret_w);
    let state = ScrollState::with_offset(Offset::new(edit_state.scroll_x, edit_state.scroll_y));
    let outcome = state.reveal_rect(caret_rect.inflate(4.0, 4.0), metrics, ScrollAxes::BOTH);
    let changed = (edit_state.scroll_x - outcome.after.x).abs() > f32::EPSILON
        || (edit_state.scroll_y - outcome.after.y).abs() > f32::EPSILON;
    edit_state.scroll_x = outcome.after.x;
    edit_state.scroll_y = outcome.after.y;
    changed
}

/// Resolves single-line horizontal viewport and content extents.
///
/// Padded bounds define the viewport. A matching artifact supplies shaped text
/// width; otherwise UTF-8 byte length times `0.6 × font_px` is used. Content
/// width is never smaller than viewport width, and content height equals it.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("scroll horizontally".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value);
/// let _ = input;
/// ```
pub(crate) fn scroll_metrics(
    bounds: Rect,
    layout: &LayoutResult,
    display: &str,
    style: TextInputStyle,
) -> ScrollMetrics {
    let content = text_input_content_rect(bounds, style);
    let text_width = text_layout_from_artifact(layout, display)
        .map(|layout| layout.width())
        .unwrap_or_else(|| display.len() as f32 * (style.text.px_size as f32 * 0.6).max(1.0));
    ScrollMetrics::new(
        Size::new(content.w, content.h),
        Size::new(text_width.max(content.w), content.h),
    )
}

/// Resolves multi-line viewport and two-axis content extents.
///
/// Matching layout supplies shaped width and height. Without it, width is
/// approximated per UTF-8 byte and height as `1.2 × font_px`. Each content axis
/// is floored at the corresponding padded viewport extent.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("wrapped content".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value).multiline();
/// let _ = input;
/// ```
pub(crate) fn multi_line_scroll_metrics(
    bounds: Rect,
    layout: &LayoutResult,
    display: &str,
    style: TextInputStyle,
) -> ScrollMetrics {
    let content = text_input_content_rect(bounds, style);
    let (text_width, text_height) = text_layout_from_artifact(layout, display)
        .map(|layout| (layout.width(), layout.height()))
        .unwrap_or_else(|| {
            (
                display.len() as f32 * (style.text.px_size as f32 * 0.6).max(1.0),
                style.text.px_size as f32 * 1.2,
            )
        });
    ScrollMetrics::new(
        Size::new(content.w, content.h),
        Size::new(text_width.max(content.w), text_height.max(content.h)),
    )
}

/// Returns a buffer representing the public value without mutating stored state.
///
/// When the persistent buffer already equals the external string it is cloned,
/// preserving its representation. Otherwise a temporary buffer is rebuilt from
/// the external value, which therefore wins for layout and painting.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("external".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value.clone());
/// value.set("new external".into());
/// assert_eq!(value.read(), "new external");
/// let _ = input;
/// ```
pub(crate) fn read_display_buffer(
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
) -> TextBuffer {
    let external = value.read();
    let buffer_value = buffer.read();
    if buffer_value.as_str() == external {
        buffer_value
    } else {
        TextBuffer::from_string(external)
    }
}

/// Returns a buffer representing the public value and repairs stale storage.
///
/// Equal state returns the existing buffer clone without invalidation. A stale
/// buffer is rebuilt from the external string, stored through its signal, and
/// returned. Editing paths call this before applying actions.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("initial".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value.clone());
/// value.set("replacement".into());
/// let _ = input; // the next edit rebuilds its persistent buffer first
/// ```
pub(crate) fn sync_buffer_from_value(
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
) -> TextBuffer {
    let external = value.read();
    let buffer_value = buffer.read();
    if buffer_value.as_str() == external {
        buffer_value
    } else {
        let synced = TextBuffer::from_string(external);
        buffer.set(synced.clone());
        synced
    }
}

/// Borrows a text artifact only when its exact source equals `expected_text`.
///
/// Non-text artifacts, absent artifacts, and even one-byte text differences
/// return `None`, preventing stale caret/hit geometry from being reused.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("exact".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value);
/// let _ = input; // layout artifacts are reused only while their text matches
/// ```
pub(crate) fn text_layout_from_artifact<'a>(
    layout: &'a LayoutResult,
    expected_text: &str,
) -> Option<&'a TextLayoutHandle> {
    match layout.artifact.as_ref()? {
        LayoutArtifact::Text(layout) if layout.text() == expected_text => Some(layout),
        _ => None,
    }
}

/// Resolves the single-line system IME candidate-window anchor.
///
/// Returns `None` without an exact text layout artifact. X follows the composed
/// display caret minus horizontal scroll. The rectangle uses a fixed six-pixel
/// vertical inset; bounds shorter than 12 pixels can therefore produce a
/// negative height and should be avoided by callers.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new(String::new())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value);
/// let _ = input; // the runtime asks the widget for this IME anchor
/// ```
pub(crate) fn ime_cursor_rect(
    bounds: Rect,
    layout: &LayoutResult,
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
    edit: &Signal<TextEditState>,
    style: TextInputStyle,
) -> Option<Rect> {
    let buffer_value = read_display_buffer(value, buffer);
    let value_text = buffer_value.as_str();
    let edit_state = edit.read();
    let (display, caret) = display_text_for_edit(
        &value_text,
        edit_state.caret_byte.min(value_text.len()),
        edit_state.preedit.as_ref(),
    );
    let layout_handle = text_layout_from_artifact(layout, &display)?;
    let content = text_input_content_rect(bounds, style);
    let x = content.x - edit_state.scroll_x + layout_handle.caret_x_at(caret);
    Some(Rect::new(x, bounds.y + 6.0, 1.0, bounds.h.max(1.0) - 12.0))
}

/// Resolves the multi-line system IME candidate-window anchor.
///
/// Returns `None` without an exact display-text artifact. Both scroll offsets
/// are subtracted from shaped caret geometry. Width is the configured caret
/// width; height is clamped to at least one logical pixel.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new("one\ntwo".into())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value).multiline();
/// let _ = input;
/// ```
pub(crate) fn ime_cursor_rect_multi_line(
    bounds: Rect,
    layout: &LayoutResult,
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
    edit: &Signal<TextEditState>,
    style: TextInputStyle,
) -> Option<Rect> {
    let buffer_value = read_display_buffer(value, buffer);
    let value_text = buffer_value.as_str();
    let edit_state = edit.read();
    let (display, caret) = display_text_for_edit(
        &value_text,
        edit_state.caret_byte.min(value_text.len()),
        edit_state.preedit.as_ref(),
    );
    let layout_handle = text_layout_from_artifact(layout, &display)?;
    let content = text_input_content_rect(bounds, style);
    let caret = layout_handle.caret_rect_at(caret, style.caret_w);
    Some(Rect::new(
        content.x - edit_state.scroll_x + caret.x,
        content.y - edit_state.scroll_y + caret.y,
        style.caret_w,
        caret.h.max(1.0),
    ))
}
