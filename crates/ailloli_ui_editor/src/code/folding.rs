//! Logical-line fold regions, discovery, and collapsed-state transfer.

use std::collections::HashMap;

use crate::code::EditorLanguage;

/// Collapsible range of zero-based logical lines.
///
/// The line at [`FoldRegion::start_line`] remains visible when the region is
/// collapsed; lines through [`FoldRegion::end_line`] are hidden. Callers are
/// responsible for constructing ranges whose end is not before their start.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::FoldRegion;
///
/// let region = FoldRegion::new(3, 6).collapsed(true);
/// assert!(!region.hides_line(3));
/// assert!(region.hides_line(4));
/// assert!(region.hides_line(6));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FoldRegion {
    /// Stable identifier derived from the start and end lines.
    pub id: FoldRegionId,
    /// Zero-based header line that remains visible while collapsed.
    pub start_line: usize,
    /// Zero-based inclusive last line hidden while collapsed.
    pub end_line: usize,
    /// Whether the region currently hides its body lines.
    pub collapsed: bool,
}

/// Packed identity for a fold region's line range.
///
/// The representation is collision-free when both line indices fit in 32 bits.
/// Values outside that range are not rejected and can collide, so persistent
/// callers should keep both inputs at or below [`u32::MAX`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::FoldRegionId;
///
/// assert_eq!(FoldRegionId::from_lines(2, 9), FoldRegionId((2_u64 << 32) | 9));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FoldRegionId(pub u64);

/// Constructs and queries fold regions.
impl FoldRegion {
    /// Creates an expanded region spanning `start_line..=end_line`.
    ///
    /// The inputs are stored without ordering validation. Consequently, a
    /// reversed range hides no lines and reports a hidden count of zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::FoldRegion;
    ///
    /// let region = FoldRegion::new(4, 7);
    /// assert_eq!((region.start_line, region.end_line), (4, 7));
    /// assert!(!region.collapsed);
    /// ```
    pub fn new(start_line: usize, end_line: usize) -> Self {
        Self {
            id: FoldRegionId::from_lines(start_line, end_line),
            start_line,
            end_line,
            collapsed: false,
        }
    }

    /// Sets whether the region is collapsed and returns it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::FoldRegion;
    ///
    /// assert!(FoldRegion::new(0, 2).collapsed(true).collapsed);
    /// ```
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    /// Returns whether a zero-based line is hidden by this region.
    ///
    /// The header line is never hidden and the end line is inclusive.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::FoldRegion;
    ///
    /// let region = FoldRegion::new(5, 6).collapsed(true);
    /// assert_eq!((region.hides_line(5), region.hides_line(6)), (false, true));
    /// ```
    pub fn hides_line(self, line: usize) -> bool {
        self.collapsed && line > self.start_line && line <= self.end_line
    }

    /// Returns the number of body lines in the range, whether expanded or not.
    ///
    /// Reversed ranges return zero rather than underflowing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::FoldRegion;
    ///
    /// assert_eq!(FoldRegion::new(2, 5).hidden_line_count(), 3);
    /// assert_eq!(FoldRegion::new(5, 2).hidden_line_count(), 0);
    /// ```
    pub fn hidden_line_count(self) -> usize {
        self.end_line.saturating_sub(self.start_line)
    }
}

/// Creates packed fold-region identifiers.
impl FoldRegionId {
    /// Packs the start line into the high 32 bits and ORs in the end line.
    ///
    /// Inputs are not range-checked. The identifier is uniquely reversible
    /// only when both inputs fit in [`u32`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::FoldRegionId;
    ///
    /// let id = FoldRegionId::from_lines(12, 34);
    /// assert_eq!(id.0 >> 32, 12);
    /// assert_eq!(id.0 as u32, 34);
    /// ```
    pub fn from_lines(start_line: usize, end_line: usize) -> Self {
        Self(((start_line as u64) << 32) | end_line as u64)
    }
}

/// Transfers collapsed state from matching previous region identifiers.
///
/// Regions without a previous match keep their supplied state and the order of
/// `next` is preserved. If `previous` contains a duplicate identifier, its last
/// occurrence wins.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::merge_fold_regions_with_previous, FoldRegion};
///
/// let previous = [FoldRegion::new(1, 3).collapsed(true)];
/// let merged = merge_fold_regions_with_previous(vec![FoldRegion::new(1, 3)], &previous);
/// assert!(merged[0].collapsed);
/// ```
pub fn merge_fold_regions_with_previous(
    mut next: Vec<FoldRegion>,
    previous: &[FoldRegion],
) -> Vec<FoldRegion> {
    let collapsed_by_id: HashMap<_, _> = previous
        .iter()
        .map(|region| (region.id, region.collapsed))
        .collect();
    for region in &mut next {
        if let Some(collapsed) = collapsed_by_id.get(&region.id) {
            region.collapsed = *collapsed;
        }
    }
    next
}

