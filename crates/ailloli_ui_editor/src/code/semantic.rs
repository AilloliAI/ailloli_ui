//! UI-agnostic LSP enrichment and JSON SCIP import models.

use std::ops::Range;

use crate::code::{
    CodeFileSummary, CodeSymbol, Diagnostic, DiagnosticSeverity, Document, DocumentId,
    DocumentVersion, EditorLanguage, SymbolEdge, SymbolEdgeKind, SymbolId, SymbolKind,
    SymbolSource,
};

/// Opaque caller/backend request identifier used for cancellation.
///
/// Zero is valid and allocation policy belongs to the backend.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::LspRequestId;
/// assert_eq!(LspRequestId(42).0, 42);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct LspRequestId(pub u64);

/// Feature set advertised synchronously by an LSP backend.
///
/// Every capability defaults to `false`; callers should not invoke optional
/// query methods unless the corresponding flag is true.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::LspCapabilities;
/// let capabilities = LspCapabilities::default();
/// assert!(!capabilities.document_symbols && !capabilities.diagnostics);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct LspCapabilities {
    /// Backend can return document symbols.
    pub document_symbols: bool,
    /// Backend supports definition lookup outside this minimal trait surface.
    pub definitions: bool,
    /// Backend can return semantic references.
    pub references: bool,
    /// Backend can return diagnostics.
    pub diagnostics: bool,
    /// Backend supports hover outside this minimal trait surface.
    pub hover: bool,
    /// Backend supports semantic tokens outside this minimal trait surface.
    pub semantic_tokens: bool,
}

/// Typed failure from an LSP backend or adapter boundary.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{LspError, LspRequestId};
/// assert_eq!(LspError::RequestCancelled(LspRequestId(1)), LspError::RequestCancelled(LspRequestId(1)));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LspError {
    /// No backend is configured or reachable.
    BackendUnavailable,
    /// A named optional operation was not advertised or implemented.
    CapabilityUnavailable(&'static str),
    /// The specified request was cancelled.
    RequestCancelled(LspRequestId),
    /// Protocol/response validation failure with an owned explanation.
    Protocol(String),
    /// Transport or operating-system I/O failure with an owned explanation.
    Io(String),
}

/// Synchronous, UI-agnostic interface for optional language-server enrichment.
///
/// Lifecycle methods default to success, cancellation defaults to a typed
/// cancellation error, and query methods default to capability errors. Backends
/// own transport, queuing, concurrency, timeouts, and request-ID allocation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{LspBackend, NoopLspBackend};
/// let backend = NoopLspBackend;
/// assert_eq!(backend.capabilities().document_symbols, false);
/// ```
pub trait LspBackend {
    /// Returns the backend's currently advertised optional operations.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{LspBackend, NoopLspBackend};
    /// assert_eq!(NoopLspBackend.capabilities().diagnostics, false);
    /// ```
    fn capabilities(&self) -> LspCapabilities;

