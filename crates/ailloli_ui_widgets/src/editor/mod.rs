//! UI adapter for the Ailloli UI editor engine.
//!
//! The editor engine lives in `ailloli_ui_editor`; this module only exposes the
//! public widget builder and wires it into the retained runtime.

mod adapter;
mod builder;
mod code_builder;
mod code_widget;
mod pane;
mod widget;

pub use ailloli_ui_editor::{
    CodeEditorConfig, CodeEditorFeatureFlags, CodeFileSummary, CodeSymbol, CodeTheme, Diagnostic,
    DiagnosticHit, DiagnosticSeverity, DiagnosticSource, Document, DocumentId, DocumentSource,
    DocumentVersion, EditorLanguage, EditorScrollbarConfig, EditorScrollbarStyle, EditorStyle,
    EditorWrapMode, FoldRegion, FoldRegionId, GutterConfig, LexicalRustSymbolIndexer, LspBackend,
    LspCapabilities, LspDiagnostic, LspEnrichment, LspError, LspRequestId, NoopLspBackend,
    ScipDocumentIndex, ScipImportError, ScipNavigationLink, ScipOccurrence, ScipOccurrenceRole,
    ScipProjectIndex, ScipProjectMetadata, ScipProjectSummary, ScipRelation, ScipSymbol,
    SearchMatch, SearchQuery, SearchState, SymbolEdge, SymbolEdgeKind, SymbolId, SymbolIndexer,
    SymbolKind, SymbolSource,
};
pub use builder::Editor;
pub use code_builder::CodeEditor;
pub use pane::{
    EditorPane, EditorPaneAction, EditorPaneSize, EditorPaneStyle, EditorPaneTab, EditorPaneTabKind,
};
