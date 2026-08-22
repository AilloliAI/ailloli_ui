//! Deterministic Rust syntax tokenization and optional Tree-sitter enrichment.

use std::ops::Range;

/// Syntax token emitted by code highlighters.
///
/// The half-open range uses UTF-8 byte offsets into the exact source passed to
/// the highlighter.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::code::{SyntaxKind, SyntaxToken};
/// let token = SyntaxToken { range: 0..2, kind: SyntaxKind::Keyword };
/// assert_eq!(token.range, 0..2);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SyntaxToken {
    /// Half-open UTF-8 source byte range.
    pub range: Range<usize>,
    /// Language-neutral semantic paint category.
    pub kind: SyntaxKind,
}

/// Language-neutral syntax token categories.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::code::SyntaxKind;
/// assert_ne!(SyntaxKind::Comment, SyntaxKind::Identifier);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SyntaxKind {
    /// Reserved language word or structural keyword-like node.
    Keyword,
    /// Type name or primitive type.
    Type,
    /// Function or macro name.
    Function,
    /// String or character literal.
    String,
    /// Numeric literal.
    Number,
    /// Line or block comment.
    Comment,
    /// Operator sequence.
    Operator,
    /// Delimiter or punctuation byte.
    Punctuation,
    /// Identifier or Rust lifetime.
    Identifier,
}

/// ASCII Rust keywords recognized by the lexical highlighter.
const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

/// Tokenizes a useful ASCII-oriented subset of Rust without parsing.
///
/// The deterministic, linear scan recognizes line comments, quoted strings and
/// chars, raw strings, lifetimes, numeric/identifier runs, keywords, delimiters,
/// and operators. It then marks an identifier following `fn` and identifiers
/// followed by optional ASCII whitespace plus `!` as functions. Non-ASCII
/// identifier bytes and unrecognized punctuation are skipped. Unterminated
/// strings/comments extend to EOF; returned ranges are ordered UTF-8 bytes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::code::{highlight_rust_lexical, SyntaxKind};
/// let text = "fn main() { println!(\"hi\"); }";
/// let tokens = highlight_rust_lexical(text);
/// assert!(tokens.iter().any(|t| t.kind == SyntaxKind::Keyword && &text[t.range.clone()] == "fn"));
/// assert!(tokens.iter().any(|t| t.kind == SyntaxKind::Function && &text[t.range.clone()] == "main"));
/// assert!(tokens.iter().any(|t| t.kind == SyntaxKind::String && &text[t.range.clone()] == "\"hi\""));
/// ```
pub fn highlight_rust_lexical(text: &str) -> Vec<SyntaxToken> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                let start = i;
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                tokens.push(token(start..i, SyntaxKind::Comment));
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i = (i + 2).min(bytes.len());
                    } else if bytes[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
                tokens.push(token(start..i, SyntaxKind::String));
            }
            b'r' if raw_string_end(text, i).is_some() => {
                let start = i;
                i = raw_string_end(text, i).unwrap_or(i + 1);
                tokens.push(token(start..i, SyntaxKind::String));
            }
            b'\'' if lifetime_end(bytes, i).is_some() => {
                let start = i;
                i = lifetime_end(bytes, i).unwrap_or(i + 1);
                tokens.push(token(start..i, SyntaxKind::Identifier));
            }
            b'\'' => {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i = (i + 2).min(bytes.len());
                    } else if bytes[i] == b'\'' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
                tokens.push(token(start..i, SyntaxKind::String));
            }
            b'0'..=b'9' => {
                let start = i;
                i += 1;
                while i < bytes.len() && is_ident_continue(bytes[i]) {
                    i += 1;
                }
                tokens.push(token(start..i, SyntaxKind::Number));
            }
            ch if is_ident_start(ch) => {
                let start = i;
                i += 1;
                while i < bytes.len() && is_ident_continue(bytes[i]) {
                    i += 1;
                }
                let word = &text[start..i];
                let kind = if RUST_KEYWORDS.contains(&word) {
                    SyntaxKind::Keyword
                } else if word.chars().next().is_some_and(char::is_uppercase) {
                    SyntaxKind::Type
                } else {
                    SyntaxKind::Identifier
                };
                tokens.push(token(start..i, kind));
            }
            b'{' | b'}' | b'(' | b')' | b'[' | b']' | b',' | b';' | b':' | b'.' => {
                tokens.push(token(i..i + 1, SyntaxKind::Punctuation));
                i += 1;
            }
            b'+' | b'-' | b'*' | b'=' | b'!' | b'<' | b'>' | b'&' | b'|' | b'%' => {
                let start = i;
                i += 1;
                while i < bytes.len() && matches!(bytes[i], b'=' | b'>' | b'&' | b'|') {
                    i += 1;
                }
                tokens.push(token(start..i, SyntaxKind::Operator));
            }
            _ => i += 1,
        }
    }

    mark_function_identifiers(text, &mut tokens);
    mark_macro_identifiers(text, &mut tokens);
    tokens
}

