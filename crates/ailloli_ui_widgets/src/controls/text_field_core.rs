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
pub(crate) struct TextFieldEventOptions {
    pub consume_handled_events: bool,
}

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

pub(crate) fn text_input_content_rect(bounds: Rect, style: TextInputStyle) -> Rect {
    Rect::new(
        bounds.x + style.pad_x,
        bounds.y + style.pad_y,
        (bounds.w - style.pad_x * 2.0).max(0.0),
        (bounds.h - style.pad_y * 2.0).max(0.0),
    )
}

#[allow(clippy::too_many_arguments)]
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
        Event::Ime(ImeEvent::End) => {
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
        _ => false,
    };

    if handled && options.consume_handled_events {
        ctx.stop_propagation();
    }
    handled
}

#[allow(clippy::too_many_arguments)]
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
        Event::Ime(ImeEvent::End) => {
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
        _ => false,
    };

    if handled && options.consume_handled_events {
        ctx.stop_propagation();
    }
    handled
}

#[allow(clippy::too_many_arguments)]
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

pub(crate) fn text_layout_from_artifact<'a>(
    layout: &'a LayoutResult,
    expected_text: &str,
) -> Option<&'a TextLayoutHandle> {
    match layout.artifact.as_ref()? {
        LayoutArtifact::Text(layout) if layout.text() == expected_text => Some(layout),
        _ => None,
    }
}

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
