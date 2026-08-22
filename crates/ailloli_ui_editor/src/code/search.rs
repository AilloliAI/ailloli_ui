//! Cached, non-overlapping UTF-8 byte-range text search.

use std::ops::Range;

use crate::code::DocumentVersion;

/// Query used by the code editor search engine.
///
/// Matching is case-insensitive and substring-based by default. Case folding
/// and whole-word boundaries intentionally use ASCII semantics.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::SearchQuery;
///
/// let query = SearchQuery::new("name").case_sensitive(true).whole_word(true);
/// assert_eq!(query.text, "name");
/// assert!(query.case_sensitive && query.whole_word);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SearchQuery {
    /// Search needle; an empty string matches nothing.
    pub text: String,
    /// Whether ASCII letter case must match exactly.
    pub case_sensitive: bool,
    /// Whether adjacent ASCII letters, digits, or underscores reject a match.
    pub whole_word: bool,
}

/// Builds search queries.
impl SearchQuery {
    /// Creates a case-insensitive substring query.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::SearchQuery;
    ///
    /// let query = SearchQuery::new("TODO");
    /// assert!(!query.case_sensitive);
    /// assert!(!query.whole_word);
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            case_sensitive: false,
            whole_word: false,
        }
    }

    /// Sets exact ASCII case matching and returns the query.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::SearchQuery;
    ///
    /// assert!(SearchQuery::new("x").case_sensitive(true).case_sensitive);
    /// ```
    pub fn case_sensitive(mut self, case_sensitive: bool) -> Self {
        self.case_sensitive = case_sensitive;
        self
    }

    /// Sets ASCII whole-word filtering and returns the query.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::SearchQuery;
    ///
    /// assert!(SearchQuery::new("let").whole_word(true).whole_word);
    /// ```
    pub fn whole_word(mut self, whole_word: bool) -> Self {
        self.whole_word = whole_word;
        self
    }
}

/// One non-overlapping search result expressed as a UTF-8 byte range.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::SearchMatch;
///
/// let found = SearchMatch { range: 2..5 };
/// assert_eq!(found.range.len(), 3);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// Half-open UTF-8 byte range in the searched text.
    pub range: Range<usize>,
}

/// Mutable search results, selection, and cache state.
///
/// The cache key contains only the query and [`DocumentVersion`]. Callers must
/// change the version whenever the searched text changes or explicitly call
/// [`SearchState::invalidate_cache`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{DocumentVersion, SearchQuery, SearchState};
///
/// let mut state = SearchState::new(SearchQuery::new("go"));
/// assert!(state.refresh("go stop go", DocumentVersion(1)));
/// assert_eq!(state.matches.len(), 2);
/// assert_eq!(state.active_index, Some(0));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchState {
    /// Query used by the next refresh.
    pub query: SearchQuery,
    /// Ordered, non-overlapping matches from the last refresh.
    pub matches: Vec<SearchMatch>,
    /// Selected match index, or `None` when no match is selected.
    pub active_index: Option<usize>,
    /// Query/version pair used to detect redundant refreshes.
    cache_key: Option<SearchCacheKey>,
}

/// Creates an empty search state with an empty query.
impl Default for SearchState {
    /// Creates an empty, unrefreshed state with the default empty query.
    fn default() -> Self {
        Self {
            query: SearchQuery::new(""),
            matches: Vec::new(),
            active_index: None,
            cache_key: None,
        }
    }
}

/// Updates search results and active-match selection.
impl SearchState {
    /// Creates an unrefreshed state for `query`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{SearchQuery, SearchState};
    ///
    /// let state = SearchState::new(SearchQuery::new("needle"));
    /// assert_eq!(state.query.text, "needle");
    /// assert!(state.matches.is_empty() && state.active_index.is_none());
    /// ```
    pub fn new(query: SearchQuery) -> Self {
        Self {
            query,
            ..Self::default()
        }
    }

