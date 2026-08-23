//! Local lexical, Tree-sitter, and optional universal-ctags symbol indexing.

use std::ops::Range;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::code::{Document, DocumentId, DocumentVersion, EditorLanguage};

/// Stable symbol ID inside one indexed summary.
///
/// Zero is reserved by built-in indexers as the synthetic document root; real
/// symbols receive one-based IDs. The type itself accepts every `u64`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::SymbolId;
/// assert_eq!(SymbolId(1).0, 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SymbolId(pub u64);

/// Language-neutral symbol record for one code document.
///
/// Ranges are half-open UTF-8 byte offsets. Built-in indexers keep them within
/// the document, but this public value type performs no validation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{CodeSymbol, EditorLanguage, SymbolId, SymbolKind, SymbolSource};
/// let symbol = CodeSymbol { id: SymbolId(1), name: "run".into(), kind: SymbolKind::Function, language: EditorLanguage::Rust, range: 0..10, selection_range: 3..6, parent: None, signature: Some("fn run()".into()), docs: None, source: SymbolSource::Lexical };
/// assert_eq!(symbol.selection_range, 3..6);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodeSymbol {
    /// Summary-local identifier; zero conventionally denotes the document root.
    pub id: SymbolId,
    /// Human-readable symbol name.
    pub name: String,
    /// Language-neutral category.
    pub kind: SymbolKind,
    /// Language inherited from the indexed document.
    pub language: EditorLanguage,
    /// Half-open full declaration extent in UTF-8 bytes.
    pub range: Range<usize>,
    /// Half-open name/selection extent in UTF-8 bytes.
    pub selection_range: Range<usize>,
    /// Optional containing symbol ID; `None` means document root.
    pub parent: Option<SymbolId>,
    /// Optional compact declaration signature.
    pub signature: Option<String>,
    /// Optional extracted documentation text.
    pub docs: Option<String>,
    /// Indexer/enrichment provenance.
    pub source: SymbolSource,
}

/// Language-neutral symbol categories serialized by exact variant name.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::SymbolKind;
/// assert_ne!(SymbolKind::Method, SymbolKind::Function);
/// assert_eq!(serde_json::to_string(&SymbolKind::TypeAlias).unwrap(), "\"TypeAlias\"");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SymbolKind {
    /// Language module.
    Module,
    /// Namespace or ctags module-like scope.
    Namespace,
    /// Generic type/class not represented more specifically.
    Type,
    /// Implementation block.
    Impl,
    /// Structure declaration.
    Struct,
    /// Enumeration declaration.
    Enum,
    /// Enumeration variant.
    EnumVariant,
    /// Trait declaration.
    Trait,
    /// Interface declaration.
    Interface,
    /// Free function.
    Function,
    /// Function contained by an impl or trait.
    Method,
    /// Constructor.
    Constructor,
    /// Structure/class field.
    Field,
    /// Variable.
    Variable,
    /// Constant or static item.
    Constant,
    /// Type alias.
    TypeAlias,
    /// Macro definition.
    Macro,
    /// Import/use declaration.
    Import,
    /// Unrecognized producer category.
    Unknown,
}

/// Provenance of a symbol record.
///
/// Merge preference is Tree-sitter, LSP, SCIP, ctags, then lexical.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::SymbolSource;
/// assert_ne!(SymbolSource::TreeSitter, SymbolSource::Lexical);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SymbolSource {
    /// Parsed from a Tree-sitter syntax tree.
    TreeSitter,
    /// Detected by the lightweight line-oriented fallback.
    Lexical,
    /// Imported from universal ctags JSON Lines.
    Ctags,
    /// Enriched by a language server.
    Lsp,
    /// Imported from SCIP JSON.
    Scip,
}

