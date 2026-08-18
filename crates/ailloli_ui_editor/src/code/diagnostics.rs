use std::ops::Range;

/// Diagnostic attached to a byte range in a code document.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Diagnostic {
    pub range: Range<usize>,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub source: DiagnosticSource,
    pub document_version: Option<crate::code::DocumentVersion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DiagnosticSource {
    Local,
    Lsp,
}

impl Diagnostic {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticHit {
    pub index: usize,
    pub diagnostic: Diagnostic,
}

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
