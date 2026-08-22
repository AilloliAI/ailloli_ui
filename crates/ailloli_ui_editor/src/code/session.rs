//! Document-aware editor session orchestration.

use ailloli_ui_text::TextBuffer;

#[cfg(feature = "tree_sitter")]
use crate::code::highlight_rust_tree_sitter_hybrid;
use crate::code::{
    collapsed_region_hiding_line, diagnostic_at_byte, fold_regions_for_document,
    highlight_rust_lexical, line_for_byte, line_start_byte, lsp_diagnostics_to_diagnostics,
    merge_fold_regions_with_previous, CodeEditorConfig, Diagnostic, DiagnosticHit,
    DiagnosticSource, Document, DocumentId, DocumentVersion, EditorLanguage, FoldRegion,
    LspDiagnostic, LspEnrichment, SearchMatch, SearchQuery, SearchState, SyntaxToken,
};
use crate::EditorSession;

/// Code-editor session: document metadata plus generic editor state and caches.
///
/// The public fields support adapter composition, but direct mutation can break
/// the version stamps that guard syntax, fold, and search caches. Prefer the
/// provided synchronization and setter methods when cache coherence matters.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId};
/// use ailloli_ui_text::TextBuffer;
/// let session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("fn main() {}")), CodeEditorConfig::default());
/// assert_eq!(session.editor.buffer.as_str(), session.document.buffer.as_str());
/// assert!(session.diagnostics.is_empty() && session.fold_regions.is_empty());
/// ```
#[derive(Debug, Clone)]
pub struct CodeEditorSession {
    /// Canonical document metadata and content.
    pub document: Document,
    /// Generic editable buffer, caret, selection, scroll, and input state.
    pub editor: EditorSession,
    /// Generic and code-specific behavior/style configuration.
    pub config: CodeEditorConfig,
    /// Local and current-version LSP diagnostics in display order.
    pub diagnostics: Vec<Diagnostic>,
    /// Selected diagnostic index, or `None`.
    pub active_diagnostic_index: Option<usize>,
    /// Query, cached matches, and active search result.
    pub search: SearchState,
    /// Current syntax-token cache.
    pub syntax_tokens: Vec<SyntaxToken>,
    /// Document identity from which syntax tokens were derived.
    pub syntax_tokens_document_id: Option<DocumentId>,
    /// Document version from which syntax tokens were derived.
    pub syntax_tokens_version: Option<DocumentVersion>,
    /// Language used to derive syntax tokens.
    pub syntax_tokens_language: Option<EditorLanguage>,
    /// Current logical fold regions.
    pub fold_regions: Vec<FoldRegion>,
    /// Document identity from which folds were derived.
    pub fold_regions_document_id: Option<DocumentId>,
    /// Document version from which folds were derived.
    pub fold_regions_version: Option<DocumentVersion>,
    /// Language used to derive folds.
    pub fold_regions_language: Option<EditorLanguage>,
}

/// Synchronizes document/editor state and derived code caches.
impl CodeEditorSession {
    /// Creates a session with synchronized buffers and empty derived state.
    ///
    /// The document buffer is cheaply cloned into a generic [`EditorSession`]
    /// using `config.editor`. Document version, dirty state, and metadata are
    /// preserved; every cache provenance stamp starts as `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId};
    /// use ailloli_ui_text::TextBuffer;
    /// let session = CodeEditorSession::new(Document::new(DocumentId(7), TextBuffer::from_string("text")), CodeEditorConfig::default());
    /// assert_eq!(session.document.id, DocumentId(7));
    /// assert!(session.syntax_tokens_document_id.is_none());
    /// ```
    pub fn new(document: Document, config: CodeEditorConfig) -> Self {
        let editor = EditorSession::with_config(document.buffer.clone(), config.editor);
        Self {
            document,
            editor,
            config,
            diagnostics: Vec::new(),
            active_diagnostic_index: None,
            search: SearchState::default(),
            syntax_tokens: Vec::new(),
            syntax_tokens_document_id: None,
            syntax_tokens_version: None,
            syntax_tokens_language: None,
            fold_regions: Vec::new(),
            fold_regions_document_id: None,
            fold_regions_version: None,
            fold_regions_language: None,
        }
    }