/// Complete symbol graph for one document version.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{CodeFileSummary, DocumentId, DocumentVersion, EditorLanguage};
/// let summary = CodeFileSummary { document_id: DocumentId(1), path: None, language: EditorLanguage::Rust, version: DocumentVersion(0), symbols: vec![], edges: vec![] };
/// assert!(summary.symbols.is_empty() && summary.edges.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodeFileSummary {
    /// Document identity copied from the indexed document or supplied by caller.
    pub document_id: DocumentId,
    /// Optional document path.
    pub path: Option<PathBuf>,
    /// Document language.
    pub language: EditorLanguage,
    /// Document version represented by the graph.
    pub version: DocumentVersion,
    /// Symbols in stable indexer-defined order.
    pub symbols: Vec<CodeSymbol>,
    /// Directed, normally normalized graph edges.
    pub edges: Vec<SymbolEdge>,
}

/// Serializes symbol summaries for storage and inspection.
impl CodeFileSummary {
    /// Returns pretty-printed JSON containing every summary field.
    ///
    /// # Errors
    ///
    /// Propagates a Serde JSON serialization error. Current fields are ordinary
    /// serializable values, so errors are not expected in normal use.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeFileSummary, DocumentId, DocumentVersion, EditorLanguage};
    /// let summary = CodeFileSummary { document_id: DocumentId(1), path: None, language: EditorLanguage::Rust, version: DocumentVersion(0), symbols: vec![], edges: vec![] };
    /// let json = summary.to_json_pretty().unwrap();
    /// assert!(json.contains("\"symbols\": []"));
    /// ```
    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

/// Directed relation between two summary-local symbol IDs.
///
/// ID zero conventionally denotes the document root for containment/import
/// edges, but the value type does not enforce endpoint existence.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{SymbolEdge, SymbolEdgeKind, SymbolId};
/// let edge = SymbolEdge { from: SymbolId(0), to: SymbolId(1), kind: SymbolEdgeKind::Contains };
/// assert_eq!(edge.to, SymbolId(1));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SymbolEdge {
    /// Source symbol ID or synthetic root zero.
    pub from: SymbolId,
    /// Target symbol ID.
    pub to: SymbolId,
    /// Directed relation category.
    pub kind: SymbolEdgeKind,
}

/// Language-neutral directed edge categories.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::SymbolEdgeKind;
/// assert_ne!(SymbolEdgeKind::Calls, SymbolEdgeKind::References);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SymbolEdgeKind {
    /// Parent/root contains a declaration.
    Contains,
    /// Caller invokes a function, method, or macro.
    Calls,
    /// Root/import scope imports a declaration.
    Imports,
    /// Source occurrence references a target.
    References,
    /// Source type extends a target.
    Extends,
    /// Source type implements a target.
    Implements,
}

/// Synchronous interface for indexing one document into a complete summary.
///
/// Implementations decide error handling; built-in ctags' trait implementation
/// is intentionally infallible and returns an empty summary on runner failures.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{Document, DocumentId, EditorLanguage, LexicalRustSymbolIndexer, SymbolIndexer};
/// use ailloli_ui_text::TextBuffer;
/// let document = Document::new(DocumentId(1), TextBuffer::from_string("fn run() {}")) .with_language(EditorLanguage::Rust);
/// assert_eq!(LexicalRustSymbolIndexer.index_document(&document).symbols[0].name, "run");
/// ```
pub trait SymbolIndexer {
    /// Indexes the document snapshot synchronously.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId, EditorLanguage, LexicalRustSymbolIndexer, SymbolIndexer};
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::from_string("struct App;")) .with_language(EditorLanguage::Rust);
    /// let summary = LexicalRustSymbolIndexer.index_document(&document);
    /// assert_eq!(summary.symbols.len(), 1);
    /// ```
    fn index_document(&mut self, document: &Document) -> CodeFileSummary;
}

