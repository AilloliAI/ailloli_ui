//! Single- and multi-line editable text widgets plus a legacy draw helper.

use std::cell::Cell;

use crate::layout::layout_ext::finish_view_sized;
use crate::transactional_layout::TransactionalLayoutPending;
use ailloli_ui_core::event::{Event, ImePreedit};
#[cfg(test)]
use ailloli_ui_core::geometry::Size;
use ailloli_ui_core::geometry::{Constraints, Rect};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{Color, FontId, ScrollbarAxis, TextStyle, Theme};
use ailloli_ui_runtime::component::reactive::with_untracked_reads;
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{ActivationPolicy, EventCtx, FocusPolicy, InputRole, Selection};
use ailloli_ui_runtime::layout::{LayoutArtifact, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawRRect, Invalidation};
use ailloli_ui_text::{TextBuffer, TextEditState, TextInputMode, TextSystem};
#[cfg(test)]
use ailloli_ui_text::{TextEditAction, TextLayoutParams, WrapMode};

use crate::text::editable_text::{draw_editable_mono_line, EditableTextStyle};

#[cfg(test)]
use super::text_field_core::apply_edit_action;
use super::text_field_core::{
    display_text_for_edit, handle_multi_line_text_event, handle_single_line_text_event,
    ime_cursor_rect, ime_cursor_rect_multi_line, layout_multi_line_text, layout_single_line_text,
    multi_line_scrollbar_geometry, paint_committed_single_line_text, paint_multi_line_text,
    read_display_buffer, reveal_caret_multi_line_from_current_layout, TextFieldEventOptions,
};
use crate::scrollbar::{thumb_color_for_state, ScrollbarInteraction, ScrollbarVisualState};

/// Shared callback receiving a complete owned value after an edit changes text.
///
/// It is not called for caret, selection, scroll, or preedit-only changes. The
/// callback is synchronous and may dispatch zero or more application actions.
///
/// # Examples
///
/// ```
/// use std::rc::Rc;
/// use ailloli_ui_widgets::controls::text_input::TextInputChangeHandler;
/// let handler: TextInputChangeHandler<()> = Rc::new(|ctx, _value| ctx.request_repaint());
/// let _ = handler;
/// ```
pub type TextInputChangeHandler<A> = std::rc::Rc<dyn Fn(&mut EventCtx<A>, String)>;

// -----------------------------------------------------------------------------
// Legacy draw API (used by current app_ui). Kept for compatibility.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
/// Colors, typography, logical-pixel metrics, and caret cadence for text input.
///
/// `caret_blink_ms` is one visible or hidden half-period in milliseconds; a
/// non-positive value suppresses the caret. The legacy [`draw_text_input`]
/// cannot display `placeholder` because it has no placeholder argument, while
/// [`TextInput`] uses it when its bound value and IME preedit are empty.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::TextInputStyle;
/// let style = TextInputStyle::from_theme(Theme::dark());
/// assert_eq!(style.text.px_size, 14);
/// assert_eq!(style.caret_blink_ms, 500);
/// ```
pub struct TextInputStyle {
    /// Input background fill.
    pub bg: Color,
    /// Unfocused one-pixel border color.
    pub border: Color,
    /// Focused one-pixel border color.
    pub border_focused: Color,
    /// Caret fill color.
    pub caret: Color,
    /// Placeholder glyph color used by the retained widget.
    pub placeholder: Color,
    /// Selection background color, hidden during IME preedit.
    pub selection_bg: Color,
    /// Uniform corner radius in logical pixels.
    pub radius: f32,
    /// Horizontal content inset in logical pixels.
    pub pad_x: f32,
    /// Vertical content inset in logical pixels.
    pub pad_y: f32,
    /// Editable text font, integer logical-pixel size, color, and decoration.
    pub text: TextStyle,
    /// Caret width in logical pixels.
    pub caret_w: f32,
    /// Blink half-period in milliseconds; non-positive disables the caret.
    pub caret_blink_ms: i64,
}

