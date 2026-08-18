use std::rc::Rc;
use std::sync::Arc;

use crate::layout::layout_ext::finish_view_sized;
use crate::layout::{Column, Container};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{
    AlignItems, Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius,
};
use ailloli_ui_core::{Color, FontId, IconId, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, IntoViewKeyExt, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawImage, DrawRRect, DrawText};
use ailloli_ui_text::{PreparedTextLayout, TextLayoutParams, WrapMode};
use lucide_icons::Icon as LucideIcon;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AccordionSize {
    Compact,
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AccordionMode {
    #[default]
    Single,
    Multiple,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccordionStyle {
    pub background: Color,
    pub border: Border,
    pub focus_ring: Border,
    pub header_background: Color,
    pub header_hovered: Color,
    pub header_pressed: Color,
    pub header_open: Color,
    pub title_text: TextStyle,
    pub disabled_text: TextStyle,
    pub icon_tint: Color,
    pub disabled_icon_tint: Color,
    pub radius: Radius,
    pub header_height: f32,
    pub padding: f32,
    pub gap: f32,
    pub header_padding_x: f32,
    pub content_padding_x: f32,
    pub content_padding_y: f32,
    pub content_indent: f32,
    pub icon_size: f32,
    pub disabled_opacity: f32,
}

impl Default for AccordionStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), AccordionSize::Default)
    }
}

impl AccordionStyle {
    pub fn from_theme(theme: Theme, size: AccordionSize) -> Self {
        let palette = theme.palette();
        let (header_height, padding, gap, header_padding_x, text_size) = match size {
            AccordionSize::Compact => (30.0, 6.0, 3.0, 8.0, 12),
            AccordionSize::Default => (36.0, 8.0, 4.0, 10.0, 13),
        };
        Self {
            background: palette.surface,
            border: Border::new(1.0, palette.border),
            focus_ring: Border::new(1.0, palette.focus),
            header_background: Color::TRANSPARENT,
            header_hovered: palette.surface_elevated,
            header_pressed: Color::hex_rgb(0x20252A),
            header_open: palette.accent.with_alpha(0.16),
            title_text: TextStyle::new(FontId::Ui, text_size, palette.text),
            disabled_text: TextStyle::new(
                FontId::Ui,
                text_size,
                palette.text_muted.with_alpha(0.70),
            ),
            icon_tint: palette.text_muted,
            disabled_icon_tint: palette.text_muted.with_alpha(0.58),
            radius: Radius::uniform(theme.radius().md),
            header_height,
            padding,
            gap,
            header_padding_x,
            content_padding_x: 12.0,
            content_padding_y: 8.0,
            content_indent: 18.0,
            icon_size: 16.0,
            disabled_opacity: 0.42,
        }
    }
}

type AccordionToggleHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, String, bool)>;

#[derive(Clone)]
pub struct AccordionItem<A = ()> {
    id: String,
    title: String,
    disabled: Binding<bool>,
    content: View<A>,
}

impl<A: 'static> AccordionItem<A> {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            disabled: Binding::Static(false),
            content: View::empty(),
        }
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.content = child.into_view();
        self
    }
}

pub struct Accordion<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    mode: AccordionMode,
    items: Vec<AccordionItem<A>>,
    open_ids: Option<Binding<Vec<String>>>,
    bound_open_ids: Option<Signal<Vec<String>>>,
    default_open_ids: Vec<String>,
    on_toggle: Option<AccordionToggleHandler<A>>,
    style: AccordionStyle,
}

crate::impl_layout_builders!(Accordion);

