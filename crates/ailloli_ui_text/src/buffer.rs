//! Rope-backed text buffer with per-paragraph revision tracking.
//!
//! - `ropey` enables O(log n) edits and fast paragraph splitting.
//! - Each paragraph (separated by `\n`) has its own `revision`; edits bump only
//!   paragraphs intersecting the changed byte range.
//! - Layout caches can invalidate per paragraph instead of flushing entirely.

use core::ops::Range;

use ropey::Rope;

/// Metadata for one paragraph: UTF-8 byte range and revision counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParagraphMeta {
    pub byte_range: Range<usize>,
    pub revision: u64,
}

/// Mutable text storage with a paragraph index (logical line = `\n`-separated).
#[derive(Debug, Clone)]
pub struct TextBuffer {
    rope: Rope,
    revision: u64,
    paragraphs: Vec<ParagraphMeta>,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl TextBuffer {
    pub fn new() -> Self {
        Self::from_string(String::new())
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        let s = s.into();
        let rope = Rope::from_str(&s);
        let mut me = Self {
            rope,
            revision: 0,
            paragraphs: Vec::new(),
        };
        me.rebuild_paragraphs();
        me
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn as_str(&self) -> String {
        self.rope.to_string()
    }

    pub fn paragraphs(&self) -> &[ParagraphMeta] {
        &self.paragraphs
    }

    pub fn paragraph_text(&self, idx: usize) -> Option<String> {
        let p = self.paragraphs.get(idx)?;
        let slice = self.rope.byte_slice(p.byte_range.start..p.byte_range.end);
        Some(slice.to_string())
    }

    pub fn revision_of_paragraph(&self, idx: usize) -> Option<u64> {
        self.paragraphs.get(idx).map(|p| p.revision)
    }

    /// Index of the paragraph containing byte `b` (clamped to the last paragraph).
    pub fn paragraph_at(&self, b: usize) -> usize {
        match self.paragraphs.binary_search_by(|p| {
            if p.byte_range.contains(&b) {
                core::cmp::Ordering::Equal
            } else if b < p.byte_range.start {
                core::cmp::Ordering::Greater
            } else {
                core::cmp::Ordering::Less
            }
        }) {
            Ok(i) => i,
            Err(i) => i.min(self.paragraphs.len().saturating_sub(1)),
        }
    }

    /// Replaces `range` (UTF-8 bytes) with `new_text`. Bumps global and touched paragraph revisions.
    pub fn edit(&mut self, range: Range<usize>, new_text: &str) {
        let start = range.start.min(self.rope.len_bytes());
        let end = range.end.min(self.rope.len_bytes());
        let start = start.min(end);

        let touched = self.paragraphs_in_byte_range(start..end);
        let start_char = self.rope.byte_to_char(start);
        let end_char = self.rope.byte_to_char(end);

        if end_char > start_char {
            self.rope.remove(start_char..end_char);
        }
        if !new_text.is_empty() {
            self.rope.insert(start_char, new_text);
        }

        self.revision = self.revision.wrapping_add(1);
        self.rebuild_paragraphs_after_edit(touched);
    }

    /// Renvoie les indices de paragraphes intersectant `byte_range`.
    pub fn paragraphs_in_byte_range(&self, byte_range: Range<usize>) -> Vec<usize> {
        if self.paragraphs.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (i, p) in self.paragraphs.iter().enumerate() {
            if p.byte_range.end <= byte_range.start {
                continue;
            }
            if p.byte_range.start >= byte_range.end {
                break;
            }
            out.push(i);
        }
        out
    }

    fn rebuild_paragraphs(&mut self) {
        let revision = self.revision;
        self.paragraphs.clear();
        let total = self.rope.len_bytes();
        if total == 0 {
            self.paragraphs.push(ParagraphMeta {
                byte_range: 0..0,
                revision,
            });
            return;
        }
        let s = self.rope.to_string();
        let mut start = 0;
        for (i, ch) in s.char_indices() {
            if ch == '\n' {
                let end = i + ch.len_utf8();
                self.paragraphs.push(ParagraphMeta {
                    byte_range: start..end,
                    revision,
                });
                start = end;
            }
        }
        if start < total {
            self.paragraphs.push(ParagraphMeta {
                byte_range: start..total,
                revision,
            });
        }
    }

    fn rebuild_paragraphs_after_edit(&mut self, touched_before: Vec<usize>) {
        let before = std::mem::take(&mut self.paragraphs);
        self.rebuild_paragraphs();
        let rev = self.revision;
        for (i, p) in self.paragraphs.iter_mut().enumerate() {
            let touched = touched_before.contains(&i)
                || before
                    .get(i)
                    .is_none_or(|prev| prev.byte_range != p.byte_range);
            if touched {
                p.revision = rev;
            } else {
                p.revision = before[i].revision;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_string_splits_paragraphs() {
        let b = TextBuffer::from_string("hello\nworld\n!");
        assert_eq!(b.paragraphs().len(), 3);
        assert_eq!(b.paragraph_text(0).as_deref(), Some("hello\n"));
        assert_eq!(b.paragraph_text(1).as_deref(), Some("world\n"));
        assert_eq!(b.paragraph_text(2).as_deref(), Some("!"));
    }

    #[test]
    fn edit_bumps_revision_and_touches_only_affected_paragraphs() {
        let mut b = TextBuffer::from_string("para_one\npara_two\npara_three\n");
        let rev0 = b.revision();
        let revs_before: Vec<u64> = b.paragraphs().iter().map(|p| p.revision).collect();

        // Edit dans le paragraphe #1 (para_two).
        let p1 = b.paragraphs()[1].byte_range.clone();
        let mid = p1.start + 2;
        b.edit(mid..mid, "X");

        assert!(b.revision() > rev0);
        let revs_after: Vec<u64> = b.paragraphs().iter().map(|p| p.revision).collect();
        // Paragraph 0 revision must be unchanged.
        assert_eq!(revs_before[0], revs_after[0]);
        // Le paragraphe 1 doit avoir une nouvelle revision.
        assert_ne!(revs_before[1], revs_after[1]);
    }

    #[test]
    fn paragraph_at_returns_index_of_containing_paragraph() {
        let b = TextBuffer::from_string("aaaa\nbbbb\ncccc");
        assert_eq!(b.paragraph_at(0), 0);
        assert_eq!(b.paragraph_at(4), 0);
        assert_eq!(b.paragraph_at(5), 1);
        assert_eq!(b.paragraph_at(9), 1);
        assert_eq!(b.paragraph_at(11), 2);
    }

    #[test]
    fn paragraphs_in_byte_range_handles_cross_boundary() {
        let b = TextBuffer::from_string("aa\nbb\ncc");
        // "aa\n" = 0..3, "bb\n" = 3..6, "cc" = 6..8.
        let inter = b.paragraphs_in_byte_range(2..6);
        assert_eq!(inter, vec![0, 1]);
        let inter_all = b.paragraphs_in_byte_range(0..8);
        assert_eq!(inter_all, vec![0, 1, 2]);
    }
}