    /// Notifies the backend that a document opened.
    ///
    /// The default performs no work and returns `Ok(())` regardless of document
    /// contents or metadata.
    ///
    /// # Errors
    ///
    /// The default implementation never fails. A backend override may return a
    /// categorized [`LspError`] for transport, protocol, availability, or I/O
    /// failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, LspBackend, NoopLspBackend};
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::new());
    /// assert!(NoopLspBackend.open_document(&document).is_ok());
    /// ```
    fn open_document(&mut self, _document: &Document) -> Result<(), LspError> {
        Ok(())
    }

    /// Notifies the backend that document content or metadata changed.
    ///
    /// The default is a no-op success and performs no version validation.
    ///
    /// # Errors
    ///
    /// The default implementation never fails. A backend override may return a
    /// categorized [`LspError`] for transport, protocol, availability, or I/O
    /// failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, LspBackend, NoopLspBackend};
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::new());
    /// assert!(NoopLspBackend.change_document(&document).is_ok());
    /// ```
    fn change_document(&mut self, _document: &Document) -> Result<(), LspError> {
        Ok(())
    }

    /// Notifies the backend that a document closed.
    ///
    /// The default is a no-op success.
    ///
    /// # Errors
    ///
    /// The default implementation never fails. A backend override may return a
    /// categorized [`LspError`] for transport, protocol, availability, or I/O
    /// failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, LspBackend, NoopLspBackend};
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::new());
    /// assert!(NoopLspBackend.close_document(&document).is_ok());
    /// ```
    fn close_document(&mut self, _document: &Document) -> Result<(), LspError> {
        Ok(())
    }

    /// Requests cancellation of an opaque request ID.
    ///
    /// The default returns [`LspError::RequestCancelled`] containing the same ID;
    /// it does not track or signal any real request.
    ///
    /// # Errors
    ///
    /// The default returns [`LspError::RequestCancelled`] with the supplied ID.
    /// Backend overrides may additionally report availability, protocol,
    /// transport, or I/O failures.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{LspBackend, LspError, LspRequestId, NoopLspBackend};
    /// assert_eq!(NoopLspBackend.cancel(LspRequestId(7)), Err(LspError::RequestCancelled(LspRequestId(7))));
    /// ```
    fn cancel(&mut self, request_id: LspRequestId) -> Result<(), LspError> {
        Err(LspError::RequestCancelled(request_id))
    }

    /// Returns semantic symbols for a document.
    ///
    /// The default returns `CapabilityUnavailable("document_symbols")`.
    ///
    /// # Errors
    ///
    /// The default returns [`LspError::CapabilityUnavailable`]. A backend may
    /// instead return cancellation, transport, protocol, availability, or I/O
    /// failures while servicing the query.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, LspBackend, LspError, NoopLspBackend};
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::new());
    /// assert_eq!(NoopLspBackend.document_symbols(&document), Err(LspError::CapabilityUnavailable("document_symbols")));
    /// ```
    fn document_symbols(
        &mut self,
        _document: &Document,
    ) -> Result<Vec<SemanticDocumentSymbol>, LspError> {
        Err(LspError::CapabilityUnavailable("document_symbols"))
    }

    /// Returns semantic reference edges for a document.
    ///
    /// The default returns `CapabilityUnavailable("references")`.
    ///
    /// # Errors
    ///
    /// The default returns [`LspError::CapabilityUnavailable`]. A backend may
    /// instead return cancellation, transport, protocol, availability, or I/O
    /// failures while servicing the query.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, LspBackend, LspError, NoopLspBackend};
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::new());
    /// assert_eq!(NoopLspBackend.references(&document), Err(LspError::CapabilityUnavailable("references")));
    /// ```
    fn references(&mut self, _document: &Document) -> Result<Vec<SemanticReference>, LspError> {
        Err(LspError::CapabilityUnavailable("references"))
    }

    /// Returns version-tagged diagnostics for a document.
    ///
    /// The default returns `CapabilityUnavailable("diagnostics")`.
    ///
    /// # Errors
    ///
    /// The default returns [`LspError::CapabilityUnavailable`]. A backend may
    /// instead return cancellation, transport, protocol, availability, or I/O
    /// failures while servicing the query.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, LspBackend, LspError, NoopLspBackend};
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::new());
    /// assert_eq!(NoopLspBackend.diagnostics(&document), Err(LspError::CapabilityUnavailable("diagnostics")));
    /// ```
    fn diagnostics(&mut self, _document: &Document) -> Result<Vec<LspDiagnostic>, LspError> {
        Err(LspError::CapabilityUnavailable("diagnostics"))
    }
}

/// Backend whose capabilities are all false and whose methods use trait defaults.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{LspBackend, LspCapabilities, NoopLspBackend};
/// assert_eq!(NoopLspBackend.capabilities(), LspCapabilities::default());
/// ```
#[derive(Debug, Default)]
pub struct NoopLspBackend;

/// Advertises no optional LSP capabilities.
impl LspBackend for NoopLspBackend {
    /// Returns [`LspCapabilities::default`].
    fn capabilities(&self) -> LspCapabilities {
        LspCapabilities::default()
    }
}

/// Language-server/SCIP symbol payload before conversion to local symbol IR.
///
/// Ranges are unvalidated UTF-8 byte offsets. `detail` typically contains a
/// signature, while `None` means the backend supplied no detail.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::SemanticDocumentSymbol, SymbolKind, SymbolSource};
/// let symbol = SemanticDocumentSymbol { name: "run".into(), kind: SymbolKind::Function, range: 0..8, selection_range: 3..6, detail: None, source: SymbolSource::Lsp };
/// assert_eq!(symbol.selection_range, 3..6);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SemanticDocumentSymbol {
    /// Display name, including an empty name if supplied by the backend.
    pub name: String,
    /// Language-neutral symbol category.
    pub kind: SymbolKind,
    /// Half-open extent in document UTF-8 bytes.
    pub range: Range<usize>,
    /// Half-open name/selection range in document UTF-8 bytes.
    pub selection_range: Range<usize>,
    /// Optional backend detail copied into the local signature field.
    pub detail: Option<String>,
    /// Claimed semantic source; conversion functions override this field.
    pub source: SymbolSource,
}

