//! Public builder for the document-aware code editor widget.

use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_editor::{
    CodeEditorConfig, CodeEditorFeatureFlags, CodeFileSummary, CodeTheme, Diagnostic, Document,
    EditorLanguage, EditorScrollbarStyle, EditorWrapMode, FoldRegion, GutterConfig, SearchQuery,
};
use ailloli_ui_runtime::component::{IntoView, Signal, View};
use ailloli_ui_runtime::input::EventCtx;
use std::rc::Rc;

use crate::layout::layout_ext::finish_view_sized;

use super::code_widget::CodeEditorComponent;
use super::widget::sanitize_caret_follow_margin_lines;

/// Shared retained callback invoked after a text edit updates the document.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{Document, DocumentId};
/// use ailloli_ui_runtime::component::State;
/// use ailloli_ui_text::TextBuffer;
/// use ailloli_ui_widgets::editor::CodeEditor;
/// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new())))
///     .on_document_change_ctx(|_ctx, _document| {});
/// let _ = editor;
/// ```
pub(crate) type DocumentChangeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, Document)>;

/// Code-oriented editor backed by an [`ailloli_ui_editor::Document`] signal.
///
/// Edits synchronize the document version/dirty flag, update the supplied
/// signal, refresh syntax/search/folds, and then invoke the optional change
/// handler. Defaults come from [`CodeEditorConfig`]; optional prop vectors
/// replace their corresponding session models when enabled. The default wrap
/// mode is no-wrap, so long lines scroll horizontally. Overflowing axes paint
/// interactive overlay scrollbars whose thumbs can be dragged and whose tracks
/// page by one viewport; those interactions never move the caret. Editing
/// and keyboard navigation follow the caret on both axes and preserve three
/// visible lines below it by default. A caret already in the safe region never
/// moves the viewport. Clicks never move the viewport, and pointer selection
/// auto-scrolls only outside the text viewport.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{Document, DocumentId};
/// use ailloli_ui_runtime::component::State;
/// use ailloli_ui_text::TextBuffer;
/// use ailloli_ui_widgets::editor::CodeEditor;
/// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new())));
/// let _ = editor;
/// ```
pub struct CodeEditor<A = ()> {
    /// Outer logical sizing policy.
    pub(crate) layout: LayoutStyle,
    /// Parent-flex participation metadata.
    pub(crate) flex_item: FlexItemStyle,
    /// Caller-owned document state synchronized in both directions.
    pub(crate) document: Signal<Document>,
    /// Editing, rendering, and feature configuration.
    pub(crate) config: CodeEditorConfig,
    /// Optional language override applied to the document.
    pub(crate) language: Option<EditorLanguage>,
    /// Optional initial horizontal/vertical offsets in logical pixels.
    pub(crate) initial_scroll: Option<(f32, f32)>,
    /// Optional initial search query.
    pub(crate) search_query: Option<SearchQuery>,
    /// Optional active search match index.
    pub(crate) search_active_match: Option<usize>,
    /// Diagnostics projected over the document.
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// Optional active diagnostic index.
    pub(crate) active_diagnostic: Option<usize>,
    /// Optional caller-provided fold regions.
    pub(crate) fold_regions: Option<Vec<FoldRegion>>,
    /// Optional precomputed symbol summary.
    pub(crate) symbol_summary: Option<CodeFileSummary>,
    /// Optional initial UTF-8 byte anchor/caret pair.
    pub(crate) initial_selection: Option<(usize, usize)>,
    /// Visible-line safety margin maintained below the revealed caret.
    pub(crate) caret_follow_margin_lines: f32,
    /// Optional callback invoked after document-changing edits.
    pub(crate) on_document_change: Option<DocumentChangeHandler<A>>,
}

crate::impl_layout_builders!(CodeEditor);

