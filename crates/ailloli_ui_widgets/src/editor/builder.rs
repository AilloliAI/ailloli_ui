//! Public builder for the generic multi-paragraph editor widget.

use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_editor::{EditorConfig, EditorStyle, EditorWrapMode};
use ailloli_ui_runtime::component::{IntoView, Signal, View};
use ailloli_ui_text::TextBuffer;

use crate::layout::layout_ext::finish_view_sized;

use super::widget::EditorComponent;

/// Multi-paragraph editor backed by a shared [`TextBuffer`] signal.
///
/// Edits update the supplied signal synchronously. The default uses
/// [`EditorConfig::default`], automatic layout, and no initial selection. The
/// retained widget is focusable, supports multiline keyboard/IME input, and
/// has a 320×180 logical-pixel intrinsic cap/height before constraints.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::State;
/// use ailloli_ui_text::TextBuffer;
/// use ailloli_ui_widgets::editor::Editor;
/// let editor = Editor::new(State::new(TextBuffer::from_string("hello")));
/// let _ = editor;
/// ```
pub struct Editor {
    /// Outer logical sizing policy.
    pub(crate) layout: LayoutStyle,
    /// Parent-flex participation metadata.
    pub(crate) flex_item: FlexItemStyle,
    /// Caller-owned text buffer synchronized in both directions.
    pub(crate) buffer: Signal<TextBuffer>,
    /// Editing and rendering configuration.
    pub(crate) config: EditorConfig,
    /// Optional initial UTF-8 byte anchor/caret pair.
    pub(crate) initial_selection: Option<(usize, usize)>,
}

crate::impl_layout_builders_unit!(Editor);

impl Editor {
    /// Creates an editor bound to shared rope-buffer state.
    ///
    /// A standalone [`ailloli_ui_runtime::component::State`] is convenient for examples; component code may
    /// pass a context-owned [`Signal`] so edits schedule invalidation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::Editor;
    /// let editor = Editor::new(State::new(TextBuffer::new()));
    /// let _ = editor;
    /// ```
    pub fn new(buffer: impl Into<Signal<TextBuffer>>) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            buffer: buffer.into(),
            config: EditorConfig::default(),
            initial_selection: None,
        }
    }

    /// Replaces editor colors and logical-pixel metrics.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::EditorStyle;
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::Editor;
    /// let editor = Editor::new(State::new(TextBuffer::new())).style(EditorStyle::default());
    /// let _ = editor;
    /// ```
    pub fn style(mut self, style: EditorStyle) -> Self {
        self.config.style = style;
        self
    }

    /// Selects soft wrapping or horizontal no-wrap behavior.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::EditorWrapMode;
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::Editor;
    /// let editor = Editor::new(State::new(TextBuffer::new())).wrap_mode(EditorWrapMode::NoWrap);
    /// let _ = editor;
    /// ```
    pub fn wrap_mode(mut self, wrap_mode: EditorWrapMode) -> Self {
        self.config.wrap_mode = wrap_mode;
        self
    }

    /// Sets initial anchor/caret UTF-8 byte offsets.
    ///
    /// Both values are independently clamped to buffer length when the retained
    /// component is first built. They are not ordered or validated as UTF-8
    /// character boundaries and do not overwrite later retained selection state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::Editor;
    /// let editor = Editor::new(State::new(TextBuffer::from_string("hello"))).initial_selection(0, 5);
    /// let _ = editor;
    /// ```
    pub fn initial_selection(mut self, anchor: usize, caret: usize) -> Self {
        self.initial_selection = Some((anchor, caret));
        self
    }
}

/// Converts the builder into a retained editor component with flex/size hints.
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