    /// Replaces the query and clears cached results when it differs.
    ///
    /// Supplying an equal query is a no-op and preserves both matches and the
    /// active index.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{DocumentVersion, SearchQuery, SearchState};
    ///
    /// let mut state = SearchState::new(SearchQuery::new("old"));
    /// state.refresh("old", DocumentVersion(1));
    /// state.set_query(SearchQuery::new("new"));
    /// assert!(state.matches.is_empty() && state.active_index.is_none());
    /// ```
    pub fn set_query(&mut self, query: SearchQuery) {
        if self.query != query {
            self.query = query;
            self.matches.clear();
            self.active_index = None;
            self.cache_key = None;
        }
    }

    /// Recomputes results unless the query/version cache key is unchanged.
    ///
    /// Returns `true` after recomputation and `false` for a cache hit. An
    /// existing active index is clamped to the new final match; non-empty fresh
    /// results otherwise select index zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{DocumentVersion, SearchQuery, SearchState};
    ///
    /// let mut state = SearchState::new(SearchQuery::new("a"));
    /// assert!(state.refresh("a a", DocumentVersion(4)));
    /// assert!(!state.refresh("different text", DocumentVersion(4)));
    /// assert_eq!(state.matches.len(), 2);
    /// ```
    pub fn refresh(&mut self, text: &str, version: DocumentVersion) -> bool {
        let key = SearchCacheKey {
            version,
            query: self.query.clone(),
        };
        if self.cache_key.as_ref() == Some(&key) {
            return false;
        }
        self.matches = find_matches(text, &self.query);
        self.active_index = if self.matches.is_empty() {
            None
        } else {
            Some(self.active_index.unwrap_or(0).min(self.matches.len() - 1))
        };
        self.cache_key = Some(key);
        true
    }

    /// Selects and returns the following match, wrapping at the end.
    ///
    /// Because a successful refresh selects index zero, the first call after a
    /// multi-match refresh advances to index one. With no results, this returns
    /// `None` and clears the active index.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{DocumentVersion, SearchQuery, SearchState};
    ///
    /// let mut state = SearchState::new(SearchQuery::new("x"));
    /// state.refresh("x x", DocumentVersion(1));
    /// assert_eq!(state.next_match().map(|m| m.range.clone()), Some(2..3));
    /// assert_eq!(state.next_match().map(|m| m.range.clone()), Some(0..1));
    /// ```
    pub fn next_match(&mut self) -> Option<&SearchMatch> {
        if self.matches.is_empty() {
            self.active_index = None;
            return None;
        }
        let next = self
            .active_index
            .map(|idx| (idx + 1) % self.matches.len())
            .unwrap_or(0);
        self.active_index = Some(next);
        self.matches.get(next)
    }

    /// Selects and returns the preceding match, wrapping at the start.
    ///
    /// With no results, this returns `None` and clears the active index.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{DocumentVersion, SearchQuery, SearchState};
    ///
    /// let mut state = SearchState::new(SearchQuery::new("x"));
    /// state.refresh("x x", DocumentVersion(1));
    /// assert_eq!(state.previous_match().map(|m| m.range.clone()), Some(2..3));
    /// ```
    pub fn previous_match(&mut self) -> Option<&SearchMatch> {
        if self.matches.is_empty() {
            self.active_index = None;
            return None;
        }
        let previous = self
            .active_index
            .map(|idx| {
                if idx == 0 {
                    self.matches.len() - 1
                } else {
                    idx - 1
                }
            })
            .unwrap_or(0);
        self.active_index = Some(previous);
        self.matches.get(previous)
    }

    /// Selects an in-range match index, otherwise clears the selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{DocumentVersion, SearchQuery, SearchState};
    ///
    /// let mut state = SearchState::new(SearchQuery::new("x"));
    /// state.refresh("x x", DocumentVersion(1));
    /// state.set_active_index(Some(9));
    /// assert_eq!(state.active_index, None);
    /// state.set_active_index(Some(1));
    /// assert_eq!(state.active_index, Some(1));
    /// ```
    pub fn set_active_index(&mut self, active_index: Option<usize>) {
        self.active_index = active_index.filter(|idx| *idx < self.matches.len());
    }

