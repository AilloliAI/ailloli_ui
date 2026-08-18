use crate::layout::layout_ext::finish_view_sized;
use ailloli_ui_core::event::{Event, ImePreedit};
#[cfg(test)]
use ailloli_ui_core::geometry::Size;
use ailloli_ui_core::geometry::{Constraints, Rect};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{Color, FontId, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy, InputRole, Selection};
use ailloli_ui_runtime::layout::{LayoutArtifact, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawRRect};
use ailloli_ui_text::{TextBuffer, TextEditState, TextInputMode, TextSystem};
#[cfg(test)]
use ailloli_ui_text::{TextEditAction, TextLayoutParams, WrapMode};

use crate::text::editable_text::{draw_editable_mono_line, EditableTextStyle};

#[cfg(test)]
use super::text_field_core::{apply_edit_action, display_text_for_edit};
use super::text_field_core::{
    handle_multi_line_text_event, handle_single_line_text_event, ime_cursor_rect,
    ime_cursor_rect_multi_line, layout_multi_line_text, layout_single_line_text,
    paint_multi_line_text, paint_single_line_text, reveal_caret_multi_line_from_current_layout,
    TextFieldEventOptions,
};

pub type TextInputChangeHandler<A> = std::rc::Rc<dyn Fn(&mut EventCtx<A>, String)>;

// -----------------------------------------------------------------------------
// Legacy draw API (used by current app_ui). Kept for compatibility.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct TextInputStyle {
    pub bg: Color,
    pub border: Color,
    pub border_focused: Color,
    pub caret: Color,
    pub placeholder: Color,
    pub selection_bg: Color,
    pub radius: f32,
    pub pad_x: f32,
    pub pad_y: f32,
    pub text: TextStyle,
    pub caret_w: f32,
    pub caret_blink_ms: i64,
}

impl Default for TextInputStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl TextInputStyle {
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        let spacing = theme.spacing();
        Self {
            bg: palette.surface,
            border: palette.border,
            border_focused: palette.focus,
            caret: palette.text,
            placeholder: palette.text_muted,
            selection_bg: palette.accent.with_alpha(0.34),
            radius: theme.radius().input().tl,
            pad_x: spacing.md,
            pad_y: spacing.sm,
            text: TextStyle::new(FontId::Mono, 14, palette.text),
            caret_w: 1.0,
            caret_blink_ms: 500,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw_text_input(
    rect: Rect,
    text: &str,
    caret_byte: usize,
    selection: Option<Selection>,
    preedit: Option<&ImePreedit>,
    focused: bool,
    now_ms: i64,
    style: TextInputStyle,
    text_system: &mut TextSystem,
) -> Vec<DrawCmd> {
    let mut out = Vec::new();

    out.push(DrawCmd::RRect(DrawRRect {
        rect,
        radius: style.radius,
        color: style.bg,
    }));
    out.push(DrawCmd::Border(DrawBorder {
        rect,
        radius: Radius::uniform(style.radius),
        border: Border::new(
            1.0,
            if focused {
                style.border_focused
            } else {
                style.border
            },
        ),
    }));

    let baseline_x = rect.x + style.pad_x;
    let baseline_y = rect.y + style.pad_y + (style.text.px_size as f32);

    let editable = EditableTextStyle {
        text: style.text,
        caret: style.caret,
        caret_w: style.caret_w,
        caret_blink_ms: style.caret_blink_ms,
        selection_bg: Some(style.selection_bg),
    };

    out.extend(draw_editable_mono_line(
        baseline_x,
        baseline_y,
        text,
        caret_byte,
        selection,
        preedit,
        focused,
        now_ms,
        editable,
        text_system,
    ));

    out
}

// -----------------------------------------------------------------------------
// Widget API (ailloli_ui_runtime View/Widget)
// -----------------------------------------------------------------------------

/// Single-line text field with focus, selection, IME, and `TextBuffer` backing.
pub struct TextInput<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    value: Option<Signal<String>>,
    placeholder: Option<Binding<String>>,
    style: TextInputStyle,
    mode: TextInputMode,
    on_change: Option<TextInputChangeHandler<A>>,
}

crate::impl_layout_builders!(TextInput);

impl<A: 'static> Default for TextInput<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> TextInput<A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            value: None,
            placeholder: None,
            style: TextInputStyle::default(),
            mode: TextInputMode::SingleLine,
            on_change: None,
        }
    }

    pub fn bind(mut self, value: impl Into<Signal<String>>) -> Self {
        self.value = Some(value.into());
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<Binding<String>>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.style.text.px_size = size.round().max(1.0) as u16;
        self
    }

    pub fn font_family(mut self, font: FontId) -> Self {
        self.style.text.font = font;
        self
    }

    pub fn text_style(mut self, style: TextStyle) -> Self {
        self.style.text = style;
        self
    }

    pub fn input_style(mut self, style: TextInputStyle) -> Self {
        self.style = style;
        self
    }

    pub fn multiline(mut self) -> Self {
        self.mode = TextInputMode::MultiLine;
        self
    }

    pub fn on_change(mut self, f: impl Fn(String) -> A + 'static) -> Self {
        self.on_change = Some(std::rc::Rc::new(move |ctx, value| ctx.dispatch(f(value))));
        self
    }

    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, String) + 'static) -> Self {
        self.on_change = Some(std::rc::Rc::new(f));
        self
    }
}

