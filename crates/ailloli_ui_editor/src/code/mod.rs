//! Code-document metadata plus syntax, search, folding, diagnostics, and symbols.
//!
//! # Examples
//!
//! ```
//! use ailloli_ui_editor::{code::detect_language, EditorLanguage};
//! assert_eq!(detect_language(Some(std::path::Path::new("main.rs"))), EditorLanguage::Rust);
//! ```

use std::path::{Path, PathBuf};

use ailloli_ui_fs::FileUri;
use ailloli_ui_text::TextBuffer;

/// Code-editor and gutter configuration.
pub mod config;
/// Source diagnostics and byte hit testing.
pub mod diagnostics;
/// Logical-line fold regions and discovery.
pub mod folding;
/// Cached byte-range text search.
pub mod search;
/// Optional LSP and SCIP semantic enrichment models.
pub mod semantic;
/// Document-aware editor session state.
pub mod session;
/// Local symbol indexing and graph summaries.
pub mod symbols;
/// Rust lexical and optional Tree-sitter syntax tokens.
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
#[cfg(feature = "tree_sitter")]
pub use symbols::TreeSitterRustSymbolIndexer;
pub use symbols::{
    index_symbols_with_fallback, parse_ctags_json_lines, CodeFileSummary, CodeSymbol, CtagsError,
    CtagsRunnerConfig, CtagsSymbolIndexer, LexicalRustSymbolIndexer, SymbolEdge, SymbolEdgeKind,
    SymbolId, SymbolIndexer, SymbolKind, SymbolSource,
};
#[cfg(feature = "tree_sitter")]
pub use syntax::highlight_rust_tree_sitter_hybrid;
pub use syntax::{highlight_rust_lexical, SyntaxKind, SyntaxToken};

/// Stable identifier for an open document in an editor session.
///
/// Zero is valid and no global allocator is implied.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::DocumentId;
/// let id = DocumentId(7);
/// assert_eq!(id.0, 7);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DocumentId(pub u64);

/// Monotonic revision of a document's buffer content.
///
/// The type permits every `i32`; callers define monotonic update policy and
/// must handle negative or exhausted external revisions explicitly.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::DocumentVersion;
/// assert_eq!(DocumentVersion(0).0, 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DocumentVersion(pub i32);

/// Syntax/language selection used by highlighting, folding, and indexing.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::EditorLanguage;
/// assert_ne!(EditorLanguage::PlainText, EditorLanguage::Unknown);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EditorLanguage {
    /// Explicit unstructured text, also the default for no path.
    PlainText,
    /// Rust source.
    Rust,
    /// TypeScript or TSX source.
    TypeScript,
    /// JavaScript, JSX, MJS, or CJS source.
    JavaScript,
    /// JSON data.
    Json,
    /// HTML or HTM markup.
    Html,
    /// CSS stylesheet.
    Css,
    /// Markdown (`.md` or `.markdown`).
    Markdown,
    /// A path was present but its extension was not recognized.
    Unknown,
}

/// Origin used for path hints and persistence routing.
///
/// `Memory` is the default. A non-file URI deliberately has no local path hint.
///
/// # Examples
///
/// ```
/// use std::path::{Path, PathBuf};
/// use ailloli_ui_editor::DocumentSource;
/// let source = DocumentSource::LocalPath(PathBuf::from("src/lib.rs"));
/// assert_eq!(source.path_hint(), Some(Path::new("src/lib.rs")));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DocumentSource {
    /// In-memory content with no external source.
    #[default]
    Memory,
    /// Host-local path.
    LocalPath(PathBuf),
    /// Provider-neutral URI, including remote schemes.
    Uri(FileUri),
}

/// Path-hint access for document source variants.
impl DocumentSource {
    /// Returns a borrowed language/path hint when one exists.
    ///
    /// URI paths are interpreted lexically as [`Path`] values even for remote
    /// schemes; `Memory` returns `None`. No filesystem access occurs.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    /// use ailloli_ui_editor::DocumentSource;
    /// assert_eq!(DocumentSource::Memory.path_hint(), None);
    /// assert_eq!(DocumentSource::LocalPath("main.rs".into()).path_hint(), Some(Path::new("main.rs")));
    /// ```
    pub fn path_hint(&self) -> Option<&Path> {
        match self {
            Self::Memory => None,
            Self::LocalPath(path) => Some(path.as_path()),
            Self::Uri(uri) => Some(Path::new(uri.path())),
        }
    }
}

