//! Best-effort decoration of one accepted Project Text batch. These are display
//! accounting limits, not a regex instruction count or a wall-clock deadline.
use cedar_protocol::SearchMatch;
use eframe::egui;
use regex::RegexBuilder;
use std::ops::Range;

const QUERY_BYTES: usize = 64;
const SNIPPET_BYTES: usize = 2054;
const SCAN_BYTES: usize = 32 * 1024;
const FIND_CALLS: usize = 2048;
const VISIT_BYTES: usize = 512 * 1024;
const WORK_UNITS: usize = 8 * 1024 * 1024;
const ROW_RANGES: usize = 64;
const BATCH_RANGES: usize = 1024;

pub(crate) const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(83, 69, 34);

#[derive(Default)]
pub(super) struct Prepared {
    offsets: Vec<Range<usize>>,
    ranges: Vec<Range<usize>>,
    #[cfg(test)]
    pub(super) accounting: Accounting,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Accounting {
    #[cfg(test)]
    builds: usize,
    scanned: usize,
    calls: usize,
    visits: usize,
    work: usize,
}

impl Accounting {
    fn charge(&mut self, bytes: usize, scalars: usize) -> bool {
        let Some(calls) = self.calls.checked_add(1).filter(|n| *n <= FIND_CALLS) else {
            return false;
        };
        let Some(visits) = self.visits.checked_add(bytes).filter(|n| *n <= VISIT_BYTES) else {
            return false;
        };
        let Some(work) = bytes
            .checked_mul(scalars)
            .and_then(|n| self.work.checked_add(n))
            .filter(|n| *n <= WORK_UNITS)
        else {
            return false;
        };
        self.calls = calls;
        self.visits = visits;
        self.work = work;
        true
    }
}

impl Prepared {
    pub(super) fn prepare(query: &str, case_sensitive: bool, rows: &[SearchMatch]) -> Self {
        Self::with_nfa_limit(query, case_sensitive, rows, 8 * 1024 * 1024)
    }

    fn with_nfa_limit(
        query: &str,
        case_sensitive: bool,
        rows: &[SearchMatch],
        nfa_limit: usize,
    ) -> Self {
        let mut result = Self::default();
        if query.is_empty()
            || query.len() > QUERY_BYTES
            || query.len() > 4096
            || query.contains(['\r', '\n', '\0'])
            || rows.len() > 500
            || !rows
                .iter()
                .any(|row| !row.text.is_empty() && row.text.len() <= SNIPPET_BYTES)
        {
            return result;
        }
        let Some(bound) = query.len().checked_mul(2).filter(|n| *n <= 8192) else {
            return result;
        };
        let pattern = regex::escape(query);
        if pattern.len() > bound
            || pattern.len() > 8192
            || result.offsets.try_reserve_exact(rows.len()).is_err()
            || result.ranges.try_reserve_exact(BATCH_RANGES).is_err()
        {
            return Self::default();
        }
        result.offsets.resize(rows.len(), 0..0);
        let mut accounting = Accounting::default();
        #[cfg(test)]
        {
            accounting.builds = 1;
        }
        let matcher = RegexBuilder::new(&pattern)
            .unicode(true)
            .case_insensitive(!case_sensitive)
            .size_limit(nfa_limit)
            .dfa_size_limit(2 * 1024 * 1024)
            .nest_limit(8)
            .build();
        let Ok(matcher) = matcher else {
            #[cfg(test)]
            {
                result.accounting = accounting;
            }
            return result;
        };
        let scalars = query.chars().count();
        'rows: for (index, row) in rows.iter().enumerate() {
            let text = &row.text;
            if text.is_empty() || text.len() > SNIPPET_BYTES {
                continue;
            }
            let Some(scanned) = accounting
                .scanned
                .checked_add(text.len())
                .filter(|n| *n <= SCAN_BYTES)
            else {
                break;
            };
            if result.ranges.len() == BATCH_RANGES {
                break;
            }
            accounting.scanned = scanned;
            // The protocol does not identify synthetic crop markers. Conservatively
            // omit real edge ellipses too; middle ellipses retain literal meaning.
            let start = usize::from(text.starts_with('…')) * '…'.len_utf8();
            let end = text.len() - usize::from(text.ends_with('…')) * '…'.len_utf8();
            let Some(content) = text.get(start..end) else {
                continue;
            };
            let trimmed = text.trim();
            let trim_start = text.len() - text.trim_start().len();
            let trim_end = trim_start + trimmed.len();
            let mut temporary: [Range<usize>; ROW_RANGES + 1] = std::array::from_fn(|_| 0..0);
            let mut count = 0;
            let mut matches = matcher.find_iter(content);
            loop {
                // Charge the entire original row, even when a search starts later
                // or omits crop markers. This is conservative accounting only.
                if !accounting.charge(text.len(), scalars) {
                    break 'rows;
                }
                let Some(found) = matches.next() else {
                    break;
                };
                temporary[count] = start + found.start()..start + found.end();
                count += 1;
                if count > ROW_RANGES {
                    continue 'rows;
                }
            }
            // Trim-crossing matches are skipped whole, never shortened.
            let visible = temporary[..count]
                .iter()
                .filter(|span| span.start >= trim_start && span.end <= trim_end);
            let visible_count = visible.clone().count();
            if result
                .ranges
                .len()
                .checked_add(visible_count)
                .is_none_or(|n| n > BATCH_RANGES)
            {
                break;
            }
            let first = result.ranges.len();
            result.ranges.extend(visible.cloned());
            result.offsets[index] = first..result.ranges.len();
        }
        #[cfg(test)]
        {
            result.accounting = accounting;
        }
        result
    }

