use std::collections::HashMap;

use crate::code::EditorLanguage;

/// Collapsible logical line range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FoldRegion {
    pub id: FoldRegionId,
    pub start_line: usize,
    pub end_line: usize,
    pub collapsed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FoldRegionId(pub u64);

impl FoldRegion {
    pub fn new(start_line: usize, end_line: usize) -> Self {
        Self {
            id: FoldRegionId::from_lines(start_line, end_line),
            start_line,
            end_line,
            collapsed: false,
        }
    }

    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    pub fn hides_line(self, line: usize) -> bool {
        self.collapsed && line > self.start_line && line <= self.end_line
    }

    pub fn hidden_line_count(self) -> usize {
        self.end_line.saturating_sub(self.start_line)
    }
}

impl FoldRegionId {
    pub fn from_lines(start_line: usize, end_line: usize) -> Self {
        Self(((start_line as u64) << 32) | end_line as u64)
    }
}

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

pub fn fold_region_at_line(regions: &[FoldRegion], line: usize) -> Option<usize> {
    regions
        .iter()
        .position(|region| region.start_line == line && region.end_line > region.start_line)
}

pub fn collapsed_region_hiding_line(regions: &[FoldRegion], line: usize) -> Option<FoldRegion> {
    regions
        .iter()
        .copied()
        .find(|region| region.hides_line(line))
}

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

pub fn line_for_byte(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
}

#[cfg(feature = "tree-sitter")]
pub fn fold_regions_for_document(language: EditorLanguage, text: &str) -> Vec<FoldRegion> {
    match language {
        EditorLanguage::Rust => rust_tree_sitter_fold_regions(text),
        _ => Vec::new(),
    }
}

#[cfg(not(feature = "tree-sitter"))]
pub fn fold_regions_for_document(_language: EditorLanguage, _text: &str) -> Vec<FoldRegion> {
    Vec::new()
}

#[cfg(feature = "tree-sitter")]
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

#[cfg(feature = "tree-sitter")]
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

#[cfg(feature = "tree-sitter")]
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
