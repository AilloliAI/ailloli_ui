use std::cell::Cell;
use std::rc::Rc;

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::style::{
    Border, FlexItemStyle, InteractionState, LayoutSizeHint, LayoutStyle, Radius, StateStyle,
};
use ailloli_ui_core::{Color, Constraints, FontId, Offset, Rect, Size, TextStyle, Theme};
use ailloli_ui_runtime::app::ExternalUrl;
use ailloli_ui_runtime::component::{Binding, IntoView, Memo, View, Widget};
use ailloli_ui_runtime::input::{ActivationPolicy, EventCtx, FocusPolicy, HoverCursorRole};
use ailloli_ui_runtime::layout::{
    ChildLayout, LayoutArtifact, LayoutChild, LayoutCtx, LayoutResult,
};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};

/// Visual states for a text link.
///
/// State styles are paint-only: keep the same font and size in every state.
#[derive(Clone, Debug)]
pub struct LinkStyle {
    pub text: StateStyle<TextStyle>,
    pub focus_ring: Border,
    pub focus_ring_offset: f32,
}

impl Default for LinkStyle {
    fn default() -> Self {
        let theme = Theme::default();
        let palette = theme.palette();
        let normal = TextStyle::new(FontId::Ui, 14, palette.accent).underline();
        Self {
            text: StateStyle {
                normal,
                hovered: Some(TextStyle::new(FontId::Ui, 14, Color::WHITE).underline()),
                pressed: Some(normal.without_decoration()),
                focused: None,
                disabled: Some(
                    TextStyle::new(FontId::Ui, 14, palette.text_muted.with_alpha(0.65)).underline(),
                ),
            },
            focus_ring: Border::new(1.0, palette.focus),
            focus_ring_offset: 2.0,
        }
    }
}

impl LinkStyle {
    pub fn resolve_text(&self, state: InteractionState) -> TextStyle {
        let resolved = self.text.resolve(state);
        TextStyle {
            font: self.text.normal.font,
            px_size: self.text.normal.px_size,
            ..resolved
        }
    }

    fn visual_inflate(&self) -> f32 {
        let widths = self.focus_ring.layout_widths();
        self.focus_ring_offset
            + widths
                .left
                .max(widths.top)
                .max(widths.right)
                .max(widths.bottom)
    }
}

enum LinkContent<A> {
    Empty,
    Label(String),
    Child(View<A>),
}

/// An accessible external HTTP(S) link with a single composable child.
///
/// Use [`Link::with_label`] for the common text form, or [`Link::child`] for
/// icons and composed content. `href` always means an external system URL;
/// this widget deliberately has no generic action callback.
///
/// # Future routing gate
///
/// Internal navigation is intentionally unavailable until the framework owns
/// a provider-neutral route type, navigation state, history semantics,
/// deep-link policy, focus/accessibility behavior, and consumer tests. The
/// intended future syntax is documentation only:
///
/// ```text
/// Link::with_label("Settings").route(Route::Settings)
/// ```
pub struct Link<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    content: LinkContent<A>,
    href: Option<ExternalUrl>,
    disabled: Binding<bool>,
    style: LinkStyle,
}

crate::impl_layout_builders!(Link);

impl<A: 'static> Default for Link<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Link<A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            content: LinkContent::Empty,
            href: None,
            disabled: Binding::Static(false),
            style: LinkStyle::default(),
        }
    }

    pub fn with_label(label: impl Into<String>) -> Self {
        Self {
            content: LinkContent::Label(label.into()),
            ..Self::new()
        }
    }

    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.content = LinkContent::Child(child.into_view());
        self
    }

    pub fn href(mut self, href: impl AsRef<str>) -> Self {
        self.href = ExternalUrl::parse(href).ok();
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    pub fn style(mut self, style: TextStyle) -> Self {
        self.style.text.normal = style;
        self
    }

    pub fn link_style(mut self, style: LinkStyle) -> Self {
        self.style = style;
        self
    }
}

struct LinkWidget {
    layout: LayoutStyle,
    href: Option<ExternalUrl>,
    disabled: Binding<bool>,
    style: LinkStyle,
    laid_out_content: Cell<bool>,
    interaction: Rc<Cell<InteractionState>>,
}