impl<A: 'static> Default for Accordion<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Accordion<A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            mode: AccordionMode::Single,
            items: Vec::new(),
            open_ids: None,
            bound_open_ids: None,
            default_open_ids: Vec::new(),
            on_toggle: None,
            style: AccordionStyle::default(),
        }
    }

    pub fn single(mut self) -> Self {
        self.mode = AccordionMode::Single;
        self.default_open_ids.truncate(1);
        self
    }

    pub fn multiple(mut self) -> Self {
        self.mode = AccordionMode::Multiple;
        self
    }

    pub fn mode(mut self, mode: AccordionMode) -> Self {
        self.mode = mode;
        if self.mode == AccordionMode::Single {
            self.default_open_ids.truncate(1);
        }
        self
    }

    pub fn item(mut self, item: AccordionItem<A>) -> Self {
        self.items.push(item);
        self
    }

    pub fn default_open(mut self, id: impl Into<String>) -> Self {
        let id = id.into();
        if self.mode == AccordionMode::Single {
            self.default_open_ids = vec![id];
        } else if !self.default_open_ids.iter().any(|open| open == &id) {
            self.default_open_ids.push(id);
        }
        self
    }

    pub fn default_open_many(mut self, ids: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut out = Vec::new();
        for id in ids {
            let id = id.into();
            if !out.iter().any(|open| open == &id) {
                out.push(id);
            }
        }
        if self.mode == AccordionMode::Single {
            out.truncate(1);
        }
        self.default_open_ids = out;
        self
    }

    pub fn open_ids(mut self, ids: impl Into<Binding<Vec<String>>>) -> Self {
        self.open_ids = Some(ids.into());
        self
    }

    pub fn bind_open_ids(mut self, ids: impl Into<Signal<Vec<String>>>) -> Self {
        let signal = ids.into();
        self.open_ids = Some(Binding::Signal(signal.clone()));
        self.bound_open_ids = Some(signal);
        self
    }

    pub fn accordion_style(mut self, style: AccordionStyle) -> Self {
        self.style = style;
        self
    }

    pub fn accordion_size(mut self, size: AccordionSize) -> Self {
        self.style = AccordionStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn on_toggle(mut self, f: impl Fn(String, bool) -> A + 'static) -> Self {
        self.on_toggle = Some(Rc::new(move |ctx, id, open| ctx.dispatch(f(id, open))));
        self
    }

    pub fn on_toggle_ctx(mut self, f: impl Fn(&mut EventCtx<A>, String, bool) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(f));
        self
    }
}

struct AccordionComponent<A> {
    layout: LayoutStyle,
    mode: AccordionMode,
    items: Vec<AccordionItem<A>>,
    open_ids: Option<Binding<Vec<String>>>,
    bound_open_ids: Option<Signal<Vec<String>>>,
    default_open_ids: Vec<String>,
    on_toggle: Option<AccordionToggleHandler<A>>,
    style: AccordionStyle,
}

impl<A: 'static> ComponentNode<A> for AccordionComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let internal_open = context.signal(sanitize_open_ids(&self.default_open_ids, self.mode));
        let open_binding = self
            .open_ids
            .clone()
            .unwrap_or_else(|| Binding::Signal(internal_open.clone()));
        let mutable_open = self
            .bound_open_ids
            .clone()
            .or_else(|| self.open_ids.is_none().then_some(internal_open));
        let open_ids = sanitize_open_ids(&open_binding.read(), self.mode);

        let mut content = Column::new()
            .gap(self.style.gap)
            .align_items(AlignItems::Stretch);

        for item in &self.items {
            let is_open = open_ids.iter().any(|open| open == &item.id);
            content = content.child(
                AccordionHeader {
                    id: item.id.clone(),
                    title: item.title.clone(),
                    disabled: item.disabled.clone(),
                    open_ids: open_binding.clone(),
                    mutable_open: mutable_open.clone(),
                    mode: self.mode,
                    open: is_open,
                    on_toggle: self.on_toggle.clone(),
                    style: self.style.clone(),
                }
                .into_view()
                .key(format!("accordion-header-{}", item.id)),
            );
            if is_open {
                content = content.child(
                    Container::new()
                        .fill_width()
                        .padding(0.0)
                        .child(
                            Container::new()
                                .fill_width()
                                .padding(self.style.content_padding_y)
                                .child(item.content.clone()),
                        )
                        .key(format!("accordion-content-{}", item.id)),
                );
            }
        }

        let mut container = Container::<A>::new()
            .background(self.style.background)
            .border(self.style.border.widths.top, self.style.border.colors.top)
            .radius(self.style.radius.tl)
            .padding(self.style.padding)
            .child(content);
        container.layout = self.layout;
        container.into_view()
    }
}