/// Directed semantic relation between two summary-local symbol IDs.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::SemanticReference, SymbolEdgeKind, SymbolId, SymbolSource};
/// let reference = SemanticReference { from: SymbolId(1), to: SymbolId(2), kind: SymbolEdgeKind::Calls, source: SymbolSource::Lsp };
/// assert_eq!(reference.to, SymbolId(2));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SemanticReference {
    /// Source symbol ID.
    pub from: SymbolId,
    /// Target symbol ID.
    pub to: SymbolId,
    /// Directed relation category.
    pub kind: SymbolEdgeKind,
    /// Provenance retained in this input model but omitted from [`SymbolEdge`].
    pub source: SymbolSource,
}

/// Raw version-tagged LSP diagnostic.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{DiagnosticSeverity, DocumentVersion, LspDiagnostic};
/// let diagnostic = LspDiagnostic { range: 1..3, severity: DiagnosticSeverity::Warning, message: "unused".into(), document_version: DocumentVersion(2) };
/// assert_eq!(diagnostic.document_version, DocumentVersion(2));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LspDiagnostic {
    /// Unvalidated half-open UTF-8 byte range.
    pub range: Range<usize>,
    /// Diagnostic importance.
    pub severity: DiagnosticSeverity,
    /// Exact backend message.
    pub message: String,
    /// Document version to which the diagnostic applies.
    pub document_version: DocumentVersion,
}

/// Converts semantic symbols into document-local symbols sourced as LSP.
///
/// IDs are assigned from one in input order. Both ranges are numerically
/// clamped to document byte length without UTF-8 boundary repair or ordering;
/// parent and docs are `None`, and `detail` becomes the signature. Each input's
/// own `source` field is ignored.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::{lsp_symbols_to_code_symbols, SemanticDocumentSymbol}, Document, DocumentId, SymbolId, SymbolKind, SymbolSource};
/// use ailloli_ui_text::TextBuffer;
/// let document = Document::new(DocumentId(1), TextBuffer::from_string("fn x"));
/// let input = [SemanticDocumentSymbol { name: "x".into(), kind: SymbolKind::Function, range: 0..99, selection_range: 3..4, detail: Some("fn x".into()), source: SymbolSource::Scip }];
/// let symbols = lsp_symbols_to_code_symbols(&document, &input);
/// assert_eq!((symbols[0].id, symbols[0].source, symbols[0].range.clone()), (SymbolId(1), SymbolSource::Lsp, 0..4));
/// ```
pub fn lsp_symbols_to_code_symbols(
    document: &Document,
    symbols: &[SemanticDocumentSymbol],
) -> Vec<CodeSymbol> {
    semantic_symbols_to_code_symbols(document, symbols, SymbolSource::Lsp)
}

/// Converts semantic symbols into document-local symbols sourced as SCIP.
///
/// Conversion otherwise follows [`lsp_symbols_to_code_symbols`]: positional
/// one-based IDs, numeric length clamping, no parent/docs, and detail-to-signature.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::{scip_symbols_to_code_symbols, SemanticDocumentSymbol}, Document, DocumentId, SymbolKind, SymbolSource};
/// use ailloli_ui_text::TextBuffer;
/// let document = Document::new(DocumentId(1), TextBuffer::from_string("x"));
/// let input = [SemanticDocumentSymbol { name: "x".into(), kind: SymbolKind::Variable, range: 0..1, selection_range: 0..1, detail: None, source: SymbolSource::Lsp }];
/// assert_eq!(scip_symbols_to_code_symbols(&document, &input)[0].source, SymbolSource::Scip);
/// ```
pub fn scip_symbols_to_code_symbols(
    document: &Document,
    symbols: &[SemanticDocumentSymbol],
) -> Vec<CodeSymbol> {
    semantic_symbols_to_code_symbols(document, symbols, SymbolSource::Scip)
}

/// Copies semantic references into source-agnostic symbol edges.
///
/// Order and duplicates are preserved; each input's `source` field is dropped.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::{semantic_references_to_edges, SemanticReference}, SymbolEdgeKind, SymbolId, SymbolSource};
/// let references = [SemanticReference { from: SymbolId(1), to: SymbolId(2), kind: SymbolEdgeKind::References, source: SymbolSource::Lsp }];
/// let edges = semantic_references_to_edges(&references);
/// assert_eq!((edges[0].from, edges[0].to), (SymbolId(1), SymbolId(2)));
/// ```
pub fn semantic_references_to_edges(references: &[SemanticReference]) -> Vec<SymbolEdge> {
    references
        .iter()
        .map(|reference| SymbolEdge {
            from: reference.from,
            to: reference.to,
            kind: reference.kind,
        })
        .collect()
}

