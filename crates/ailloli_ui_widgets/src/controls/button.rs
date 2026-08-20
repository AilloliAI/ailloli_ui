use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use crate::text::Text;
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::event::{Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, EdgeInsets, Rect, Size};
use ailloli_ui_core::style::{
    AlignItems, Background, Border, BoxShadow, BoxStyle, FlexItemStyle, InteractionState,
    JustifyContent, LayoutSizeHint, LayoutStyle, Radius, StateStyle,
};
use ailloli_ui_core::{Color, FontId, Offset, TextStyle, Theme};
use ailloli_ui_runtime::component::{Binding, IntoView, Memo, View, Widget};
use ailloli_ui_runtime::input::{
    ActivationPolicy, ClickAction, EventCtx, FocusPolicy, IntoClickAction,
};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawBoxShadow, DrawCmd, DrawRRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// Compatibility alias for the primary default button.
    Default,
    #[default]
    Primary,
    Secondary,
    Outline,
    Ghost,
    Destructive,
    Success,
    Warning,
    Info,
}

#[derive(Clone, Debug)]
pub struct ButtonStyle {
    pub container: StateStyle<BoxStyle>,
    pub text: StateStyle<TextStyle>,
    pub height: f32,
    pub horizontal_padding: f32,
    pub vertical_padding: f32,
    pub align_items: AlignItems,
    pub justify_content: JustifyContent,
    pub baseline_shift: f32,
}

impl Default for ButtonStyle {
    fn default() -> Self {
        Self::primary()
    }
}

impl ButtonStyle {
    pub fn primary() -> Self {
        Self::from_theme(Theme::default(), ButtonVariant::Primary)
    }

    pub fn secondary() -> Self {
        Self::from_theme(Theme::default(), ButtonVariant::Secondary)
    }

    pub fn outline() -> Self {
        Self::from_theme(Theme::default(), ButtonVariant::Outline)
    }

    pub fn ghost() -> Self {
        Self::from_theme(Theme::default(), ButtonVariant::Ghost)
    }

    pub fn destructive() -> Self {
        Self::from_theme(Theme::default(), ButtonVariant::Destructive)
    }

    pub fn success() -> Self {
        Self::from_theme(Theme::default(), ButtonVariant::Success)
    }

    pub fn warning() -> Self {
        Self::from_theme(Theme::default(), ButtonVariant::Warning)
    }

    pub fn info() -> Self {
        Self::from_theme(Theme::default(), ButtonVariant::Info)
    }

    pub fn from_theme(theme: Theme, variant: ButtonVariant) -> Self {
        let palette = theme.palette();
        let radius = theme.radius().button();
        let text_light = TextStyle::new(FontId::Ui, 14, palette.text);
        let text_dark = TextStyle::new(FontId::Ui, 14, Color::hex_rgb(0x090B0C));
        let text_muted = TextStyle::new(FontId::Ui, 14, palette.text_muted.with_alpha(0.70));
        let border = Border::new(1.0, palette.border);
        let transparent = Background::color(Color::TRANSPARENT);
        let disabled = BoxStyle::new()
            .background(Background::color(palette.surface_elevated.with_alpha(0.48)))
            .radius(radius);

        let (normal, hovered, pressed, text) = match variant {
            ButtonVariant::Default | ButtonVariant::Primary => (
                BoxStyle::new()
                    .background(Background::color(palette.accent))
                    .radius(radius),
                BoxStyle::new()
                    .background(Background::color(theme.button_bg_hover))
                    .radius(radius),
                BoxStyle::new()
                    .background(Background::color(theme.button_bg_pressed))
                    .radius(radius),
                text_light,
            ),
            ButtonVariant::Secondary => (
                BoxStyle::new()
                    .background(Background::color(palette.surface_elevated))
                    .border(border)
                    .radius(radius),
                BoxStyle::new()
                    .background(Background::color(Color::hex_rgb(0x20252A)))
                    .border(border)
                    .radius(radius),
                BoxStyle::new()
                    .background(Background::color(Color::hex_rgb(0x15191D)))
                    .border(border)
                    .radius(radius),
                text_light,
            ),
            ButtonVariant::Outline => (
                BoxStyle::new()
                    .background(transparent)
                    .border(border)
                    .radius(radius),
                BoxStyle::new()
                    .background(Background::color(palette.surface.with_alpha(0.86)))
                    .border(Border::new(1.0, palette.accent.with_alpha(0.72)))
                    .radius(radius),
                BoxStyle::new()
                    .background(Background::color(palette.surface_elevated))
                    .border(Border::new(1.0, palette.accent))
                    .radius(radius),
                text_light,
            ),
            ButtonVariant::Ghost => (
                BoxStyle::new().background(transparent).radius(radius),
                BoxStyle::new()
                    .background(Background::color(palette.surface.with_alpha(0.72)))
                    .radius(radius),
                BoxStyle::new()
                    .background(Background::color(palette.surface_elevated))
                    .radius(radius),
                text_light,
            ),
            ButtonVariant::Destructive => tone_button(palette.danger, radius),
            ButtonVariant::Success => tone_button(palette.success, radius),
            ButtonVariant::Warning => tone_button(palette.warning, radius),
            ButtonVariant::Info => tone_button(palette.info, radius),
        };

        let text = if matches!(variant, ButtonVariant::Warning | ButtonVariant::Info) {
            text_dark
        } else {
            text
        };

        Self {
            container: StateStyle {
                normal,
                hovered: Some(hovered),
                pressed: Some(pressed),
                focused: None,
                disabled: Some(disabled),
            },
            text: StateStyle {
                normal: text,
                hovered: None,
                pressed: None,
                focused: None,
                disabled: Some(text_muted),
            },
            height: 36.0,
            horizontal_padding: 12.0,
            vertical_padding: 8.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            baseline_shift: 0.0,
        }
    }

