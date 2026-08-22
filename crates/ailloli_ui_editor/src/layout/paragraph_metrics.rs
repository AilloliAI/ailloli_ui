//! Cached paragraph dimensions, revision metadata, and prefix-sum lookup.

use std::collections::HashMap;
use std::ops::Range;

use super::layout_cache::LayoutCacheKey;

/// Cached metrics for one logical paragraph under one layout configuration.
///
/// Width and height are logical pixels, `line_count` counts shaped visual lines,
/// and `local_version` mirrors the source paragraph revision.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::layout::ParagraphMetrics;
/// let metrics = ParagraphMetrics { width: 80.0, height: 36.0, line_count: 2, local_version: 7 };
/// assert_eq!(metrics.line_count, 2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParagraphMetrics {
    /// Maximum shaped line width in logical pixels.
    pub width: f32,
    /// Complete paragraph visual height in logical pixels.
    pub height: f32,
    /// Number of shaped visual lines, normally at least one.
    pub line_count: usize,
    /// Source paragraph revision from which these metrics were computed.
    pub local_version: u64,
}

/// Aggregate metadata from the most recent layout pass.
///
/// A first pass has no prior revision set and therefore reports
/// `dirty_range == None`. Later changes use a half-open paragraph-index range;
/// a changed paragraph count marks `0..paragraph_count` dirty.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_editor::{layout::{build_visible_text_layout, LayoutCache, ParagraphMetricsCache}, EditorConfig, EditorStyle, EditorViewport};
/// use ailloli_ui_text::{TextBuffer, TextEditState, TextSystem};
/// let buffer = TextBuffer::from_string("a\nb");
/// let viewport = EditorViewport::new(Rect::new(0.0, 0.0, 100.0, 60.0), EditorConfig::default(), &TextEditState::new());
/// let (mut layouts, mut metrics, mut text_system) = (LayoutCache::default(), ParagraphMetricsCache::default(), TextSystem::new());
/// build_visible_text_layout(&buffer, viewport, EditorStyle::default(), &mut layouts, &mut metrics, &mut text_system);
/// assert_eq!(metrics.metadata().unwrap().paragraph_count, 2);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphMetricsMetadata {
    /// Number of logical paragraphs observed in the last pass.
    pub paragraph_count: usize,
    /// Smallest half-open range whose stored revisions changed, if any.
    pub dirty_range: Option<Range<usize>>,
    /// Maximum paragraph revision, or zero for no paragraphs.
    pub local_version: u64,
    /// Sum of visible paragraph heights in logical pixels.
    pub total_height: f32,
    /// Maximum visible paragraph width in logical pixels.
    pub max_width: f32,
    /// Per-paragraph revisions retained for the next difference calculation.
    revisions: Vec<u64>,
}

/// Cache hit/miss counts for one visible-layout build.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::layout::ParagraphMetricsStats;
/// let stats = ParagraphMetricsStats::default();
/// assert_eq!((stats.hits, stats.misses), (0, 0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParagraphMetricsStats {
    /// Paragraph metric entries reused from the cache.
    pub hits: usize,
    /// Paragraph metric entries computed and inserted.
    pub misses: usize,
}

/// Per-engine paragraph metrics cache.
///
/// Metrics are indexed by the same geometry key as layouts. Aggregate metadata
/// is absent until a visible-layout build completes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::layout::ParagraphMetricsCache;
/// let cache = ParagraphMetricsCache::default();
/// assert!(cache.metadata().is_none());
/// ```
#[derive(Debug, Clone, Default)]
pub struct ParagraphMetricsCache {
    /// Per-layout-key paragraph metrics.
    metrics: HashMap<LayoutCacheKey, ParagraphMetrics>,
    /// Aggregate information from the last completed build.
    metadata: Option<ParagraphMetricsMetadata>,
}