/// Converts current-version LSP diagnostics into local diagnostics.
///
/// Stale versions are discarded. Endpoints clamp numerically to document byte
/// length without UTF-8 boundary repair; empty or reversed results are removed.
/// Remaining inputs preserve order and become [`crate::DiagnosticSource::Lsp`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::lsp_diagnostics_to_diagnostics, DiagnosticSeverity, Document, DocumentId, DocumentVersion, LspDiagnostic};
/// use ailloli_ui_text::TextBuffer;
/// let document = Document::new(DocumentId(1), TextBuffer::from_string("abc"));
/// let diagnostics = [
///     LspDiagnostic { range: 1..99, severity: DiagnosticSeverity::Error, message: "bad".into(), document_version: DocumentVersion(0) },
///     LspDiagnostic { range: 0..1, severity: DiagnosticSeverity::Hint, message: "stale".into(), document_version: DocumentVersion(1) },
/// ];
/// let mapped = lsp_diagnostics_to_diagnostics(&document, &diagnostics);
/// assert_eq!(mapped.len(), 1);
/// assert_eq!(mapped[0].range, 1..3);
/// ```
pub fn lsp_diagnostics_to_diagnostics(
    document: &Document,
    diagnostics: &[LspDiagnostic],
) -> Vec<Diagnostic> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.document_version == document.version)
        .map(|diagnostic| {
            Diagnostic::lsp(
                clamp_range(diagnostic.range.clone(), document.buffer.len_bytes()),
                diagnostic.severity,
                diagnostic.message.clone(),
                diagnostic.document_version,
            )
        })
        .filter(|diagnostic| diagnostic.range.start < diagnostic.range.end)
        .collect()
}

/// Collects one synchronous enrichment snapshot from advertised capabilities.
///
/// Queries run in symbol, reference, then diagnostic order and stop at the first
/// error. Unadvertised query methods are not called and yield empty vectors.
/// Diagnostics are current-version filtered and clamped; symbols/references are
/// returned in backend form for later mapping. This function owns no timeout,
/// queue, cancellation, or concurrency policy.
///
/// # Errors
///
/// Returns the first error produced by an advertised backend query.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::collect_lsp_enrichment, Document, DocumentId, NoopLspBackend};
/// use ailloli_ui_text::TextBuffer;
/// let document = Document::new(DocumentId(1), TextBuffer::new());
/// let enrichment = collect_lsp_enrichment(&mut NoopLspBackend, &document).unwrap();
/// assert!(enrichment.symbols.is_empty() && enrichment.diagnostics.is_empty());
/// ```
pub fn collect_lsp_enrichment<B: LspBackend>(
    backend: &mut B,
    document: &Document,
) -> Result<LspEnrichment, LspError> {
    let capabilities = backend.capabilities();
    let symbols = if capabilities.document_symbols {
        backend.document_symbols(document)?
    } else {
        Vec::new()
    };
    let references = if capabilities.references {
        backend.references(document)?
    } else {
        Vec::new()
    };
    let diagnostics = if capabilities.diagnostics {
        lsp_diagnostics_to_diagnostics(document, &backend.diagnostics(document)?)
    } else {
        Vec::new()
    };

    Ok(LspEnrichment {
        document_version: document.version,
        capabilities,
        symbols,
        references,
        diagnostics,
    })
}

/// Complete enrichment snapshot tied to one document version.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{DocumentVersion, LspCapabilities, LspEnrichment};
/// let enrichment = LspEnrichment { document_version: DocumentVersion(3), capabilities: LspCapabilities::default(), symbols: vec![], references: vec![], diagnostics: vec![] };
/// assert_eq!(enrichment.document_version, DocumentVersion(3));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LspEnrichment {
    /// Document version captured before backend queries.
    pub document_version: DocumentVersion,
    /// Capability snapshot used to decide which queries ran.
    pub capabilities: LspCapabilities,
    /// Raw semantic symbols from the backend.
    pub symbols: Vec<SemanticDocumentSymbol>,
    /// Raw semantic references from the backend.
    pub references: Vec<SemanticReference>,
    /// Current-version mapped LSP diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}

