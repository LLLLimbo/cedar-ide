//! Case-search v1 is an escaped literal, never user regex syntax. Its pinned
//! regex-syntax 0.8.11 table implements Unicode 16.0.0 default simple C+S folding.
//! No normalization, full-fold expansion or locale/Turkic tailoring is applied.
use crate::error;
use cedar_protocol::{RemoteError, MAX_SCOPED_SEARCH_QUERY_BYTES};
use regex::{Regex, RegexBuilder};
use std::ops::Range;

const MAX_PATTERN_BYTES: usize = 8192;
// Approximate limits PER NFA and PER lazy-DFA cache, not aggregate matcher/RSS.
const NFA_SIZE_LIMIT: usize = 8 * 1024 * 1024;
const DFA_SIZE_LIMIT: usize = 2 * 1024 * 1024;
const NEST_LIMIT: u32 = 8;

pub(super) struct CaseMatcher(Regex);

impl CaseMatcher {
    pub(super) fn new(query: &str, case_sensitive: bool) -> Result<Self, RemoteError> {
        Self::build(query, case_sensitive, NFA_SIZE_LIMIT)
    }

    // The only nonconstant-cap entry point exists in unit-test builds.
    #[cfg(test)]
    pub(super) fn with_test_nfa_limit(
        query: &str,
        case_sensitive: bool,
        nfa_size_limit: usize,
    ) -> Result<Self, RemoteError> {
        Self::build(query, case_sensitive, nfa_size_limit)
    }

    fn build(
        query: &str,
        case_sensitive: bool,
        nfa_size_limit: usize,
    ) -> Result<Self, RemoteError> {
        if query.is_empty()
            || query.len() > MAX_SCOPED_SEARCH_QUERY_BYTES
            || query.contains(['\r', '\n', '\0'])
        {
            return Err(error(
                "invalid_scoped_search",
                "Invalid literal search query",
            ));
        }
        // Escaping adds at most one ASCII backslash per input byte. Check this
        // conservative bound before allocation, and the actual pattern after.
        let bound = query
            .len()
            .checked_mul(2)
            .filter(|bytes| *bytes <= MAX_PATTERN_BYTES)
            .ok_or_else(matcher_limit)?;
        let pattern = regex::escape(query);
        if pattern.len() > bound || pattern.len() > MAX_PATTERN_BYTES {
            return Err(matcher_limit());
        }
        RegexBuilder::new(&pattern)
            .unicode(true)
            .case_insensitive(!case_sensitive)
            .size_limit(nfa_size_limit)
            .dfa_size_limit(DFA_SIZE_LIMIT)
            .nest_limit(NEST_LIMIT)
            .build()
            .map(Self)
            .map_err(|failure| match failure {
                regex::Error::CompiledTooBig(_) => matcher_limit(),
                // Never put a syntax/debug error (which can contain the query)
                // into the response or logs. No fallback to another operation.
                _ => error(
                    "case_search_matcher_error",
                    "Cannot build literal search matcher",
                ),
            })
    }

    pub(super) fn find(&self, line: &str) -> Option<Range<usize>> {
        self.0.find(line).map(|found| found.range())
    }
}

fn matcher_limit() -> RemoteError {
    error(
        "case_search_matcher_limit",
        "Literal search matcher exceeds its resource limit",
    )
}

#[cfg(test)]
mod tests;