/// Resolves, updates, and invalidates paragraph metrics.
impl ParagraphMetricsCache {
    /// Returns a copied metrics entry for an internal layout key.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_editor::{layout::{build_visible_text_layout, LayoutCache, ParagraphMetricsCache}, EditorConfig, EditorStyle, EditorViewport};
    /// use ailloli_ui_text::{TextBuffer, TextEditState, TextSystem};
    /// let buffer = TextBuffer::from_string("cached");
    /// let viewport = EditorViewport::new(Rect::new(0.0, 0.0, 100.0, 50.0), EditorConfig::default(), &TextEditState::new());
    /// let (mut layouts, mut metrics, mut system) = (LayoutCache::default(), ParagraphMetricsCache::default(), TextSystem::new());
    /// build_visible_text_layout(&buffer, viewport, EditorStyle::default(), &mut layouts, &mut metrics, &mut system);
    /// let second = build_visible_text_layout(&buffer, viewport, EditorStyle::default(), &mut layouts, &mut metrics, &mut system);
    /// assert!(second.metrics_stats.hits >= 1);
    /// ```
    pub(crate) fn get(&self, key: &LayoutCacheKey) -> Option<ParagraphMetrics> {
        self.metrics.get(key).copied()
    }

    /// Inserts or replaces metrics for an internal layout key.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::layout::ParagraphMetricsCache;
    /// let cache = ParagraphMetricsCache::default();
    /// assert!(cache.metadata().is_none()); // public layout builders populate entries
    /// ```
    pub(crate) fn insert(&mut self, key: LayoutCacheKey, metrics: ParagraphMetrics) {
        self.metrics.insert(key, metrics);
    }

    /// Replaces aggregate metadata and computes its revision difference.
    ///
    /// `revisions` should yield exactly `paragraph_count` entries, but the
    /// method does not enforce that invariant. Heights and widths are stored
    /// verbatim. `local_version` becomes the maximum revision or zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_editor::{layout::{build_visible_text_layout, LayoutCache, ParagraphMetricsCache}, EditorConfig, EditorStyle, EditorViewport};
    /// use ailloli_ui_text::{TextBuffer, TextEditState, TextSystem};
    /// let buffer = TextBuffer::from_string("one\ntwo");
    /// let viewport = EditorViewport::new(Rect::new(0.0, 0.0, 120.0, 60.0), EditorConfig::default(), &TextEditState::new());
    /// let (mut layouts, mut metrics, mut system) = (LayoutCache::default(), ParagraphMetricsCache::default(), TextSystem::new());
    /// let visible = build_visible_text_layout(&buffer, viewport, EditorStyle::default(), &mut layouts, &mut metrics, &mut system);
    /// assert_eq!(metrics.metadata().unwrap().total_height, visible.content_size.h);
    /// ```
    pub(crate) fn update_metadata(
        &mut self,
        paragraph_count: usize,
        revisions: impl Iterator<Item = u64>,
        total_height: f32,
        max_width: f32,
    ) {
        let revisions: Vec<_> = revisions.collect();
        let dirty_range = self.metadata.as_ref().and_then(|previous| {
            if previous.paragraph_count != paragraph_count {
                return Some(0..paragraph_count);
            }
            let mut first = None;
            let mut last = 0;
            for (idx, revision) in revisions.iter().enumerate() {
                if previous.revisions.get(idx).copied() != Some(*revision) {
                    first.get_or_insert(idx);
                    last = idx + 1;
                }
            }
            first.map(|start| start..last)
        });
        let local_version = revisions.iter().copied().max().unwrap_or(0);
        self.metadata = Some(ParagraphMetricsMetadata {
            paragraph_count,
            dirty_range,
            local_version,
            total_height,
            max_width,
            revisions,
        });
    }

    /// Returns aggregate metadata from the last completed layout build.
    ///
    /// Returns `None` before the first build and after [`Self::clear`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::layout::ParagraphMetricsCache;
    /// assert!(ParagraphMetricsCache::default().metadata().is_none());
    /// ```
    pub fn metadata(&self) -> Option<&ParagraphMetricsMetadata> {
        self.metadata.as_ref()
    }

