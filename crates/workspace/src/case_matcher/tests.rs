use super::*;

#[test]
fn unicode_16_default_simple_folding_is_literal_and_has_no_full_or_turkic_fold() {
    for (query, equivalent) in [
        ("needle", "NEEDLE"),
        ("Σ", "σςΣ"),
        ("k", "KkK"),
        ("s", "Ssſ"),
        ("é", "Éé"),
        ("ß", "ẞß"),
        ("I", "Ii"),
    ] {
        let matcher = CaseMatcher::new(query, false).unwrap();
        for value in equivalent.chars() {
            // The first entry is a word, all remaining entries are fold classes.
            if query == "needle" {
                break;
            }
            let text = value.to_string();
            assert_eq!(
                matcher.find(&text),
                Some(0..text.len()),
                "{query:?} {text:?}"
            );
        }
        assert!(matcher.find(equivalent).is_some());
    }
    for (query, text) in [
        ("I", "İı"),
        ("i", "İı"),
        ("İ", "iIı"),
        ("ı", "iIİ"),
        ("ß", "SSss"),
        ("SS", "ßẞ"),
        ("é", "e\u{301}"),
        ("e\u{301}", "éÉ"),
    ] {
        assert_eq!(
            CaseMatcher::new(query, false).unwrap().find(text),
            None,
            "{query:?} {text:?}"
        );
    }
}

#[test]
fn all_metacharacters_flags_whitespace_and_backslashes_are_literal() {
    for query in [
        ".*", "[x]", "(?i)", "^", "$", r"\", "#", "a b", "a\tb", "a+b?", "{2}|-~&",
    ] {
        for sensitive in [true, false] {
            let matcher = CaseMatcher::new(query, sensitive).unwrap();
            let line = format!("😀prefix{query}suffix");
            let start = "😀prefix".len();
            assert_eq!(matcher.find(&line), Some(start..start + query.len()));
            assert_eq!(matcher.find("ordinary unrelated text"), None);
        }
    }
    assert_eq!(
        CaseMatcher::new("(?i)Needle", false)
            .unwrap()
            .find("NEEDLE"),
        None
    );
    assert_eq!(CaseMatcher::new("[x]", false).unwrap().find("X"), None);
}

#[test]
fn first_match_ranges_are_in_original_utf8_with_different_query_widths() {
    let prefix = "😀é前 ";
    for (query, text, width) in [("K", "K k K", 3), ("K", "k K", 1), ("s", "ſS", 2)] {
        let line = format!("{prefix}{text}");
        let found = CaseMatcher::new(query, false).unwrap().find(&line).unwrap();
        assert_eq!(found, prefix.len()..prefix.len() + width);
        assert!(line.get(found).is_some());
    }
    assert_eq!(
        CaseMatcher::new("é", true).unwrap().find("😀éé"),
        Some(4..6)
    );
}

#[test]
fn checked_mode_matches_str_find_for_literal_corpus() {
    for query in [
        "needle", "NEEDLE", "K", "k", "😀", "e\u{301}", "é", "\\.*", "\t", " ", "Σ",
    ] {
        let matcher = CaseMatcher::new(query, true).unwrap();
        for line in [
            "",
            "needle NEEDLE",
            "😀Kk ée\u{301}",
            "\\.*\t ",
            "Σσς",
            "endneedle",
        ] {
            assert_eq!(
                matcher.find(line),
                line.find(query).map(|start| start..start + query.len())
            );
        }
    }
}

#[test]
fn production_limits_accept_maximum_literal_and_escaped_patterns() {
    for query in ["a".repeat(4096), "*".repeat(4096), "K".repeat(1365)] {
        for sensitive in [true, false] {
            let matcher = CaseMatcher::new(&query, sensitive).unwrap();
            assert_eq!(matcher.find(&query), Some(0..query.len()));
            let prefix = "😀é";
            let prefixed = format!("{prefix}{query}");
            assert_eq!(
                matcher.find(&prefixed),
                Some(prefix.len()..prefix.len() + query.len())
            );
        }
    }
    for invalid in [
        "".to_string(),
        "a".repeat(4097),
        "a\nb".into(),
        "a\rb".into(),
        "a\0b".into(),
    ] {
        let error = CaseMatcher::new(&invalid, false).err().unwrap();
        assert_eq!(error.code, "invalid_scoped_search");
        assert!(!error.message.contains(&invalid) || invalid.is_empty());
    }
}

#[test]
fn complex_folded_literal_reaches_nfa_and_obeys_test_only_cap() {
    // Many fold classes exceed literal-extraction shortcuts. A short literal
    // may compile without an NFA and is deliberately not the negative witness.
    let query = "sKΣéẞ".repeat(32);
    assert!(CaseMatcher::new(&query, false).is_ok());
    let failure = CaseMatcher::with_test_nfa_limit(&query, false, 1)
        .err()
        .unwrap();
    assert_eq!(failure.code, "case_search_matcher_limit");
    assert!(!failure.message.contains(&query));
}

#[test]
fn shared_offset_crop_preserves_frozen_legacy_preview_bytes() {
    fn frozen(line: &str, query: &str) -> String {
        if line.len() <= crate::MAX_SEARCH_LINE_BYTES {
            return line.to_owned();
        }
        let found = line.find(query).unwrap_or(0);
        let mut start = found.saturating_sub(crate::MAX_SEARCH_LINE_BYTES / 4);
        while !line.is_char_boundary(start) {
            start += 1;
        }
        let mut end = (start + crate::MAX_SEARCH_LINE_BYTES).min(line.len());
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        format!(
            "{}{}{}",
            if start > 0 { "…" } else { "" },
            &line[start..end],
            if end < line.len() { "…" } else { "" }
        )
    }
    for count in [0, 1, 511, 512, 513, 1023, 1024, 2047, 2048, 2049, 4096] {
        for pad in ["a", "é", "😀"] {
            let line = format!("{}needle{}", pad.repeat(count), pad.repeat(1200));
            for query in ["needle", "absent", pad] {
                assert_eq!(crate::search_preview(&line, query), frozen(&line, query));
                if let Some(at) = line.find(query) {
                    assert_eq!(crate::search_preview_at(&line, at), frozen(&line, query));
                }
            }
        }
    }
}

#[test]
fn maximum_ascii_query_matches_wide_kelvin_text_under_production_component_caps() {
    let query = "K".repeat(4096);
    let text = "K".repeat(4096);
    let matcher = CaseMatcher::new(&query, false).unwrap();
    assert_eq!(matcher.find(&text), Some(0..text.len()));
}