impl Default for TextInputStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl TextInputStyle {
    /// Resolves input colors, spacing, radius, and mono typography from `theme`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{FontId, Theme};
    /// use ailloli_ui_widgets::controls::TextInputStyle;
    /// let style = TextInputStyle::from_theme(Theme::dark());
    /// assert_eq!(style.text.font, FontId::Mono);
    /// assert_eq!(style.caret_w, 1.0);
    /// ```
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
/// Draws one legacy single-line text input into newly owned commands.
///
/// Paint order is background, border, optional selection, glyphs, then optional
/// caret. `caret_byte` and [`Selection`] offsets use UTF-8 bytes and are
/// length-clamped. Selection is omitted during IME preedit. A focused caret is
/// visible on even `now_ms / caret_blink_ms` half-periods; non-positive cadence
/// suppresses it. This helper does not clip horizontally or draw placeholder
/// text and performs no event handling.
///
/// # Panics
///
/// Panics when a non-empty preedit is inserted at a clamped `caret_byte` that
/// is not a UTF-8 character boundary.
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
///     Rect::new(0.0, 0.0, 200.0, 36.0), "hello", 5, None, None,
///     false, 0, TextInputStyle::default(), &mut text,
/// );
/// assert_eq!(commands.len(), 3); // background, border, glyphs
/// ```
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

/// Editable text field with focus, selection, IME, scrolling, and buffer backing.
///
/// The default mode is single-line; [`Self::multiline`] enables word-or-anywhere
/// wrapping and two-axis caret reveal. The widget is always focusable and must
/// be attached to a writable [`Signal<String>`] with [`Self::bind`]. External
/// signal changes take precedence and are synchronized into the edit buffer on
/// the next edit. Placeholder bindings never become the editable value.
/// Single-line inputs accept native horizontal wheel deltas and `Shift` plus a
/// vertical wheel delta. Multiline inputs expose an interactive overlay thumb
/// that supports centered track clicks and captured dragging without moving the caret or
/// invoking the change callback.
///
/// # Panics
///
/// Converting the builder into a view panics if [`Self::bind`] was not called.
///
/// # Examples
///
/// ```
/// use std::{cell::RefCell, rc::Rc};
/// use ailloli_ui_runtime::component::Signal;
/// use ailloli_ui_widgets::controls::TextInput;
/// let value = Signal::new(Rc::new(RefCell::new(String::new())), Rc::new(|| {}));
/// let input: TextInput<()> = TextInput::new().bind(value).placeholder("Name");
/// let _ = input;
/// ```
pub struct TextInput<A = ()> {
    /// Layout configuration used to resolve intrinsic text geometry.
    pub(crate) layout: LayoutStyle,
    /// Flex-item behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Required writable public string value.
    value: Option<Signal<String>>,
    /// Optional static or reactive placeholder.
    placeholder: Option<Binding<String>>,
    /// Resolved paint and text metrics.
    style: TextInputStyle,
    /// Single-line or multi-line editing/keymap mode.
    mode: TextInputMode,
    /// Optional callback receiving changed complete text.
    on_change: Option<TextInputChangeHandler<A>>,
}

crate::impl_layout_builders!(TextInput);

impl<A: 'static> Default for TextInput<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> TextInput<A> {
    /// Creates an unbound, single-line input with the default style.
    ///
    /// The builder cannot become a view until [`Self::bind`] supplies its value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TextInput;
    /// let input: TextInput<()> = TextInput::new();
    /// let _ = input; // bind before converting into a view
    /// ```
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