/// Deterministic line-oriented Rust fallback indexer.
///
/// It performs no parsing or I/O and recognizes only trimmed lines beginning
/// exactly with `use `, `fn `, `struct `, `enum `, or `trait `.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{Document, DocumentId, EditorLanguage, LexicalRustSymbolIndexer, SymbolIndexer};
/// use ailloli_ui_text::TextBuffer;
/// let document = Document::new(DocumentId(1), TextBuffer::from_string("fn run() {}")) .with_language(EditorLanguage::Rust);
/// assert_eq!(LexicalRustSymbolIndexer.index_document(&document).symbols.len(), 1);
/// ```
#[derive(Debug, Default)]
pub struct LexicalRustSymbolIndexer;

/// Delegates to the lexical Rust summarizer.
impl SymbolIndexer for LexicalRustSymbolIndexer {
    /// Returns [`summarize_rust_lexical`] without failure.
    fn index_document(&mut self, document: &Document) -> CodeFileSummary {
        summarize_rust_lexical(document)
    }
}

/// Symbol indexer backed by fixture JSON Lines or a ctags subprocess.
///
/// The [`SymbolIndexer`] implementation suppresses every [`CtagsError`] into an
/// empty metadata-preserving summary. Use [`Self::try_index_document`] when the
/// caller needs diagnostics.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{CtagsSymbolIndexer, Document, DocumentId, EditorLanguage, SymbolIndexer};
/// use ailloli_ui_text::TextBuffer;
/// let document = Document::new(DocumentId(1), TextBuffer::from_string("fn run() {}")) .with_language(EditorLanguage::Rust);
/// let mut indexer = CtagsSymbolIndexer::from_json_lines(r#"{"name":"run","kind":"function","line":1}"#);
/// assert_eq!(indexer.index_document(&document).symbols[0].name, "run");
/// ```
#[derive(Debug, Clone)]
pub struct CtagsSymbolIndexer {
    /// In-memory JSON Lines or external-runner source.
    source: CtagsSymbolSource,
}

/// Input source used by a ctags indexer.
#[derive(Debug, Clone)]
enum CtagsSymbolSource {
    /// Exact JSON Lines fixture/content parsed without I/O.
    JsonLines(String),
    /// External subprocess configuration.
    Runner(CtagsRunnerConfig),
}

/// Selects the default external ctags runner configuration.
impl Default for CtagsSymbolIndexer {
    /// Creates a runner-backed indexer using [`CtagsRunnerConfig::default`].
    fn default() -> Self {
        Self {
            source: CtagsSymbolSource::Runner(CtagsRunnerConfig::default()),
        }
    }
}

/// Constructs and executes ctags indexing sources.
impl CtagsSymbolIndexer {
    /// Creates an indexer that parses supplied JSON Lines without subprocess I/O.
    ///
    /// Invalid JSON lines and entries with absent/empty names are silently
    /// skipped at indexing time.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CtagsSymbolIndexer, Document, DocumentId, SymbolIndexer};
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::from_string("fn f() {}"));
    /// let mut indexer = CtagsSymbolIndexer::from_json_lines("not json\n{\"name\":\"f\",\"kind\":\"function\",\"line\":1}");
    /// assert_eq!(indexer.index_document(&document).symbols.len(), 1);
    /// ```
    pub fn from_json_lines(json_lines: impl Into<String>) -> Self {
        Self {
            source: CtagsSymbolSource::JsonLines(json_lines.into()),
        }
    }

    /// Creates an indexer that spawns the configured ctags binary per document.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CtagsRunnerConfig, CtagsSymbolIndexer};
    /// let indexer = CtagsSymbolIndexer::from_runner_config(CtagsRunnerConfig::default());
    /// assert!(format!("{indexer:?}").contains("Runner"));
    /// ```
    pub fn from_runner_config(config: CtagsRunnerConfig) -> Self {
        Self {
            source: CtagsSymbolSource::Runner(config),
        }
    }

    /// Indexes a document while preserving external runner failures.
    ///
    /// Fixture mode parses in memory and currently cannot return an error.
    /// Runner mode writes a predictable temp path based on document ID/version,
    /// spawns ctags with JSON output, polls every 5 ms until the configured
    /// timeout, then captures stdout/stderr. The stdout limit is checked only
    /// after capture; stderr is unbounded. Concurrent identical documents can
    /// collide, and write/spawn failure may leave the temp file behind.
    ///
    /// # Errors
    ///
    /// Returns typed spawn, timeout, status, output-size, I/O, or UTF-8 errors.
    /// Invalid individual JSON lines are still skipped rather than returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CtagsSymbolIndexer, Document, DocumentId};
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::from_string("fn f() {}"));
    /// let mut indexer = CtagsSymbolIndexer::from_json_lines(r#"{"name":"f","kind":"function","line":1}"#);
    /// assert_eq!(indexer.try_index_document(&document).unwrap().symbols.len(), 1);
    /// ```
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