pub struct TextInputWidget<A = ()> {
    layout: LayoutStyle,
    value: Signal<String>,
    buffer: Signal<TextBuffer>,
    edit: Signal<TextEditState>,
    pending_reveal: Signal<bool>,
    placeholder: Option<Binding<String>>,
    style: TextInputStyle,
    mode: TextInputMode,
    on_change: Option<TextInputChangeHandler<A>>,
}

impl<A: 'static> Widget<A> for TextInputWidget<A> {
    fn debug_name(&self) -> &'static str {
        "TextInput"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let (size, text_layout) = if self.mode == TextInputMode::MultiLine {
            layout_multi_line_text(
                ctx,
                constraints,
                self.layout,
                &self.value,
                &self.buffer,
                &self.edit,
                self.placeholder.as_ref().map(|p| p.read()),
                self.style,
            )
        } else {
            layout_single_line_text(
                ctx,
                constraints,
                self.layout,
                &self.value,
                &self.buffer,
                &self.edit,
                self.placeholder.as_ref().map(|p| p.read()),
                self.style,
            )
        };

        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: text_layout.map(LayoutArtifact::Text),
        }
    }

    fn layout_committed(&self, _ctx: &mut LayoutCtx<'_>, bounds: Rect, layout: &LayoutResult) {
        if self.mode != TextInputMode::MultiLine || !self.pending_reveal.read() {
            return;
        }
        self.pending_reveal.set(false);
        let _ = reveal_caret_multi_line_from_current_layout(
            bounds,
            layout,
            &self.value,
            &self.buffer,
            &self.edit,
            self.style,
        );
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, layout: &LayoutResult) {
        let style = self.style;
        let focused = ctx.is_focused();

        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: bounds,
            radius: style.radius,
            color: style.bg,
        }));
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: bounds,
            radius: Radius::uniform(style.radius),
            border: Border::new(
                1.0,
                if focused {
                    style.border_focused
                } else {
                    style.border
                },
            ),
        }));
        if self.mode == TextInputMode::MultiLine {
            paint_multi_line_text(
                ctx,
                bounds,
                layout,
                &self.value,
                &self.buffer,
                &self.edit,
                self.placeholder.as_ref().map(|p| p.read()),
                style,
                focused,
            );
        } else {
            paint_single_line_text(
                ctx,
                bounds,
                layout,
                &self.value,
                &self.buffer,
                &self.edit,
                self.placeholder.as_ref().map(|p| p.read()),
                style,
                focused,
            );
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, layout: &LayoutResult) {
        let before = self.value.read();
        if self.mode == TextInputMode::MultiLine {
            handle_multi_line_text_event(
                ctx,
                event,
                bounds,
                layout,
                &self.value,
                &self.buffer,
                &self.edit,
                &self.pending_reveal,
                self.style,
                TextFieldEventOptions::default(),
            );
        } else {
            handle_single_line_text_event(
                ctx,
                event,
                bounds,
                layout,
                &self.value,
                &self.buffer,
                &self.edit,
                self.style,
                TextFieldEventOptions::default(),
            );
        }
        let after = self.value.read();
        if after != before {
            if let Some(on_change) = &self.on_change {
                on_change(ctx, after);
            }
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::Focusable
    }

    fn input_role(&self) -> InputRole {
        if self.mode == TextInputMode::MultiLine {
            InputRole::TextMultiLine
        } else {
            InputRole::TextSingleLine
        }
    }

    fn ime_cursor_rect(&self, bounds: Rect, layout: &LayoutResult) -> Option<Rect> {
        if self.mode == TextInputMode::MultiLine {
            ime_cursor_rect_multi_line(
                bounds,
                layout,
                &self.value,
                &self.buffer,
                &self.edit,
                self.style,
            )
        } else {
            ime_cursor_rect(
                bounds,
                layout,
                &self.value,
                &self.buffer,
                &self.edit,
                self.style,
            )
        }
    }
}

