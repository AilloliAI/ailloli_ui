use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, FileEvent};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{
    Border, BoxShadow, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius,
};
use ailloli_ui_core::{Color, FontId, TextStyle, Theme, UploadAccept, UploadFile};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{ClickAction, EventCtx, FocusPolicy, IntoClickAction};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawRRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, WrapMode};

type UploadDropHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, Vec<UploadFile>)>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UploadDropzoneVariant {
    Compact,
    #[default]
    Default,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UploadDropzoneStyle {
    pub background: Color,
    pub background_hovered: Color,
    pub border: Border,
    pub border_hovered: Border,
    pub focus_ring: Border,
    pub button_background: Color,
    pub button_text: TextStyle,
    pub title_text: TextStyle,
    pub description_text: TextStyle,
    pub disabled_opacity: f32,
    pub shadows: Vec<BoxShadow>,
    pub width: f32,
    pub height: f32,
    pub radius: Radius,
    pub button_height: f32,
}

impl Default for UploadDropzoneStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), UploadDropzoneVariant::Default)
    }
}

impl UploadDropzoneStyle {
    pub fn from_theme(theme: Theme, variant: UploadDropzoneVariant) -> Self {
        let palette = theme.palette();
        let compact = variant == UploadDropzoneVariant::Compact;
        Self {
            background: palette.surface,
            background_hovered: palette.accent.with_alpha(0.12),
            border: Border::new(1.0, palette.border),
            border_hovered: Border::new(1.0, palette.accent),
            focus_ring: Border::new(2.0, palette.focus),
            button_background: palette.accent,
            button_text: TextStyle::new(FontId::Ui, if compact { 12 } else { 13 }, Color::WHITE),
            title_text: TextStyle::new(FontId::Ui, if compact { 13 } else { 15 }, palette.text),
            description_text: TextStyle::new(
                FontId::Ui,
                if compact { 11 } else { 12 },
                palette.text_muted,
            ),
            disabled_opacity: 0.45,
            shadows: Vec::new(),
            width: if compact { 260.0 } else { 360.0 },
            height: if compact { 104.0 } else { 142.0 },
            radius: Radius::uniform(theme.radius().md),
            button_height: if compact { 28.0 } else { 32.0 },
        }
    }
}

pub struct UploadDropzone<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    title: Binding<String>,
    description: Binding<String>,
    disabled: Binding<bool>,
    multiple: bool,
    accept: UploadAccept,
    style: UploadDropzoneStyle,
    on_browse: Option<Rc<ClickAction<A>>>,
    on_drop: Option<UploadDropHandler<A>>,
}

crate::impl_layout_builders!(UploadDropzone);

impl<A: 'static> Default for UploadDropzone<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> UploadDropzone<A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            title: Binding::Static("Drag & drop files here".to_string()),
            description: Binding::Static("or browse files".to_string()),
            disabled: Binding::Static(false),
            multiple: false,
            accept: UploadAccept::default(),
            style: UploadDropzoneStyle::default(),
            on_browse: None,
            on_drop: None,
        }
    }

    pub fn title(mut self, value: impl Into<Binding<String>>) -> Self {
        self.title = value.into();
        self
    }

    pub fn description(mut self, value: impl Into<Binding<String>>) -> Self {
        self.description = value.into();
        self
    }

    pub fn disabled(mut self, value: impl Into<Binding<bool>>) -> Self {
        self.disabled = value.into();
        self
    }

    pub fn multiple(mut self, value: bool) -> Self {
        self.multiple = value;
        self
    }

    pub fn accept(mut self, patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.accept = UploadAccept::new(patterns);
        self
    }

    pub fn upload_style(mut self, style: UploadDropzoneStyle) -> Self {
        self.style = style;
        self
    }

    pub fn upload_variant(mut self, variant: UploadDropzoneVariant) -> Self {
        self.style = UploadDropzoneStyle::from_theme(Theme::default(), variant);
        self
    }

    pub fn on_browse(mut self, action: impl IntoClickAction<A>) -> Self {
        self.on_browse = Some(Rc::new(action.into_click_action()));
        self
    }

    pub fn on_drop(mut self, f: impl Fn(Vec<UploadFile>) -> A + 'static) -> Self {
        self.on_drop = Some(Rc::new(move |ctx, files| ctx.dispatch(f(files))));
        self
    }

    pub fn on_drop_ctx(mut self, f: impl Fn(&mut EventCtx<A>, Vec<UploadFile>) + 'static) -> Self {
        self.on_drop = Some(Rc::new(f));
        self
    }
}

struct UploadDropzoneComponent<A> {
    props: UploadDropzone<A>,
}

impl<A: 'static> ComponentNode<A> for UploadDropzoneComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        View::leaf(UploadDropzoneWidget {
            layout: self.props.layout,
            title: self.props.title.clone(),
            description: self.props.description.clone(),
            disabled: self.props.disabled.clone(),
            multiple: self.props.multiple,
            accept: self.props.accept.clone(),
            style: self.props.style.clone(),
            on_browse: self.props.on_browse.clone(),
            on_drop: self.props.on_drop.clone(),
            hovering: context.signal(false),
        })
    }
}

