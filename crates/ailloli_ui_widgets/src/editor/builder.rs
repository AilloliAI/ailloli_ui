use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_editor::{EditorConfig, EditorStyle, EditorWrapMode};
use ailloli_ui_runtime::component::{IntoView, Signal, View};
use ailloli_ui_text::TextBuffer;

use crate::layout::layout_ext::finish_view_sized;

use super::widget::EditorComponent;

/// Multi-paragraph editor backed by a shared [`TextBuffer`] signal.
pub struct Editor {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    pub(crate) buffer: Signal<TextBuffer>,
    pub(crate) config: EditorConfig,
    pub(crate) initial_selection: Option<(usize, usize)>,
}

crate::impl_layout_builders_unit!(Editor);

impl Editor {
    pub fn new(buffer: impl Into<Signal<TextBuffer>>) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            buffer: buffer.into(),
            config: EditorConfig::default(),
            initial_selection: None,
        }
    }

    pub fn style(mut self, style: EditorStyle) -> Self {
        self.config.style = style;
        self
    }

    pub fn wrap_mode(mut self, wrap_mode: EditorWrapMode) -> Self {
        self.config.wrap_mode = wrap_mode;
        self
    }

    pub fn initial_selection(mut self, anchor: usize, caret: usize) -> Self {
        self.initial_selection = Some((anchor, caret));
        self
    }
}

impl<A: 'static> IntoView<A> for Editor {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(EditorComponent {
                layout: self.layout,
                buffer: self.buffer,
                config: self.config,
                initial_selection: self.initial_selection,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}