/// JSON-deserializable SCIP-like project index.
///
/// This is the crate's compact interchange schema, not a protobuf SCIP decoder.
/// Document order determines one-based IDs in project summaries.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{ScipProjectIndex, ScipProjectMetadata};
/// let index = ScipProjectIndex { metadata: ScipProjectMetadata { project_root: "/repo".into(), tool_info: "tool 1".into() }, documents: vec![] };
/// assert!(index.documents.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipProjectIndex {
    /// Project-wide origin/tool metadata.
    pub metadata: ScipProjectMetadata,
    /// Indexed files in deterministic caller-provided order.
    pub documents: Vec<ScipDocumentIndex>,
}

/// Project provenance stored with SCIP import data.
///
/// Both strings are opaque and may be empty; no path canonicalization or tool
/// version parsing occurs.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::ScipProjectMetadata;
/// let metadata = ScipProjectMetadata { project_root: "workspace".into(), tool_info: "scip-tool".into() };
/// assert_eq!(metadata.project_root, "workspace");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipProjectMetadata {
    /// Opaque project-root string.
    pub project_root: String,
    /// Opaque producer name/version string.
    pub tool_info: String,
}

/// One imported SCIP document and its local semantic records.
///
/// Ranges throughout are unvalidated UTF-8 byte offsets. Symbol relations are
/// resolved only against this document; occurrences additionally drive
/// cross-document navigation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{DocumentVersion, EditorLanguage, ScipDocumentIndex};
/// let document = ScipDocumentIndex { path: "src/lib.rs".into(), language: EditorLanguage::Rust, version: DocumentVersion(1), symbols: vec![], occurrences: vec![], relations: vec![] };
/// assert_eq!(document.language, EditorLanguage::Rust);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipDocumentIndex {
    /// Opaque project-relative or absolute path string.
    pub path: String,
    /// Language attached by the index producer.
    pub language: EditorLanguage,
    /// Producer-supplied document version.
    pub version: DocumentVersion,
    /// Definitions/descriptors in positional-ID order.
    pub symbols: Vec<ScipSymbol>,
    /// Definition/reference occurrences used for navigation.
    pub occurrences: Vec<ScipOccurrence>,
    /// Local symbol-name relations used for graph edges.
    pub relations: Vec<ScipRelation>,
}

/// Imported SCIP symbol descriptor.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{ScipSymbol, SymbolKind};
/// let symbol = ScipSymbol { symbol: "pkg/foo#".into(), name: "foo".into(), kind: SymbolKind::Function, range: 0..8, selection_range: 3..6, signature: None, docs: None };
/// assert_eq!(symbol.name, "foo");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipSymbol {
    /// Canonical SCIP symbol string used to resolve relations/occurrences.
    pub symbol: String,
    /// Human-readable symbol name.
    pub name: String,
    /// Language-neutral symbol category.
    pub kind: SymbolKind,
    /// Unvalidated half-open full extent in document UTF-8 bytes.
    pub range: Range<usize>,
    /// Unvalidated half-open name range in document UTF-8 bytes.
    pub selection_range: Range<usize>,
    /// Optional exact signature; `None` differs from an empty signature.
    pub signature: Option<String>,
    /// Optional exact documentation; `None` differs from empty documentation.
    pub docs: Option<String>,
}

/// Whether a SCIP occurrence defines or references its symbol.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::ScipOccurrenceRole;
/// assert_ne!(ScipOccurrenceRole::Definition, ScipOccurrenceRole::Reference);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ScipOccurrenceRole {
    /// Target location for navigation.
    Definition,
    /// Source location linked to the first matching definition.
    Reference,
}

/// One definition or reference occurrence of a canonical SCIP symbol.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{ScipOccurrence, ScipOccurrenceRole};
/// let occurrence = ScipOccurrence { symbol: "pkg/foo#".into(), range: 4..7, role: ScipOccurrenceRole::Reference };
/// assert_eq!(occurrence.range, 4..7);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipOccurrence {
    /// Canonical SCIP symbol string.
    pub symbol: String,
    /// Unvalidated half-open occurrence range in UTF-8 bytes.
    pub range: Range<usize>,
    /// Definition or reference role.
    pub role: ScipOccurrenceRole,
}

/// Local relation between two canonical SCIP symbol strings.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{ScipRelation, SymbolEdgeKind};
/// let relation = ScipRelation { from_symbol: "a".into(), to_symbol: "b".into(), kind: SymbolEdgeKind::Calls };
/// assert_eq!(relation.kind, SymbolEdgeKind::Calls);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipRelation {
    /// Canonical source symbol string.
    pub from_symbol: String,
    /// Canonical target symbol string.
    pub to_symbol: String,
    /// Directed relation category.
    pub kind: SymbolEdgeKind,
}