impl<A: 'static> IntoView<A> for UploadDropzone<A> {
    fn into_view(self) -> View<A> {
        let flex_item = self.flex_item;
        let hint = LayoutSizeHint::from_layout(self.layout);
        finish_view_sized(
            View::component(UploadDropzoneComponent { props: self }),
            flex_item,
            hint,
        )
    }
}

struct UploadDropzoneWidget<A> {
    layout: LayoutStyle,
    title: Binding<String>,
    description: Binding<String>,
    disabled: Binding<bool>,
    multiple: bool,
    accept: UploadAccept,
    style: UploadDropzoneStyle,
    on_browse: Option<Rc<ClickAction<A>>>,
    on_drop: Option<UploadDropHandler<A>>,
    hovering: Signal<bool>,
}

impl<A: 'static> Widget<A> for UploadDropzoneWidget<A> {
    fn debug_name(&self) -> &'static str {
        "UploadDropzone"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = apply_layout_size(
            Size::new(self.style.width, self.style.height),
            self.layout,
            constraints,
        );
        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds,
            visual_bounds: paint_bounds,
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let disabled = self.disabled.read();
        let hovering = self.hovering.read() && !disabled;
        let mut bg = if hovering {
            self.style.background_hovered
        } else {
            self.style.background
        };
        if disabled {
            bg = bg.with_alpha(self.style.disabled_opacity);
        }
        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: bounds,
            radius: self.style.radius.tl,
            color: bg,
        }));
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: bounds,
            radius: self.style.radius,
            border: if ctx.is_focused() && !disabled {
                self.style.focus_ring
            } else if hovering {
                self.style.border_hovered
            } else {
                self.style.border
            },
        }));

        let title = self.title.read();
        let description = self.description.read();
        let title_style = if disabled {
            TextStyle {
                color: self
                    .style
                    .title_text
                    .color
                    .with_alpha(self.style.disabled_opacity),
                ..self.style.title_text
            }
        } else {
            self.style.title_text
        };
        let description_style = if disabled {
            TextStyle {
                color: self
                    .style
                    .description_text
                    .color
                    .with_alpha(self.style.disabled_opacity),
                ..self.style.description_text
            }
        } else {
            self.style.description_text
        };
        let center_x = bounds.x + bounds.w * 0.5;
        push_text_centered(ctx, &title, center_x, bounds.y + 36.0, title_style);
        push_text_centered(
            ctx,
            &description,
            center_x,
            bounds.y + 58.0,
            description_style,
        );

        let button = self.button_rect(bounds);
        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: button,
            radius: 7.0,
            color: if disabled {
                self.style
                    .button_background
                    .with_alpha(self.style.disabled_opacity)
            } else {
                self.style.button_background
            },
        }));
        push_text_centered(
            ctx,
            "Browse Files",
            button.x + button.w * 0.5,
            button.y + button.h * 0.5 + self.style.button_text.px_size as f32 * 0.35,
            if disabled {
                TextStyle {
                    color: self
                        .style
                        .button_text
                        .color
                        .with_alpha(self.style.disabled_opacity),
                    ..self.style.button_text
                }
            } else {
                self.style.button_text
            },
        );
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
            }) if self.button_rect(bounds).contains(pos.x, pos.y) => {
                if let Some(action) = &self.on_browse {
                    action.run(ctx);
                    ctx.stop_propagation();
                }
            }
            Event::File(FileEvent::Hover { pos, files }) if bounds.contains(pos.x, pos.y) => {
                let accepted = files.iter().any(|file| self.accept.accepts(file));
                self.hovering.set(accepted);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::File(FileEvent::HoverCancelled) if self.hovering.read() => {
                self.hovering.set(false);
                ctx.request_repaint();
            }
            Event::File(FileEvent::Drop { pos, files }) if bounds.contains(pos.x, pos.y) => {
                self.hovering.set(false);
                let mut accepted: Vec<UploadFile> = files
                    .iter()
                    .filter(|file| self.accept.accepts(file))
                    .cloned()
                    .collect();
                if !self.multiple {
                    accepted.truncate(1);
                }
                if !accepted.is_empty() {
                    if let Some(on_drop) = &self.on_drop {
                        on_drop(ctx, accepted);
                        ctx.stop_propagation();
                    }
                }
                ctx.request_repaint();
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        if self.disabled.read() || (self.on_browse.is_none() && self.on_drop.is_none()) {
            FocusPolicy::NotFocusable
        } else {
            FocusPolicy::Focusable
        }
    }
}

impl<A> UploadDropzoneWidget<A> {
    fn button_rect(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x + (bounds.w - 116.0) * 0.5,
            bounds.bottom() - self.style.button_height - 18.0,
            116.0,
            self.style.button_height,
        )
    }
}

fn push_text_centered(
    ctx: &mut PaintCtx<'_>,
    text: &str,
    center_x: f32,
    baseline_y: f32,
    style: TextStyle,
) {
    let layout = ctx.text_system.as_deref_mut().map(|ts| {
        ts.layout_cached(TextLayoutParams {
            text,
            style,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        })
    });
    if let Some(layout) = layout {
        let x = center_x - layout.metrics.width * 0.5;
        ctx.push(DrawCmd::Text(DrawText {
            pos: [x, baseline_y],
            color: style.color,
            layout,
        }));
    }
}
