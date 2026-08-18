use std::collections::HashMap;
use std::ops::Range;

use super::layout_cache::LayoutCacheKey;

/// Cached metrics for one logical paragraph under one layout configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParagraphMetrics {
    pub width: f32,
    pub height: f32,
    pub line_count: usize,
    pub local_version: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphMetricsMetadata {
    pub paragraph_count: usize,
    pub dirty_range: Option<Range<usize>>,
    pub local_version: u64,
    pub total_height: f32,
    pub max_width: f32,
    revisions: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParagraphMetricsStats {
    pub hits: usize,
    pub misses: usize,
}

/// Per-engine paragraph metrics cache.
#[derive(Debug, Clone, Default)]
pub struct ParagraphMetricsCache {
    metrics: HashMap<LayoutCacheKey, ParagraphMetrics>,
    metadata: Option<ParagraphMetricsMetadata>,
}

impl ParagraphMetricsCache {
    pub(crate) fn get(&self, key: &LayoutCacheKey) -> Option<ParagraphMetrics> {
        self.metrics.get(key).copied()
    }

    pub(crate) fn insert(&mut self, key: LayoutCacheKey, metrics: ParagraphMetrics) {
        self.metrics.insert(key, metrics);
    }

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

    pub fn metadata(&self) -> Option<&ParagraphMetricsMetadata> {
        self.metadata.as_ref()
    }

    pub fn clear(&mut self) {
        self.metrics.clear();
        self.metadata = None;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FenwickTree {
    values: Vec<f32>,
    tree: Vec<f32>,
}

impl FenwickTree {
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

    pub fn prefix_sum(&self, end: usize) -> f32 {
        let mut idx = end.min(self.values.len());
        let mut sum = 0.0;
        while idx > 0 {
            sum += self.tree[idx];
            idx &= idx - 1;
        }
        sum
    }

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

    fn add(&mut self, idx: usize, delta: f32) {
        let mut tree_idx = idx + 1;
        while tree_idx < self.tree.len() {
            self.tree[tree_idx] += delta;
            tree_idx += tree_idx & (!tree_idx + 1);
        }
    }
}