    pub fn text_only(fg: Color) -> Self {
        let text = TextStyle::new(FontId::Ui, 13, fg);
        Self {
            container: StateStyle {
                normal: BoxStyle::new().background(Background::color(Color::TRANSPARENT)),
                hovered: None,
                pressed: None,
                focused: None,
                disabled: None,
            },
            text: StateStyle {
                normal: text,
                hovered: None,
                pressed: None,
                focused: None,
                disabled: None,
            },
            height: 0.0,
            horizontal_padding: 0.0,
            vertical_padding: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            baseline_shift: 0.0,
        }
    }

    pub fn resolve_container(&self, state: InteractionState) -> BoxStyle {
        self.container.resolve(state)
    }

    pub fn resolve_text(&self, state: InteractionState) -> TextStyle {
        self.text.resolve(state)
    }

    fn layout_border_widths(&self) -> EdgeInsets {
        let mut widths = self.container.normal.border.layout_widths();
        for style in [
            self.container.hovered.as_ref(),
            self.container.pressed.as_ref(),
            self.container.focused.as_ref(),
            self.container.disabled.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            let other = style.border.layout_widths();
            widths.left = widths.left.max(other.left);
            widths.top = widths.top.max(other.top);
            widths.right = widths.right.max(other.right);
            widths.bottom = widths.bottom.max(other.bottom);
        }
        widths
    }

    fn layout_visual_bounds(&self, rect: Rect) -> Rect {
        let mut bounds = self.container.normal.visual_bounds(rect);
        for style in [
            self.container.hovered.as_ref(),
            self.container.pressed.as_ref(),
            self.container.focused.as_ref(),
            self.container.disabled.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            bounds = union_rect(bounds, style.visual_bounds(rect));
        }
        bounds
    }

    fn update_containers(&mut self, mut f: impl FnMut(BoxStyle) -> BoxStyle) {
        self.container.normal = f(self.container.normal.clone());
        if let Some(style) = self.container.hovered.take() {
            self.container.hovered = Some(f(style));
        }
        if let Some(style) = self.container.pressed.take() {
            self.container.pressed = Some(f(style));
        }
        if let Some(style) = self.container.focused.take() {
            self.container.focused = Some(f(style));
        }
        if let Some(style) = self.container.disabled.take() {
            self.container.disabled = Some(f(style));
        }
    }
}

fn main_axis_offset(justify: JustifyContent, available: f32, child: f32) -> f32 {
    let free = (available - child).max(0.0);
    match justify {
        JustifyContent::Start | JustifyContent::SpaceBetween => 0.0,
        JustifyContent::Center | JustifyContent::SpaceAround | JustifyContent::SpaceEvenly => {
            free * 0.5
        }
        JustifyContent::End => free,
    }
}

fn cross_axis_offset(align: AlignItems, available: f32, child: f32) -> f32 {
    let free = (available - child).max(0.0);
    match align {
        AlignItems::Start | AlignItems::Stretch => 0.0,
        AlignItems::Center => free * 0.5,
        AlignItems::End => free,
    }
}

fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = a.right().max(b.right());
    let y1 = a.bottom().max(b.bottom());
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

fn tone_button(color: Color, radius: Radius) -> (BoxStyle, BoxStyle, BoxStyle, TextStyle) {
    (
        BoxStyle::new()
            .background(Background::color(color))
            .radius(radius),
        BoxStyle::new()
            .background(Background::color(color.with_alpha(0.92)))
            .radius(radius),
        BoxStyle::new()
            .background(Background::color(color.with_alpha(0.78)))
            .radius(radius),
        TextStyle::new(FontId::Ui, 14, Color::hex_rgb(0xF4F7F8)),
    )
}

pub fn draw_button(
    rect: Rect,
    label: &str,
    variant: ButtonVariant,
    style: ButtonStyle,
    text: &mut TextSystem,
) -> Vec<DrawCmd> {
    let variant_style = if variant == ButtonVariant::Default {
        style
    } else {
        ButtonStyle::from_theme(Theme::default(), variant)
    };
    let container = variant_style.container.normal.clone();
    let bg = match container.background {
        Background::Color(bg) => bg,
        Background::None => Color::TRANSPARENT,
    };
    let text_style = variant_style.text.normal;

    let mut out = Vec::new();
    for shadow in container.shadows.iter().copied().filter(|s| !s.inset) {
        let shadow = apply_shadow_opacity(shadow, container.opacity.0);
        if shadow.color.a > 0.0 {
            out.push(DrawCmd::BoxShadow(DrawBoxShadow {
                rect,
                radius: container.radius,
                shadow,
            }));
        }
    }
    out.extend([DrawCmd::RRect(DrawRRect {
        rect,
        radius: container.radius.tl,
        color: apply_opacity(bg, container.opacity.0),
    })]);

    let layout = text.layout_cached(TextLayoutParams {
        text: label,
        style: text_style,
        max_width: None,
        wrap_mode: WrapMode::NoWrap,
    });
    let border = container.border.layout_widths();
    let content_w =
        (rect.w - border.horizontal() - variant_style.horizontal_padding * 2.0).max(0.0);
    let content_h = (rect.h - border.vertical() - variant_style.vertical_padding * 2.0).max(0.0);
    let x = rect.x
        + border.left
        + variant_style.horizontal_padding
        + main_axis_offset(
            variant_style.justify_content,
            content_w,
            layout.metrics.width,
        );
    let baseline = layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0);
    let y = rect.y
        + border.top
        + variant_style.vertical_padding
        + cross_axis_offset(variant_style.align_items, content_h, layout.metrics.height)
        + baseline
        + variant_style.baseline_shift;
    out.push(DrawCmd::Text(DrawText {
        pos: [x, y],
        color: text_style.color,
        decoration: ailloli_ui_core::TextDecoration::None,
        layout,
    }));

    let border = apply_border_opacity(container.border, container.opacity.0);
    if border.is_visible() {
        out.push(DrawCmd::Border(DrawBorder {
            rect,
            radius: container.radius,
            border,
        }));
    }

    out
}

fn apply_opacity(mut c: Color, opacity: f32) -> Color {
    c.a = (c.a * opacity).clamp(0.0, 1.0);
    c
}

fn apply_border_opacity(mut border: Border, opacity: f32) -> Border {
    border.colors.left = apply_opacity(border.colors.left, opacity);
    border.colors.top = apply_opacity(border.colors.top, opacity);
    border.colors.right = apply_opacity(border.colors.right, opacity);
    border.colors.bottom = apply_opacity(border.colors.bottom, opacity);
    border
}

fn apply_shadow_opacity(mut shadow: BoxShadow, opacity: f32) -> BoxShadow {
    shadow.color = apply_opacity(shadow.color, opacity);
    shadow
}

fn ensure_shadow(shadows: &mut Vec<BoxShadow>) -> &mut BoxShadow {
    if shadows.is_empty() {
        shadows.push(BoxShadow::md());
    }
    shadows.last_mut().expect("shadow inserted when empty")
}

/// Clickable button with stateful box/text styles and optional child label.
pub struct Button<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    child: View<A>,
    disabled: Binding<bool>,
    on_click: Option<ClickAction<A>>,
    style: ButtonStyle,
}

crate::impl_layout_builders!(Button);