    /// Removes all per-paragraph metrics and aggregate metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::layout::ParagraphMetricsCache;
    /// let mut cache = ParagraphMetricsCache::default();
    /// cache.clear();
    /// assert!(cache.metadata().is_none());
    /// ```
    pub fn clear(&mut self) {
        self.metrics.clear();
        self.metadata = None;
    }
}

/// Prefix-sum index used to map vertical offsets to paragraph indices.
///
/// [`FenwickTree::lower_bound`] assumes finite, non-negative values so prefix
/// sums are monotonic; construction does not validate this invariant.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::layout::FenwickTree;
/// let tree = FenwickTree::from_values(&[10.0, 20.0, 30.0]);
/// assert_eq!(tree.prefix_sum(2), 30.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct FenwickTree {
    /// Original values, retained for length and diagnostics.
    values: Vec<f32>,
    /// One-based Fenwick partial sums with a zero sentinel at index zero.
    tree: Vec<f32>,
}

/// Builds and queries prefix sums.
impl FenwickTree {
    /// Builds a tree in `O(n log n)` from copied values.
    ///
    /// An empty slice is valid and produces a zero-length index.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::layout::FenwickTree;
    /// assert_eq!(FenwickTree::from_values(&[]).prefix_sum(9), 0.0);
    /// assert_eq!(FenwickTree::from_values(&[2.0, 3.0]).prefix_sum(2), 5.0);
    /// ```
    pub fn from_values(values: &[f32]) -> Self {
        let mut tree = Self {
            values: values.to_vec(),
            tree: vec![0.0; values.len() + 1],
        };
        for (idx, value) in values.iter().copied().enumerate() {
            tree.add(idx, value);
        }
        tree
    }

    /// Returns the sum of values before `end`.
    ///
    /// `end` is clamped to the number of values. Queries take `O(log n)` time.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::layout::FenwickTree;
    /// let tree = FenwickTree::from_values(&[2.0, 3.0, 5.0]);
    /// assert_eq!(tree.prefix_sum(0), 0.0);
    /// assert_eq!(tree.prefix_sum(usize::MAX), 10.0);
    /// ```
    pub fn prefix_sum(&self, end: usize) -> f32 {
        let mut idx = end.min(self.values.len());
        let mut sum = 0.0;
        while idx > 0 {
            sum += self.tree[idx];
            idx &= idx - 1;
        }
        sum
    }

    /// Locates the zero-based value whose cumulative end reaches `target`.
    ///
    /// Non-positive targets return zero. A target greater than the total sum
    /// returns the number of values. Equality belongs to the preceding value,
    /// which matches viewport offsets on a paragraph's lower edge. Queries take
    /// `O(log n)` time and require non-negative finite values for meaningful
    /// ordering.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::layout::FenwickTree;
    /// let tree = FenwickTree::from_values(&[10.0, 20.0, 30.0]);
    /// assert_eq!(tree.lower_bound(0.0), 0);
    /// assert_eq!(tree.lower_bound(10.0), 0);
    /// assert_eq!(tree.lower_bound(10.5), 1);
    /// assert_eq!(tree.lower_bound(60.5), 3);
    /// ```
    pub fn lower_bound(&self, target: f32) -> usize {
        if target <= 0.0 {
            return 0;
        }
        let mut idx = 0usize;
        let mut bit = 1usize;
        while bit < self.values.len() {
            bit <<= 1;
        }
        let mut sum = 0.0;
        while bit > 0 {
            let next = idx + bit;
            if next <= self.values.len() && sum + self.tree[next] < target {
                sum += self.tree[next];
                idx = next;
            }
            bit >>= 1;
        }
        idx.min(self.values.len())
    }

    /// Adds a delta to one value and its covering Fenwick nodes.
    fn add(&mut self, idx: usize, delta: f32) {
        let mut tree_idx = idx + 1;
        while tree_idx < self.tree.len() {
            self.tree[tree_idx] += delta;
            tree_idx += tree_idx & (!tree_idx + 1);
        }
    }
}
