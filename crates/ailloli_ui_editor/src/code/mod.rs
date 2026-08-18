use std::path::{Path, PathBuf};

use ailloli_ui_fs::FileUri;
use ailloli_ui_text::TextBuffer;

pub mod config;
pub mod diagnostics;
pub mod folding;
pub mod search;
pub mod semantic;
pub mod session;
pub mod symbols;
pub mod syntax;

pub use config::{CodeEditorConfig, CodeEditorFeatureFlags, CodeTheme, GutterConfig};
pub use diagnostics::{
    diagnostic_at_byte, Diagnostic, DiagnosticHit, DiagnosticSeverity, DiagnosticSource,
};
pub use folding::{
    collapsed_region_hiding_line, fold_region_at_line, fold_regions_for_document, line_for_byte,
    line_start_byte, merge_fold_regions_with_previous, FoldRegion, FoldRegionId,
};
pub use search::{find_matches, SearchMatch, SearchQuery, SearchState};
pub use semantic::{
    collect_lsp_enrichment, import_scip_json_str, lsp_diagnostics_to_diagnostics,
    lsp_symbols_to_code_symbols, merge_code_file_summaries, scip_document_to_summary,
    scip_project_to_summary, scip_symbols_to_code_symbols, semantic_references_to_edges,
    LspBackend, LspCapabilities, LspDiagnostic, LspEnrichment, LspError, LspRequestId,
    NoopLspBackend, ScipDocumentIndex, ScipImportError, ScipNavigationLink, ScipOccurrence,
    ScipOccurrenceRole, ScipProjectIndex, ScipProjectMetadata, ScipProjectSummary, ScipRelation,
    ScipSymbol, SemanticDocumentSymbol, SemanticReference,
};
pub use session::CodeEditorSession;
#[cfg(feature = "tree-sitter")]
pub use symbols::TreeSitterRustSymbolIndexer;
pub use symbols::{
    index_symbols_with_fallback, parse_ctags_json_lines, CodeFileSummary, CodeSymbol, CtagsError,
    CtagsRunnerConfig, CtagsSymbolIndexer, LexicalRustSymbolIndexer, SymbolEdge, SymbolEdgeKind,
    SymbolId, SymbolIndexer, SymbolKind, SymbolSource,
};
#[cfg(feature = "tree-sitter")]
pub use syntax::highlight_rust_tree_sitter_hybrid;
pub use syntax::{highlight_rust_lexical, SyntaxKind, SyntaxToken};

/// Stable identifier for an open document in an editor session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DocumentId(pub u64);

/// Monotonic revision of a document's buffer content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DocumentVersion(pub i32);

/// Syntax/language hint for future code-editor features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EditorLanguage {
    PlainText,
    Rust,
    TypeScript,
    JavaScript,
    Json,
    Html,
    Css,
    Markdown,
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DocumentSource {
    #[default]
    Memory,
    LocalPath(PathBuf),
    Uri(FileUri),
}

impl DocumentSource {
    pub fn path_hint(&self) -> Option<&Path> {
        match self {
            Self::Memory => None,
            Self::LocalPath(path) => Some(path.as_path()),
            Self::Uri(uri) => Some(Path::new(uri.path())),
        }
    }
}

/// Open document metadata plus rope-backed text.
#[derive(Debug, Clone)]
pub struct Document {
    pub id: DocumentId,
    pub path: Option<PathBuf>,
    pub source: DocumentSource,
    pub language: EditorLanguage,
    pub version: DocumentVersion,
    pub dirty: bool,
    pub buffer: TextBuffer,
}

impl Document {
    pub fn new(id: DocumentId, buffer: TextBuffer) -> Self {
        Self {
            id,
            path: None,
            source: DocumentSource::Memory,
            language: EditorLanguage::PlainText,
            version: DocumentVersion(0),
            dirty: false,
            buffer,
        }
    }

    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        self.source = DocumentSource::LocalPath(path.clone());
        self.path = Some(path);
        self
    }

    pub fn with_source(mut self, source: DocumentSource) -> Self {
        self.path = match &source {
            DocumentSource::Memory => self.path.take(),
            DocumentSource::LocalPath(path) => Some(path.clone()),
            DocumentSource::Uri(uri) if uri.scheme() == "file" => uri.to_local_path().ok(),
            DocumentSource::Uri(_) => None,
        };
        self.source = source;
        self
    }

    pub fn with_uri(self, uri: FileUri) -> Self {
        self.with_source(DocumentSource::Uri(uri))
    }

    pub fn with_language(mut self, language: EditorLanguage) -> Self {
        self.language = language;
        self
    }

    pub fn detect_language(&mut self) {
        self.language = detect_language(self.language_path_hint());
    }

    pub fn language_path_hint(&self) -> Option<&Path> {
        self.path.as_deref().or_else(|| self.source.path_hint())
    }
}

pub fn resolve_document_language(
    document: &Document,
    override_language: Option<EditorLanguage>,
) -> EditorLanguage {
    if let Some(language) = override_language {
        return language;
    }
    match document.language {
        EditorLanguage::PlainText | EditorLanguage::Unknown => {
            let detected = detect_language(document.language_path_hint());
            if detected == EditorLanguage::Unknown {
                document.language
            } else {
                detected
            }
        }
        language => language,
    }
}

pub fn detect_language(path: Option<&std::path::Path>) -> EditorLanguage {
    let Some(path) = path else {
        return EditorLanguage::PlainText;
    };
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => EditorLanguage::Rust,
        "ts" | "tsx" => EditorLanguage::TypeScript,
        "js" | "jsx" | "mjs" | "cjs" => EditorLanguage::JavaScript,
        "json" => EditorLanguage::Json,
        "html" | "htm" => EditorLanguage::Html,
        "css" => EditorLanguage::Css,
        "md" | "markdown" => EditorLanguage::Markdown,
        _ => EditorLanguage::Unknown,
    }
}