#[cfg(feature = "tree_sitter")]
/// Combines Tree-sitter Rust tokens with lexical gap filling.
///
/// Returns `None` if parser setup/parsing fails or non-whitespace input yields no
/// structural token. Otherwise ranges are clamped to UTF-8 boundaries, sorted,
/// deduplicated, and made non-overlapping by semantic priority before and after
/// lexical gap filling. Whitespace-only input returns `Some(Vec::new())`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::code::{highlight_rust_tree_sitter_hybrid, SyntaxKind};
/// let text = "fn main() { let n = 1; }";
/// let tokens = highlight_rust_tree_sitter_hybrid(text).unwrap();
/// assert!(tokens.iter().any(|t| t.kind == SyntaxKind::Function && &text[t.range.clone()] == "main"));
/// assert_eq!(highlight_rust_tree_sitter_hybrid("   "), Some(Vec::new()));
/// ```
pub fn highlight_rust_tree_sitter_hybrid(text: &str) -> Option<Vec<SyntaxToken>> {
    let mut parser = tree_sitter::Parser::new();
    let language = tree_sitter_rust::LANGUAGE.into();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(text, None)?;

    let mut tokens = Vec::new();
    collect_tree_sitter_tokens(tree.root_node(), &mut tokens);
    if tokens.is_empty() && !text.trim().is_empty() {
        return None;
    }

    let mut normalized = normalize_syntax_tokens(text, tokens);
    gap_fill_lexical_tokens(text, &mut normalized);
    Some(normalize_syntax_tokens(text, normalized))
}

#[cfg(feature = "tree_sitter")]
/// Recursively collects structural and named-item tokens from a syntax tree.
fn collect_tree_sitter_tokens(node: tree_sitter::Node<'_>, tokens: &mut Vec<SyntaxToken>) {
    if let Some(kind) = tree_sitter_node_kind(node) {
        tokens.push(token(node.start_byte()..node.end_byte(), kind));
    }

    if let Some((kind, name)) = tree_sitter_named_item(node) {
        tokens.push(token(name.start_byte()..name.end_byte(), kind));
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tree_sitter_tokens(child, tokens);
    }
}

#[cfg(feature = "tree_sitter")]
/// Maps a structural Tree-sitter Rust node to a syntax category.
fn tree_sitter_node_kind(node: tree_sitter::Node<'_>) -> Option<SyntaxKind> {
    match node.kind() {
        "line_comment" | "block_comment" => Some(SyntaxKind::Comment),
        "string_literal" | "raw_string_literal" | "char_literal" => Some(SyntaxKind::String),
        "integer_literal" | "float_literal" => Some(SyntaxKind::Number),
        "primitive_type" | "type_identifier" => Some(SyntaxKind::Type),
        "visibility_modifier" | "where_clause" | "for_lifetimes" | "attribute_item" => {
            Some(SyntaxKind::Keyword)
        }
        kind if RUST_KEYWORDS.contains(&kind) => Some(SyntaxKind::Keyword),
        _ => None,
    }
}