/// Converted project summaries plus cross-document navigation links.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{ScipProjectMetadata, ScipProjectSummary};
/// let summary = ScipProjectSummary { metadata: ScipProjectMetadata { project_root: String::new(), tool_info: String::new() }, documents: vec![], navigation: vec![] };
/// assert!(summary.documents.is_empty() && summary.navigation.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipProjectSummary {
    /// Metadata copied from the imported project.
    pub metadata: ScipProjectMetadata,
    /// Per-document local symbol summaries.
    pub documents: Vec<CodeFileSummary>,
    /// Sorted cross-document reference-to-definition links.
    pub navigation: Vec<ScipNavigationLink>,
}

/// Cross-document navigation from one reference to a definition.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{ScipNavigationLink, SymbolEdgeKind};
/// let link = ScipNavigationLink { from_path: "a.rs".into(), from_range: 1..2, to_path: "b.rs".into(), to_symbol: "pkg/b#".into(), kind: SymbolEdgeKind::References };
/// assert_eq!(link.to_path, "b.rs");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipNavigationLink {
    /// Path containing the reference.
    pub from_path: String,
    /// Unvalidated half-open reference range.
    pub from_range: Range<usize>,
    /// Path containing the chosen definition.
    pub to_path: String,
    /// Canonical SCIP target symbol.
    pub to_symbol: String,
    /// Link category, currently always [`SymbolEdgeKind::References`].
    pub kind: SymbolEdgeKind,
}

/// Failure while importing the compact SCIP JSON schema.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::import_scip_json_str, ScipImportError};
/// assert!(matches!(import_scip_json_str("not json"), Err(ScipImportError::Json(_))));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScipImportError {
    /// Owned `serde_json` parse/shape error text.
    Json(String),
}

/// Deserializes the crate's compact SCIP JSON schema.
///
/// Unknown fields are accepted by Serde defaults, while every declared field is
/// required unless its type's deserializer says otherwise. No semantic range,
/// path, symbol, or relation validation is performed.
///
/// # Errors
///
/// Returns [`ScipImportError::Json`] with Serde's owned error text for invalid
/// JSON or an incompatible shape.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::code::import_scip_json_str;
/// let index = import_scip_json_str(r#"{"metadata":{"project_root":"/repo","tool_info":"tool"},"documents":[]}"#).unwrap();
/// assert_eq!(index.metadata.project_root, "/repo");
/// ```
pub fn import_scip_json_str(json: &str) -> Result<ScipProjectIndex, ScipImportError> {
    serde_json::from_str(json).map_err(|err| ScipImportError::Json(err.to_string()))
}

/// Converts every imported document and builds cross-file navigation.
///
/// Documents receive one-based [`DocumentId`] values in input order. Metadata
/// is cloned, local symbol summaries are converted independently, and reference
/// links are sorted deterministically.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::scip_project_to_summary, ScipProjectIndex, ScipProjectMetadata};
/// let index = ScipProjectIndex { metadata: ScipProjectMetadata { project_root: "repo".into(), tool_info: "tool".into() }, documents: vec![] };
/// let summary = scip_project_to_summary(&index);
/// assert_eq!(summary.metadata.project_root, "repo");
/// assert!(summary.documents.is_empty());
/// ```
pub fn scip_project_to_summary(index: &ScipProjectIndex) -> ScipProjectSummary {
    let documents: Vec<_> = index
        .documents
        .iter()
        .enumerate()
        .map(|(idx, document)| scip_document_to_summary(DocumentId(idx as u64 + 1), document))
        .collect();
    let navigation = scip_navigation_links(index);
    ScipProjectSummary {
        metadata: index.metadata.clone(),
        documents,
        navigation,
    }
}