/// Produces an empty metadata-preserving summary on any ctags runner error.
impl SymbolIndexer for CtagsSymbolIndexer {
    /// Indexes infallibly, suppressing [`CtagsError`] into zero symbols/edges.
    fn index_document(&mut self, document: &Document) -> CodeFileSummary {
        self.try_index_document(document)
            .unwrap_or_else(|_| empty_summary(document))
    }
}

/// Indexes with Tree-sitter-first Rust fallback when available.
///
/// With `tree_sitter`, Rust first uses its parser and returns immediately if at
/// least one symbol was found; parser failure or an empty result falls back to
/// ctags. Other languages, and all builds without that feature, use ctags
/// directly. Ctags errors are suppressed by its trait implementation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{index_symbols_with_fallback, CtagsSymbolIndexer, Document, DocumentId, EditorLanguage, SymbolSource};
/// use ailloli_ui_text::TextBuffer;
/// let document = Document::new(DocumentId(1), TextBuffer::from_string("function run() {}")) .with_language(EditorLanguage::JavaScript);
/// let mut ctags = CtagsSymbolIndexer::from_json_lines(r#"{"name":"run","kind":"function","line":1}"#);
/// assert_eq!(index_symbols_with_fallback(&document, &mut ctags).symbols[0].source, SymbolSource::Ctags);
/// ```
pub fn index_symbols_with_fallback(
    document: &Document,
    ctags: &mut CtagsSymbolIndexer,
) -> CodeFileSummary {
    #[cfg(feature = "tree_sitter")]
    if document.language == EditorLanguage::Rust {
        let mut tree_sitter = TreeSitterRustSymbolIndexer;
        let summary = tree_sitter.index_document(document);
        if !summary.symbols.is_empty() {
            return summary;
        }
    }
    ctags.index_document(document)
}

/// Executes one external ctags process and returns UTF-8 JSON Lines.
///
/// # Errors
///
/// Returns [`CtagsError::MissingBinary`] when the command is absent,
/// [`CtagsError::Timeout`] when it exceeds the configured deadline,
/// [`CtagsError::OutputTooLarge`] when stdout exceeds its byte budget,
/// [`CtagsError::NonZeroStatus`] for an unsuccessful exit,
/// [`CtagsError::Utf8`] for non-UTF-8 stdout, or [`CtagsError::Io`] for temp-file
/// and child-process I/O failures.
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

/// Builds the predictable process temp path for a document snapshot.
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

/// External universal-ctags process limits and executable.
///
/// The default command is `ctags`, timeout is two seconds, and accepted stdout
/// is at most 512 KiB. The limit does not cover stderr and is enforced after the
/// child exits rather than while streaming.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use ailloli_ui_editor::CtagsRunnerConfig;
/// let config = CtagsRunnerConfig::default();
/// assert_eq!(config.binary.to_str(), Some("ctags"));
/// assert_eq!(config.timeout, Duration::from_secs(2));
/// assert_eq!(config.max_stdout_bytes, 512 * 1024);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CtagsRunnerConfig {
    /// Executable path or command name passed directly to [`Command::new`].
    pub binary: PathBuf,
    /// Maximum wall time polled in five-millisecond intervals.
    pub timeout: Duration,
    /// Maximum captured stdout bytes accepted after process completion.
    pub max_stdout_bytes: usize,
}