    /// Restores the default empty query, results, selection, and cache.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{DocumentVersion, SearchQuery, SearchState};
    ///
    /// let mut state = SearchState::new(SearchQuery::new("x"));
    /// state.refresh("x", DocumentVersion(1));
    /// state.clear();
    /// assert!(state.query.text.is_empty() && state.matches.is_empty());
    /// ```
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Forces the next [`SearchState::refresh`] to recompute.
    ///
    /// Existing results and selection remain available until that refresh.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{DocumentVersion, SearchQuery, SearchState};
    ///
    /// let mut state = SearchState::new(SearchQuery::new("x"));
    /// state.refresh("x", DocumentVersion(1));
    /// state.invalidate_cache();
    /// assert!(state.refresh("x x", DocumentVersion(1)));
    /// assert_eq!(state.matches.len(), 2);
    /// ```
    pub fn invalidate_cache(&mut self) {
        self.cache_key = None;
    }
}

/// Internal key for reusing a completed search.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchCacheKey {
    /// Caller-owned version of the searched document.
    version: DocumentVersion,
    /// Query whose results are cached.
    query: SearchQuery,
}

/// Finds ordered, non-overlapping matches for a query.
///
/// Ranges are UTF-8 byte offsets. An empty needle returns no matches;
/// case-insensitive matching folds ASCII only; whole-word mode treats ASCII
/// letters, digits, and `_` as word characters.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{code::find_matches, SearchQuery};
///
/// let query = SearchQuery::new("cat").whole_word(true);
/// let ranges: Vec<_> = find_matches("Cat scatter cat", &query)
///     .into_iter()
///     .map(|m| m.range)
///     .collect();
/// assert_eq!(ranges, [0..3, 12..15]);
/// assert!(find_matches("anything", &SearchQuery::new("")).is_empty());
/// ```
pub fn find_matches(text: &str, query: &SearchQuery) -> Vec<SearchMatch> {
    if query.text.is_empty() {
        return Vec::new();
    }
    let matches = if query.case_sensitive {
        find_matches_case_sensitive(text, &query.text)
    } else {
        find_matches_case_insensitive(text, &query.text)
    };
    if query.whole_word {
        matches
            .into_iter()
            .filter(|search_match| is_whole_word_match(text, search_match.range.clone()))
            .collect()
    } else {
        matches
    }
}

/// Finds exact-case, non-overlapping substring matches.
fn find_matches_case_sensitive(text: &str, needle: &str) -> Vec<SearchMatch> {
    let mut matches = Vec::new();
    let mut start = 0;
    while let Some(pos) = text[start..].find(needle) {
        let lo = start + pos;
        let hi = lo + needle.len();
        matches.push(SearchMatch { range: lo..hi });
        start = hi;
    }
    matches
}

/// Tests ASCII word boundaries around a valid match range.
fn is_whole_word_match(text: &str, range: Range<usize>) -> bool {
    let before = text[..range.start].chars().next_back();
    let after = text[range.end..].chars().next();
    !before.is_some_and(is_ascii_word_char) && !after.is_some_and(is_ascii_word_char)
}

/// Returns whether a character participates in an ASCII identifier-like word.
fn is_ascii_word_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

/// Finds ASCII-case-insensitive, non-overlapping substring matches.
fn find_matches_case_insensitive(text: &str, needle: &str) -> Vec<SearchMatch> {
    let mut matches = Vec::new();
    let mut start = 0;
    while start <= text.len().saturating_sub(needle.len()) {
        let mut found = None;
        for (relative, _) in text[start..].char_indices() {
            let lo = start + relative;
            let hi = lo + needle.len();
            if hi <= text.len()
                && text.is_char_boundary(hi)
                && text[lo..hi].eq_ignore_ascii_case(needle)
            {
                found = Some(lo);
                break;
            }
        }
        let Some(lo) = found else {
            break;
        };
        let hi = lo + needle.len();
        matches.push(SearchMatch { range: lo..hi });
        start = hi;
    }
    matches
}
