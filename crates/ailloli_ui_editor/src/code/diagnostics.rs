//! Version-aware diagnostic value types and byte hit testing.

use std::ops::Range;

/// Diagnostic attached to a byte range in a code document.
///
/// Ranges are UTF-8 byte offsets and are not normalized or clamped here. Empty
/// and reversed ranges are representable; hit testing uses inclusive endpoints.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{Diagnostic, DiagnosticSeverity, DiagnosticSource};
/// let diagnostic = Diagnostic::new(2..5, DiagnosticSeverity::Warning, "unused");
/// assert_eq!(diagnostic.source, DiagnosticSource::Local);
/// assert_eq!(diagnostic.range, 2..5);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Diagnostic {
    /// Unvalidated UTF-8 byte range.
    pub range: Range<usize>,
    /// Error, warning, information, or hint level.
    pub severity: DiagnosticSeverity,
    /// Exact owned human-readable message; empty is allowed.
    pub message: String,
    /// Local or language-server provenance.
    pub source: DiagnosticSource,
    /// Source document revision for LSP diagnostics, otherwise `None` by default.
    pub document_version: Option<crate::code::DocumentVersion>,
}

/// Diagnostic importance used for semantic coloring.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::DiagnosticSeverity;
/// assert_ne!(DiagnosticSeverity::Error, DiagnosticSeverity::Hint);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DiagnosticSeverity {
    /// Error.
    Error,
    /// Warning.
    Warning,
    /// Informational notice.
    Info,
    /// Low-priority hint.
    Hint,
}

/// Diagnostic producer category.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::DiagnosticSource;
/// assert_ne!(DiagnosticSource::Local, DiagnosticSource::Lsp);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DiagnosticSource {
    /// Generated locally by the editor/application.
    Local,
    /// Produced by an LSP backend.
    Lsp,
}

/// Diagnostic constructors.
impl Diagnostic {
    /// Creates a local diagnostic with no document version.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Diagnostic, DiagnosticSeverity, DiagnosticSource};
    /// let diagnostic = Diagnostic::new(0..0, DiagnosticSeverity::Hint, "");
    /// assert_eq!(diagnostic.source, DiagnosticSource::Local);
    /// assert_eq!(diagnostic.document_version, None);
    /// ```
    pub fn new(
        range: Range<usize>,
        severity: DiagnosticSeverity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            range,
            severity,
            message: message.into(),
            source: DiagnosticSource::Local,
            document_version: None,
        }
    }

    /// Creates an LSP diagnostic tied to an exact document version.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Diagnostic, DiagnosticSeverity, DiagnosticSource, DocumentVersion};
    /// let diagnostic = Diagnostic::lsp(1..3, DiagnosticSeverity::Error, "bad", DocumentVersion(4));
    /// assert_eq!(diagnostic.source, DiagnosticSource::Lsp);
    /// assert_eq!(diagnostic.document_version, Some(DocumentVersion(4)));
    /// ```
    pub fn lsp(
        range: Range<usize>,
        severity: DiagnosticSeverity,
        message: impl Into<String>,
        document_version: crate::code::DocumentVersion,
    ) -> Self {
        Self {
            range,
            severity,
            message: message.into(),
            source: DiagnosticSource::Lsp,
            document_version: Some(document_version),
        }
    }
}

/// Owned diagnostic plus its original slice index.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{Diagnostic, DiagnosticHit, DiagnosticSeverity};
/// let hit = DiagnosticHit { index: 2, diagnostic: Diagnostic::new(0..1, DiagnosticSeverity::Info, "note") };
/// assert_eq!(hit.index, 2);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticHit {
    /// Index in the searched diagnostics slice.
    pub index: usize,
    /// Cloned matching diagnostic.
    pub diagnostic: Diagnostic,
}

/// Returns the first diagnostic whose inclusive range contains `byte`.
///
/// Both start and end are inclusive, unlike standard Rust ranges; overlapping
/// diagnostics select lowest slice index. No match returns `None`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::diagnostic_at_byte, Diagnostic, DiagnosticSeverity};
/// let diagnostics = [Diagnostic::new(2..4, DiagnosticSeverity::Error, "bad")];
/// assert_eq!(diagnostic_at_byte(&diagnostics, 4).unwrap().index, 0);
/// assert!(diagnostic_at_byte(&diagnostics, 5).is_none());
/// ```
pub fn diagnostic_at_byte(diagnostics: &[Diagnostic], byte: usize) -> Option<DiagnosticHit> {
    diagnostics
        .iter()
        .enumerate()
        .find(|(_, diagnostic)| diagnostic.range.start <= byte && byte <= diagnostic.range.end)
        .map(|(index, diagnostic)| DiagnosticHit {
            index,
            diagnostic: diagnostic.clone(),
        })
}