/// Supplies the standard `ctags` command and conservative process limits.
impl Default for CtagsRunnerConfig {
    /// Returns `ctags`, two seconds, and 512 KiB stdout.
    fn default() -> Self {
        Self {
            binary: PathBuf::from("ctags"),
            timeout: Duration::from_secs(2),
            max_stdout_bytes: 512 * 1024,
        }
    }
}

/// Typed failure from the external ctags runner.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use ailloli_ui_editor::CtagsError;
/// assert_eq!(CtagsError::Timeout { timeout: Duration::from_millis(10) }, CtagsError::Timeout { timeout: Duration::from_millis(10) });
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CtagsError {
    /// Executable lookup failed; contains the configured binary text.
    MissingBinary(String),
    /// Child exceeded the configured wall-time limit.
    Timeout {
        /// Configured duration reached before observing process exit.
        timeout: Duration,
    },
    /// Captured stdout exceeded the configured byte limit.
    OutputTooLarge {
        /// Actual captured stdout byte length.
        len: usize,
        /// Configured accepted stdout byte limit.
        max: usize,
    },
    /// Child exited unsuccessfully; signal exits have `code = None`.
    NonZeroStatus {
        /// Platform exit code, or `None` when unavailable (for example a signal).
        code: Option<i32>,
        /// Lossy UTF-8 decoding of complete captured stderr.
        stderr: String,
    },
    /// Temp-file, spawn, wait, or process-control I/O failure.
    Io(String),
    /// Successful stdout was not valid UTF-8.
    Utf8(String),
}

#[cfg(feature = "tree_sitter")]
/// Rust parser-backed symbol and call-graph indexer.
///
/// It extracts modules, imports, structs/fields, enums/variants, traits, impls,
/// functions/methods, constants/statics, type aliases, macros, documentation,
/// signatures, containment, and resolved call edges. Parser setup/parse failure
/// falls back to the lexical Rust summary.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::TreeSitterRustSymbolIndexer, Document, DocumentId, EditorLanguage, SymbolIndexer, SymbolKind};
/// use ailloli_ui_text::TextBuffer;
/// let document = Document::new(DocumentId(1), TextBuffer::from_string("struct App { value: usize }")) .with_language(EditorLanguage::Rust);
/// let summary = TreeSitterRustSymbolIndexer.index_document(&document);
/// assert!(summary.symbols.iter().any(|symbol| symbol.kind == SymbolKind::Struct && symbol.name == "App"));
/// assert!(summary.symbols.iter().any(|symbol| symbol.kind == SymbolKind::Field && symbol.name == "value"));
/// ```
#[derive(Debug, Default)]
pub struct TreeSitterRustSymbolIndexer;