/// Converts one SCIP document into the local symbol/edge representation.
///
/// Symbols receive positional one-based IDs. Ranges, signatures, and docs are
/// copied without clamping; parents are always `None` and source is SCIP.
/// Relations whose canonical endpoints cannot both be resolved are dropped;
/// remaining edges are sorted and deduplicated.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::scip_document_to_summary, DocumentId, DocumentVersion, EditorLanguage, ScipDocumentIndex, ScipSymbol, SymbolId, SymbolKind, SymbolSource};
/// let document = ScipDocumentIndex { path: "lib.rs".into(), language: EditorLanguage::Rust, version: DocumentVersion(2), symbols: vec![ScipSymbol { symbol: "foo".into(), name: "foo".into(), kind: SymbolKind::Function, range: 0..3, selection_range: 0..3, signature: None, docs: None }], occurrences: vec![], relations: vec![] };
/// let summary = scip_document_to_summary(DocumentId(9), &document);
/// assert_eq!((summary.symbols[0].id, summary.symbols[0].source), (SymbolId(1), SymbolSource::Scip));
/// assert_eq!(summary.path.unwrap().to_str(), Some("lib.rs"));
/// ```
pub fn scip_document_to_summary(
    document_id: DocumentId,
    document: &ScipDocumentIndex,
) -> CodeFileSummary {
    let symbols: Vec<_> = document
        .symbols
        .iter()
        .enumerate()
        .map(|(idx, symbol)| CodeSymbol {
            id: SymbolId(idx as u64 + 1),
            name: symbol.name.clone(),
            kind: symbol.kind,
            language: document.language,
            range: symbol.range.clone(),
            selection_range: symbol.selection_range.clone(),
            parent: None,
            signature: symbol.signature.clone(),
            docs: symbol.docs.clone(),
            source: SymbolSource::Scip,
        })
        .collect();
    let edges = scip_edges(document, &symbols);
    CodeFileSummary {
        document_id,
        path: Some(document.path.clone().into()),
        language: document.language,
        version: document.version,
        symbols,
        edges,
    }
}

/// Merges symbol and edge payloads under caller-supplied document metadata.
///
/// Symbols deduplicate by name, kind, and selection range. For duplicates the
/// strongest source wins: Tree-sitter, LSP, SCIP, ctags, then lexical. Results
/// sort by selection start/source/name and receive fresh positional one-based
/// IDs. Edges from all summaries sort by kind/from/to and deduplicate exactly;
/// their IDs are not remapped to the newly assigned symbol IDs.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::merge_code_file_summaries, CodeFileSummary, CodeSymbol, DocumentId, DocumentVersion, EditorLanguage, SymbolId, SymbolKind, SymbolSource};
/// fn summary(source: SymbolSource) -> CodeFileSummary {
///     CodeFileSummary { document_id: DocumentId(1), path: None, language: EditorLanguage::Rust, version: DocumentVersion(0), symbols: vec![CodeSymbol { id: SymbolId(99), name: "f".into(), kind: SymbolKind::Function, language: EditorLanguage::Rust, range: 0..1, selection_range: 0..1, parent: None, signature: None, docs: None, source }], edges: vec![] }
/// }
/// let merged = merge_code_file_summaries(DocumentId(7), None, EditorLanguage::Rust, DocumentVersion(3), &[summary(SymbolSource::Lexical), summary(SymbolSource::Lsp)]);
/// assert_eq!(merged.symbols.len(), 1);
/// assert_eq!((merged.symbols[0].id, merged.symbols[0].source), (SymbolId(1), SymbolSource::Lsp));
/// ```
pub fn merge_code_file_summaries(
    document_id: DocumentId,
    path: Option<std::path::PathBuf>,
    language: EditorLanguage,
    version: DocumentVersion,
    summaries: &[CodeFileSummary],
) -> CodeFileSummary {
    let mut symbols: Vec<CodeSymbol> = Vec::new();
    for summary in summaries {
        for symbol in &summary.symbols {
            if let Some(existing) = symbols.iter_mut().find(|existing| {
                existing.name == symbol.name
                    && existing.kind == symbol.kind
                    && existing.selection_range == symbol.selection_range
            }) {
                if symbol_source_priority(symbol.source) < symbol_source_priority(existing.source) {
                    *existing = symbol.clone();
                }
            } else {
                symbols.push(symbol.clone());
            }
        }
    }
    symbols.sort_by_key(|symbol| {
        (
            symbol.selection_range.start,
            symbol_source_priority(symbol.source),
            symbol.name.clone(),
        )
    });
    for (idx, symbol) in symbols.iter_mut().enumerate() {
        symbol.id = SymbolId(idx as u64 + 1);
    }

    let mut edges = summaries
        .iter()
        .flat_map(|summary| summary.edges.iter().cloned())
        .collect::<Vec<_>>();
    edges.sort_by_key(|edge| (symbol_edge_kind_rank(edge.kind), edge.from.0, edge.to.0));
    edges.dedup_by_key(|edge| (symbol_edge_kind_rank(edge.kind), edge.from.0, edge.to.0));

    CodeFileSummary {
        document_id,
        path,
        language,
        version,
        symbols,
        edges,
    }
}