#[cfg(feature = "tree_sitter")]
/// Extracts the named identifier node for declarations with semantic names.
fn tree_sitter_named_item(
    node: tree_sitter::Node<'_>,
) -> Option<(SyntaxKind, tree_sitter::Node<'_>)> {
    let kind = match node.kind() {
        "function_item" => SyntaxKind::Function,
        "struct_item" | "enum_item" | "trait_item" | "type_item" => SyntaxKind::Type,
        _ => return None,
    };
    node.child_by_field_name("name").map(|name| (kind, name))
}

#[cfg(feature = "tree_sitter")]
/// Adds lexical tokens not superseded by stronger structural tokens.
fn gap_fill_lexical_tokens(text: &str, tokens: &mut Vec<SyntaxToken>) {
    let existing = tokens.clone();
    tokens.extend(
        highlight_rust_lexical(text)
            .into_iter()
            .filter(|candidate| lexical_gap_fill_allowed(candidate, &existing)),
    );
}

#[cfg(feature = "tree_sitter")]
/// Tests whether a lexical token can coexist with current structural coverage.
fn lexical_gap_fill_allowed(candidate: &SyntaxToken, existing: &[SyntaxToken]) -> bool {
    let has_overlap = existing
        .iter()
        .any(|token| ranges_overlap(&candidate.range, &token.range));
    if !has_overlap {
        return true;
    }
    matches!(
        candidate.kind,
        SyntaxKind::Operator | SyntaxKind::Punctuation | SyntaxKind::Identifier
    ) && !existing.iter().any(|token| {
        ranges_overlap(&candidate.range, &token.range)
            && syntax_kind_priority(token.kind) < syntax_kind_priority(candidate.kind)
    })
}

#[cfg(feature = "tree_sitter")]
/// Clamps, sorts, deduplicates, and resolves overlapping syntax tokens.
fn normalize_syntax_tokens(text: &str, tokens: Vec<SyntaxToken>) -> Vec<SyntaxToken> {
    let mut indexed = tokens
        .into_iter()
        .enumerate()
        .filter_map(|(idx, token)| normalize_token(text, token).map(|token| (idx, token)))
        .collect::<Vec<_>>();
    indexed.sort_by(|(a_idx, a), (b_idx, b)| {
        a.range
            .start
            .cmp(&b.range.start)
            .then(a.range.end.cmp(&b.range.end))
            .then(syntax_kind_priority(a.kind).cmp(&syntax_kind_priority(b.kind)))
            .then(a_idx.cmp(b_idx))
    });

    let mut out: Vec<SyntaxToken> = Vec::new();
    for (_, token) in indexed {
        if out
            .last()
            .is_some_and(|prev| prev.range == token.range && prev.kind == token.kind)
        {
            continue;
        }
        if let Some(pos) = out
            .iter()
            .position(|prev| ranges_overlap(&prev.range, &token.range))
        {
            let previous_priority = syntax_kind_priority(out[pos].kind);
            let current_priority = syntax_kind_priority(token.kind);
            if current_priority < previous_priority {
                out.remove(pos);
                out.push(token);
                out.sort_by(|a, b| {
                    a.range
                        .start
                        .cmp(&b.range.start)
                        .then(a.range.end.cmp(&b.range.end))
                });
            }
            continue;
        }
        out.push(token);
    }
    out
}

#[cfg(feature = "tree_sitter")]
/// Clamps one token to a non-empty valid UTF-8 source range.
fn normalize_token(text: &str, token: SyntaxToken) -> Option<SyntaxToken> {
    let start = clamp_to_char_boundary(text, token.range.start.min(text.len()));
    let end = clamp_to_char_boundary(text, token.range.end.min(text.len()));
    (end > start).then_some(SyntaxToken {
        range: start..end,
        kind: token.kind,
    })
}

