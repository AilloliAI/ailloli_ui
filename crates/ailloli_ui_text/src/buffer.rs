//! Rope-backed text buffer with per-paragraph revision tracking.
//!
//! - `ropey` enables O(log n) edits and fast paragraph splitting.
//! - Each paragraph (separated by `\n`) has its own `revision`; edits bump only
//!   paragraphs intersecting the changed byte range.
//! - Layout caches can invalidate per paragraph instead of flushing entirely.

use core::ops::Range;

use ropey::Rope;

/// Metadata for one paragraph: UTF-8 byte range and revision counter.
///
/// Paragraph ranges are half-open. A terminating newline belongs to the
/// paragraph before it. A nonempty buffer ending in `\n` has no extra empty
/// paragraph, while an entirely empty buffer has one `0..0` paragraph.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::ParagraphMeta;
/// let meta = ParagraphMeta { byte_range: 2..8, revision: 3 };
/// assert_eq!(meta.byte_range, 2..8);
/// assert_eq!(meta.revision, 3);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParagraphMeta {
    /// Half-open UTF-8 byte range in the complete buffer.
    pub byte_range: Range<usize>,
    /// Wrapping revision assigned when this indexed paragraph was rebuilt.
    pub revision: u64,
}

/// Mutable text storage with a paragraph index (logical line = `\n`-separated).
///
/// Text is stored in a [`Rope`], while paragraph metadata is rebuilt after each
/// edit. Public offsets are UTF-8 bytes, not character or grapheme indices.
/// Cloning copies the current rope handle and metadata as independent mutable
/// values.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::TextBuffer;
/// let buffer = TextBuffer::from_string("first\nsecond");
/// assert_eq!(buffer.len_bytes(), 12);
/// assert_eq!(buffer.paragraphs().len(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct TextBuffer {
    /// Ropey storage indexed by UTF-8 byte and Unicode scalar positions.
    rope: Rope,
    /// Global wrapping edit counter.
    revision: u64,
    /// Current newline-delimited paragraph index.
    paragraphs: Vec<ParagraphMeta>,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl TextBuffer {
    /// Creates an empty buffer at revision zero.
    ///
    /// The paragraph index contains one empty `0..0` entry so methods such as
    /// [`Self::paragraph_at`] always have a paragraph to return.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextBuffer;
    /// let buffer = TextBuffer::new();
    /// assert_eq!(buffer.revision(), 0);
    /// assert_eq!(buffer.paragraphs()[0].byte_range, 0..0);
    /// ```
    pub fn new() -> Self {
        Self::from_string(String::new())
    }

    /// Creates a revision-zero buffer from owned or borrowed UTF-8 text.
    ///
    /// Newline bytes terminate and belong to their preceding paragraph. A
    /// trailing newline does not create an additional empty paragraph.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextBuffer;
    /// let buffer = TextBuffer::from_string("a\nb\n");
    /// assert_eq!(buffer.paragraphs().len(), 2);
    /// assert_eq!(buffer.paragraph_text(0).as_deref(), Some("a\n"));
    /// ```
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

    /// Returns the global wrapping edit counter.
    ///
    /// Every call to [`Self::edit`] increments the counter, including edits that
    /// leave text unchanged. It starts at zero and wraps from `u64::MAX` to zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextBuffer;
    /// let mut buffer = TextBuffer::new();
    /// buffer.edit(0..0, "");
    /// assert_eq!(buffer.revision(), 1);
    /// ```
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the current UTF-8 byte length.
    ///
    /// This differs from Unicode scalar and grapheme counts for non-ASCII text.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextBuffer;
    /// assert_eq!(TextBuffer::from_string("é").len_bytes(), 2);
    /// ```
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    /// Copies the entire rope into a contiguous [`String`].
    ///
    /// Despite the method name, the result is owned because a Rope is not
    /// necessarily contiguous. This operation is O(N) in the text length.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextBuffer;
    /// let text: String = TextBuffer::from_string("hello").as_str();
    /// assert_eq!(text, "hello");
    /// ```
    pub fn as_str(&self) -> String {
        self.rope.to_string()
    }

    /// Borrows current paragraph metadata in document order.
    ///
    /// The slice is never empty for buffers built through public constructors.
    /// It is invalidated by the next mutable edit.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextBuffer;
    /// let buffer = TextBuffer::from_string("a\nb");
    /// assert_eq!(buffer.paragraphs()[0].byte_range, 0..2);
    /// assert_eq!(buffer.paragraphs()[1].byte_range, 2..3);
    /// ```
    pub fn paragraphs(&self) -> &[ParagraphMeta] {
        &self.paragraphs
    }

    /// Copies one indexed paragraph into a [`String`].
    ///
    /// The returned text includes its terminating newline when present.
    /// `None` means `idx` is outside [`Self::paragraphs`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextBuffer;
    /// let buffer = TextBuffer::from_string("a\nb");
    /// assert_eq!(buffer.paragraph_text(0).as_deref(), Some("a\n"));
    /// assert_eq!(buffer.paragraph_text(2), None);
    /// ```
    pub fn paragraph_text(&self, idx: usize) -> Option<String> {
        let p = self.paragraphs.get(idx)?;
        let slice = self.rope.byte_slice(p.byte_range.start..p.byte_range.end);
        Some(slice.to_string())
    }

    /// Returns one paragraph's revision, or `None` for an invalid index.
    ///
    /// Paragraph revisions initially equal zero. After an edit they identify
    /// index entries considered touched or whose byte range changed; they are
    /// not guaranteed to advance for a global no-op.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextBuffer;
    /// let buffer = TextBuffer::from_string("a\nb");
    /// assert_eq!(buffer.revision_of_paragraph(0), Some(0));
    /// assert_eq!(buffer.revision_of_paragraph(9), None);
    /// ```
    pub fn revision_of_paragraph(&self, idx: usize) -> Option<u64> {
        self.paragraphs.get(idx).map(|p| p.revision)
    }

    /// Index of the paragraph containing byte `b` (clamped to the last paragraph).
    ///
    /// Newline bytes belong to the paragraph they terminate. An index at or
    /// beyond `len_bytes()` maps to the last paragraph. An empty buffer returns
    /// zero for every input.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextBuffer;
    /// let buffer = TextBuffer::from_string("a\nb");
    /// assert_eq!(buffer.paragraph_at(1), 0); // newline
    /// assert_eq!(buffer.paragraph_at(2), 1);
    /// assert_eq!(buffer.paragraph_at(usize::MAX), 1);
    /// ```
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
    ///
    /// Each endpoint is first clamped to `len_bytes()`. If the range is
    /// reversed, its start is lowered to the clamped end, producing an insertion
    /// rather than swapping endpoints. Ropey maps an endpoint inside a multibyte
    /// scalar to that scalar's character index; callers should therefore pass
    /// valid UTF-8 boundaries when exact replacement is required. The global
    /// revision wraps on overflow and advances even for a no-op.
    ///
    /// Paragraph metadata is rebuilt in O(N) text time. Entries intersecting the
    /// old nonempty range or whose same-index byte range changed receive the new
    /// revision; index shifts can consequently invalidate later paragraphs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextBuffer;
    /// let mut buffer = TextBuffer::from_string("abc");
    /// buffer.edit(1..2, "X");
    /// assert_eq!(buffer.as_str(), "aXc");
    /// assert_eq!(buffer.revision(), 1);
    /// ```
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

    /// Returns paragraph indices whose half-open byte ranges intersect `byte_range`.
    ///
    /// The input is not clamped or reordered. Empty or reversed ranges return an
    /// empty vector. Because a newline belongs to its preceding paragraph, a
    /// range containing only that newline intersects that preceding paragraph.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextBuffer;
    /// let buffer = TextBuffer::from_string("aa\nbb\ncc");
    /// assert_eq!(buffer.paragraphs_in_byte_range(2..6), [0, 1]);
    /// assert!(buffer.paragraphs_in_byte_range(3..3).is_empty());
    /// ```
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

    /// Rebuilds all paragraph ranges at the current global revision.
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

    /// Rebuilds metadata and preserves revisions for same-index untouched ranges.
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