impl<A: 'static> CodeEditor<A> {
    /// Creates a code editor bound to shared document state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new())));
    /// let _ = editor;
    /// ```
    pub fn new(document: impl Into<Signal<Document>>) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            document: document.into(),
            config: CodeEditorConfig::default(),
            language: None,
            initial_scroll: None,
            search_query: None,
            search_active_match: None,
            diagnostics: Vec::new(),
            active_diagnostic: None,
            fold_regions: None,
            symbol_summary: None,
            initial_selection: None,
            caret_follow_margin_lines: 3.0,
            on_document_change: None,
        }
    }

    /// Forces one language instead of document/path-derived resolution.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, EditorLanguage};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).language(EditorLanguage::Rust);
    /// let _ = editor;
    /// ```
    pub fn language(mut self, language: EditorLanguage) -> Self {
        self.language = Some(language);
        self
    }

    /// Replaces code colors and synchronizes generic editor background/foreground.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeTheme, Document, DocumentId};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).theme(CodeTheme::dark());
    /// let _ = editor;
    /// ```
    pub fn theme(mut self, theme: CodeTheme) -> Self {
        self.config.theme = theme;
        self.config.editor.style.bg = theme.background;
        self.config.editor.style.fg = theme.foreground;
        self
    }

    /// Replaces gutter enablement, width, line-number, fold, and diagnostic settings.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, GutterConfig};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).gutter(GutterConfig::default());
    /// let _ = editor;
    /// ```
    pub fn gutter(mut self, gutter: GutterConfig) -> Self {
        self.config.gutter = gutter;
        self
    }

    /// Replaces syntax/search/diagnostic/folding/symbol feature flags.
    ///
    /// Disabled features clear or skip the corresponding retained model.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorFeatureFlags, Document, DocumentId};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).features(CodeEditorFeatureFlags::default());
    /// let _ = editor;
    /// ```
    pub fn features(mut self, features: CodeEditorFeatureFlags) -> Self {
        self.config.features = features;
        self
    }

    /// Enables/disables line numbers and ensures the gutter is enabled when true.
    ///
    /// Passing `false` hides line numbers but deliberately does not disable a
    /// gutter that may still show folds or diagnostics.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).line_numbers(false);
    /// let _ = editor;
    /// ```
    pub fn line_numbers(mut self, enabled: bool) -> Self {
        self.config.gutter.enabled = self.config.gutter.enabled || enabled;
        self.config.gutter.line_numbers = enabled;
        self
    }

    /// Selects soft wrapping or horizontal no-wrap behavior.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, EditorWrapMode};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).wrap_mode(EditorWrapMode::SoftWrap);
    /// let _ = editor;
    /// ```
    pub fn wrap_mode(mut self, wrap_mode: EditorWrapMode) -> Self {
        self.config.editor.wrap_mode = wrap_mode;
        self
    }

    /// Shows or hides editor-engine scrollbars without disabling scrolling.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).scrollbars(false);
    /// let _ = editor;
    /// ```
    pub fn scrollbars(mut self, enabled: bool) -> Self {
        self.config.scrollbars.enabled = enabled;
        self
    }

    /// Replaces editor scrollbar colors and logical-pixel geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, EditorScrollbarStyle};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).scrollbar_style(EditorScrollbarStyle::default());
    /// let _ = editor;
    /// ```
    pub fn scrollbar_style(mut self, style: EditorScrollbarStyle) -> Self {
        self.config.scrollbars.style = style;
        self
    }

    /// Sets initial x/y scroll offsets in logical pixels.
    ///
    /// Each coordinate is floored at zero; negative and `NaN` values become
    /// zero. The offsets are reapplied when an external document replacement is
    /// observed, then normal editor metrics clamp them during frame building.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).initial_scroll(12.0, 24.0);
    /// let _ = editor;
    /// ```
    pub fn initial_scroll(mut self, x: f32, y: f32) -> Self {
        self.initial_scroll = Some((x.max(0.0), y.max(0.0)));
        self
    }

    /// Supplies the retained search query when search is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, SearchQuery};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).search_query(SearchQuery::new("fn"));
    /// let _ = editor;
    /// ```
    pub fn search_query(mut self, query: SearchQuery) -> Self {
        self.search_query = Some(query);
        self
    }

    /// Requests the active match index for the supplied search query.
    ///
    /// Out-of-range handling is delegated to the editor search state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).search_active_match(0);
    /// let _ = editor;
    /// ```
    pub fn search_active_match(mut self, active_match: usize) -> Self {
        self.search_active_match = Some(active_match);
        self
    }

    /// Replaces local/LSP diagnostics when diagnostics are enabled.
    ///
    /// Empty clears diagnostics; ranges remain UTF-8 byte ranges governed by
    /// the editor model.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Diagnostic, DiagnosticSeverity, Document, DocumentId};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let diagnostic = Diagnostic::new(0..1, DiagnosticSeverity::Warning, "check");
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::from_string("x")))).diagnostics(vec![diagnostic]);
    /// let _ = editor;
    /// ```
    pub fn diagnostics(mut self, diagnostics: Vec<Diagnostic>) -> Self {
        self.diagnostics = diagnostics;
        self
    }

    /// Requests one diagnostic index as active.
    ///
    /// The value is forwarded without range validation; missing indexes simply
    /// produce no active diagnostic decoration/hit.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).active_diagnostic(0);
    /// let _ = editor;
    /// ```
    pub fn active_diagnostic(mut self, active_diagnostic: usize) -> Self {
        self.active_diagnostic = Some(active_diagnostic);
        self
    }

    /// Replaces fold-region geometry when folding is enabled.
    ///
    /// If IDs and line bounds match retained regions, their current collapsed
    /// state wins so a parent rebuild does not reopen user-toggled folds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, FoldRegion};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).fold_regions(vec![FoldRegion::new(0, 2)]);
    /// let _ = editor;
    /// ```
    pub fn fold_regions(mut self, fold_regions: Vec<FoldRegion>) -> Self {
        self.fold_regions = Some(fold_regions);
        self
    }

    /// Supplies a precomputed symbol graph when symbol support is enabled.
    ///
    /// The current widget retains this metadata but has no symbol navigation UI;
    /// it does not alter paint output yet.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeFileSummary, Document, DocumentId, DocumentVersion, EditorLanguage};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let summary = CodeFileSummary { document_id: DocumentId(1), path: None, language: EditorLanguage::Rust, version: DocumentVersion(0), symbols: vec![], edges: vec![] };
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).symbol_summary(summary);
    /// let _ = editor;
    /// ```
    pub fn symbol_summary(mut self, summary: CodeFileSummary) -> Self {
        self.symbol_summary = Some(summary);
        self
    }

    /// Sets initial anchor/caret UTF-8 byte offsets.
    ///
    /// Values are independently clamped to document byte length on first build,
    /// but are not ordered or validated as UTF-8 character boundaries.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::from_string("hello")))).initial_selection(0, 5);
    /// let _ = editor;
    /// ```
    pub fn initial_selection(mut self, anchor: usize, caret: usize) -> Self {
        self.initial_selection = Some((anchor, caret));
        self
    }

    /// Sets the visible-line safety margin kept below the revealed caret.
    ///
    /// The default is three lines. The margin applies to edits, paste, undo,
    /// redo, IME changes, and keyboard navigation. Reveal always adds only the
    /// distance by which the caret crosses the safe boundary; pointer clicks
    /// never request caret following. Negative values become zero and
    /// non-finite values restore the default.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(
    ///     DocumentId(1),
    ///     TextBuffer::new(),
    /// )))
    /// .caret_follow_margin_lines(5.0);
    /// let _ = editor;
    /// ```
    pub fn caret_follow_margin_lines(mut self, lines: f32) -> Self {
        self.caret_follow_margin_lines = sanitize_caret_follow_margin_lines(lines);
        self
    }

    /// Maps each edited document into an application action.
    ///
    /// The handler runs only after a text-changing edit has synchronized the
    /// document, not for selection, scroll, or external prop updates.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// enum Action { Changed(Document) }
    /// let editor: CodeEditor<Action> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).on_document_change(Action::Changed);
    /// let _ = editor;
    /// ```
    pub fn on_document_change(mut self, f: impl Fn(Document) -> A + 'static) -> Self {
        self.on_document_change = Some(Rc::new(move |ctx, document| ctx.dispatch(f(document))));
        self
    }

    /// Handles edited documents with mutable runtime event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::CodeEditor;
    /// let editor: CodeEditor<()> = CodeEditor::new(State::new(Document::new(DocumentId(1), TextBuffer::new()))).on_document_change_ctx(|_ctx, _document| {});
    /// let _ = editor;
    /// ```
    pub fn on_document_change_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, Document) + 'static,
    ) -> Self {
        self.on_document_change = Some(Rc::new(f));
        self
    }
}

/// Converts the builder into a retained code-editor component with layout hints.
impl<A: 'static> IntoView<A> for CodeEditor<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(CodeEditorComponent {
                layout: self.layout,
                document: self.document,
                config: self.config,
                language: self.language,
                initial_scroll: self.initial_scroll,
                search_query: self.search_query,
                search_active_match: self.search_active_match,
                diagnostics: self.diagnostics,
                active_diagnostic: self.active_diagnostic,
                fold_regions: self.fold_regions,
                symbol_summary: self.symbol_summary,
                initial_selection: self.initial_selection,
                caret_follow_margin_lines: self.caret_follow_margin_lines,
                on_document_change: self.on_document_change,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}
