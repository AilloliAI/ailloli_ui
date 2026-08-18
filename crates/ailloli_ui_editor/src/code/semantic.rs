use std::ops::Range;

use crate::code::{
    CodeFileSummary, CodeSymbol, Diagnostic, DiagnosticSeverity, Document, DocumentId,
    DocumentVersion, EditorLanguage, SymbolEdge, SymbolEdgeKind, SymbolId, SymbolKind,
    SymbolSource,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct LspRequestId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct LspCapabilities {
    pub document_symbols: bool,
    pub definitions: bool,
    pub references: bool,
    pub diagnostics: bool,
    pub hover: bool,
    pub semantic_tokens: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LspError {
    BackendUnavailable,
    CapabilityUnavailable(&'static str),
    RequestCancelled(LspRequestId),
    Protocol(String),
    Io(String),
}

pub trait LspBackend {
    fn capabilities(&self) -> LspCapabilities;

    fn open_document(&mut self, _document: &Document) -> Result<(), LspError> {
        Ok(())
    }

    fn change_document(&mut self, _document: &Document) -> Result<(), LspError> {
        Ok(())
    }

    fn close_document(&mut self, _document: &Document) -> Result<(), LspError> {
        Ok(())
    }

    fn cancel(&mut self, request_id: LspRequestId) -> Result<(), LspError> {
        Err(LspError::RequestCancelled(request_id))
    }

    fn document_symbols(
        &mut self,
        _document: &Document,
    ) -> Result<Vec<SemanticDocumentSymbol>, LspError> {
        Err(LspError::CapabilityUnavailable("document_symbols"))
    }

    fn references(&mut self, _document: &Document) -> Result<Vec<SemanticReference>, LspError> {
        Err(LspError::CapabilityUnavailable("references"))
    }

    fn diagnostics(&mut self, _document: &Document) -> Result<Vec<LspDiagnostic>, LspError> {
        Err(LspError::CapabilityUnavailable("diagnostics"))
    }
}

#[derive(Debug, Default)]
pub struct NoopLspBackend;

impl LspBackend for NoopLspBackend {
    fn capabilities(&self) -> LspCapabilities {
        LspCapabilities::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SemanticDocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: Range<usize>,
    pub selection_range: Range<usize>,
    pub detail: Option<String>,
    pub source: SymbolSource,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SemanticReference {
    pub from: SymbolId,
    pub to: SymbolId,
    pub kind: SymbolEdgeKind,
    pub source: SymbolSource,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LspDiagnostic {
    pub range: Range<usize>,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub document_version: DocumentVersion,
}

pub fn lsp_symbols_to_code_symbols(
    document: &Document,
    symbols: &[SemanticDocumentSymbol],
) -> Vec<CodeSymbol> {
    semantic_symbols_to_code_symbols(document, symbols, SymbolSource::Lsp)
}

pub fn scip_symbols_to_code_symbols(
    document: &Document,
    symbols: &[SemanticDocumentSymbol],
) -> Vec<CodeSymbol> {
    semantic_symbols_to_code_symbols(document, symbols, SymbolSource::Scip)
}

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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LspEnrichment {
    pub document_version: DocumentVersion,
    pub capabilities: LspCapabilities,
    pub symbols: Vec<SemanticDocumentSymbol>,
    pub references: Vec<SemanticReference>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipProjectIndex {
    pub metadata: ScipProjectMetadata,
    pub documents: Vec<ScipDocumentIndex>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipProjectMetadata {
    pub project_root: String,
    pub tool_info: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipDocumentIndex {
    pub path: String,
    pub language: EditorLanguage,
    pub version: DocumentVersion,
    pub symbols: Vec<ScipSymbol>,
    pub occurrences: Vec<ScipOccurrence>,
    pub relations: Vec<ScipRelation>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipSymbol {
    pub symbol: String,
    pub name: String,
    pub kind: SymbolKind,
    pub range: Range<usize>,
    pub selection_range: Range<usize>,
    pub signature: Option<String>,
    pub docs: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ScipOccurrenceRole {
    Definition,
    Reference,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipOccurrence {
    pub symbol: String,
    pub range: Range<usize>,
    pub role: ScipOccurrenceRole,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipRelation {
    pub from_symbol: String,
    pub to_symbol: String,
    pub kind: SymbolEdgeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipProjectSummary {
    pub metadata: ScipProjectMetadata,
    pub documents: Vec<CodeFileSummary>,
    pub navigation: Vec<ScipNavigationLink>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipNavigationLink {
    pub from_path: String,
    pub from_range: Range<usize>,
    pub to_path: String,
    pub to_symbol: String,
    pub kind: SymbolEdgeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScipImportError {
    Json(String),
}

pub fn import_scip_json_str(json: &str) -> Result<ScipProjectIndex, ScipImportError> {
    serde_json::from_str(json).map_err(|err| ScipImportError::Json(err.to_string()))
}

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

fn clamp_range(range: Range<usize>, len: usize) -> Range<usize> {
    range.start.min(len)..range.end.min(len)
}

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

fn symbol_source_priority(source: SymbolSource) -> u8 {
    match source {
        SymbolSource::TreeSitter => 0,
        SymbolSource::Lsp => 1,
        SymbolSource::Scip => 2,
        SymbolSource::Ctags => 3,
        SymbolSource::Lexical => 4,
    }
}

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