/// Converts semantic symbols while forcing a specified provenance.
fn semantic_symbols_to_code_symbols(
    document: &Document,
    symbols: &[SemanticDocumentSymbol],
    source: SymbolSource,
) -> Vec<CodeSymbol> {
    symbols
        .iter()
        .enumerate()
        .map(|(idx, symbol)| {
            let range = clamp_range(symbol.range.clone(), document.buffer.len_bytes());
            let selection_range =
                clamp_range(symbol.selection_range.clone(), document.buffer.len_bytes());
            CodeSymbol {
                id: SymbolId(idx as u64 + 1),
                name: symbol.name.clone(),
                kind: symbol.kind,
                language: document.language,
                range,
                selection_range,
                parent: None,
                signature: symbol.detail.clone(),
                docs: None,
                source,
            }
        })
        .collect()
}

/// Numerically clamps both range endpoints to a byte length.
fn clamp_range(range: Range<usize>, len: usize) -> Range<usize> {
    range.start.min(len)..range.end.min(len)
}

/// Resolves, sorts, and deduplicates one document's canonical-name relations.
fn scip_edges(document: &ScipDocumentIndex, symbols: &[CodeSymbol]) -> Vec<SymbolEdge> {
    let mut edges = Vec::new();
    for relation in &document.relations {
        let Some(from) = symbol_id_for_scip_name(symbols, document, &relation.from_symbol) else {
            continue;
        };
        let Some(to) = symbol_id_for_scip_name(symbols, document, &relation.to_symbol) else {
            continue;
        };
        edges.push(SymbolEdge {
            from,
            to,
            kind: relation.kind,
        });
    }
    edges.sort_by_key(|edge| (symbol_edge_kind_rank(edge.kind), edge.from.0, edge.to.0));
    edges.dedup_by_key(|edge| (symbol_edge_kind_rank(edge.kind), edge.from.0, edge.to.0));
    edges
}

/// Resolves the first imported SCIP symbol with a canonical name to positional ID.
fn symbol_id_for_scip_name(
    symbols: &[CodeSymbol],
    document: &ScipDocumentIndex,
    scip_symbol: &str,
) -> Option<SymbolId> {
    document
        .symbols
        .iter()
        .position(|symbol| symbol.symbol == scip_symbol)
        .and_then(|idx| symbols.get(idx))
        .map(|symbol| symbol.id)
}

/// Builds sorted cross-document links to the first matching definition.
fn scip_navigation_links(index: &ScipProjectIndex) -> Vec<ScipNavigationLink> {
    let definitions: Vec<_> = index
        .documents
        .iter()
        .flat_map(|document| {
            document
                .occurrences
                .iter()
                .filter(|occurrence| occurrence.role == ScipOccurrenceRole::Definition)
                .map(move |occurrence| (document.path.clone(), occurrence.clone()))
        })
        .collect();
    let mut links = Vec::new();
    for document in &index.documents {
        for occurrence in &document.occurrences {
            if occurrence.role != ScipOccurrenceRole::Reference {
                continue;
            }
            if let Some((target_path, _)) = definitions
                .iter()
                .find(|(_, definition)| definition.symbol == occurrence.symbol)
            {
                links.push(ScipNavigationLink {
                    from_path: document.path.clone(),
                    from_range: occurrence.range.clone(),
                    to_path: target_path.clone(),
                    to_symbol: occurrence.symbol.clone(),
                    kind: SymbolEdgeKind::References,
                });
            }
        }
    }
    links.sort_by(|a, b| {
        (
            a.from_path.as_str(),
            a.from_range.start,
            a.to_path.as_str(),
            a.to_symbol.as_str(),
        )
            .cmp(&(
                b.from_path.as_str(),
                b.from_range.start,
                b.to_path.as_str(),
                b.to_symbol.as_str(),
            ))
    });
    links
}

/// Returns lower-is-stronger provenance priority for symbol merging.
fn symbol_source_priority(source: SymbolSource) -> u8 {
    match source {
        SymbolSource::TreeSitter => 0,
        SymbolSource::Lsp => 1,
        SymbolSource::Scip => 2,
        SymbolSource::Ctags => 3,
        SymbolSource::Lexical => 4,
    }
}

/// Returns deterministic sort rank for symbol edge categories.
fn symbol_edge_kind_rank(kind: SymbolEdgeKind) -> u8 {
    match kind {
        SymbolEdgeKind::Contains => 0,
        SymbolEdgeKind::Imports => 1,
        SymbolEdgeKind::Calls => 2,
        SymbolEdgeKind::References => 3,
        SymbolEdgeKind::Extends => 4,
        SymbolEdgeKind::Implements => 5,
    }
}
