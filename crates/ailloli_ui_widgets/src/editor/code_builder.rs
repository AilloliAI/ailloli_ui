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

pub(crate) type DocumentChangeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, Document)>;

/// Code-oriented editor backed by an [`ailloli_ui_editor::Document`] signal.
pub struct CodeEditor<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    pub(crate) document: Signal<Document>,
    pub(crate) config: CodeEditorConfig,
    pub(crate) language: Option<EditorLanguage>,
    pub(crate) initial_scroll: Option<(f32, f32)>,
    pub(crate) search_query: Option<SearchQuery>,
    pub(crate) search_active_match: Option<usize>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) active_diagnostic: Option<usize>,
    pub(crate) fold_regions: Option<Vec<FoldRegion>>,
    pub(crate) symbol_summary: Option<CodeFileSummary>,
    pub(crate) initial_selection: Option<(usize, usize)>,
    pub(crate) on_document_change: Option<DocumentChangeHandler<A>>,
}

crate::impl_layout_builders!(CodeEditor);

impl<A: 'static> CodeEditor<A> {
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
            on_document_change: None,
        }
    }

    pub fn language(mut self, language: EditorLanguage) -> Self {
        self.language = Some(language);
        self
    }

    pub fn theme(mut self, theme: CodeTheme) -> Self {
        self.config.theme = theme;
        self.config.editor.style.bg = theme.background;
        self.config.editor.style.fg = theme.foreground;
        self
    }

    pub fn gutter(mut self, gutter: GutterConfig) -> Self {
        self.config.gutter = gutter;
        self
    }

    pub fn features(mut self, features: CodeEditorFeatureFlags) -> Self {
        self.config.features = features;
        self
    }

    pub fn line_numbers(mut self, enabled: bool) -> Self {
        self.config.gutter.enabled = self.config.gutter.enabled || enabled;
        self.config.gutter.line_numbers = enabled;
        self
    }

    pub fn wrap_mode(mut self, wrap_mode: EditorWrapMode) -> Self {
        self.config.editor.wrap_mode = wrap_mode;
        self
    }

    pub fn scrollbars(mut self, enabled: bool) -> Self {
        self.config.scrollbars.enabled = enabled;
        self
    }

    pub fn scrollbar_style(mut self, style: EditorScrollbarStyle) -> Self {
        self.config.scrollbars.style = style;
        self
    }

    pub fn initial_scroll(mut self, x: f32, y: f32) -> Self {
        self.initial_scroll = Some((x.max(0.0), y.max(0.0)));
        self
    }

    pub fn search_query(mut self, query: SearchQuery) -> Self {
        self.search_query = Some(query);
        self
    }

    pub fn search_active_match(mut self, active_match: usize) -> Self {
        self.search_active_match = Some(active_match);
        self
    }

    pub fn diagnostics(mut self, diagnostics: Vec<Diagnostic>) -> Self {
        self.diagnostics = diagnostics;
        self
    }

    pub fn active_diagnostic(mut self, active_diagnostic: usize) -> Self {
        self.active_diagnostic = Some(active_diagnostic);
        self
    }

    pub fn fold_regions(mut self, fold_regions: Vec<FoldRegion>) -> Self {
        self.fold_regions = Some(fold_regions);
        self
    }

    pub fn symbol_summary(mut self, summary: CodeFileSummary) -> Self {
        self.symbol_summary = Some(summary);
        self
    }

    pub fn initial_selection(mut self, anchor: usize, caret: usize) -> Self {
        self.initial_selection = Some((anchor, caret));
        self
    }

    pub fn on_document_change(mut self, f: impl Fn(Document) -> A + 'static) -> Self {
        self.on_document_change = Some(Rc::new(move |ctx, document| ctx.dispatch(f(document))));
        self
    }

    pub fn on_document_change_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, Document) + 'static,
    ) -> Self {
        self.on_document_change = Some(Rc::new(f));
        self
    }
}

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
                on_document_change: self.on_document_change,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}