    /// Borrows the generic editor session.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId};
    /// use ailloli_ui_text::TextBuffer;
    /// let session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("abc")), CodeEditorConfig::default());
    /// assert_eq!(session.editor_session().buffer.as_str(), "abc");
    /// ```
    pub fn editor_session(&self) -> &EditorSession {
        &self.editor
    }

    /// Mutably borrows the generic editor session.
    ///
    /// Buffer mutations made through this reference do not update the document
    /// until [`Self::sync_document_from_editor`] is called.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("abc")), CodeEditorConfig::default());
    /// session.editor_session_mut().edit.caret_byte = 2;
    /// assert_eq!(session.editor.edit.caret_byte, 2);
    /// ```
    pub fn editor_session_mut(&mut self) -> &mut EditorSession {
        &mut self.editor
    }

    /// Copies changed editor text into the document.
    ///
    /// Returns `false` when strings are identical. A change rebuilds the
    /// document buffer, sets `dirty = true`, and increments its `i32` version
    /// with saturation at [`i32::MAX`]. Other document metadata is unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, DocumentVersion};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("old")), CodeEditorConfig::default());
    /// session.editor.buffer = TextBuffer::from_string("new");
    /// assert!(session.sync_document_from_editor());
    /// assert_eq!(session.document.version, DocumentVersion(1));
    /// assert!(session.document.dirty);
    /// ```
    pub fn sync_document_from_editor(&mut self) -> bool {
        if self.document.buffer.as_str() == self.editor.buffer.as_str() {
            return false;
        }
        self.document.buffer = TextBuffer::from_string(self.editor.buffer.as_str());
        self.document.dirty = true;
        self.document.version = DocumentVersion(self.document.version.0.saturating_add(1));
        true
    }

    /// Replaces generic editor text from the document when its string differs.
    ///
    /// The generic session clamps its edit state and clears incompatible input
    /// state according to [`EditorSession::replace_buffer_if_changed`]. Document
    /// metadata and derived code caches are not changed here.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("old")), CodeEditorConfig::default());
    /// session.document.buffer = TextBuffer::from_string("new");
    /// assert!(session.sync_editor_from_document());
    /// assert_eq!(session.editor.buffer.as_str(), "new");
    /// ```
    pub fn sync_editor_from_document(&mut self) -> bool {
        self.editor
            .replace_buffer_if_changed(self.document.buffer.clone())
    }

    /// Replaces the complete document when any compared field differs.
    ///
    /// Text, ID, path, source, language, version, and dirty state participate;
    /// internal buffer revision metadata does not when text is equal. A new ID
    /// invalidates syntax/fold provenance and the search key. Text changes with
    /// the same ID invalidate syntax/fold versions and search. Cached values are
    /// retained until their next refresh.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("one")), CodeEditorConfig::default());
    /// let replacement = Document::new(DocumentId(2), TextBuffer::from_string("two"));
    /// assert!(session.replace_document_if_changed(replacement));
    /// assert_eq!(session.editor.buffer.as_str(), "two");
    /// ```
    pub fn replace_document_if_changed(&mut self, document: Document) -> bool {
        let document_changed = self.document.buffer.as_str() != document.buffer.as_str()
            || self.document.id != document.id
            || self.document.path != document.path
            || self.document.source != document.source
            || self.document.language != document.language
            || self.document.version != document.version
            || self.document.dirty != document.dirty;
        if !document_changed {
            return false;
        }

        let previous_id = self.document.id;
        let buffer_changed = self.document.buffer.as_str() != document.buffer.as_str();
        self.document = document;
        self.sync_editor_from_document();

        if previous_id != self.document.id {
            self.syntax_tokens_document_id = None;
            self.syntax_tokens_version = None;
            self.syntax_tokens_language = None;
            self.fold_regions_document_id = None;
            self.fold_regions_version = None;
            self.fold_regions_language = None;
            self.search.invalidate_cache();
        } else if buffer_changed {
            self.syntax_tokens_version = None;
            self.fold_regions_version = None;
            self.search.invalidate_cache();
        }

        true
    }