    /// Sets the required writable string signal, replacing any previous one.
    ///
    /// User edits update both the persistent [`TextBuffer`] and this signal
    /// before the optional change callback runs.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// use ailloli_ui_widgets::controls::TextInput;
    /// let value = Signal::new(Rc::new(RefCell::new("Ada".to_string())), Rc::new(|| {}));
    /// let input: TextInput<()> = TextInput::new().bind(value);
    /// let _ = input;
    /// ```
    pub fn bind(mut self, value: impl Into<Signal<String>>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Sets static or reactive placeholder text.
    ///
    /// It is displayed only when both the bound value and active IME preedit
    /// are empty. An empty placeholder remains present but paints no glyphs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TextInput;
    /// let input: TextInput<()> = TextInput::new().placeholder("Search");
    /// let _ = input;
    /// ```
    pub fn placeholder(mut self, placeholder: impl Into<Binding<String>>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Sets text size from a floating logical-pixel value.
    ///
    /// The value is rounded, clamped to at least `1`, then saturated to `u16`;
    /// `NaN` and negative infinity therefore become `1`, while positive
    /// infinity becomes `u16::MAX`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TextInput;
    /// let input: TextInput<()> = TextInput::new().size(15.6);
    /// let _ = input; // resolved text size is 16 logical pixels
    /// ```
    pub fn size(mut self, size: f32) -> Self {
        self.style.text.px_size = size.round().max(1.0) as u16;
        self
    }

    /// Replaces only the editable text font identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::FontId;
    /// use ailloli_ui_widgets::controls::TextInput;
    /// let input: TextInput<()> = TextInput::new().font_family(FontId::Ui);
    /// let _ = input;
    /// ```
    pub fn font_family(mut self, font: FontId) -> Self {
        self.style.text.font = font;
        self
    }

    /// Replaces the complete editable text style.
    ///
    /// Placeholder and selection colors remain in [`TextInputStyle`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_widgets::controls::TextInput;
    /// let input: TextInput<()> =
    ///     TextInput::new().text_style(TextStyle::new(FontId::Ui, 16, Color::WHITE));
    /// let _ = input;
    /// ```
    pub fn text_style(mut self, style: TextStyle) -> Self {
        self.style.text = style;
        self
    }

    /// Replaces every paint, spacing, typography, and caret style value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TextInput, TextInputStyle};
    /// let input: TextInput<()> = TextInput::new().input_style(TextInputStyle::default());
    /// let _ = input;
    /// ```
    pub fn input_style(mut self, style: TextInputStyle) -> Self {
        self.style = style;
        self
    }

    /// Enables multi-line key handling, wrapping, scrolling, and IME geometry.
    ///
    /// The builder has no inverse; create a new input to return to single-line
    /// mode. Height remains layout-driven and may grow from wrapped content.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TextInput;
    /// let input: TextInput<()> = TextInput::new().multiline();
    /// let _ = input;
    /// ```
    pub fn multiline(mut self) -> Self {
        self.mode = TextInputMode::MultiLine;
        self
    }

    /// Maps each changed complete value to an application action and dispatches it.
    ///
    /// The callback is not invoked for selection, caret, scrolling, or
    /// preedit-only changes. A later change-handler builder replaces it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TextInput;
    /// enum Action { Changed(String) }
    /// let input = TextInput::new().on_change(Action::Changed);
    /// let _ = input;
    /// ```
    pub fn on_change(mut self, f: impl Fn(String) -> A + 'static) -> Self {
        self.on_change = Some(std::rc::Rc::new(move |ctx, value| ctx.dispatch(f(value))));
        self
    }

    /// Installs a context-aware callback for changed complete values.
    ///
    /// A later change-handler builder replaces it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TextInput;
    /// let input = TextInput::<()>::new().on_change_ctx(|ctx, _value| ctx.request_repaint());
    /// let _ = input;
    /// ```
    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, String) + 'static) -> Self {
        self.on_change = Some(std::rc::Rc::new(f));
        self
    }
}

/// Retained text-field leaf created internally by [`TextInput`].
///
/// Its stateful fields are intentionally private; public code should construct
/// [`TextInput`] instead. The type remains nameable for widget diagnostics.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::text_input::TextInputWidget;
/// assert!(std::any::type_name::<TextInputWidget<()>>().ends_with("TextInputWidget"));
/// ```
pub struct TextInputWidget<A = ()> {
    /// Layout copied from the public builder.
    layout: LayoutStyle,
    /// Public string signal shared with the consumer.
    value: Signal<String>,
    /// Persistent editing buffer synchronized against `value`.
    buffer: Signal<TextBuffer>,
    /// Caret, selection, IME, drag, and scroll state.
    edit: Signal<TextEditState>,
    /// Revision of the composed value/preedit text that can change shaping.
    display_revision: Signal<u64>,
    /// Requests a post-layout multi-line caret reveal when no artifact existed.
    pending_reveal: Signal<bool>,
    /// Retained hover and captured multiline scrollbar gesture.
    scrollbar_interaction: Signal<ScrollbarInteraction>,
    /// Geometry-derived interaction cleanup staged for the active layout attempt.
    pending_scrollbar_interaction:
        Cell<Option<TransactionalLayoutPending<Option<ScrollbarInteraction>>>>,
    /// Optional reactive placeholder.
    placeholder: Option<Binding<String>>,
    /// Paint, spacing, and text metrics.
    style: TextInputStyle,
    /// Keymap, wrapping, and scrolling mode.
    mode: TextInputMode,
    /// Optional changed-text callback.
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
        let _ = self.value.revision();
        let _ = self.buffer.revision();
        let _ = self.display_revision.read();
        let placeholder = self
            .placeholder
            .as_ref()
            .map(|placeholder| placeholder.read());
        let (size, text_layout) = with_untracked_reads(|| {
            if self.mode == TextInputMode::MultiLine {
                layout_multi_line_text(
                    ctx,
                    constraints,
                    self.layout,
                    &self.value,
                    &self.buffer,
                    &self.edit,
                    placeholder,
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
                    placeholder,
                    self.style,
                )
            }
        });

