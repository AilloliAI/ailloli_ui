use std::ops::Range;

use crate::code::DocumentVersion;

/// Query used by the code editor search engine.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SearchQuery {
    pub text: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

impl SearchQuery {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            case_sensitive: false,
            whole_word: false,
        }
    }

    pub fn case_sensitive(mut self, case_sensitive: bool) -> Self {
        self.case_sensitive = case_sensitive;
        self
    }

    pub fn whole_word(mut self, whole_word: bool) -> Self {
        self.whole_word = whole_word;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchState {
    pub query: SearchQuery,
    pub matches: Vec<SearchMatch>,
    pub active_index: Option<usize>,
    cache_key: Option<SearchCacheKey>,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: SearchQuery::new(""),
            matches: Vec::new(),
            active_index: None,
            cache_key: None,
        }
    }
}

impl SearchState {
    pub fn new(query: SearchQuery) -> Self {
        Self {
            query,
            ..Self::default()
        }
    }

    pub fn set_query(&mut self, query: SearchQuery) {
        if self.query != query {
            self.query = query;
            self.matches.clear();
            self.active_index = None;
            self.cache_key = None;
        }
    }

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

    pub fn set_active_index(&mut self, active_index: Option<usize>) {
        self.active_index = active_index.filter(|idx| *idx < self.matches.len());
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn invalidate_cache(&mut self) {
        self.cache_key = None;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchCacheKey {
    version: DocumentVersion,
    query: SearchQuery,
}

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

fn is_whole_word_match(text: &str, range: Range<usize>) -> bool {
    let before = text[..range.start].chars().next_back();
    let after = text[range.end..].chars().next();
    !before.is_some_and(is_ascii_word_char) && !after.is_some_and(is_ascii_word_char)
}

fn is_ascii_word_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

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