#[cfg(feature = "tree_sitter")]
/// Parses Rust and returns a deterministic local symbol graph.
impl SymbolIndexer for TreeSitterRustSymbolIndexer {
    /// Uses Tree-sitter, falling back lexically only on parser setup/parse failure.
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

/// Builds a deterministic, line-oriented Rust symbol summary.
///
/// Only trimmed lines beginning exactly with `use `, `fn `, `struct `, `enum `,
/// or `trait ` are recognized, so visibility/modifier prefixes are ignored.
/// Symbols have no parents/signatures/docs and use one-based IDs. One root
/// containment/import edge is emitted per symbol. Calls are textually detected
/// as `name(` outside the symbol's own name range, at most once per function,
/// with root ID zero as caller; comments/strings can cause false positives.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::symbols::summarize_rust_lexical, Document, DocumentId, EditorLanguage, SymbolEdgeKind, SymbolKind};
/// use ailloli_ui_text::TextBuffer;
/// let document = Document::new(DocumentId(1), TextBuffer::from_string("struct App;\nfn run() {}\nfn main() { run(); }")) .with_language(EditorLanguage::Rust);
/// let summary = summarize_rust_lexical(&document);
/// assert!(summary.symbols.iter().any(|s| s.kind == SymbolKind::Struct && s.name == "App"));
/// assert!(summary.edges.iter().any(|e| e.kind == SymbolEdgeKind::Calls));
/// ```
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

#[cfg(feature = "tree_sitter")]
/// Traverses a Rust syntax tree and returns symbols in preorder.
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

#[cfg(feature = "tree_sitter")]
/// Stateful preorder collector that assigns positional symbol IDs.
struct TreeSitterSymbolCollector<'a> {
    /// Exact parsed source text.
    text: &'a str,
    /// Language copied into every produced symbol.
    language: EditorLanguage,
    /// Symbols accumulated in preorder.
    symbols: Vec<CodeSymbol>,
}

#[cfg(feature = "tree_sitter")]
/// Recursively converts supported Rust nodes into symbols.
impl<'a> TreeSitterSymbolCollector<'a> {
    /// Visits one node and descendants under the nearest container parent.
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

    /// Converts one supported syntax node into the next positional symbol.
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

    /// Looks up a previously collected symbol category by ID.
    fn kind_for_id(&self, id: SymbolId) -> Option<SymbolKind> {
        self.symbols
            .iter()
            .find(|symbol| symbol.id == id)
            .map(|symbol| symbol.kind)
    }
}

#[cfg(feature = "tree_sitter")]
/// Classification and name extraction recipe for one Rust syntax node.
struct RustSymbolSpec<'tree> {
    /// Resulting local symbol category.
    kind: SymbolKind,
    /// Node whose bytes define the selection range.
    name_node: Option<tree_sitter::Node<'tree>>,
    /// Optional derived name replacing raw name-node text.
    name_override: Option<String>,
}

#[cfg(feature = "tree_sitter")]
/// Classifies supported Rust declaration nodes and selects their names.
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

#[cfg(feature = "tree_sitter")]
/// Reports whether a symbol can become the parent of descendant declarations.
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

#[cfg(feature = "tree_sitter")]
/// Finds the implemented type node in an impl declaration.
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

#[cfg(feature = "tree_sitter")]
/// Returns valid source text for a syntax node, or empty on an invalid range.
fn node_text<'a>(text: &'a str, node: tree_sitter::Node<'_>) -> &'a str {
    text.get(node.start_byte()..node.end_byte())
        .unwrap_or_default()
}

#[cfg(feature = "tree_sitter")]
/// Collapses all whitespace runs to single ASCII spaces.
fn compact_node_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(feature = "tree_sitter")]
/// Derives the final imported component from a Rust use declaration.
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

#[cfg(feature = "tree_sitter")]
/// Clamps range endpoints down to valid UTF-8 boundaries inside source length.
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

#[cfg(feature = "tree_sitter")]
/// Extracts a compact declaration header or field/import signature.
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

#[cfg(feature = "tree_sitter")]
/// Collects contiguous preceding Rust doc comments and `#[doc = "..."]` lines.
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