        let mut result = LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: text_layout.map(LayoutArtifact::Text),
        };
        let geometries = with_untracked_reads(|| {
            if self.mode == TextInputMode::MultiLine {
                multi_line_scrollbar_geometry(
                    Rect::new(0.0, 0.0, size.w, size.h),
                    &result,
                    &self.value,
                    &self.buffer,
                    &self.edit,
                    self.style,
                )
                .into_iter()
                .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        });
        let mut interaction = with_untracked_reads(|| self.scrollbar_interaction.read());
        let interaction = interaction
            .reconcile(ctx.layout_pass(), &geometries)
            .then_some(interaction);
        if ctx.layout_pass().is_committed() {
            self.pending_scrollbar_interaction
                .set(TransactionalLayoutPending::new(ctx, interaction));
        }
        result.overlay_hit_bounds = geometries
            .iter()
            .map(|geometry| geometry.hit_track)
            .collect();
        result
    }

    fn layout_committed(&self, ctx: &mut LayoutCtx<'_>, bounds: Rect, layout: &LayoutResult) {
        if let Some(interaction) = self
            .pending_scrollbar_interaction
            .take()
            .and_then(|pending| pending.into_committed(ctx))
            .flatten()
        {
            self.scrollbar_interaction.set(interaction);
        }
        if self.mode != TextInputMode::MultiLine
            || !with_untracked_reads(|| self.pending_reveal.read())
        {
            return;
        }
        self.pending_reveal.set(false);
        with_untracked_reads(|| {
            let _ = reveal_caret_multi_line_from_current_layout(
                bounds,
                layout,
                &self.value,
                &self.buffer,
                &self.edit,
                self.style,
            );
        });
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
            let geometry = multi_line_scrollbar_geometry(
                bounds,
                layout,
                &self.value,
                &self.buffer,
                &self.edit,
                style,
            );
            let visual = geometry
                .map(|geometry| {
                    self.scrollbar_interaction
                        .read()
                        .visual_state(geometry.axis, ctx.is_hovered())
                })
                .unwrap_or(ScrollbarVisualState::Normal);
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
                thumb_color_for_state(style.border.with_alpha(0.62), visual),
            );
        } else {
            paint_committed_single_line_text(
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
        if self.mode == TextInputMode::MultiLine && matches!(event, Event::Pointer(_)) {
            let geometry = multi_line_scrollbar_geometry(
                bounds,
                layout,
                &self.value,
                &self.buffer,
                &self.edit,
                self.style,
            );
            let geometries = geometry.into_iter().collect::<Vec<_>>();
            let edit_state = self.edit.read();
            let mut interaction = self.scrollbar_interaction.read();
            let response = interaction.handle_event(ctx, event, &geometries);
            if response.state_changed {
                self.scrollbar_interaction.set(interaction);
            }
            if let Some((axis, target)) = response.scroll_to {
                if axis == ScrollbarAxis::Vertical && target != edit_state.scroll_y {
                    let mut next = edit_state;
                    next.scroll_y = target;
                    self.edit.set(next);
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
        let before_display = self.composed_display_text();
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
        if self.composed_display_text() != before_display {
            self.display_revision
                .update(|revision| *revision = revision.wrapping_add(1).max(1));
        }
        if after != before {
            if let Some(on_change) = &self.on_change {
                on_change(ctx, after);
            }
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::Focusable
    }

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::AllowOnFocusOnly
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

impl<A> TextInputWidget<A> {
    /// Returns the exact value plus preedit string currently presented by paint.
    fn composed_display_text(&self) -> String {
        with_untracked_reads(|| {
            let buffer = read_display_buffer(&self.value, &self.buffer);
            let value = buffer.as_str();
            let edit = self.edit.read();
            display_text_for_edit(
                &value,
                edit.caret_byte.min(value.len()),
                edit.preedit.as_ref(),
            )
            .0
        })
    }
}

/// Component properties used to allocate persistent edit and buffer signals.
struct TextInputComponent<A> {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Caller-owned UTF-8 text value.
    value: Signal<String>,
    /// Optional reactive placeholder for an empty value.
    placeholder: Option<Binding<String>>,
    /// Text, border, caret, and selection styling.
    style: TextInputStyle,
    /// Single-line or multiline input behavior.
    mode: TextInputMode,
    /// Optional callback receiving text after a committed change.
    on_change: Option<TextInputChangeHandler<A>>,
}

impl<A: 'static> ComponentNode<A> for TextInputComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let edit = context.signal_with_invalidation(TextEditState::new(), Invalidation::Paint);
        let buffer = context.signal_with_invalidation_factory(
            || with_untracked_reads(|| TextBuffer::from_string(self.value.read())),
            Invalidation::Paint,
        );
        // This is administrative post-layout work, not a geometry input. Event
        // paths explicitly request layout when they arm it; clearing it after a
        // successful commit only needs the paint requested by the resulting
        // scroll-state update.
        let pending_reveal = context.signal_with_invalidation(false, Invalidation::Paint);
        let scrollbar_interaction =
            context.signal_with_invalidation(ScrollbarInteraction::default(), Invalidation::Paint);
        let display_revision = context.signal_with_invalidation(0_u64, Invalidation::Layout);

        View::leaf(TextInputWidget {
            layout: self.layout,
            value: self.value.clone(),
            buffer,
            edit,
            display_revision,
            pending_reveal,
            scrollbar_interaction,
            pending_scrollbar_interaction: Cell::new(None),
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
    //! Scenarios for IME display composition, layout-derived IME geometry, and
    //! synchronization of persistent edit buffers with the public string signal.

    use super::*;
    use ailloli_ui_core::ElementId;
    use ailloli_ui_runtime::app::RuntimeHandle;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Creates a deterministic signal whose invalidation callback is a no-op.
    fn signal<T: 'static>(value: T) -> Signal<T> {
        Signal::new(Rc::new(RefCell::new(value)), Rc::new(|| {}))
    }

    /// Builds a single-line retained widget with synchronized value and buffer.
    fn widget_with_value(value: &str) -> TextInputWidget {
        TextInputWidget {
            layout: LayoutStyle::default(),
            value: signal(value.to_string()),
            buffer: signal(TextBuffer::from_string(value.to_string())),
            edit: signal(TextEditState::new()),
            display_revision: signal(0),
            pending_reveal: signal(false),
            scrollbar_interaction: signal(ScrollbarInteraction::default()),
            pending_scrollbar_interaction: Cell::new(None),
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
        let preedit = ImePreedit::new("é");
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
    fn text_input_paint_skips_a_mismatched_committed_artifact() {
        let widget = widget_with_value("fresh");
        let mut text_system = TextSystem::new();
        let stale = text_system.layout_cached(TextLayoutParams {
            text: "stale",
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
            artifact: Some(LayoutArtifact::Text(stale)),
        };
        let mut paint = PaintCtx::with_text_system(&mut text_system);

        <TextInputWidget as Widget<()>>::paint(
            &widget,
            &mut paint,
            Rect::new(0.0, 0.0, 200.0, 40.0),
            &layout,
        );

        assert!(paint.layers.iter().all(|layer| {
            layer
                .cmds
                .iter()
                .all(|cmd| !matches!(cmd, DrawCmd::Text(_)))
        }));
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