/// Open document metadata plus rope-backed text.
///
/// Fields are public for controlled editor composition. Changing `buffer`
/// directly does not increment `version` or set `dirty`; session methods should
/// mediate edits when those invariants matter.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{Document, DocumentId, DocumentVersion, EditorLanguage};
/// use ailloli_ui_text::TextBuffer;
/// let document = Document::new(DocumentId(1), TextBuffer::from_string("hello"));
/// assert_eq!(document.version, DocumentVersion(0));
/// assert_eq!(document.language, EditorLanguage::PlainText);
/// assert!(!document.dirty);
/// ```
#[derive(Debug, Clone)]
pub struct Document {
    /// Stable session-local document identity.
    pub id: DocumentId,
    /// Optional local path retained for compatibility and language detection.
    pub path: Option<PathBuf>,
    /// Canonical memory/path/URI origin description.
    pub source: DocumentSource,
    /// Explicit or detected language.
    pub language: EditorLanguage,
    /// Content revision; initialized to zero.
    pub version: DocumentVersion,
    /// Whether content differs from its externally saved version.
    pub dirty: bool,
    /// Rope-backed UTF-8 content.
    pub buffer: TextBuffer,
}

/// Document construction and language-hint operations.
impl Document {
    /// Creates a clean, version-zero in-memory plain-text document.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, DocumentSource};
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(2), TextBuffer::new());
    /// assert_eq!(document.source, DocumentSource::Memory);
    /// assert!(document.path.is_none());
    /// ```
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

    /// Sets both `path` and [`DocumentSource::LocalPath`] without detection.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, DocumentSource};
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::new()).with_path("src/main.rs");
    /// assert!(matches!(document.source, DocumentSource::LocalPath(_)));
    /// assert_eq!(document.path.unwrap().to_str(), Some("src/main.rs"));
    /// ```
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        self.source = DocumentSource::LocalPath(path.clone());
        self.path = Some(path);
        self
    }

    /// Replaces the origin and synchronizes the optional local path.
    ///
    /// `Memory` preserves an existing legacy `path`, a local file URI converts
    /// to a host path when possible, and non-file URIs clear `path`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, DocumentSource};
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::new())
    ///     .with_source(DocumentSource::Uri(FileUri::parse("sftp://host/main.rs")?));
    /// assert!(document.path.is_none());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
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

    /// Convenience wrapper for [`Self::with_source`] with a URI origin.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, DocumentSource};
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::new()).with_uri(FileUri::parse("sftp://host/doc")?);
    /// assert!(matches!(document.source, DocumentSource::Uri(_)));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn with_uri(self, uri: FileUri) -> Self {
        self.with_source(DocumentSource::Uri(uri))
    }

    /// Replaces the language without inspecting the path or buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, EditorLanguage};
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::new()).with_language(EditorLanguage::Rust);
    /// assert_eq!(document.language, EditorLanguage::Rust);
    /// ```
    pub fn with_language(mut self, language: EditorLanguage) -> Self {
        self.language = language;
        self
    }

    /// Replaces the current language with extension-based detection.
    ///
    /// With no hint this selects `PlainText`; an unrecognized extension selects
    /// `Unknown`, even if a different explicit language was previously set.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, EditorLanguage};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut document = Document::new(DocumentId(1), TextBuffer::new()).with_path("lib.rs");
    /// document.detect_language();
    /// assert_eq!(document.language, EditorLanguage::Rust);
    /// ```
    pub fn detect_language(&mut self) {
        self.language = detect_language(self.language_path_hint());
    }

    /// Returns `path` first, then the source-derived hint.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    /// use ailloli_ui_editor::{Document, DocumentId};
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::new()).with_path("main.rs");
    /// assert_eq!(document.language_path_hint(), Some(Path::new("main.rs")));
    /// ```
    pub fn language_path_hint(&self) -> Option<&Path> {
        self.path.as_deref().or_else(|| self.source.path_hint())
    }
}

/// Resolves an override, explicit language, or extension-derived fallback.
///
/// `override_language` always wins, including `Unknown`. Without it, explicit
/// languages other than `PlainText`/`Unknown` win. Failed detection preserves
/// the document's existing plain/unknown value.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{resolve_document_language, Document, DocumentId, EditorLanguage};
/// use ailloli_ui_text::TextBuffer;
/// let document = Document::new(DocumentId(1), TextBuffer::new()).with_path("lib.rs");
/// assert_eq!(resolve_document_language(&document, None), EditorLanguage::Rust);
/// assert_eq!(resolve_document_language(&document, Some(EditorLanguage::Markdown)), EditorLanguage::Markdown);
/// ```
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

/// Detects language solely from a case-insensitive filename extension.
///
/// No path returns `PlainText`; an absent, non-UTF-8, or unsupported extension
/// returns `Unknown`. The function performs no I/O and never examines content.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use ailloli_ui_editor::{code::detect_language, EditorLanguage};
/// assert_eq!(detect_language(Some(Path::new("view.TSX"))), EditorLanguage::TypeScript);
/// assert_eq!(detect_language(Some(Path::new("README"))), EditorLanguage::Unknown);
/// assert_eq!(detect_language(None), EditorLanguage::PlainText);
/// ```
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