impl<A: 'static> Default for Button<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Button<A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            child: View::empty(),
            disabled: Binding::Static(false),
            on_click: None,
            style: ButtonStyle::primary(),
        }
    }

    pub fn with_label(label: impl Into<String>) -> Self {
        let style = ButtonStyle::primary();
        Self::new().child(Text::new(label.into()).style(style.text.normal).nowrap())
    }

    pub fn with_label_variant(label: impl Into<String>, variant: ButtonVariant) -> Self {
        let style = ButtonStyle::from_theme(Theme::default(), variant);
        Self::new()
            .button_style(style.clone())
            .child(Text::new(label.into()).style(style.text.normal).nowrap())
    }

    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.child = child.into_view();
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    pub fn on_click(mut self, action: impl IntoClickAction<A>) -> Self
    where
        A: Clone,
    {
        self.on_click = Some(action.into_click_action());
        self
    }

    pub fn on_click_ctx(mut self, f: impl Fn(&mut EventCtx<A>) + 'static) -> Self {
        self.on_click = Some(ClickAction::handler(f));
        self
    }

    pub fn style(mut self, style: TextStyle) -> Self {
        // Compat: only updates label text in the normal state.
        self.style.text.normal = style;
        self
    }

    pub fn button_style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.style = ButtonStyle::from_theme(Theme::default(), variant);
        self
    }

    pub fn align_items(mut self, value: AlignItems) -> Self {
        self.style.align_items = value;
        self
    }

    pub fn justify_content(mut self, value: JustifyContent) -> Self {
        self.style.justify_content = value;
        self
    }

    pub fn background(mut self, color: Color) -> Self {
        self.style
            .update_containers(|style| style.background(Background::color(color)));
        self
    }

    pub fn radius(mut self, value: f32) -> Self {
        let radius = Radius::uniform(value);
        self.style.update_containers(|style| style.radius(radius));
        self
    }

    pub fn border(mut self, width: f32, color: Color) -> Self {
        self.style
            .update_containers(|style| style.border(Border::new(width, color)));
        self
    }

    pub fn border_width(mut self, width: f32) -> Self {
        self.style.update_containers(|mut style| {
            style.border = style.border.with_width(width);
            style
        });
        self
    }

    pub fn border_color(mut self, color: Color) -> Self {
        self.style.update_containers(|mut style| {
            style.border = style.border.with_color(color);
            style
        });
        self
    }

    pub fn border_left(mut self, width: f32, color: Color) -> Self {
        self.style.update_containers(|mut style| {
            style.border = style.border.with_left(width, color);
            style
        });
        self
    }

    pub fn border_top(mut self, width: f32, color: Color) -> Self {
        self.style.update_containers(|mut style| {
            style.border = style.border.with_top(width, color);
            style
        });
        self
    }

    pub fn border_right(mut self, width: f32, color: Color) -> Self {
        self.style.update_containers(|mut style| {
            style.border = style.border.with_right(width, color);
            style
        });
        self
    }

    pub fn border_bottom(mut self, width: f32, color: Color) -> Self {
        self.style.update_containers(|mut style| {
            style.border = style.border.with_bottom(width, color);
            style
        });
        self
    }

    pub fn shadow(mut self, shadow: BoxShadow) -> Self {
        self.style.update_containers(|style| style.shadow(shadow));
        self
    }

    pub fn shadow_none(mut self) -> Self {
        self.style.update_containers(|style| style.clear_shadows());
        self
    }

    pub fn shadow_color(mut self, color: Color) -> Self {
        self.style.update_containers(|mut style| {
            ensure_shadow(&mut style.shadows).color = color;
            style
        });
        self
    }

    pub fn shadow_blur(mut self, value: f32) -> Self {
        self.style.update_containers(|mut style| {
            ensure_shadow(&mut style.shadows).blur_radius = value.max(0.0);
            style
        });
        self
    }

    pub fn shadow_offset(mut self, x: f32, y: f32) -> Self {
        self.style.update_containers(|mut style| {
            ensure_shadow(&mut style.shadows).offset = Offset::new(x, y);
            style
        });
        self
    }

    pub fn shadow_spread(mut self, value: f32) -> Self {
        self.style.update_containers(|mut style| {
            ensure_shadow(&mut style.shadows).spread = value.max(0.0);
            style
        });
        self
    }
}

