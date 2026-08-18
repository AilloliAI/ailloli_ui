use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_core::{Color, Constraints, FontId, Rect, Size, TextStyle};
use ailloli_ui_runtime::component::{Binding, IntoView, View, Widget};
use ailloli_ui_runtime::input::EventCtx;
use ailloli_ui_runtime::layout::{LayoutArtifact, LayoutResult};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawCmd, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

/// Text label (shaped via `ailloli_ui_text::TextSystem`) with static or reactive content.
pub struct Text {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    content: Binding<String>,
    style: TextStyle,
    wrap_mode: WrapMode,
}

crate::impl_layout_builders_unit!(Text);

impl Text {
    pub fn new(content: impl Into<Binding<String>>) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            content: content.into(),
            style: TextStyle::new(FontId::Ui, 14, Color::WHITE),
            wrap_mode: WrapMode::WordOrAnywhere,
        }
    }

    pub fn content(mut self, content: impl Into<Binding<String>>) -> Self {
        self.content = content.into();
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.style.px_size = size.round().max(1.0) as u16;
        self
    }

    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }

    pub fn wrap_mode(mut self, wrap_mode: WrapMode) -> Self {
        self.wrap_mode = wrap_mode;
        self
    }

    pub fn wrap_words(self) -> Self {
        self.wrap_mode(WrapMode::Word)
    }

    pub fn wrap_anywhere(self) -> Self {
        self.wrap_mode(WrapMode::WordOrAnywhere)
    }

    pub fn nowrap(self) -> Self {
        self.wrap_mode(WrapMode::NoWrap)
    }
}

pub struct TextWidget {
    layout: LayoutStyle,
    content: Binding<String>,
    style: TextStyle,
    wrap_mode: WrapMode,
}

fn layout_via_system(
    ts: &mut TextSystem,
    text: &str,
    style: TextStyle,
    max_width: f32,
    wrap_mode: WrapMode,
) -> ailloli_ui_text::TextLayoutHandle {
    ts.layout_cached(TextLayoutParams {
        text,
        style,
        max_width: Some(max_width.max(0.0)),
        wrap_mode,
    })
}

impl<A: 'static> Widget<A> for TextWidget {
    fn debug_name(&self) -> &'static str {
        "Text"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let content = self.content.read();
        let max_w = self
            .layout
            .width
            .resolve(constraints.max_w)
            .unwrap_or(constraints.max_w);
        let prepared = ctx
            .text_system
            .as_deref_mut()
            .map(|ts| layout_via_system(ts, &content, self.style, max_w, self.wrap_mode));
        let intrinsic = if let Some(prepared) = prepared.as_ref() {
            Size::new(prepared.metrics.width, prepared.metrics.height)
        } else {
            Size::new(0.0, self.style.px_size as f32 * 1.2)
        };
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: prepared.map(LayoutArtifact::Text),
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, layout: &LayoutResult) {
        let content = self.content.read();
        let prepared = match layout.artifact.as_ref() {
            Some(LayoutArtifact::Text(prepared)) if prepared.text() == content.as_str() => {
                prepared.clone()
            }
            _ => {
                let Some(ts) = ctx.text_system.as_deref_mut() else {
                    return;
                };
                layout_via_system(ts, &content, self.style, bounds.w, self.wrap_mode)
            }
        };
        let baseline = prepared.lines.first().map(|l| l.baseline_y).unwrap_or(0.0);
        let cmd = DrawCmd::Text(DrawText {
            // Contract: pos.y is baseline (Phase 27).
            pos: [bounds.x, bounds.y + baseline],
            color: self.style.color,
            layout: prepared,
        });

        ctx.push(cmd);
    }

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }
}

impl<A: 'static> IntoView<A> for Text {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(TextWidget {
                layout: self.layout,
                content: self.content,
                style: self.style,
                wrap_mode: self.wrap_mode,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}