/// Parses universal-ctags JSON Lines into local symbol records.
///
/// Invalid JSON lines and entries with missing/empty names are skipped. IDs are
/// assigned after filtering, ranges derive from one-based `line`/`end` fields,
/// kinds use a fixed mapping, signature falls back through pattern then typeref,
/// and parents resolve only to earlier scope-matching entries. No errors are
/// returned and containment edges are not included in this symbol-only result.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{parse_ctags_json_lines, Document, DocumentId, EditorLanguage, SymbolKind, SymbolSource};
/// use ailloli_ui_text::TextBuffer;
/// let document = Document::new(DocumentId(1), TextBuffer::from_string("struct App;\n")) .with_language(EditorLanguage::Rust);
/// let symbols = parse_ctags_json_lines(&document, "not json\n{\"name\":\"App\",\"kind\":\"struct\",\"line\":1}");
/// assert_eq!(symbols.len(), 1);
/// assert_eq!((symbols[0].kind, symbols[0].source), (SymbolKind::Struct, SymbolSource::Ctags));
/// assert_eq!(symbols[0].selection_range, 7..10);
/// ```
pub fn parse_ctags_json_lines(document: &Document, json_lines: &str) -> Vec<CodeSymbol> {
    parse_ctags_entries(document, json_lines)
        .into_iter()
        .map(|entry| entry.symbol)
        .collect()
}

/// Parses ctags symbols and adds normalized containment/import edges.
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

/// Parses, filters, converts, and parent-links ctags entries.
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

/// Converted ctags symbol plus unresolved scope metadata.
#[derive(Debug, Clone)]
struct CtagsParsedSymbol {
    /// Local symbol record.
    symbol: CodeSymbol,
    /// Optional producer scope string.
    scope: Option<String>,
    /// Optional producer scope-kind string.
    scope_kind: Option<String>,
}

#[allow(dead_code)]
/// Legacy symbol-only ctags parser retained for regression comparison.
fn parse_ctags_json_lines_legacy(document: &Document, json_lines: &str) -> Vec<CodeSymbol> {
    json_lines
        .lines()
        .filter_map(|line| serde_json::from_str::<CtagsEntry>(line).ok())
        .filter(|entry| entry.name.as_deref().is_some_and(|name| !name.is_empty()))
        .enumerate()
        .map(|(idx, entry)| ctags_entry_to_symbol(document, idx, entry).symbol)
        .collect()
}

/// Builds a zero-symbol, zero-edge summary preserving document metadata.
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

/// Deserializable subset of universal-ctags JSON output.
#[derive(Debug, serde::Deserialize)]
struct CtagsEntry {
    /// Symbol name; absent/empty entries are discarded.
    name: Option<String>,
    /// Long or short ctags kind name.
    kind: Option<String>,
    /// One-based starting line.
    line: Option<usize>,
    /// One-based inclusive ending line.
    end: Option<usize>,
    /// Source-pattern fallback for signature text.
    pattern: Option<String>,
    /// Preferred signature text.
    signature: Option<String>,
    /// Type-reference fallback for signature text.
    typeref: Option<String>,
    /// Optional containing scope string.
    scope: Option<String>,
    /// Optional containing scope category.
    #[serde(rename = "scopeKind")]
    scope_kind: Option<String>,
    /// Producer language, currently parsed then ignored.
    language: Option<String>,
    /// Producer roles, currently parsed then ignored.
    roles: Option<String>,
}

/// Converts one filtered ctags entry to a positional local symbol.
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

/// Resolves each ctags scope against the nearest earlier matching entry.
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

/// Maps supported long/short ctags kinds to local categories.
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

/// Converts optional one-based ctags line bounds to a document byte range.
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

/// Narrows a full ctags range to its first matching name occurrence.
fn ctags_selection_range(text: &str, name: &str, range: Range<usize>) -> Range<usize> {
    if let Some(name_start) = text[range.clone()].find(name) {
        let absolute = range.start + name_start;
        absolute..absolute + name.len()
    } else {
        range
    }
}

#[allow(dead_code)]
/// Legacy combined full/selection range helper retained for regression use.
fn ctags_range(text: &str, name: &str, line: Option<usize>, end: Option<usize>) -> Range<usize> {
    ctags_selection_range(text, name, ctags_full_range(text, line, end))
}

/// Resolves a zero-based logical line to its starting byte, if present.
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

