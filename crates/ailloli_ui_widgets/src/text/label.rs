//! Retained shaped text label with static or reactive string content.

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

/// Text label shaped through [`TextSystem`] with static or reactive content.
///
/// Defaults to 14-logical-pixel white UI text and
/// [`WrapMode::WordOrAnywhere`]. Without a text system, layout falls back to
/// zero width and `1.2 * px_size` height, while paint emits nothing.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::text::Text;
/// let label = Text::new("Hello").size(16.0).nowrap();
/// let _ = label;
/// ```
pub struct Text {
    /// Outer logical sizing policy.
    pub(crate) layout: LayoutStyle,
    /// Parent-flex participation metadata.
    pub(crate) flex_item: FlexItemStyle,
    /// Static or reactive UTF-8 label content.
    content: Binding<String>,
    /// Font, size, color, and line-height configuration.
    style: TextStyle,
    /// Wrapping policy used during text layout.
    wrap_mode: WrapMode,
}

crate::impl_layout_builders_unit!(Text);

impl Text {
    /// Creates a label from a static string or reactive string binding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::text::Text;
    /// let label = Text::new(String::from("Hello"));
    /// let _ = label;
    /// ```
    pub fn new(content: impl Into<Binding<String>>) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            content: content.into(),
            style: TextStyle::new(FontId::Ui, 14, Color::WHITE),
            wrap_mode: WrapMode::WordOrAnywhere,
        }
    }

    /// Replaces the static or reactive content binding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::text::Text;
    /// let label = Text::new("old").content("new");
    /// let _ = label;
    /// ```
    pub fn content(mut self, content: impl Into<Binding<String>>) -> Self {
        self.content = content.into();
        self
    }

    /// Sets the font size after rounding to an integer logical pixel.
    ///
    /// The result is clamped to `1..=u16::MAX`; `NaN` becomes one and positive
    /// infinity saturates at `u16::MAX` through Rust's float-to-int cast.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::text::Text;
    /// let label = Text::new("Hello").size(15.6);
    /// let _ = label;
    /// ```
    pub fn size(mut self, size: f32) -> Self {
        self.style.px_size = size.round().max(1.0) as u16;
        self
    }

    /// Replaces the complete font, size, color, and decoration style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextStyle};
    /// use ailloli_ui_widgets::text::Text;
    /// let label = Text::new("Code").style(TextStyle::new(FontId::Mono, 13, Color::WHITE));
    /// let _ = label;
    /// ```
    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }

    /// Replaces the shaping wrap policy.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::text::{Text, WrapMode};
    /// let label = Text::new("two words").wrap_mode(WrapMode::Word);
    /// let _ = label;
    /// ```
    pub fn wrap_mode(mut self, wrap_mode: WrapMode) -> Self {
        self.wrap_mode = wrap_mode;
        self
    }

    /// Selects wrapping at word boundaries only.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::text::Text;
    /// let label = Text::new("two words").wrap_words();
    /// let _ = label;
    /// ```
    pub fn wrap_words(self) -> Self {
        self.wrap_mode(WrapMode::Word)
    }

    /// Selects word wrapping with arbitrary fallback breaks.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::text::Text;
    /// let label = Text::new("averylongword").wrap_anywhere();
    /// let _ = label;
    /// ```
    pub fn wrap_anywhere(self) -> Self {
        self.wrap_mode(WrapMode::WordOrAnywhere)
    }

    /// Disables wrapping regardless of available width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::text::Text;
    /// let label = Text::new("one line").nowrap();
    /// let _ = label;
    /// ```
    pub fn nowrap(self) -> Self {
        self.wrap_mode(WrapMode::NoWrap)
    }
}

/// Frozen retained state used by the public [`Text`] builder.
///
/// This low-level type is public for runtime composition but its fields remain
/// private; applications normally construct [`Text`] instead.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::{IntoView, View};
/// use ailloli_ui_widgets::text::Text;
/// let _view: View<()> = Text::new("Hello").into_view();
/// ```
pub struct TextWidget {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Static or reactive UTF-8 label content.
    content: Binding<String>,
    /// Font, size, color, and line-height configuration.
    style: TextStyle,
    /// Wrapping policy used during text layout.
    wrap_mode: WrapMode,
}

/// Shapes one label and bounds its line width to a non-negative value.
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

/// Implements binding-aware layout, artifact reuse, and baseline painting.
impl<A: 'static> Widget<A> for TextWidget {
    fn debug_name(&self) -> &'static str {
        "Text"
    }

    fn layout_dependency_revision(&self) -> u64 {
        self.content.revision()
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
            decoration: self.style.decoration,
            layout: prepared,
        });

        ctx.push(cmd);
    }

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }
}

/// Converts the builder into a retained leaf widget.
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