pub struct ButtonWidget<A> {
    layout: LayoutStyle,
    disabled: Binding<bool>,
    on_click: Option<ClickAction<A>>,
    style: ButtonStyle,
}

impl<A: 'static> Widget<A> for ButtonWidget<A> {
    fn debug_name(&self) -> &'static str {
        "Button"
    }

    fn layout(
        &self,
        engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let pad_x = self.style.horizontal_padding;
        let pad_y = self.style.vertical_padding;
        let border = self.style.layout_border_widths();
        let inner = Constraints {
            min_w: (constraints.min_w - border.horizontal() - pad_x * 2.0).max(0.0),
            max_w: (constraints.max_w - border.horizontal() - pad_x * 2.0).max(0.0),
            min_h: (constraints.min_h - border.vertical() - pad_y * 2.0).max(0.0),
            max_h: (constraints.max_h - border.vertical() - pad_y * 2.0).max(0.0),
        };

        let mut child_layouts = Vec::new();
        let mut child_size = Size::default();

        if let Some(child) = children.first_mut() {
            let r = child.layout(engine, ctx, inner);
            child_size = r.size;
            child_layouts.push(ChildLayout {
                offset: Offset::default(),
                size: r.size,
                paint_bounds: Rect::new(0.0, 0.0, r.size.w, r.size.h),
                visual_bounds: r.visual_bounds,
            });
        }

        let intrinsic = Size::new(
            child_size.w + border.horizontal() + pad_x * 2.0,
            self.style
                .height
                .max(child_size.h + border.vertical() + pad_y * 2.0),
        );
        let size = apply_layout_size(intrinsic, self.layout, constraints);

        if let Some(child_layout) = child_layouts.first_mut() {
            let content_w = (size.w - border.horizontal() - pad_x * 2.0).max(0.0);
            let content_h = (size.h - border.vertical() - pad_y * 2.0).max(0.0);
            child_layout.offset = Offset::new(
                border.left
                    + pad_x
                    + main_axis_offset(self.style.justify_content, content_w, child_layout.size.w),
                border.top
                    + pad_y
                    + cross_axis_offset(self.style.align_items, content_h, child_layout.size.h),
            );
        }
        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        let visual_bounds = self.style.layout_visual_bounds(paint_bounds);

        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds,
            visual_bounds,
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout_result: &LayoutResult) {
        let i = ctx.interaction();
        let state = InteractionState {
            hovered: i.hovered,
            pressed: i.pressed,
            focused: i.focused,
            disabled: self.disabled.read(),
        };

        let container = self.style.resolve_container(state);
        for shadow in container.shadows.iter().copied().filter(|s| !s.inset) {
            let shadow = apply_shadow_opacity(shadow, container.opacity.0);
            if shadow.color.a > 0.0 {
                ctx.push(DrawCmd::BoxShadow(DrawBoxShadow {
                    rect: bounds,
                    radius: container.radius,
                    shadow,
                }));
            }
        }

        if let Background::Color(bg) = container.background {
            let bg = apply_opacity(bg, container.opacity.0);
            ctx.push(DrawCmd::RRect(DrawRRect {
                rect: bounds,
                radius: container.radius.tl,
                color: bg,
            }));
        }
    }

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout_result: &LayoutResult) {
        let i = ctx.interaction();
        let state = InteractionState {
            hovered: i.hovered,
            pressed: i.pressed,
            focused: i.focused,
            disabled: self.disabled.read(),
        };

        let container = self.style.resolve_container(state);
        let border = apply_border_opacity(container.border, container.opacity.0);
        if border.is_visible() {
            ctx.push(DrawCmd::Border(DrawBorder {
                rect: bounds,
                radius: container.radius,
                border,
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
            }) if bounds.contains(pos.x, pos.y) => {
                if let Some(on_click) = &self.on_click {
                    on_click.run(ctx);
                    ctx.stop_propagation();
                }
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                if matches!(
                    &key.key,
                    Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space)
                ) {
                    if let Some(on_click) = &self.on_click {
                        on_click.run(ctx);
                        ctx.stop_propagation();
                    }
                }
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::Focusable
    }

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }
}

impl<A: 'static> IntoView<A> for Button<A> {
    fn into_view(self) -> View<A> {
        let widget = ButtonWidget {
            layout: self.layout,
            disabled: self.disabled,
            on_click: self.on_click,
            style: self.style,
        };
        finish_view_sized(
            View::node(widget, vec![self.child]),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}