impl<A: 'static> IntoView<A> for Accordion<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(AccordionComponent {
                layout: self.layout,
                mode: self.mode,
                items: self.items,
                open_ids: self.open_ids,
                bound_open_ids: self.bound_open_ids,
                default_open_ids: self.default_open_ids,
                on_toggle: self.on_toggle,
                style: self.style,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

struct AccordionHeader<A> {
    id: String,
    title: String,
    disabled: Binding<bool>,
    open_ids: Binding<Vec<String>>,
    mutable_open: Option<Signal<Vec<String>>>,
    mode: AccordionMode,
    open: bool,
    on_toggle: Option<AccordionToggleHandler<A>>,
    style: AccordionStyle,
}

impl<A: 'static> IntoView<A> for AccordionHeader<A> {
    fn into_view(self) -> View<A> {
        View::leaf(AccordionHeaderWidget {
            id: self.id,
            title: self.title,
            disabled: self.disabled,
            open_ids: self.open_ids,
            mutable_open: self.mutable_open,
            mode: self.mode,
            open: self.open,
            on_toggle: self.on_toggle,
            style: self.style,
        })
    }
}

struct AccordionHeaderWidget<A> {
    id: String,
    title: String,
    disabled: Binding<bool>,
    open_ids: Binding<Vec<String>>,
    mutable_open: Option<Signal<Vec<String>>>,
    mode: AccordionMode,
    open: bool,
    on_toggle: Option<AccordionToggleHandler<A>>,
    style: AccordionStyle,
}

impl<A: 'static> Widget<A> for AccordionHeaderWidget<A> {
    fn debug_name(&self) -> &'static str {
        "AccordionHeader"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let text_w = measure_text(ctx, &self.title, self.text_style()).unwrap_or(120.0);
        let intrinsic = Size::new(
            self.style.header_padding_x * 2.0 + self.style.icon_size + self.style.gap + text_w,
            self.style.header_height,
        );
        let size = constraints.constrain(intrinsic);
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
        let disabled = self.disabled.read();
        let interaction = ctx.interaction();
        let mut bg = self.style.header_background;
        if self.open {
            bg = self.style.header_open;
        } else if interaction.pressed && !disabled {
            bg = self.style.header_pressed;
        } else if interaction.hovered && !disabled {
            bg = self.style.header_hovered;
        }
        let opacity = if disabled {
            self.style.disabled_opacity
        } else {
            1.0
        };

        if bg.a > 0.0 {
            ctx.push(DrawCmd::RRect(DrawRRect {
                rect: bounds,
                radius: self.style.radius.tl,
                color: bg.with_alpha(bg.a * opacity),
            }));
        }

        let icon_rect = Rect::new(
            bounds.x + self.style.header_padding_x,
            bounds.y + (bounds.h - self.style.icon_size) * 0.5,
            self.style.icon_size,
            self.style.icon_size,
        );
        let icon = if self.open {
            IconId::Lucide(LucideIcon::ChevronDown)
        } else {
            IconId::Lucide(LucideIcon::ChevronRight)
        };
        let icon_tint = if disabled {
            self.style.disabled_icon_tint
        } else {
            self.style.icon_tint
        };
        ctx.push(DrawCmd::Image(DrawImage {
            rect: icon_rect,
            icon,
            tint: icon_tint.with_alpha(icon_tint.a * opacity),
            rotation_rad: 0.0,
        }));

        paint_text_centered(
            ctx,
            &self.title,
            self.text_style(),
            bounds,
            icon_rect.right() + self.style.gap,
            opacity,
        );

        if interaction.focused && !disabled {
            ctx.push(DrawCmd::Border(DrawBorder {
                rect: bounds,
                radius: self.style.radius,
                border: self.style.focus_ring,
            }));
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.disabled.read() {
            return;
        }
        match event {
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if bounds.contains(pos.x, pos.y) => self.toggle(ctx),
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                if matches!(
                    &key.key,
                    Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space)
                ) {
                    self.toggle(ctx);
                }
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        if self.disabled.read() {
            FocusPolicy::NotFocusable
        } else {
            FocusPolicy::Focusable
        }
    }
}