/// Returns the next newline byte or EOF after a valid starting byte.
fn line_end_offset(text: &str, start: usize) -> usize {
    text[start..]
        .find('\n')
        .map(|offset| start + offset)
        .unwrap_or(text.len())
}

/// Emits one root/parent containment or import edge per symbol.
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

/// Sorts edges by category/from/to and removes exact duplicates.
fn normalize_edges(edges: &mut Vec<SymbolEdge>) {
    edges.sort_by_key(|edge| (symbol_edge_rank(edge.kind), edge.from.0, edge.to.0));
    edges.dedup_by_key(|edge| (edge.from, edge.to, edge.kind));
}

/// Returns deterministic edge-category ordering rank.
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

/// Finds coarse root-origin call edges with textual `name(` searches.
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

#[cfg(feature = "tree_sitter")]
/// Collects and normalizes resolved Rust call-expression edges.
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

#[cfg(feature = "tree_sitter")]
/// Recursively visits call expressions and appends resolvable edges.
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

#[cfg(feature = "tree_sitter")]
/// Resolves one non-recursive call expression to a caller/callee edge.
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

#[cfg(feature = "tree_sitter")]
/// Finds the smallest containing function/method, breaking ties by ID.
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

#[cfg(feature = "tree_sitter")]
/// Chooses the nearest matching function/method/macro for a call target.
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

#[cfg(feature = "tree_sitter")]
/// Ranks callee scope relative to a caller from same parent through unrelated.
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

#[cfg(feature = "tree_sitter")]
/// Walks parent IDs upward until root or an unresolved parent.
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

#[cfg(feature = "tree_sitter")]
/// Final callable name plus optional explicit scope qualifier.
struct CallTarget {
    /// Unqualified callable name.
    name: String,
    /// Explicit scope from a scoped identifier, if any.
    scope: Option<String>,
}

#[cfg(feature = "tree_sitter")]
/// Extracts a call target from a Tree-sitter call-expression node.
fn call_expression_target(node: tree_sitter::Node<'_>, text: &str) -> Option<CallTarget> {
    let function = node.child_by_field_name("function")?;
    final_call_target(function, text)
}

#[cfg(feature = "tree_sitter")]
/// Recursively extracts the final callable identifier and optional scope.
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

#[cfg(feature = "tree_sitter")]
/// Returns the last recursively discoverable child identifier.
fn last_identifier_child(node: tree_sitter::Node<'_>, text: &str) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter_map(|child| final_call_target(child, text).map(|target| target.name))
        .last()
}

#[cfg(feature = "tree_sitter")]
/// Extracts a non-empty qualifier from a scoped identifier.
fn call_scope_name(node: tree_sitter::Node<'_>, text: &str) -> Option<String> {
    if node.kind() != "scoped_identifier" {
        return None;
    }
    node.child_by_field_name("path")
        .or_else(|| node.child_by_field_name("scope"))
        .map(|scope| node_text(text, scope).trim().to_string())
        .filter(|scope| !scope.is_empty())
}

#[cfg(feature = "tree_sitter")]
/// Resolves a symbol's parent ID to its borrowed name.
fn symbol_parent_name<'a>(symbols: &'a [CodeSymbol], symbol: &CodeSymbol) -> Option<&'a str> {
    let parent = symbol.parent?;
    symbols
        .iter()
        .find(|candidate| candidate.id == parent)
        .map(|candidate| candidate.name.as_str())
}

/// Appends a non-empty lexical symbol and repairs its selection range.
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

/// Takes the leading ASCII identifier-like substring.
fn take_ident(rest: &str) -> String {
    rest.chars()
        .take_while(|ch| *ch == '_' || ch.is_ascii_alphanumeric())
        .collect()
}

/// Iterates newline-inclusive logical lines with starting UTF-8 byte offsets.
fn lines_with_offsets(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0;
    text.split_inclusive('\n').map(move |line| {
        let current = offset;
        offset += line.len();
        (current, line)
    })
}