#[cfg(feature = "tree_sitter")]
/// Moves an offset backward to the nearest UTF-8 boundary.
fn clamp_to_char_boundary(text: &str, mut at: usize) -> usize {
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

#[cfg(feature = "tree_sitter")]
/// Tests intersection of two half-open byte ranges.
fn ranges_overlap(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

#[cfg(feature = "tree_sitter")]
/// Returns the lower-is-stronger overlap priority for a syntax category.
fn syntax_kind_priority(kind: SyntaxKind) -> u8 {
    match kind {
        SyntaxKind::Comment => 0,
        SyntaxKind::String => 1,
        SyntaxKind::Number => 2,
        SyntaxKind::Keyword => 3,
        SyntaxKind::Function => 4,
        SyntaxKind::Type => 5,
        SyntaxKind::Identifier => 6,
        SyntaxKind::Operator => 7,
        SyntaxKind::Punctuation => 8,
    }
}

/// Reclassifies the next identifier token after each `fn` keyword.
fn mark_function_identifiers(text: &str, tokens: &mut [SyntaxToken]) {
    for idx in 0..tokens.len().saturating_sub(1) {
        if tokens[idx].kind == SyntaxKind::Keyword && &text[tokens[idx].range.clone()] == "fn" {
            if let Some(next) = tokens[idx + 1..]
                .iter_mut()
                .find(|token| token.kind == SyntaxKind::Identifier)
            {
                next.kind = SyntaxKind::Function;
            }
        }
    }
}

/// Reclassifies identifiers followed by optional whitespace and `!` as macros.
fn mark_macro_identifiers(text: &str, tokens: &mut [SyntaxToken]) {
    for token in tokens.iter_mut() {
        if token.kind != SyntaxKind::Identifier {
            continue;
        }
        let mut at = token.range.end;
        while at < text.len() && text.as_bytes()[at].is_ascii_whitespace() {
            at += 1;
        }
        if text.as_bytes().get(at) == Some(&b'!') {
            token.kind = SyntaxKind::Function;
        }
    }
}

/// Returns the end of a Rust-like lifetime, rejecting character literals.
fn lifetime_end(bytes: &[u8], start: usize) -> Option<usize> {
    let first = *bytes.get(start + 1)?;
    if !is_ident_start(first) {
        return None;
    }
    let mut end = start + 2;
    while end < bytes.len() && is_ident_continue(bytes[end]) {
        end += 1;
    }
    if bytes.get(end) == Some(&b'\'') {
        return None;
    }
    Some(end)
}

/// Returns the end of a Rust raw string, using EOF for an unterminated string.
fn raw_string_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(start) != Some(&b'r') {
        return None;
    }
    let mut hashes = 0usize;
    let mut quote = start + 1;
    while bytes.get(quote) == Some(&b'#') {
        hashes += 1;
        quote += 1;
    }
    if bytes.get(quote) != Some(&b'"') {
        return None;
    }
    let mut i = quote + 1;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let mut matched = true;
            for offset in 0..hashes {
                if bytes.get(i + 1 + offset) != Some(&b'#') {
                    matched = false;
                    break;
                }
            }
            if matched {
                return Some((i + 1 + hashes).min(text.len()));
            }
        }
        i += 1;
    }
    Some(text.len())
}

/// Constructs a syntax token without normalizing its range.
fn token(range: Range<usize>, kind: SyntaxKind) -> SyntaxToken {
    SyntaxToken { range, kind }
}

/// Tests the first byte of an ASCII Rust-like identifier.
fn is_ident_start(ch: u8) -> bool {
    ch == b'_' || ch.is_ascii_alphabetic()
}

/// Tests a continuation byte of an ASCII Rust-like identifier.
fn is_ident_continue(ch: u8) -> bool {
    is_ident_start(ch) || ch.is_ascii_digit()
}