impl<A> AccordionHeaderWidget<A> {
    fn text_style(&self) -> TextStyle {
        if self.disabled.read() {
            self.style.disabled_text
        } else {
            self.style.title_text
        }
    }

    fn toggle(&self, ctx: &mut EventCtx<A>) {
        let current = sanitize_open_ids(&self.open_ids.read(), self.mode);
        let next_open = !current.iter().any(|id| id == &self.id);
        let next = toggled_open_ids(&current, &self.id, next_open, self.mode);
        let changed = next != current;
        if changed {
            if let Some(open) = &self.mutable_open {
                open.set(next);
            }
            ctx.request_repaint();
        }
        if let Some(on_toggle) = &self.on_toggle {
            on_toggle(ctx, self.id.clone(), next_open);
        }
        if changed || self.on_toggle.is_some() {
            ctx.stop_propagation();
        }
    }
}

fn sanitize_open_ids(ids: &[String], mode: AccordionMode) -> Vec<String> {
    let mut out = Vec::new();
    for id in ids {
        if !out.iter().any(|open| open == id) {
            out.push(id.clone());
        }
        if mode == AccordionMode::Single && !out.is_empty() {
            break;
        }
    }
    out
}

fn toggled_open_ids(
    current: &[String],
    id: &str,
    next_open: bool,
    mode: AccordionMode,
) -> Vec<String> {
    match (mode, next_open) {
        (AccordionMode::Single, true) => vec![id.to_string()],
        (AccordionMode::Single, false) => Vec::new(),
        (AccordionMode::Multiple, true) => {
            let mut out = current.to_vec();
            if !out.iter().any(|open| open == id) {
                out.push(id.to_string());
            }
            out
        }
        (AccordionMode::Multiple, false) => current
            .iter()
            .filter(|open| open.as_str() != id)
            .cloned()
            .collect(),
    }
}

fn measure_text(ctx: &mut LayoutCtx<'_>, text: &str, style: TextStyle) -> Option<f32> {
    ctx.text_system.as_deref_mut().map(|text_system| {
        text_system
            .layout_cached(TextLayoutParams {
                text,
                style,
                max_width: None,
                wrap_mode: WrapMode::NoWrap,
            })
            .metrics
            .width
    })
}

fn layout_text(
    ctx: &mut PaintCtx<'_>,
    text: &str,
    style: TextStyle,
) -> Option<Arc<PreparedTextLayout>> {
    ctx.text_system.as_deref_mut().map(|text_system| {
        text_system.layout_cached(TextLayoutParams {
            text,
            style,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        })
    })
}

fn paint_text_centered(
    ctx: &mut PaintCtx<'_>,
    text: &str,
    style: TextStyle,
    bounds: Rect,
    x: f32,
    opacity: f32,
) {
    let Some(layout) = layout_text(ctx, text, style) else {
        return;
    };
    let baseline = layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0);
    let y = bounds.y + (bounds.h - layout.metrics.height) * 0.5 + baseline;
    ctx.push(DrawCmd::Text(DrawText {
        pos: [x, y],
        color: style.color.with_alpha(style.color.a * opacity),
        layout,
    }));
}