impl<A: 'static> Widget<A> for LinkWidget {
    fn debug_name(&self) -> &'static str {
        "Link"
    }

    fn layout(
        &self,
        engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let mut child_layouts = Vec::new();
        let mut intrinsic = Size::default();
        if let Some(child) = children.first_mut() {
            let result = child.layout(engine, ctx, constraints.loosen());
            intrinsic = result.size;
            child_layouts.push(ChildLayout {
                offset: Offset::default(),
                size: result.size,
                paint_bounds: Rect::new(0.0, 0.0, result.size.w, result.size.h),
                visual_bounds: result.visual_bounds,
            });
        }
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        self.laid_out_content.set(size.w > 0.0 && size.h > 0.0);
        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        let inflate = self.style.visual_inflate();
        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds,
            visual_bounds: paint_bounds.inflate(inflate, inflate),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        let input = ctx.interaction();
        self.interaction.set(InteractionState {
            hovered: input.hovered,
            pressed: input.pressed,
            focused: input.focused,
            disabled: self.disabled.read() || self.href.is_none(),
        });
    }

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let state = self.interaction.get();
        if state.focused && !state.disabled && self.style.focus_ring.is_visible() {
            ctx.push(DrawCmd::Border(DrawBorder {
                rect: bounds.inflate(self.style.focus_ring_offset, self.style.focus_ring_offset),
                radius: Radius::uniform(self.style.focus_ring_offset),
                border: self.style.focus_ring,
            }));
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.disabled.read() || !self.laid_out_content.get() {
            return;
        }
        let Some(href) = self.href.as_ref() else {
            return;
        };
        let activate = match event {
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) => bounds.contains(pos.x, pos.y),
            Event::Keyboard(key) => {
                key.state == KeyState::Pressed
                    && !key.repeat
                    && matches!(&key.key, Key::Named(NamedKey::Enter))
            }
            _ => false,
        };
        if activate {
            let _ = ctx.open_external_url(href);
            ctx.stop_propagation();
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        if self.href.is_some() && !self.disabled.read() && self.laid_out_content.get() {
            FocusPolicy::Focusable
        } else {
            FocusPolicy::NotFocusable
        }
    }

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }

    fn hover_cursor_role(&self) -> HoverCursorRole {
        if self.href.is_some() && !self.disabled.read() && self.laid_out_content.get() {
            HoverCursorRole::Pointer
        } else {
            HoverCursorRole::Default
        }
    }
}

struct LinkLabelWidget {
    label: String,
    style: LinkStyle,
    interaction: Rc<Cell<InteractionState>>,
}

fn layout_label(
    text_system: &mut TextSystem,
    label: &str,
    style: TextStyle,
) -> ailloli_ui_text::TextLayoutHandle {
    text_system.layout_cached(TextLayoutParams {
        text: label,
        style,
        max_width: None,
        wrap_mode: WrapMode::NoWrap,
    })
}

impl<A: 'static> Widget<A> for LinkLabelWidget {
    fn debug_name(&self) -> &'static str {
        "LinkLabel"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let prepared = ctx
            .text_system
            .as_deref_mut()
            .map(|text| layout_label(text, &self.label, self.style.text.normal));
        let intrinsic = prepared
            .as_ref()
            .map(|layout| Size::new(layout.metrics.width, layout.metrics.height))
            .unwrap_or_else(|| Size::new(0.0, self.style.text.normal.px_size as f32 * 1.2));
        let size = constraints.constrain(intrinsic);
        let bounds = Rect::new(0.0, 0.0, size.w, size.h);
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: bounds,
            visual_bounds: bounds,
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: prepared.map(LayoutArtifact::Text),
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, layout: &LayoutResult) {
        let state = self.interaction.get();
        let style = self.style.resolve_text(state);
        let prepared = match layout.artifact.as_ref() {
            Some(LayoutArtifact::Text(prepared)) => prepared.clone(),
            None => {
                let Some(text) = ctx.text_system.as_deref_mut() else {
                    return;
                };
                layout_label(text, &self.label, self.style.text.normal)
            }
        };
        let baseline = prepared
            .lines
            .first()
            .map(|line| line.baseline_y)
            .unwrap_or(0.0);
        ctx.push(DrawCmd::Text(DrawText {
            pos: [bounds.x, bounds.y + baseline],
            color: style.color,
            decoration: style.decoration,
            layout: prepared,
        }));
    }
}

impl<A: 'static> IntoView<A> for Link<A> {
    fn into_view(self) -> View<A> {
        let interaction = Rc::new(Cell::new(InteractionState::default()));
        let child = match self.content {
            LinkContent::Empty => View::empty(),
            LinkContent::Label(label) => View::leaf(LinkLabelWidget {
                label,
                style: self.style.clone(),
                interaction: interaction.clone(),
            }),
            LinkContent::Child(child) => child,
        };
        let widget = LinkWidget {
            layout: self.layout,
            href: self.href,
            disabled: self.disabled,
            style: self.style,
            laid_out_content: Cell::new(false),
            interaction,
        };
        finish_view_sized(
            View::node(widget, vec![child]),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}
