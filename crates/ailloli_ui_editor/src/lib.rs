//! UI-agnostic multi-paragraph editor engine.
//!
//! `ailloli_ui_editor` owns editor text state, viewport math, visible paragraph
//! layout, hit-testing, caret/selection geometry, IME display text, and the
//! neutral paint model consumed by UI adapters.

/// Code-document configuration, models, syntax, search, and semantic adapters.
pub mod code;
/// Generic editor configuration.
pub mod config;
/// Stateful frame construction and geometry queries.
pub mod engine;
/// Neutral frame output values.
pub mod frame;
/// Caret, selection, hit-test, IME, and scrolling primitives.
pub mod input;
/// Paragraph shaping, metrics caching, and viewport virtualization.
pub mod layout;
/// Neutral editor paint models and painters.
pub mod paint;
/// Mutable editor interaction state.
pub mod state;
/// Generic editor colors and metrics.
pub mod style;
/// Viewport resolution and coordinate transforms.
pub mod viewport;

pub use code::{
    index_symbols_with_fallback, parse_ctags_json_lines, resolve_document_language,
    CodeEditorConfig, CodeEditorFeatureFlags, CodeEditorSession, CodeFileSummary, CodeSymbol,
    CodeTheme, CtagsError, CtagsRunnerConfig, CtagsSymbolIndexer, Diagnostic, DiagnosticHit,
    DiagnosticSeverity, DiagnosticSource, Document, DocumentId, DocumentSource, DocumentVersion,
    EditorLanguage, FoldRegion, FoldRegionId, GutterConfig, LexicalRustSymbolIndexer, LspBackend,
    LspCapabilities, LspDiagnostic, LspEnrichment, LspError, LspRequestId, NoopLspBackend,
    ScipDocumentIndex, ScipImportError, ScipNavigationLink, ScipOccurrence, ScipOccurrenceRole,
    ScipProjectIndex, ScipProjectMetadata, ScipProjectSummary, ScipRelation, ScipSymbol,
    SearchMatch, SearchQuery, SearchState, SymbolEdge, SymbolEdgeKind, SymbolId, SymbolIndexer,
    SymbolKind, SymbolSource,
};
pub use config::{EditorConfig, EditorWrapMode};
pub use engine::{code_scrollbar_geometries, EditorEngine};
pub use frame::{EditorFrame, EditorFrameDebugMetrics};
pub use input::{
    EditorHitTest, EditorHitZone, EditorInputOutcome, EditorZoneHitTest, SelectionGranularity,
};
pub use paint::EditorPaintItem;
pub use state::{EditorClickZone, EditorSession};
pub use style::{EditorScrollbarConfig, EditorScrollbarStyle, EditorStyle};
pub use viewport::{editor_content_rect, EditorViewport};
