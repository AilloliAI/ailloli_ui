use std::ops::Range;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::code::{Document, DocumentId, DocumentVersion, EditorLanguage};

/// Stable symbol id inside one indexed summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SymbolId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodeSymbol {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub language: EditorLanguage,
    pub range: Range<usize>,
    pub selection_range: Range<usize>,
    pub parent: Option<SymbolId>,
    pub signature: Option<String>,
    pub docs: Option<String>,
    pub source: SymbolSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SymbolKind {
    Module,
    Namespace,
    Type,
    Impl,
    Struct,
    Enum,
    EnumVariant,
    Trait,
    Interface,
    Function,
    Method,
    Constructor,
    Field,
    Variable,
    Constant,
    TypeAlias,
    Macro,
    Import,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SymbolSource {
    TreeSitter,
    Lexical,
    Ctags,
    Lsp,
    Scip,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodeFileSummary {
    pub document_id: DocumentId,
    pub path: Option<PathBuf>,
    pub language: EditorLanguage,
    pub version: DocumentVersion,
    pub symbols: Vec<CodeSymbol>,
    pub edges: Vec<SymbolEdge>,
}

impl CodeFileSummary {
    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SymbolEdge {
    pub from: SymbolId,
    pub to: SymbolId,
    pub kind: SymbolEdgeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SymbolEdgeKind {
    Contains,
    Calls,
    Imports,
    References,
    Extends,
    Implements,
}

pub trait SymbolIndexer {
    fn index_document(&mut self, document: &Document) -> CodeFileSummary;
}

#[derive(Debug, Default)]
pub struct LexicalRustSymbolIndexer;

impl SymbolIndexer for LexicalRustSymbolIndexer {
    fn index_document(&mut self, document: &Document) -> CodeFileSummary {
        summarize_rust_lexical(document)
    }
}

#[derive(Debug, Clone)]
pub struct CtagsSymbolIndexer {
    source: CtagsSymbolSource,
}

#[derive(Debug, Clone)]
enum CtagsSymbolSource {
    JsonLines(String),
    Runner(CtagsRunnerConfig),
}

impl Default for CtagsSymbolIndexer {
    fn default() -> Self {
        Self {
            source: CtagsSymbolSource::Runner(CtagsRunnerConfig::default()),
        }
    }
}

impl CtagsSymbolIndexer {
    pub fn from_json_lines(json_lines: impl Into<String>) -> Self {
        Self {
            source: CtagsSymbolSource::JsonLines(json_lines.into()),
        }
    }

    pub fn from_runner_config(config: CtagsRunnerConfig) -> Self {
        Self {
            source: CtagsSymbolSource::Runner(config),
        }
    }

    pub fn try_index_document(
        &mut self,
        document: &Document,
    ) -> Result<CodeFileSummary, CtagsError> {
        let json_lines = match &self.source {
            CtagsSymbolSource::JsonLines(json_lines) => json_lines.clone(),
            CtagsSymbolSource::Runner(config) => run_ctags(document, config)?,
        };
        Ok(summary_from_ctags_json_lines(document, &json_lines))
    }
}

impl SymbolIndexer for CtagsSymbolIndexer {
    fn index_document(&mut self, document: &Document) -> CodeFileSummary {
        self.try_index_document(document)
            .unwrap_or_else(|_| empty_summary(document))
    }
}

pub fn index_symbols_with_fallback(
    document: &Document,
    ctags: &mut CtagsSymbolIndexer,
) -> CodeFileSummary {
    #[cfg(feature = "tree-sitter")]
    if document.language == EditorLanguage::Rust {
        let mut tree_sitter = TreeSitterRustSymbolIndexer;
        let summary = tree_sitter.index_document(document);
        if !summary.symbols.is_empty() {
            return summary;
        }
    }
    ctags.index_document(document)
}

fn run_ctags(document: &Document, config: &CtagsRunnerConfig) -> Result<String, CtagsError> {
    let temp_path = ctags_temp_path(document);
    std::fs::write(&temp_path, document.buffer.as_str())
        .map_err(|err| CtagsError::Io(err.to_string()))?;
    let mut child = Command::new(&config.binary)
        .arg("--output-format=json")
        .arg("--fields=+nksSzt")
        .arg("--extras=+q")
        .arg("-f")
        .arg("-")
        .arg(&temp_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                CtagsError::MissingBinary(config.binary.display().to_string())
            } else {
                CtagsError::Io(err.to_string())
            }
        })?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() >= config.timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&temp_path);
                return Err(CtagsError::Timeout {
                    timeout: config.timeout,
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(err) => {
                let _ = child.kill();
                let _ = std::fs::remove_file(&temp_path);
                return Err(CtagsError::Io(err.to_string()));
            }
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|err| CtagsError::Io(err.to_string()))?;
    let _ = std::fs::remove_file(&temp_path);
    if output.stdout.len() > config.max_stdout_bytes {
        return Err(CtagsError::OutputTooLarge {
            len: output.stdout.len(),
            max: config.max_stdout_bytes,
        });
    }
    if !output.status.success() {
        return Err(CtagsError::NonZeroStatus {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    String::from_utf8(output.stdout).map_err(|err| CtagsError::Utf8(err.to_string()))
}

fn ctags_temp_path(document: &Document) -> PathBuf {
    let ext = document
        .path
        .as_deref()
        .and_then(|path| path.extension())
        .and_then(|ext| ext.to_str())
        .unwrap_or("txt");
    std::env::temp_dir().join(format!(
        "ailloli_ui_ctags_{}_{}.{}",
        document.id.0, document.version.0, ext
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CtagsRunnerConfig {
    pub binary: PathBuf,
    pub timeout: Duration,
    pub max_stdout_bytes: usize,
}

impl Default for CtagsRunnerConfig {
    fn default() -> Self {
        Self {
            binary: PathBuf::from("ctags"),
            timeout: Duration::from_secs(2),
            max_stdout_bytes: 512 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CtagsError {
    MissingBinary(String),
    Timeout { timeout: Duration },
    OutputTooLarge { len: usize, max: usize },
    NonZeroStatus { code: Option<i32>, stderr: String },
    Io(String),
    Utf8(String),
}

#[cfg(feature = "tree-sitter")]
#[derive(Debug, Default)]
pub struct TreeSitterRustSymbolIndexer;

#[cfg(feature = "tree-sitter")]
impl SymbolIndexer for TreeSitterRustSymbolIndexer {
    fn index_document(&mut self, document: &Document) -> CodeFileSummary {
        let text = document.buffer.as_str();
        let mut parser = tree_sitter::Parser::new();
        let language = tree_sitter_rust::LANGUAGE.into();
        if parser.set_language(&language).is_err() {
            return summarize_rust_lexical(document);
        }
        let Some(tree) = parser.parse(&text, None) else {
            return summarize_rust_lexical(document);
        };

        let symbols = collect_tree_sitter_rust_symbols(tree.root_node(), &text, document.language);
        let mut edges = containment_edges(&symbols);
        edges.extend(collect_tree_sitter_rust_call_edges(
            tree.root_node(),
            &text,
            &symbols,
        ));
        normalize_edges(&mut edges);

        CodeFileSummary {
            symbols,
            edges,
            ..empty_summary(document)
        }
    }
}

pub fn summarize_rust_lexical(document: &Document) -> CodeFileSummary {
    let text = document.buffer.as_str();
    let mut symbols = Vec::new();
    for (line_start, line) in lines_with_offsets(&text) {
        let trimmed = line.trim_start();
        let leading = line.len() - trimmed.len();
        let start = line_start + leading;
        if let Some(rest) = trimmed.strip_prefix("use ") {
            let rest = rest.trim();
            let name = rest
                .trim_end_matches(';')
                .rsplit("::")
                .next()
                .unwrap_or(rest)
                .trim()
                .to_string();
            push_symbol(
                &mut symbols,
                document.language,
                SymbolKind::Import,
                name,
                start,
                line,
            );
        } else if let Some(rest) = trimmed.strip_prefix("fn ") {
            let name = take_ident(rest);
            push_symbol(
                &mut symbols,
                document.language,
                SymbolKind::Function,
                name,
                start,
                line,
            );
        } else if let Some(rest) = trimmed.strip_prefix("struct ") {
            let name = take_ident(rest);
            push_symbol(
                &mut symbols,
                document.language,
                SymbolKind::Struct,
                name,
                start,
                line,
            );
        } else if let Some(rest) = trimmed.strip_prefix("enum ") {
            let name = take_ident(rest);
            push_symbol(
                &mut symbols,
                document.language,
                SymbolKind::Enum,
                name,
                start,
                line,
            );
        } else if let Some(rest) = trimmed.strip_prefix("trait ") {
            let name = take_ident(rest);
            push_symbol(
                &mut symbols,
                document.language,
                SymbolKind::Trait,
                name,
                start,
                line,
            );
        }
    }

    let mut edges = containment_edges(&symbols);
    edges.extend(call_edges(&text, &symbols));
    normalize_edges(&mut edges);

    CodeFileSummary {
        symbols,
        edges,
        ..empty_summary(document)
    }
}

#[cfg(feature = "tree-sitter")]
fn collect_tree_sitter_rust_symbols(
    root: tree_sitter::Node<'_>,
    text: &str,
    language: EditorLanguage,
) -> Vec<CodeSymbol> {
    let mut collector = TreeSitterSymbolCollector {
        text,
        language,
        symbols: Vec::new(),
    };
    collector.collect(root, None);
    collector.symbols
}

#[cfg(feature = "tree-sitter")]
struct TreeSitterSymbolCollector<'a> {
    text: &'a str,
    language: EditorLanguage,
    symbols: Vec<CodeSymbol>,
}

#[cfg(feature = "tree-sitter")]
impl<'a> TreeSitterSymbolCollector<'a> {
    fn collect(&mut self, node: tree_sitter::Node<'_>, parent: Option<SymbolId>) {
        let mut next_parent = parent;
        if let Some(symbol) = self.symbol_for_node(node, parent) {
            next_parent = symbol_container_parent(symbol.kind).then_some(symbol.id);
            self.symbols.push(symbol);
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect(child, next_parent.or(parent));
        }
    }

    fn symbol_for_node(
        &self,
        node: tree_sitter::Node<'_>,
        parent: Option<SymbolId>,
    ) -> Option<CodeSymbol> {
        let spec = tree_sitter_rust_symbol_spec(
            self.text,
            node,
            parent.and_then(|id| self.kind_for_id(id)),
        )?;
        let name_node = spec.name_node?;
        let name = spec
            .name_override
            .unwrap_or_else(|| node_text(self.text, name_node).trim().to_string());
        if name.is_empty() {
            return None;
        }
        let id = SymbolId(self.symbols.len() as u64 + 1);
        Some(CodeSymbol {
            id,
            name,
            kind: spec.kind,
            language: self.language,
            range: valid_range(self.text, node.start_byte()..node.end_byte()),
            selection_range: valid_range(self.text, name_node.start_byte()..name_node.end_byte()),
            parent,
            signature: signature_for_symbol(self.text, node, spec.kind),
            docs: docs_for_symbol(self.text, node),
            source: SymbolSource::TreeSitter,
        })
    }

    fn kind_for_id(&self, id: SymbolId) -> Option<SymbolKind> {
        self.symbols
            .iter()
            .find(|symbol| symbol.id == id)
            .map(|symbol| symbol.kind)
    }
}

#[cfg(feature = "tree-sitter")]
struct RustSymbolSpec<'tree> {
    kind: SymbolKind,
    name_node: Option<tree_sitter::Node<'tree>>,
    name_override: Option<String>,
}

#[cfg(feature = "tree-sitter")]
fn tree_sitter_rust_symbol_spec<'tree>(
    text: &str,
    node: tree_sitter::Node<'tree>,
    parent_kind: Option<SymbolKind>,
) -> Option<RustSymbolSpec<'tree>> {
    let name = || node.child_by_field_name("name");
    match node.kind() {
        "mod_item" => Some(RustSymbolSpec {
            kind: SymbolKind::Module,
            name_node: name(),
            name_override: None,
        }),
        "use_declaration" => Some(RustSymbolSpec {
            kind: SymbolKind::Import,
            name_node: Some(node),
            name_override: Some(import_name(node_text(text, node))),
        }),
        "struct_item" => Some(RustSymbolSpec {
            kind: SymbolKind::Struct,
            name_node: name(),
            name_override: None,
        }),
        "enum_item" => Some(RustSymbolSpec {
            kind: SymbolKind::Enum,
            name_node: name(),
            name_override: None,
        }),
        "trait_item" => Some(RustSymbolSpec {
            kind: SymbolKind::Trait,
            name_node: name(),
            name_override: None,
        }),
        "impl_item" => {
            let type_node = impl_type_node(node).or_else(name);
            let type_text = type_node
                .map(|type_node| compact_node_text(node_text(text, type_node)))
                .unwrap_or_else(|| "impl".to_string());
            Some(RustSymbolSpec {
                kind: SymbolKind::Impl,
                name_node: type_node,
                name_override: Some(format!("impl {type_text}")),
            })
        }
        "function_item" | "function_signature_item" => Some(RustSymbolSpec {
            kind: if matches!(parent_kind, Some(SymbolKind::Impl | SymbolKind::Trait)) {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            },
            name_node: name(),
            name_override: None,
        }),
        "const_item" | "static_item" => Some(RustSymbolSpec {
            kind: SymbolKind::Constant,
            name_node: name(),
            name_override: None,
        }),
        "type_item" => Some(RustSymbolSpec {
            kind: SymbolKind::TypeAlias,
            name_node: name(),
            name_override: None,
        }),
        "macro_definition" => Some(RustSymbolSpec {
            kind: SymbolKind::Macro,
            name_node: name(),
            name_override: None,
        }),
        "field_declaration" if parent_kind == Some(SymbolKind::Struct) => Some(RustSymbolSpec {
            kind: SymbolKind::Field,
            name_node: name(),
            name_override: None,
        }),
        "enum_variant" if parent_kind == Some(SymbolKind::Enum) => Some(RustSymbolSpec {
            kind: SymbolKind::EnumVariant,
            name_node: name(),
            name_override: None,
        }),
        _ => None,
    }
}

#[cfg(feature = "tree-sitter")]
fn symbol_container_parent(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Module
            | SymbolKind::Impl
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Trait
    )
}

#[cfg(feature = "tree-sitter")]
fn impl_type_node(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    node.child_by_field_name("type").or_else(|| {
        let mut cursor = node.walk();
        let found = node.children(&mut cursor).find(|child| {
            matches!(
                child.kind(),
                "type_identifier"
                    | "generic_type"
                    | "scoped_type_identifier"
                    | "reference_type"
                    | "primitive_type"
            )
        });
        found
    })
}

#[cfg(feature = "tree-sitter")]
fn node_text<'a>(text: &'a str, node: tree_sitter::Node<'_>) -> &'a str {
    text.get(node.start_byte()..node.end_byte())
        .unwrap_or_default()
}

#[cfg(feature = "tree-sitter")]
fn compact_node_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(feature = "tree-sitter")]
fn import_name(text: &str) -> String {
    text.trim()
        .trim_start_matches("pub")
        .trim_start()
        .trim_start_matches("use")
        .trim()
        .trim_end_matches(';')
        .rsplit("::")
        .next()
        .unwrap_or(text)
        .trim_matches(|ch: char| matches!(ch, '{' | '}' | '*' | ' ' | ';'))
        .to_string()
}

#[cfg(feature = "tree-sitter")]
fn valid_range(text: &str, range: Range<usize>) -> Range<usize> {
    let mut start = range.start.min(text.len());
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = range.end.min(text.len());
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    start..end
}

#[cfg(feature = "tree-sitter")]
fn signature_for_symbol(
    text: &str,
    node: tree_sitter::Node<'_>,
    kind: SymbolKind,
) -> Option<String> {
    if matches!(
        kind,
        SymbolKind::Field | SymbolKind::EnumVariant | SymbolKind::Import
    ) {
        return Some(compact_node_text(
            node_text(text, node)
                .trim()
                .trim_end_matches(',')
                .trim_end_matches(';'),
        ));
    }
    let raw = node_text(text, node).trim();
    if raw.is_empty() {
        return None;
    }
    let end = raw
        .find('{')
        .or_else(|| raw.find(';').map(|idx| idx + 1))
        .unwrap_or_else(|| raw.lines().next().map_or(raw.len(), str::len));
    let signature = compact_node_text(raw[..end].trim().trim_end_matches('{').trim());
    (!signature.is_empty()).then_some(signature)
}

#[cfg(feature = "tree-sitter")]
fn docs_for_symbol(text: &str, node: tree_sitter::Node<'_>) -> Option<String> {
    let prefix = &text[..node.start_byte().min(text.len())];
    let mut docs = Vec::new();
    for line in prefix.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if docs.is_empty() {
                continue;
            }
            break;
        }
        if let Some(doc) = trimmed.strip_prefix("///") {
            docs.push(doc.trim().to_string());
        } else if let Some(doc) = trimmed.strip_prefix("//!") {
            docs.push(doc.trim().to_string());
        } else if let Some(doc) = trimmed
            .strip_prefix("#[doc = \"")
            .and_then(|rest| rest.strip_suffix("\"]"))
        {
            docs.push(doc.trim().to_string());
        } else if trimmed.starts_with("#[") {
            continue;
        } else {
            break;
        }
    }
    docs.reverse();
    (!docs.is_empty()).then_some(docs.join("\n"))
}

pub fn parse_ctags_json_lines(document: &Document, json_lines: &str) -> Vec<CodeSymbol> {
    parse_ctags_entries(document, json_lines)
        .into_iter()
        .map(|entry| entry.symbol)
        .collect()
}

fn summary_from_ctags_json_lines(document: &Document, json_lines: &str) -> CodeFileSummary {
    let symbols = parse_ctags_json_lines(document, json_lines);
    let mut edges = containment_edges(&symbols);
    normalize_edges(&mut edges);
    CodeFileSummary {
        symbols,
        edges,
        ..empty_summary(document)
    }
}

fn parse_ctags_entries(document: &Document, json_lines: &str) -> Vec<CtagsParsedSymbol> {
    let entries: Vec<CtagsParsedSymbol> = json_lines
        .lines()
        .filter_map(|line| serde_json::from_str::<CtagsEntry>(line).ok())
        .filter(|entry| entry.name.as_deref().is_some_and(|name| !name.is_empty()))
        .enumerate()
        .map(|(idx, entry)| ctags_entry_to_symbol(document, idx, entry))
        .collect();
    attach_ctags_parents(entries)
}

#[derive(Debug, Clone)]
struct CtagsParsedSymbol {
    symbol: CodeSymbol,
    scope: Option<String>,
    scope_kind: Option<String>,
}

#[allow(dead_code)]
fn parse_ctags_json_lines_legacy(document: &Document, json_lines: &str) -> Vec<CodeSymbol> {
    json_lines
        .lines()
        .filter_map(|line| serde_json::from_str::<CtagsEntry>(line).ok())
        .filter(|entry| entry.name.as_deref().is_some_and(|name| !name.is_empty()))
        .enumerate()
        .map(|(idx, entry)| ctags_entry_to_symbol(document, idx, entry).symbol)
        .collect()
}

fn empty_summary(document: &Document) -> CodeFileSummary {
    CodeFileSummary {
        document_id: document.id,
        path: document.path.clone(),
        language: document.language,
        version: document.version,
        symbols: Vec::new(),
        edges: Vec::new(),
    }
}

#[derive(Debug, serde::Deserialize)]
struct CtagsEntry {
    name: Option<String>,
    kind: Option<String>,
    line: Option<usize>,
    end: Option<usize>,
    pattern: Option<String>,
    signature: Option<String>,
    typeref: Option<String>,
    scope: Option<String>,
    #[serde(rename = "scopeKind")]
    scope_kind: Option<String>,
    language: Option<String>,
    roles: Option<String>,
}

fn ctags_entry_to_symbol(document: &Document, idx: usize, entry: CtagsEntry) -> CtagsParsedSymbol {
    let name = entry.name.unwrap_or_default();
    let text = document.buffer.as_str();
    let range = ctags_full_range(&text, entry.line, entry.end);
    let selection_range = ctags_selection_range(&text, &name, range.clone());
    let signature = entry
        .signature
        .or(entry.pattern)
        .or(entry.typeref)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let _ctags_language = entry.language;
    let _ctags_roles = entry.roles;
    CtagsParsedSymbol {
        symbol: CodeSymbol {
            id: SymbolId(idx as u64 + 1),
            name,
            kind: ctags_kind_to_symbol_kind(entry.kind.as_deref()),
            language: document.language,
            selection_range,
            range,
            parent: None,
            signature,
            docs: None,
            source: SymbolSource::Ctags,
        },
        scope: entry.scope,
        scope_kind: entry.scope_kind,
    }
}

fn attach_ctags_parents(mut entries: Vec<CtagsParsedSymbol>) -> Vec<CtagsParsedSymbol> {
    for idx in 0..entries.len() {
        let Some(scope) = entries[idx].scope.clone() else {
            continue;
        };
        let scope_name = scope.rsplit("::").next().unwrap_or(scope.as_str());
        let scope_kind = entries[idx]
            .scope_kind
            .as_deref()
            .map(|kind| ctags_kind_to_symbol_kind(Some(kind)));
        let parent = entries
            .iter()
            .take(idx)
            .rev()
            .find(|entry| {
                entry.symbol.name == scope_name
                    && scope_kind
                        .is_none_or(|kind| kind == SymbolKind::Unknown || kind == entry.symbol.kind)
            })
            .map(|entry| entry.symbol.id);
        entries[idx].symbol.parent = parent;
    }
    entries
}

fn ctags_kind_to_symbol_kind(kind: Option<&str>) -> SymbolKind {
    match kind.unwrap_or_default() {
        "function" | "f" => SymbolKind::Function,
        "method" | "m" => SymbolKind::Method,
        "struct" | "s" => SymbolKind::Struct,
        "enum" | "e" => SymbolKind::Enum,
        "trait" | "t" => SymbolKind::Trait,
        "impl" => SymbolKind::Impl,
        "class" | "c" => SymbolKind::Type,
        "type" | "typedef" => SymbolKind::TypeAlias,
        "interface" | "i" => SymbolKind::Interface,
        "field" | "member" | "F" => SymbolKind::Field,
        "enumerator" | "enumConstant" => SymbolKind::EnumVariant,
        "variable" | "v" => SymbolKind::Variable,
        "constant" | "C" => SymbolKind::Constant,
        "macro" | "macrodef" => SymbolKind::Macro,
        "namespace" | "module" | "n" => SymbolKind::Namespace,
        "import" | "use" => SymbolKind::Import,
        _ => SymbolKind::Unknown,
    }
}

fn ctags_full_range(text: &str, line: Option<usize>, end: Option<usize>) -> Range<usize> {
    let start = line
        .and_then(|line| line_start_offset(text, line.saturating_sub(1)))
        .unwrap_or(0)
        .min(text.len());
    let end = end
        .and_then(|line| line_start_offset(text, line))
        .unwrap_or_else(|| line_end_offset(text, start))
        .min(text.len());
    start..end.max(start)
}

fn ctags_selection_range(text: &str, name: &str, range: Range<usize>) -> Range<usize> {
    if let Some(name_start) = text[range.clone()].find(name) {
        let absolute = range.start + name_start;
        absolute..absolute + name.len()
    } else {
        range
    }
}

#[allow(dead_code)]
fn ctags_range(text: &str, name: &str, line: Option<usize>, end: Option<usize>) -> Range<usize> {
    ctags_selection_range(text, name, ctags_full_range(text, line, end))
}

fn line_start_offset(text: &str, zero_based_line: usize) -> Option<usize> {
    if zero_based_line == 0 {
        return Some(0);
    }
    let mut current = 0;
    for (offset, ch) in text.char_indices() {
        if ch == '\n' {
            current += 1;
            if current == zero_based_line {
                return Some(offset + ch.len_utf8());
            }
        }
    }
    None
}

fn line_end_offset(text: &str, start: usize) -> usize {
    text[start..]
        .find('\n')
        .map(|offset| start + offset)
        .unwrap_or(text.len())
}

fn containment_edges(symbols: &[CodeSymbol]) -> Vec<SymbolEdge> {
    symbols
        .iter()
        .map(|symbol| SymbolEdge {
            from: symbol.parent.unwrap_or(SymbolId(0)),
            to: symbol.id,
            kind: if symbol.kind == SymbolKind::Import {
                SymbolEdgeKind::Imports
            } else {
                SymbolEdgeKind::Contains
            },
        })
        .collect()
}

fn normalize_edges(edges: &mut Vec<SymbolEdge>) {
    edges.sort_by_key(|edge| (symbol_edge_rank(edge.kind), edge.from.0, edge.to.0));
    edges.dedup_by_key(|edge| (edge.from, edge.to, edge.kind));
}

fn symbol_edge_rank(kind: SymbolEdgeKind) -> u8 {
    match kind {
        SymbolEdgeKind::Contains => 0,
        SymbolEdgeKind::Imports => 1,
        SymbolEdgeKind::Calls => 2,
        SymbolEdgeKind::References => 3,
        SymbolEdgeKind::Extends => 4,
        SymbolEdgeKind::Implements => 5,
    }
}

fn call_edges(text: &str, symbols: &[CodeSymbol]) -> Vec<SymbolEdge> {
    let functions: Vec<_> = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Function)
        .collect();
    let mut edges = Vec::new();
    for function in &functions {
        let needle = format!("{}(", function.name);
        let mut start = 0;
        while let Some(pos) = text[start..].find(&needle) {
            let absolute = start + pos;
            if !function.selection_range.contains(&absolute) {
                edges.push(SymbolEdge {
                    from: SymbolId(0),
                    to: function.id,
                    kind: SymbolEdgeKind::Calls,
                });
                break;
            }
            start = absolute + needle.len();
        }
    }
    edges
}

#[cfg(feature = "tree-sitter")]
fn collect_tree_sitter_rust_call_edges(
    root: tree_sitter::Node<'_>,
    text: &str,
    symbols: &[CodeSymbol],
) -> Vec<SymbolEdge> {
    let mut edges = Vec::new();
    collect_tree_sitter_rust_call_edges_from_node(root, text, symbols, &mut edges);
    normalize_edges(&mut edges);
    edges
}

#[cfg(feature = "tree-sitter")]
fn collect_tree_sitter_rust_call_edges_from_node(
    node: tree_sitter::Node<'_>,
    text: &str,
    symbols: &[CodeSymbol],
    edges: &mut Vec<SymbolEdge>,
) {
    if node.kind() == "call_expression" {
        if let Some(edge) = call_edge_for_node(node, text, symbols) {
            edges.push(edge);
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tree_sitter_rust_call_edges_from_node(child, text, symbols, edges);
    }
}

#[cfg(feature = "tree-sitter")]
fn call_edge_for_node(
    node: tree_sitter::Node<'_>,
    text: &str,
    symbols: &[CodeSymbol],
) -> Option<SymbolEdge> {
    let caller = caller_for_byte(symbols, node.start_byte())?;
    let callee = resolve_callee_symbol(symbols, caller, &call_expression_target(node, text)?)?;
    if caller.id == callee.id {
        return None;
    }
    Some(SymbolEdge {
        from: caller.id,
        to: callee.id,
        kind: SymbolEdgeKind::Calls,
    })
}

#[cfg(feature = "tree-sitter")]
fn caller_for_byte(symbols: &[CodeSymbol], byte: usize) -> Option<&CodeSymbol> {
    symbols
        .iter()
        .filter(|symbol| {
            matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
                && symbol.range.start <= byte
                && byte <= symbol.range.end
        })
        .min_by_key(|symbol| (symbol.range.end - symbol.range.start, symbol.id.0))
}

#[cfg(feature = "tree-sitter")]
fn resolve_callee_symbol<'a>(
    symbols: &'a [CodeSymbol],
    caller: &CodeSymbol,
    target: &CallTarget,
) -> Option<&'a CodeSymbol> {
    symbols
        .iter()
        .filter(|symbol| {
            symbol.name == target.name
                && matches!(
                    symbol.kind,
                    SymbolKind::Function | SymbolKind::Method | SymbolKind::Macro
                )
                && target
                    .scope
                    .as_deref()
                    .is_none_or(|scope| symbol_parent_name(symbols, symbol) == Some(scope))
        })
        .min_by_key(|symbol| {
            (
                callee_scope_distance(symbols, caller, symbol),
                symbol.range.start.abs_diff(caller.range.start),
                symbol.id.0,
            )
        })
}

#[cfg(feature = "tree-sitter")]
fn callee_scope_distance(
    symbols: &[CodeSymbol],
    caller: &CodeSymbol,
    callee: &CodeSymbol,
) -> usize {
    if caller.parent == callee.parent {
        return 0;
    }
    if caller.parent.is_some_and(|parent| parent == callee.id) {
        return 1;
    }
    let caller_ancestors = symbol_ancestors(symbols, caller);
    if callee
        .parent
        .is_some_and(|parent| caller_ancestors.contains(&parent))
    {
        return 2;
    }
    if callee.parent.is_none() {
        return 3;
    }
    4
}

#[cfg(feature = "tree-sitter")]
fn symbol_ancestors(symbols: &[CodeSymbol], symbol: &CodeSymbol) -> Vec<SymbolId> {
    let mut ancestors = Vec::new();
    let mut current = symbol.parent;
    while let Some(parent) = current {
        ancestors.push(parent);
        current = symbols
            .iter()
            .find(|candidate| candidate.id == parent)
            .and_then(|candidate| candidate.parent);
    }
    ancestors
}

#[cfg(feature = "tree-sitter")]
struct CallTarget {
    name: String,
    scope: Option<String>,
}

#[cfg(feature = "tree-sitter")]
fn call_expression_target(node: tree_sitter::Node<'_>, text: &str) -> Option<CallTarget> {
    let function = node.child_by_field_name("function")?;
    final_call_target(function, text)
}

#[cfg(feature = "tree-sitter")]
fn final_call_target(node: tree_sitter::Node<'_>, text: &str) -> Option<CallTarget> {
    let name = match node.kind() {
        "identifier" | "field_identifier" => node_text(text, node).to_string(),
        "scoped_identifier" | "field_expression" => node
            .child_by_field_name("name")
            .map(|name| node_text(text, name).to_string())
            .or_else(|| last_identifier_child(node, text))?,
        _ => last_identifier_child(node, text)?,
    }
    .trim()
    .to_string();
    if name.is_empty() {
        return None;
    }
    Some(CallTarget {
        name,
        scope: call_scope_name(node, text),
    })
}

#[cfg(feature = "tree-sitter")]
fn last_identifier_child(node: tree_sitter::Node<'_>, text: &str) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter_map(|child| final_call_target(child, text).map(|target| target.name))
        .last()
}

#[cfg(feature = "tree-sitter")]
fn call_scope_name(node: tree_sitter::Node<'_>, text: &str) -> Option<String> {
    if node.kind() != "scoped_identifier" {
        return None;
    }
    node.child_by_field_name("path")
        .or_else(|| node.child_by_field_name("scope"))
        .map(|scope| node_text(text, scope).trim().to_string())
        .filter(|scope| !scope.is_empty())
}

#[cfg(feature = "tree-sitter")]
fn symbol_parent_name<'a>(symbols: &'a [CodeSymbol], symbol: &CodeSymbol) -> Option<&'a str> {
    let parent = symbol.parent?;
    symbols
        .iter()
        .find(|candidate| candidate.id == parent)
        .map(|candidate| candidate.name.as_str())
}

fn push_symbol(
    symbols: &mut Vec<CodeSymbol>,
    language: EditorLanguage,
    kind: SymbolKind,
    name: String,
    start: usize,
    line: &str,
) {
    if name.is_empty() {
        return;
    }
    let id = SymbolId(symbols.len() as u64 + 1);
    let range = start..start + line.trim_end_matches('\n').len();
    let name_start = range.start
        + line[range.start.saturating_sub(start)..]
            .find(&name)
            .unwrap_or(0);
    symbols.push(CodeSymbol {
        id,
        name,
        kind,
        language,
        range,
        selection_range: name_start..name_start + symbols.last().map_or(0, |_| 0),
        parent: None,
        signature: None,
        docs: None,
        source: SymbolSource::Lexical,
    });
    let last = symbols.last_mut().expect("symbol just pushed");
    last.selection_range = name_start..name_start + last.name.len();
}

fn take_ident(rest: &str) -> String {
    rest.chars()
        .take_while(|ch| *ch == '_' || ch.is_ascii_alphanumeric())
        .collect()
}

fn lines_with_offsets(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0;
    text.split_inclusive('\n').map(move |line| {
        let current = offset;
        offset += line.len();
        (current, line)
    })
}