    #[cfg(test)]
    pub(super) fn counts(&self) -> [usize; 5] {
        [
            self.accounting.builds,
            self.accounting.scanned,
            self.accounting.calls,
            self.accounting.visits,
            self.accounting.work,
        ]
    }

    pub(super) fn row(&self, index: usize) -> &[Range<usize>] {
        self.offsets
            .get(index)
            .and_then(|range| self.ranges.get(range.clone()))
            .unwrap_or(&[])
    }
}

/// Map original UTF-8 ranges onto the unchanged, already-shaped visible Galley.
/// A bad prefix invalidates the whole row; a partially visible span is skipped.
/// Only callers painting a visible, current, enabled result row use this helper.
pub(crate) fn rectangles(
    galley: &egui::Galley,
    trim_start: usize,
    ranges: &[Range<usize>],
) -> Vec<egui::Rect> {
    let mut result = Vec::new();
    let text = galley.job.text.as_str();
    if ranges.is_empty()
        || ranges.len() > ROW_RANGES
        || text.len() > SNIPPET_BYTES
        || galley.rows.len() != 1
        || text.chars().any(char::is_control)
    {
        return result;
    }
    let row = &galley.rows[0];
    if row.ends_with_newline || !row.rect.is_finite() || !row.rect.is_positive() {
        return result;
    }
    let count = if galley.elided {
        if row.glyphs.last().is_none_or(|glyph| glyph.chr != '…') {
            return result;
        }
        row.glyphs.len() - 1
    } else {
        row.glyphs.len()
    };
    if count == 0 || count > SNIPPET_BYTES || result.try_reserve_exact(ranges.len()).is_err() {
        return result;
    }
    let mut boundaries = [0usize; SNIPPET_BYTES + 1];
    let mut characters = text.char_indices();
    let mut prior_x = f32::NEG_INFINITY;
    for (index, glyph) in row.glyphs[..count].iter().enumerate() {
        let Some((byte, character)) = characters.next() else {
            return Vec::new();
        };
        if character != glyph.chr
            || !glyph.pos.is_finite()
            || !glyph.advance_width.is_finite()
            || glyph.advance_width < 0.0
            || glyph.pos.x < prior_x
        {
            return Vec::new();
        }
        boundaries[index] = byte;
        boundaries[index + 1] = byte + character.len_utf8();
        prior_x = glyph.pos.x;
    }
    if !galley.elided && characters.next().is_some() {
        return Vec::new();
    }
    let visible_end = boundaries[count];
    let mut previous_end = 0;
    for span in ranges {
        if span.start >= span.end || span.start < previous_end {
            return Vec::new();
        }
        previous_end = span.end;
        let Some(start) = span.start.checked_sub(trim_start) else {
            continue;
        };
        let Some(end) = span.end.checked_sub(trim_start) else {
            continue;
        };
        if end > text.len() || end > visible_end {
            continue;
        }
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return Vec::new();
        }
        let (Ok(first), Ok(after)) = (
            boundaries[..=count].binary_search(&start),
            boundaries[..=count].binary_search(&end),
        ) else {
            return Vec::new();
        };
        if first >= after
            || row.glyphs[first..after]
                .iter()
                .any(|glyph| glyph.advance_width <= 0.0)
        {
            continue;
        }
        let rect = egui::Rect::from_min_max(
            egui::pos2(row.glyphs[first].pos.x, row.rect.top()),
            egui::pos2(row.glyphs[after - 1].max_x(), row.rect.bottom()),
        );
        if !rect.is_finite() || !rect.is_positive() || !row.rect.contains_rect(rect) {
            continue;
        }
        if galley.elided && rect.right() > row.glyphs[count].pos.x {
            continue;
        }
        result.push(rect);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rows(texts: &[&str]) -> Vec<SearchMatch> {
        texts
            .iter()
            .enumerate()
            .map(|(index, text)| SearchMatch {
                path: format!("src/{index}.rs"),
                line: 1,
                text: (*text).into(),
            })
            .collect()
    }
    #[test]
    fn literal_simple_fold_keeps_original_utf8_ranges() {
        for (query, text, expected) in [
            ("k", "😀KkK", vec![4..7, 7..8, 8..9]),
            ("K", "k", vec![0..1]),
            ("s", "ſS", vec![0..2, 2..3]),
            ("Σ", "σςΣ", vec![0..2, 2..4, 4..6]),
            ("ß", "ẞßSS", vec![0..3, 3..5]),
            ("i", "Iİıi", vec![0..1, 5..6]),
            ("é", "e\u{301}é", vec![3..5]),
            (".*[x](?i)", "a.*[x](?i)b", vec![1..10]),
        ] {
            let prepared = Prepared::prepare(query, false, &rows(&[text]));
            assert_eq!(prepared.row(0), expected, "{query:?}/{text:?}");
            assert_eq!(prepared.accounting.builds, 1);
        }
        assert!(Prepared::prepare("K", true, &rows(&["k"]))
            .row(0)
            .is_empty());
    }
    #[test]
    fn crop_markers_trim_and_scalar_matches_are_conservative() {
        let prepared = Prepared::prepare("…", true, &rows(&["…a…b…", " … ", "…"]));
        assert_eq!(prepared.row(0), [4..7]);
        assert_eq!(prepared.row(1), [1..4]);
        assert!(prepared.row(2).is_empty());

        assert_eq!(
            Prepared::prepare(" a", true, &rows(&[" a a "])).row(0),
            [2..4]
        );
        assert_eq!(
            Prepared::prepare("\u{301}", true, &rows(&["e\u{301}"])).row(0),
            [1..3]
        );
        assert_eq!(
            Prepared::prepare("aa", true, &rows(&["aaaaa"])).row(0),
            [0..2, 2..4]
        );
    }
    #[test]
    fn decoration_limits_never_need_to_reject_the_search_payload() {
        for query in [
            "",
            "a\nb",
            "a\rb",
            "a\0b",
            &"a".repeat(65),
            &"😀".repeat(17),
        ] {
            let p = Prepared::prepare(query, false, &rows(&["a"]));
            assert_eq!(p.accounting.builds, 0);
            assert!(p.row(0).is_empty());
        }
        let large = "x".repeat(SNIPPET_BYTES + 1);
        let p = Prepared::prepare("x", true, &rows(&[&large, "x"]));
        assert!(p.row(0).is_empty());
        assert_eq!(p.row(1), [0..1]);
        assert_eq!(p.accounting.scanned, 1);
        let p = Prepared::prepare("sKΣéẞ".repeat(6).as_str(), false, &rows(&["sKΣéẞ"]));
        assert_eq!(p.accounting.builds, 1);
        let p = Prepared::with_nfa_limit("sKΣéẞ".repeat(6).as_str(), false, &rows(&["sKΣéẞ"]), 1);
        assert!(p.row(0).is_empty());
        assert_eq!(p.accounting.builds, 1);
    }
    #[test]
    fn exact_row_and_batch_range_caps_are_atomic() {
        let a64 = "a".repeat(64);
        let a65 = "a".repeat(65);
        let p = Prepared::prepare("a", true, &rows(&[&a64, &a65, "a"]));
        assert_eq!(p.row(0).len(), 64);
        assert!(p.row(1).is_empty());
        assert_eq!(p.row(2), [0..1]);
        let text = vec![a64.as_str(); 17];
        let p = Prepared::prepare("a", true, &rows(&text));
        assert_eq!(p.ranges.len(), 1024);
        assert_eq!(p.row(15).len(), 64);
        assert!(p.row(16).is_empty());
        let a60 = "a".repeat(60);
        let mut text = vec![a64.as_str(); 15];
        text.extend([a60.as_str(), "aaaaa", "aaaa"]);
        let p = Prepared::prepare("a", true, &rows(&text));
        assert_eq!(p.ranges.len(), 1020);
        assert!(p.row(16).is_empty());
        assert!(p.row(17).is_empty());
    }
    #[test]
    fn aggregate_work_is_charged_before_search_and_stops_in_order() {
        let long = "x".repeat(1024);
        let mut text = vec![long.as_str(); 32];
        text.extend(["x", "a"]);
        let p = Prepared::prepare("a", true, &rows(&text));
        assert_eq!(p.accounting.scanned, SCAN_BYTES);
        assert_eq!(p.accounting.calls, 32);
        assert!(p.row(33).is_empty());
        let a65 = "a".repeat(65);
        let p = Prepared::prepare("a", true, &rows(&vec![a65.as_str(); 500]));
        assert_eq!(p.accounting.calls, FIND_CALLS);
        assert!(p.ranges.is_empty());
        let full = "a".repeat(SNIPPET_BYTES);
        let p = Prepared::prepare("a", true, &rows(&vec![full.as_str(); 50]));
        assert_eq!(
            p.accounting.visits,
            (VISIT_BYTES / SNIPPET_BYTES) * SNIPPET_BYTES
        );
        assert!(p.ranges.is_empty());
        let p = Prepared::prepare(&"a".repeat(64), true, &rows(&vec![full.as_str(); 50]));
        assert_eq!(
            p.accounting.work,
            (WORK_UNITS / (SNIPPET_BYTES * 64)) * (SNIPPET_BYTES * 64)
        );
        assert_eq!(p.row(0).len(), 32);
        assert!(p.row(1).is_empty());
    }
}