struct TextInputComponent<A> {
    layout: LayoutStyle,
    value: Signal<String>,
    placeholder: Option<Binding<String>>,
    style: TextInputStyle,
    mode: TextInputMode,
    on_change: Option<TextInputChangeHandler<A>>,
}

impl<A: 'static> ComponentNode<A> for TextInputComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let edit = context.signal(TextEditState::new());
        let buffer = context.signal(TextBuffer::from_string(self.value.read()));
        let pending_reveal = context.signal(false);

        View::leaf(TextInputWidget {
            layout: self.layout,
            value: self.value.clone(),
            buffer,
            edit,
            pending_reveal,
            placeholder: self.placeholder.clone(),
            style: self.style,
            mode: self.mode,
            on_change: self.on_change.clone(),
        })
    }
}

impl<A: 'static> IntoView<A> for TextInput<A> {
    fn into_view(self) -> View<A> {
        let value = self
            .value
            .expect("TextInput::bind(...) is required for now");

        finish_view_sized(
            View::component(TextInputComponent {
                layout: self.layout,
                value,
                placeholder: self.placeholder,
                style: self.style,
                mode: self.mode,
                on_change: self.on_change,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailloli_ui_core::ElementId;
    use ailloli_ui_runtime::app::RuntimeHandle;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn signal<T: 'static>(value: T) -> Signal<T> {
        Signal::new(Rc::new(RefCell::new(value)), Rc::new(|| {}))
    }

    fn widget_with_value(value: &str) -> TextInputWidget {
        TextInputWidget {
            layout: LayoutStyle::default(),
            value: signal(value.to_string()),
            buffer: signal(TextBuffer::from_string(value.to_string())),
            edit: signal(TextEditState::new()),
            pending_reveal: signal(false),
            placeholder: None,
            style: TextInputStyle {
                text: TextStyle::new(FontId::Mono, 14, Color::new(1.0, 1.0, 1.0, 1.0)),
                ..TextInputStyle::default()
            },
            mode: TextInputMode::SingleLine,
            on_change: None,
        }
    }

    #[test]
    fn display_text_for_edit_inserts_preedit_without_mutating_text() {
        let preedit = ImePreedit {
            text: "é".into(),
            selection: None,
        };
        let (display, caret) = display_text_for_edit("caf", 3, Some(&preedit));

        assert_eq!(display, "café");
        assert_eq!(caret, "café".len());
    }

    #[test]
    fn text_input_ime_cursor_uses_layout_artifact() {
        let widget = widget_with_value("hello");
        let mut ts = TextSystem::new();
        let handle = ts.layout_cached(TextLayoutParams {
            text: "hello",
            style: widget.style.text,
            max_width: Some(200.0),
            wrap_mode: WrapMode::NoWrap,
        });
        let layout = LayoutResult {
            size: Size::new(200.0, 40.0),
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, 200.0, 40.0),
            visual_bounds: Rect::new(0.0, 0.0, 200.0, 40.0),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: Some(LayoutArtifact::Text(handle)),
        };

        let rect = <TextInputWidget as Widget<()>>::ime_cursor_rect(
            &widget,
            Rect::new(0.0, 0.0, 200.0, 40.0),
            &layout,
        )
        .expect("ime cursor rect");

        assert!(rect.h > 0.0);
    }

    #[test]
    fn text_input_edit_updates_persistent_buffer_and_public_string() {
        let widget = widget_with_value("a");
        let runtime = RuntimeHandle::<()>::new();
        let mut ctx = EventCtx::new(runtime, ElementId(1));
        let layout = LayoutResult {
            size: Size::new(200.0, 40.0),
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, 200.0, 40.0),
            visual_bounds: Rect::new(0.0, 0.0, 200.0, 40.0),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        };

        apply_edit_action(
            &mut ctx,
            &widget.value,
            &widget.buffer,
            &widget.edit,
            TextEditAction::InsertText {
                text: "!".to_string(),
            },
            Rect::new(0.0, 0.0, 200.0, 40.0),
            &layout,
            widget.style,
        );

        assert_eq!(widget.buffer.read().as_str(), "!a");
        assert_eq!(widget.value.read(), "!a");
    }
}