    /// Replaces diagnostics and retains only an in-range active index.
    ///
    /// Input order, ranges, duplicates, and sources are preserved verbatim.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Diagnostic, DiagnosticSeverity, Document, DocumentId};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::new()), CodeEditorConfig::default());
    /// session.set_diagnostics(vec![Diagnostic::new(0..0, DiagnosticSeverity::Hint, "hint")]);
    /// assert_eq!(session.diagnostics.len(), 1);
    /// ```
    pub fn set_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) {
        self.diagnostics = diagnostics;
        self.active_diagnostic_index = self
            .active_diagnostic_index
            .filter(|idx| *idx < self.diagnostics.len());
    }

    /// Replaces all LSP diagnostics with current-version mapped inputs.
    ///
    /// Local diagnostics remain in order. Stale LSP inputs are discarded,
    /// ranges clamp to document length, and empty ranges are removed. Returns
    /// whether the resulting complete diagnostic vector differs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, DiagnosticSeverity, Document, DocumentId, DocumentVersion, LspDiagnostic};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("abc")), CodeEditorConfig::default());
    /// let input = [LspDiagnostic { range: 1..9, severity: DiagnosticSeverity::Error, message: "bad".into(), document_version: DocumentVersion(0) }];
    /// assert!(session.apply_lsp_diagnostics(&input));
    /// assert_eq!(session.diagnostics[0].range, 1..3);
    /// ```
    pub fn apply_lsp_diagnostics(&mut self, diagnostics: &[LspDiagnostic]) -> bool {
        let mut next = self.diagnostics.clone();
        next.retain(|diagnostic| diagnostic.source != DiagnosticSource::Lsp);
        next.extend(lsp_diagnostics_to_diagnostics(&self.document, diagnostics));
        if next == self.diagnostics {
            return false;
        }
        self.set_diagnostics(next);
        true
    }

    /// Applies only the diagnostic portion of current-version LSP enrichment.
    ///
    /// A stale enrichment returns `false` without changes. Current enrichment
    /// removes prior LSP diagnostics, retains local diagnostics, and accepts only
    /// entries explicitly marked LSP with the current document version. Symbols,
    /// references, and capabilities are intentionally ignored by this session.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, DocumentVersion, LspCapabilities, LspEnrichment};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::new()), CodeEditorConfig::default());
    /// let stale = LspEnrichment { document_version: DocumentVersion(9), capabilities: LspCapabilities::default(), symbols: vec![], references: vec![], diagnostics: vec![] };
    /// assert!(!session.apply_lsp_enrichment(&stale));
    /// ```
    pub fn apply_lsp_enrichment(&mut self, enrichment: &LspEnrichment) -> bool {
        if enrichment.document_version != self.document.version {
            return false;
        }
        let mut next = self.diagnostics.clone();
        next.retain(|diagnostic| diagnostic.source != DiagnosticSource::Lsp);
        next.extend(
            enrichment
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.source == DiagnosticSource::Lsp
                        && diagnostic.document_version == Some(self.document.version)
                })
                .cloned(),
        );
        if next == self.diagnostics {
            return false;
        }
        self.set_diagnostics(next);
        true
    }

    /// Selects an in-range diagnostic index, otherwise stores `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Diagnostic, DiagnosticSeverity, Document, DocumentId};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::new()), CodeEditorConfig::default());
    /// session.set_diagnostics(vec![Diagnostic::new(0..0, DiagnosticSeverity::Info, "note")]);
    /// session.set_active_diagnostic_index(Some(1));
    /// assert_eq!(session.active_diagnostic_index, None);
    /// ```
    pub fn set_active_diagnostic_index(&mut self, active_diagnostic_index: Option<usize>) {
        self.active_diagnostic_index =
            active_diagnostic_index.filter(|idx| *idx < self.diagnostics.len());
    }

    /// Returns the first diagnostic whose inclusive range contains `byte`.
    ///
    /// The returned diagnostic is cloned and includes its slice index.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Diagnostic, DiagnosticSeverity, Document, DocumentId};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::new()), CodeEditorConfig::default());
    /// session.set_diagnostics(vec![Diagnostic::new(2..4, DiagnosticSeverity::Error, "bad")]);
    /// assert_eq!(session.diagnostic_at_byte(4).unwrap().index, 0);
    /// ```
    pub fn diagnostic_at_byte(&self, byte: usize) -> Option<DiagnosticHit> {
        diagnostic_at_byte(&self.diagnostics, byte)
    }

    /// Installs externally computed matches and clears active selection.
    ///
    /// The query and existing cache key are left unchanged. A later refresh may
    /// preserve these matches on a cache hit or replace them after invalidation.
    /// Ranges are not validated against the document.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, SearchMatch};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("abc")), CodeEditorConfig::default());
    /// session.set_search_matches(vec![SearchMatch { range: 1..2 }]);
    /// assert_eq!(session.search.matches[0].range, 1..2);
    /// assert_eq!(session.search.active_index, None);
    /// ```
    pub fn set_search_matches(&mut self, search_matches: Vec<SearchMatch>) {
        self.search.matches = search_matches;
        self.search.set_active_index(None);
    }

    /// Sets a query and immediately refreshes it against the current document.
    ///
    /// An equal query can reuse the current version/query cache; a changed query
    /// clears old results before recomputation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, SearchQuery};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("go go")), CodeEditorConfig::default());
    /// session.set_search_query(SearchQuery::new("go"));
    /// assert_eq!(session.search.matches.len(), 2);
    /// ```
    pub fn set_search_query(&mut self, query: SearchQuery) {
        self.search.set_query(query);
        self.refresh_search();
    }

    /// Selects an in-range search match, otherwise clears the selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, SearchQuery};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("x x")), CodeEditorConfig::default());
    /// session.set_search_query(SearchQuery::new("x"));
    /// session.set_search_active_index(Some(1));
    /// assert_eq!(session.search.active_index, Some(1));
    /// ```
    pub fn set_search_active_index(&mut self, active_index: Option<usize>) {
        self.search.set_active_index(active_index);
    }

    /// Refreshes search results for the current text, version, and query.
    ///
    /// Returns `true` when recomputed and `false` on a cache hit. Direct text
    /// mutation without a corresponding document-version change can therefore
    /// produce a stale cache hit.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, SearchQuery};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("abc")), CodeEditorConfig::default());
    /// session.search.set_query(SearchQuery::new("b"));
    /// assert!(session.refresh_search());
    /// assert!(!session.refresh_search());
    /// ```
    pub fn refresh_search(&mut self) -> bool {
        self.search
            .refresh(&self.document.buffer.as_str(), self.document.version)
    }

    /// Refreshes, advances to, and returns the next match with wraparound.
    ///
    /// A refresh selects index zero, so for multiple fresh matches this call
    /// returns index one. With no matches it returns `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, SearchQuery};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("x x")), CodeEditorConfig::default());
    /// session.search.set_query(SearchQuery::new("x"));
    /// assert_eq!(session.search_next().map(|m| m.range.clone()), Some(2..3));
    /// ```
    pub fn search_next(&mut self) -> Option<&SearchMatch> {
        self.refresh_search();
        self.search.next_match()
    }

    /// Refreshes, moves to, and returns the previous match with wraparound.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, SearchQuery};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("x x")), CodeEditorConfig::default());
    /// session.search.set_query(SearchQuery::new("x"));
    /// assert_eq!(session.search_previous().map(|m| m.range.clone()), Some(2..3));
    /// ```
    pub fn search_previous(&mut self) -> Option<&SearchMatch> {
        self.refresh_search();
        self.search.previous_match()
    }

    /// Restores empty query, results, active index, and search cache.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, SearchQuery};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("x")), CodeEditorConfig::default());
    /// session.set_search_query(SearchQuery::new("x"));
    /// session.clear_search();
    /// assert!(session.search.query.text.is_empty());
    /// ```
    pub fn clear_search(&mut self) {
        self.search.clear();
    }

    /// Refreshes syntax and selects a syntax token or lexical word at `byte`.
    ///
    /// Returns whether generic editor caret/selection state changed. Byte input
    /// is clamped by selection helpers and the editor buffer remains unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, EditorLanguage};
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::from_string("hello world")).with_language(EditorLanguage::PlainText);
    /// let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    /// assert!(session.select_word_at_byte(1));
    /// assert_eq!(session.editor.edit.selection.unwrap().normalized(), (0, 5));
    /// ```
    pub fn select_word_at_byte(&mut self, byte: usize) -> bool {
        self.refresh_syntax_tokens();
        self.editor
            .select_word_at_byte(byte, Some(&self.syntax_tokens), self.document.language)
    }

    /// Selects the logical line containing `byte`, excluding its newline.
    ///
    /// Returns whether generic editor state changed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("one\ntwo")), CodeEditorConfig::default());
    /// assert!(session.select_line_at_byte(5));
    /// assert_eq!(session.editor.edit.selection.unwrap().normalized(), (4, 7));
    /// ```
    pub fn select_line_at_byte(&mut self, byte: usize) -> bool {
        self.editor.select_line_at_byte(byte)
    }

    /// Installs manual syntax tokens and clears all provenance stamps.
    ///
    /// Tokens are stored without normalization. Because stamps become `None`,
    /// the next syntax refresh (including word selection) replaces this manual
    /// vector with language-derived tokens.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{code::{SyntaxKind, SyntaxToken}, CodeEditorConfig, CodeEditorSession, Document, DocumentId};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("x")), CodeEditorConfig::default());
    /// session.set_syntax_tokens(vec![SyntaxToken { range: 0..1, kind: SyntaxKind::Identifier }]);
    /// assert_eq!(session.syntax_tokens.len(), 1);
    /// assert!(session.syntax_tokens_version.is_none());
    /// ```
    pub fn set_syntax_tokens(&mut self, syntax_tokens: Vec<SyntaxToken>) {
        self.syntax_tokens = syntax_tokens;
        self.syntax_tokens_document_id = None;
        self.syntax_tokens_version = None;
        self.syntax_tokens_language = None;
    }

    /// Installs manual fold regions and clears all provenance stamps.
    ///
    /// Regions and collapsed states are stored without validation. The next fold
    /// refresh treats them as belonging to no document and replaces them rather
    /// than preserving their collapsed state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, FoldRegion};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::new()), CodeEditorConfig::default());
    /// session.set_fold_regions(vec![FoldRegion::new(0, 2)]);
    /// assert_eq!(session.fold_regions.len(), 1);
    /// assert!(session.fold_regions_document_id.is_none());
    /// ```
    pub fn set_fold_regions(&mut self, fold_regions: Vec<FoldRegion>) {
        self.fold_regions = fold_regions;
        self.fold_regions_document_id = None;
        self.fold_regions_version = None;
        self.fold_regions_language = None;
    }

    /// Refreshes language-derived fold regions when identity/version/language changes.
    ///
    /// On the same document ID, matching region IDs inherit prior collapsed
    /// state. A different ID discards it. Without `tree_sitter`, or for an
    /// unsupported language, the refreshed vector is empty. Provenance stamps
    /// are updated even when discovery returns no regions.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("plain")), CodeEditorConfig::default());
    /// session.refresh_fold_regions();
    /// assert_eq!(session.fold_regions_document_id, Some(DocumentId(1)));
    /// assert!(session.fold_regions.is_empty());
    /// ```
    pub fn refresh_fold_regions(&mut self) {
        let same_document = self.fold_regions_document_id == Some(self.document.id);
        if same_document
            && self.fold_regions_version == Some(self.document.version)
            && self.fold_regions_language == Some(self.document.language)
        {
            return;
        }
        let next =
            fold_regions_for_document(self.document.language, &self.document.buffer.as_str());
        self.fold_regions = if same_document {
            merge_fold_regions_with_previous(next, &self.fold_regions)
        } else {
            next
        };
        self.fold_regions_document_id = Some(self.document.id);
        self.fold_regions_version = Some(self.document.version);
        self.fold_regions_language = Some(self.document.language);
    }

    /// Toggles one fold region by slice index.
    ///
    /// Returns `false` for an out-of-range index. Collapsing also moves a caret
    /// hidden by any collapsed region to that region's header; expanding does
    /// not move it. A valid index returns `true` even if caret state is unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, FoldRegion};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("a\nb")), CodeEditorConfig::default());
    /// session.set_fold_regions(vec![FoldRegion::new(0, 1)]);
    /// assert!(session.toggle_fold_region(0));
    /// assert!(session.fold_regions[0].collapsed);
    /// assert!(!session.toggle_fold_region(9));
    /// ```
    pub fn toggle_fold_region(&mut self, index: usize) -> bool {
        let Some(region) = self.fold_regions.get_mut(index) else {
            return false;
        };
        region.collapsed = !region.collapsed;
        if region.collapsed {
            self.move_caret_out_of_folded_regions();
        }
        true
    }

    /// Moves a caret hidden by the first collapsed region to its header line.
    ///
    /// Returns `false` when the caret is visible or setting it causes no editor
    /// state change. Selection is cleared and desired horizontal position reset
    /// by the generic caret setter. The document and editor buffers are expected
    /// to be synchronized and the caret to be a valid UTF-8 boundary.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, FoldRegion};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = CodeEditorSession::new(Document::new(DocumentId(1), TextBuffer::from_string("a\nb\nc")), CodeEditorConfig::default());
    /// session.editor.edit.caret_byte = 4;
    /// session.fold_regions = vec![FoldRegion::new(0, 2).collapsed(true)];
    /// assert!(session.move_caret_out_of_folded_regions());
    /// assert_eq!(session.editor.edit.caret_byte, 0);
    /// ```
    pub fn move_caret_out_of_folded_regions(&mut self) -> bool {
        let line = line_for_byte(&self.document.buffer.as_str(), self.editor.edit.caret_byte);
        let Some(region) = collapsed_region_hiding_line(&self.fold_regions, line) else {
            return false;
        };
        let byte = line_start_byte(&self.document.buffer.as_str(), region.start_line);
        self.editor.edit.set_caret(&self.editor.buffer, byte, false)
    }

    /// Refreshes syntax tokens when document ID, version, or language changes.
    ///
    /// Rust uses hybrid Tree-sitter tokens when enabled and successful, then
    /// falls back to the lexical highlighter. Every other language yields an
    /// empty vector. Provenance stamps are updated even for empty results, so an
    /// unchanged subsequent call is a no-op.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{CodeEditorConfig, CodeEditorSession, Document, DocumentId, EditorLanguage};
    /// use ailloli_ui_text::TextBuffer;
    /// let document = Document::new(DocumentId(1), TextBuffer::from_string("fn main() {}"))
    ///     .with_language(EditorLanguage::Rust);
    /// let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    /// session.refresh_syntax_tokens();
    /// assert!(!session.syntax_tokens.is_empty());
    /// assert_eq!(session.syntax_tokens_language, Some(EditorLanguage::Rust));
    /// ```
    pub fn refresh_syntax_tokens(&mut self) {
        if self.syntax_tokens_document_id == Some(self.document.id)
            && self.syntax_tokens_version == Some(self.document.version)
            && self.syntax_tokens_language == Some(self.document.language)
        {
            return;
        }
        self.syntax_tokens = match self.document.language {
            EditorLanguage::Rust => {
                #[cfg(feature = "tree_sitter")]
                {
                    highlight_rust_tree_sitter_hybrid(&self.document.buffer.as_str())
                        .unwrap_or_else(|| highlight_rust_lexical(&self.document.buffer.as_str()))
                }
                #[cfg(not(feature = "tree_sitter"))]
                {
                    highlight_rust_lexical(&self.document.buffer.as_str())
                }
            }
            _ => Vec::new(),
        };
        self.syntax_tokens_document_id = Some(self.document.id);
        self.syntax_tokens_version = Some(self.document.version);
        self.syntax_tokens_language = Some(self.document.language);
    }
}