/// Returns the index of the first non-empty region starting on `line`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::fold_region_at_line, FoldRegion};
///
/// let regions = [FoldRegion::new(2, 2), FoldRegion::new(2, 5)];
/// assert_eq!(fold_region_at_line(&regions, 2), Some(1));
/// assert_eq!(fold_region_at_line(&regions, 5), None);
/// ```
pub fn fold_region_at_line(regions: &[FoldRegion], line: usize) -> Option<usize> {
    regions
        .iter()
        .position(|region| region.start_line == line && region.end_line > region.start_line)
}

/// Returns the first collapsed region that hides `line`.
///
/// Overlapping regions are resolved by slice order rather than by smallest or
/// outermost range.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::collapsed_region_hiding_line, FoldRegion};
///
/// let regions = [FoldRegion::new(1, 4).collapsed(true)];
/// assert_eq!(collapsed_region_hiding_line(&regions, 3), Some(regions[0]));
/// assert_eq!(collapsed_region_hiding_line(&regions, 1), None);
/// ```
pub fn collapsed_region_hiding_line(regions: &[FoldRegion], line: usize) -> Option<FoldRegion> {
    regions
        .iter()
        .copied()
        .find(|region| region.hides_line(line))
}

/// Returns the UTF-8 byte offset at which a zero-based logical line starts.
///
/// Line zero starts at byte zero. A line beyond the available newline
/// separators maps to `text.len()`, including in an empty string.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::code::line_start_byte;
///
/// assert_eq!(line_start_byte("é\nnext", 1), 3);
/// assert_eq!(line_start_byte("one", 9), 3);
/// ```
pub fn line_start_byte(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut current = 0;
    for (offset, ch) in text.char_indices() {
        if ch == '\n' {
            current += 1;
            if current == line {
                return offset + ch.len_utf8();
            }
        }
    }
    text.len()
}

/// Counts newline bytes before a UTF-8 byte offset.
///
/// Offsets beyond the text are clamped to `text.len()`.
///
/// # Panics
///
/// Panics when an in-bounds `byte` is not a UTF-8 character boundary.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::code::line_for_byte;
///
/// let text = "é\nnext";
/// assert_eq!(line_for_byte(text, 2), 0);
/// assert_eq!(line_for_byte(text, 3), 1);
/// assert_eq!(line_for_byte(text, usize::MAX), 1);
/// ```
pub fn line_for_byte(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
}

#[cfg(feature = "tree_sitter")]
/// Discovers multi-line foldable constructs for a document.
///
/// With the `tree_sitter` feature, Rust documents are parsed and their regions
/// are sorted and deduplicated; unsupported languages return an empty vector.
/// Parser failures also produce an empty vector.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::fold_regions_for_document, EditorLanguage};
///
/// let regions = fold_regions_for_document(EditorLanguage::Rust, "fn f() {\n    1;\n}\n");
/// assert_eq!(regions.len(), 1);
/// assert_eq!((regions[0].start_line, regions[0].end_line), (0, 2));
/// assert!(fold_regions_for_document(EditorLanguage::PlainText, "a\nb").is_empty());
/// ```
pub fn fold_regions_for_document(language: EditorLanguage, text: &str) -> Vec<FoldRegion> {
    match language {
        EditorLanguage::Rust => rust_tree_sitter_fold_regions(text),
        _ => Vec::new(),
    }
}

#[cfg(not(feature = "tree_sitter"))]
/// Returns no discovered regions when parser support is disabled.
///
/// Manually supplied [`FoldRegion`] values remain usable without the
/// `tree_sitter` feature.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::fold_regions_for_document, EditorLanguage};
///
/// assert!(fold_regions_for_document(EditorLanguage::Rust, "fn f() {\n}\n").is_empty());
/// ```
pub fn fold_regions_for_document(_language: EditorLanguage, _text: &str) -> Vec<FoldRegion> {
    Vec::new()
}

#[cfg(feature = "tree_sitter")]
/// Parses Rust text and returns its sorted, deduplicated fold regions.
fn rust_tree_sitter_fold_regions(text: &str) -> Vec<FoldRegion> {
    let mut parser = tree_sitter::Parser::new();
    let language = tree_sitter_rust::LANGUAGE.into();
    if parser.set_language(&language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        return Vec::new();
    };
    let mut regions = Vec::new();
    collect_rust_fold_regions(tree.root_node(), &mut regions);
    regions.sort_by_key(|region| (region.start_line, region.end_line));
    regions.dedup_by_key(|region| (region.start_line, region.end_line));
    regions
}

#[cfg(feature = "tree_sitter")]
/// Recursively collects the foldable nodes below `node`.
fn collect_rust_fold_regions(node: tree_sitter::Node<'_>, regions: &mut Vec<FoldRegion>) {
    if is_foldable_rust_node(node) {
        let start = node.start_position().row;
        let end = node.end_position().row;
        if end > start {
            regions.push(FoldRegion::new(start, end));
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_rust_fold_regions(child, regions);
    }
}

#[cfg(feature = "tree_sitter")]
/// Reports whether a Rust syntax node defines a foldable construct.
fn is_foldable_rust_node(node: tree_sitter::Node<'_>) -> bool {
    matches!(
        node.kind(),
        "mod_item"
            | "impl_item"
            | "function_item"
            | "trait_item"
            | "struct_item"
            | "enum_item"
            | "block"
            | "block_comment"
    )
}
