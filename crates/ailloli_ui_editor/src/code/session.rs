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

/// Code-editor session: document metadata plus the generic editor session.
#[derive(Debug, Clone)]
pub struct CodeEditorSession {
    pub document: Document,
    pub editor: EditorSession,
    pub config: CodeEditorConfig,
    pub diagnostics: Vec<Diagnostic>,
    pub active_diagnostic_index: Option<usize>,
    pub search: SearchState,
    pub syntax_tokens: Vec<SyntaxToken>,
    pub syntax_tokens_document_id: Option<DocumentId>,
    pub syntax_tokens_version: Option<DocumentVersion>,
    pub syntax_tokens_language: Option<EditorLanguage>,
    pub fold_regions: Vec<FoldRegion>,
    pub fold_regions_document_id: Option<DocumentId>,
    pub fold_regions_version: Option<DocumentVersion>,
    pub fold_regions_language: Option<EditorLanguage>,
}

impl CodeEditorSession {
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

    pub fn editor_session(&self) -> &EditorSession {
        &self.editor
    }

    pub fn editor_session_mut(&mut self) -> &mut EditorSession {
        &mut self.editor
    }

    pub fn sync_document_from_editor(&mut self) -> bool {
        if self.document.buffer.as_str() == self.editor.buffer.as_str() {
            return false;
        }
        self.document.buffer = TextBuffer::from_string(self.editor.buffer.as_str());
        self.document.dirty = true;
        self.document.version = DocumentVersion(self.document.version.0.saturating_add(1));
        true
    }

    pub fn sync_editor_from_document(&mut self) -> bool {
        self.editor
            .replace_buffer_if_changed(self.document.buffer.clone())
    }

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

    pub fn set_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) {
        self.diagnostics = diagnostics;
        self.active_diagnostic_index = self
            .active_diagnostic_index
            .filter(|idx| *idx < self.diagnostics.len());
    }

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

    pub fn set_active_diagnostic_index(&mut self, active_diagnostic_index: Option<usize>) {
        self.active_diagnostic_index =
            active_diagnostic_index.filter(|idx| *idx < self.diagnostics.len());
    }

    pub fn diagnostic_at_byte(&self, byte: usize) -> Option<DiagnosticHit> {
        diagnostic_at_byte(&self.diagnostics, byte)
    }

    pub fn set_search_matches(&mut self, search_matches: Vec<SearchMatch>) {
        self.search.matches = search_matches;
        self.search.set_active_index(None);
    }

    pub fn set_search_query(&mut self, query: SearchQuery) {
        self.search.set_query(query);
        self.refresh_search();
    }

    pub fn set_search_active_index(&mut self, active_index: Option<usize>) {
        self.search.set_active_index(active_index);
    }

    pub fn refresh_search(&mut self) -> bool {
        self.search
            .refresh(&self.document.buffer.as_str(), self.document.version)
    }

    pub fn search_next(&mut self) -> Option<&SearchMatch> {
        self.refresh_search();
        self.search.next_match()
    }

    pub fn search_previous(&mut self) -> Option<&SearchMatch> {
        self.refresh_search();
        self.search.previous_match()
    }

    pub fn clear_search(&mut self) {
        self.search.clear();
    }

    pub fn select_word_at_byte(&mut self, byte: usize) -> bool {
        self.refresh_syntax_tokens();
        self.editor
            .select_word_at_byte(byte, Some(&self.syntax_tokens), self.document.language)
    }

    pub fn select_line_at_byte(&mut self, byte: usize) -> bool {
        self.editor.select_line_at_byte(byte)
    }

    pub fn set_syntax_tokens(&mut self, syntax_tokens: Vec<SyntaxToken>) {
        self.syntax_tokens = syntax_tokens;
        self.syntax_tokens_document_id = None;
        self.syntax_tokens_version = None;
        self.syntax_tokens_language = None;
    }

    pub fn set_fold_regions(&mut self, fold_regions: Vec<FoldRegion>) {
        self.fold_regions = fold_regions;
        self.fold_regions_document_id = None;
        self.fold_regions_version = None;
        self.fold_regions_language = None;
    }

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

    pub fn move_caret_out_of_folded_regions(&mut self) -> bool {
        let line = line_for_byte(&self.document.buffer.as_str(), self.editor.edit.caret_byte);
        let Some(region) = collapsed_region_hiding_line(&self.fold_regions, line) else {
            return false;
        };
        let byte = line_start_byte(&self.document.buffer.as_str(), region.start_line);
        self.editor.edit.set_caret(&self.editor.buffer, byte, false)
    }

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
