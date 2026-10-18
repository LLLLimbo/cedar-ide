//! Programmatic Stage 2 integration through the production stdio agent.
//!
//! Build the normal cedar-agent separately, then run:
//! CEDAR_AGENT_BIN=/absolute/path/to/cedar-agent cargo test -p cedar-client \
//!     --test case_search_roundtrip --locked -- --ignored
//!
//! As with stdio_roundtrip, all-targets builds compile these tests, but execution
//! requires the explicit ignored-test command and a built normal agent. These
//! tests do not exercise application admission, GUI controls, or a Stage 4 ledger.

use cedar_client::Client;
use cedar_protocol::{
    CaseSearchDescriptor, CaseSearchRequest, CaseSearchResult, Operation, Payload,
    ScopedSearchDescriptor, ScopedSearchIncompleteReason, ScopedSearchRequest, ScopedSearchResult,
    SearchMatch, MAX_SCOPED_SEARCH_RESULTS, MAX_SCOPED_SEARCH_SNIPPET_BYTES, PROTOCOL_VERSION,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[path = "support/capability_inventory.rs"]
mod capability_inventory;

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn spawn(root: &Path) -> Client {
    let agent = PathBuf::from(std::env::var_os("CEDAR_AGENT_BIN").expect("set CEDAR_AGENT_BIN"));
    // This is the ordinary shipping entry point, with execution trust off.
    // There is no fixture peer, in-process workspace, or test-only host mode.
    let client = Client::spawn_agent(&agent, root, false).unwrap();
    let Payload::Hello {
        protocol,
        agent: Some(info),
        scoped_search,
        case_search,
        ..
    } = client.handshake()
    else {
        panic!("normal agent must provide a validated Hello and metadata");
    };
    assert_eq!(*protocol, PROTOCOL_VERSION);
    assert_eq!(
        scoped_search.map(|descriptor| descriptor.version),
        Some(ScopedSearchDescriptor::V1.version)
    );
    assert_eq!(*case_search, Some(CaseSearchDescriptor::V1));
    capability_inventory::assert_shipping_inventory(info);
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    assert!(client.supports_scoped_search());
    assert!(client.supports_case_search());
    client
}

fn request(
    query: &str,
    scope: &str,
    masks: &[&str],
    limit: usize,
    case_sensitive: bool,
) -> CaseSearchRequest {
    CaseSearchRequest {
        query: query.into(),
        scope: scope.into(),
        masks: masks.iter().map(|mask| (*mask).into()).collect(),
        limit,
        case_sensitive,
    }
}

fn search_case(client: &mut Client, request: CaseSearchRequest) -> CaseSearchResult {
    let Payload::CaseMatches { result } = client
        .request(Operation::SearchCase {
            request: request.clone(),
        })
        .unwrap()
    else {
        panic!("SearchCase must return case_matches");
    };
    // The production Client already validates this before exposing rows. Keep
    // explicit exact-echo assertions here too, including order, bytes and limit.
    assert_eq!(result.request, request);
    result.validate_for(&request).unwrap();
    assert!(client.is_connected());
    result
}

fn complete_case(client: &mut Client, request: CaseSearchRequest) -> CaseSearchResult {
    let result = search_case(client, request);
    assert!(
        !result.incomplete,
        "unexpected reasons: {:?}",
        result.reasons
    );
    assert!(result.reasons.is_empty());
    if !result.matches.is_empty() {
        assert!(result.examined_entries > 0);
        assert!(result.read_bytes > 0);
    }
    result
}

fn search_in(client: &mut Client, request: &CaseSearchRequest) -> ScopedSearchResult {
    let request = ScopedSearchRequest {
        query: request.query.clone(),
        scope: request.scope.clone(),
        masks: request.masks.clone(),
        limit: request.limit,
    };
    let Payload::ScopedMatches { result } = client
        .request(Operation::SearchIn {
            request: request.clone(),
        })
        .unwrap()
    else {
        panic!("SearchIn must retain scoped_matches");
    };
    assert_eq!(result.request, request);
    result.validate_for(&request).unwrap();
    assert!(!result.incomplete);
    assert!(result.reasons.is_empty());
    result
}

fn legacy_search(client: &mut Client, query: &str) -> Vec<SearchMatch> {
    let Payload::Matches { matches, truncated } = client
        .request(Operation::Search {
            query: query.into(),
            limit: 500,
        })
        .unwrap()
    else {
        panic!("Search must retain matches");
    };
    assert!(!truncated);
    matches
}

fn case_rows(result: &CaseSearchResult) -> Vec<(&str, usize, &str)> {
    result
        .matches
        .iter()
        .map(|row| (row.path.as_str(), row.line, row.text.as_str()))
        .collect()
}

fn old_rows(matches: &[SearchMatch]) -> Vec<(&str, usize, &str)> {
    matches
        .iter()
        .map(|row| (row.path.as_str(), row.line, row.text.as_str()))
        .collect()
}

#[test]
#[ignore = "requires a compiled normal cedar-agent; run with CEDAR_AGENT_BIN"]
fn normal_agent_root_scopes_and_mask_case_preserve_both_old_search_routes() {
    let root = tempfile::tempdir().unwrap();
    for (path, text) in [
        ("origin.txt", "needle origin\n"),
        ("outside.txt", "NEEDLE\n"),
        ("src Ω/target.rs", "NEEDLE\nneedle\nNeedle needle\n"),
        ("src Ω/nested/child.rs", "nEeDlE\n"),
        ("src Ω/case.RS", "NEEDLE\n"),
        ("src Ω/nope.txt", "NEEDLE\n"),
        ("src Ω/target/ignored.rs", "NEEDLE needle\n"),
    ] {
        write(root.path(), path, text);
    }
    let mut client = spawn(root.path());
    let legacy_before = legacy_search(&mut client, "needle");
    let scoped_request = request("needle", "src Ω", &["*.rs"], 500, true);
    let scoped_before = search_in(&mut client, &scoped_request);
    assert_eq!(
        old_rows(&legacy_before),
        [
            ("origin.txt", 1, "needle origin"),
            ("src Ω/target.rs", 2, "needle"),
            ("src Ω/target.rs", 3, "Needle needle"),
        ]
    );
    assert_eq!(
        old_rows(&scoped_before.matches),
        [
            ("src Ω/target.rs", 2, "needle"),
            ("src Ω/target.rs", 3, "Needle needle"),
        ]
    );

    let exact_root = complete_case(&mut client, request("needle", "", &[], 500, true));
    assert_eq!(case_rows(&exact_root), old_rows(&legacy_before));
    let folded_root = complete_case(&mut client, request("needle", "", &[], 500, false));
    assert_eq!(
        case_rows(&folded_root),
        [
            ("origin.txt", 1, "needle origin"),
            ("outside.txt", 1, "NEEDLE"),
            ("src Ω/case.RS", 1, "NEEDLE"),
            ("src Ω/nested/child.rs", 1, "nEeDlE"),
            ("src Ω/nope.txt", 1, "NEEDLE"),
            ("src Ω/target.rs", 1, "NEEDLE"),
            ("src Ω/target.rs", 2, "needle"),
            ("src Ω/target.rs", 3, "Needle needle"),
        ]
    );
    let exact_scoped = complete_case(&mut client, scoped_request.clone());
    assert_eq!(case_rows(&exact_scoped), old_rows(&scoped_before.matches));
    let folded_scoped = complete_case(
        &mut client,
        request("needle", "src Ω", &["*.rs"], 500, false),
    );
    assert_eq!(
        case_rows(&folded_scoped),
        [
            ("src Ω/nested/child.rs", 1, "nEeDlE"),
            ("src Ω/target.rs", 1, "NEEDLE"),
            ("src Ω/target.rs", 2, "needle"),
            ("src Ω/target.rs", 3, "Needle needle"),
        ]
    );
    let nested = complete_case(
        &mut client,
        request("needle", "src Ω/nested", &["*.md", "*.rs"], 500, false),
    );
    assert_eq!(case_rows(&nested), [("src Ω/nested/child.rs", 1, "nEeDlE")]);
    let upper_mask = complete_case(
        &mut client,
        request("needle", "src Ω", &["*.RS"], 500, false),
    );
    assert_eq!(case_rows(&upper_mask), [("src Ω/case.RS", 1, "NEEDLE")]);
    let no_matches = complete_case(
        &mut client,
        request("needle", "src Ω", &["*.zz"], 500, false),
    );
    assert!(no_matches.matches.is_empty());
    assert_eq!(no_matches.read_bytes, 0);

    // Interleaving the new operation must not change the legacy payloads.
    let legacy_after = legacy_search(&mut client, "needle");
    assert_eq!(old_rows(&legacy_after), old_rows(&legacy_before));
    let scoped_after = search_in(&mut client, &scoped_request);
    assert_eq!(
        serde_json::to_value(&scoped_after).unwrap(),
        serde_json::to_value(&scoped_before).unwrap()
    );
    client.close_and_wait(Duration::from_secs(5)).unwrap();
}

#[test]
#[ignore = "requires a compiled normal cedar-agent; run with CEDAR_AGENT_BIN"]
fn normal_agent_uses_unicode_16_simple_fold_without_normalization_or_full_fold() {
    let root = tempfile::tempdir().unwrap();
    // Expectations are pinned to Unicode 16.0.0 CaseFolding.txt C+S entries,
    // excluding F and T, as audited for regex-syntax 0.8.11 / regex 1.13.1.
    type FoldExample<'a> = (&'a str, &'a str, &'a str, &'a [usize], &'a [usize]);
    let examples: &[FoldExample<'_>] = &[
        ("sigma.txt", "Σ\nσ\nς\n", "σ", &[1, 2, 3], &[2]),
        (
            "accent.txt",
            "É\né\ne\u{301}\nE\u{301}\n",
            "é",
            &[1, 2],
            &[2],
        ),
        ("kelvin.txt", "K\nk\nK\n", "k", &[1, 2, 3], &[2]),
        ("long-s.txt", "S\ns\nſ\n", "s", &[1, 2, 3], &[2]),
        ("turkish.txt", "I\ni\nİ\nı\n", "i", &[1, 2], &[2]),
        ("sharp-s.txt", "ß\nẞ\nSS\nss\n", "ß", &[1, 2], &[1]),
        ("full-fold.txt", "ß\nẞ\nSS\nss\n", "ss", &[3, 4], &[4]),
        (
            "decomposed.txt",
            "É\né\ne\u{301}\nE\u{301}\n",
            "e\u{301}",
            &[3, 4],
            &[3],
        ),
    ];
    for (name, text, ..) in examples {
        write(root.path(), &format!("unicode Ω/{name}"), text);
    }
    let mut client = spawn(root.path());
    for &(name, text, query, folded_lines, exact_lines) in examples {
        for (case_sensitive, expected_lines) in [(false, folded_lines), (true, exact_lines)] {
            let result = complete_case(
                &mut client,
                request(query, "unicode Ω", &[name], 500, case_sensitive),
            );
            let path = format!("unicode Ω/{name}");
            let lines: Vec<_> = text.lines().collect();
            let expected: Vec<_> = expected_lines
                .iter()
                .map(|&line| (path.as_str(), line, lines[line - 1]))
                .collect();
            assert_eq!(
                case_rows(&result),
                expected,
                "{name}, exact={case_sensitive}"
            );
        }
    }
    client.close_and_wait(Duration::from_secs(5)).unwrap();
}

#[test]
#[ignore = "requires a compiled normal cedar-agent; run with CEDAR_AGENT_BIN"]
fn normal_agent_treats_punctuation_flags_spaces_and_tabs_as_literal_text() {
    let root = tempfile::tempdir().unwrap();
    let queries = [
        ".*",
        "[x]",
        "(?i)NeEdLe",
        "^",
        "$",
        "\\",
        "#",
        r"\bNeEdLe\b",
        " \tNeEdLe  ",
    ];
    for (index, query) in queries.iter().enumerate() {
        write(
            root.path(),
            &format!("literal-{index}.txt"),
            &format!(
                "PREFIX {query} SUFFIX\nprefix {} suffix\nplain needle x only\n",
                query.to_ascii_lowercase()
            ),
        );
    }
    let mut client = spawn(root.path());
    for (index, query) in queries.iter().enumerate() {
        let path = format!("literal-{index}.txt");
        let first = format!("PREFIX {query} SUFFIX");
        let second = format!("prefix {} suffix", query.to_ascii_lowercase());
        let folded = complete_case(&mut client, request(query, "", &[&path], 500, false));
        assert_eq!(
            case_rows(&folded),
            [
                (path.as_str(), 1, first.as_str()),
                (path.as_str(), 2, second.as_str())
            ],
            "literal query {query:?}"
        );
        let exact = complete_case(&mut client, request(query, "", &[&path], 500, true));
        let mut expected = vec![(path.as_str(), 1, first.as_str())];
        if second.contains(*query) {
            expected.push((path.as_str(), 2, second.as_str()));
        }
        assert_eq!(case_rows(&exact), expected, "literal query {query:?}");
    }
    client.close_and_wait(Duration::from_secs(5)).unwrap();
}

#[test]
#[ignore = "requires a compiled normal cedar-agent; run with CEDAR_AGENT_BIN"]
fn normal_agent_previews_use_first_original_utf8_offsets_and_keep_old_cropping() {
    let root = tempfile::tempdir().unwrap();
    let prefix = "🦀é".repeat(601);
    write(
        root.path(),
        "kelvin.txt",
        &format!("{prefix}K{}k", "z".repeat(2600)),
    );
    write(
        root.path(),
        "ascii.txt",
        &format!("{prefix}K{}", "z".repeat(2600)),
    );
    write(
        root.path(),
        "lines.txt",
        "\r\n\r\n🦀é nEeDlE nEeDlE\r\nNEEDLE",
    );
    let mut client = spawn(root.path());
    // The first match starts at original byte 3606, beyond the first window.
    // A 512-byte prefix is é + 85*(🦀é). K and K have different UTF-8 widths.
    let expected_kelvin = format!("…é{}K{}…", "🦀é".repeat(85), "z".repeat(1533));
    let expected_ascii = format!("…é{}K{}…", "🦀é".repeat(85), "z".repeat(1535));
    for query in ["k", "K", "K"] {
        let result = complete_case(&mut client, request(query, "", &["kelvin.txt"], 500, false));
        assert_eq!(
            case_rows(&result),
            [("kelvin.txt", 1, expected_kelvin.as_str())]
        );
        assert_eq!(
            result.matches[0].text.len(),
            MAX_SCOPED_SEARCH_SNIPPET_BYTES
        );
    }
    let reverse_width = complete_case(&mut client, request("K", "", &["ascii.txt"], 500, false));
    assert_eq!(
        case_rows(&reverse_width),
        [("ascii.txt", 1, expected_ascii.as_str())]
    );
    let exact_request = request("K", "", &["kelvin.txt"], 500, true);
    let exact = complete_case(&mut client, exact_request.clone());
    assert_eq!(
        case_rows(&exact),
        [("kelvin.txt", 1, expected_kelvin.as_str())]
    );
    let scoped = search_in(&mut client, &exact_request);
    assert_eq!(case_rows(&exact), old_rows(&scoped.matches));
    let legacy = legacy_search(&mut client, "K");
    assert_eq!(case_rows(&exact), old_rows(&legacy));

    let last_exact = complete_case(&mut client, request("k", "", &["kelvin.txt"], 500, true));
    let expected_last = format!("…{}k", "z".repeat(512));
    assert_eq!(
        case_rows(&last_exact),
        [("kelvin.txt", 1, expected_last.as_str())]
    );
    let lines = complete_case(
        &mut client,
        request("needle", "", &["lines.txt"], 500, false),
    );
    assert_eq!(
        case_rows(&lines),
        [
            ("lines.txt", 3, "🦀é nEeDlE nEeDlE"),
            ("lines.txt", 4, "NEEDLE")
        ]
    );
    client.close_and_wait(Duration::from_secs(5)).unwrap();
}

#[test]
#[ignore = "requires a compiled normal cedar-agent; run with CEDAR_AGENT_BIN"]
fn normal_agent_bounds_maximum_literal_queries_and_retains_uncropped_request_echo() {
    let root = tempfile::tempdir().unwrap();
    let wide_query = "K".repeat(4096);
    let escaped_query = "*".repeat(4096);
    write(root.path(), "wide.txt", &"K".repeat(4096));
    write(root.path(), "escaped.txt", &escaped_query);
    let mut client = spawn(root.path());
    let started = std::time::Instant::now();
    let wide = search_case(
        &mut client,
        request(&wide_query, "", &["wide.txt"], 1007, false),
    );
    eprintln!(
        "maximum folded literal: query_bytes=4096, elapsed_ms={}, entries={}, read_bytes={}, rows={}, reasons={:?}",
        started.elapsed().as_millis(), wide.examined_entries, wide.read_bytes, wide.matches.len(), wide.reasons
    );
    // A valid-sized query need not finish compilation/matching within the
    // cooperative budget. This boundary test must not invent a latency promise.
    // Smaller original-width cases above independently require matching rows.
    let expected_wide = format!("{}…", "K".repeat(682));
    if wide.incomplete {
        assert_eq!(wide.reasons, [ScopedSearchIncompleteReason::TimeLimit]);
        assert!(
            wide.matches.is_empty()
                || case_rows(&wide) == [("wide.txt", 1, expected_wide.as_str())]
        );
        assert!(wide.examined_entries <= 2);
        assert!(wide.read_bytes <= 3 * 4096);
    } else {
        assert_eq!(case_rows(&wide), [("wide.txt", 1, expected_wide.as_str())]);
        assert!(!wide.matches[0].text.contains(&wide_query));
    }
    assert_eq!(wide.request.query.len(), 4096);
    assert_eq!(wide.request.limit, 1007);
    assert!(client.is_connected());
    for case_sensitive in [false, true] {
        let escaped = complete_case(
            &mut client,
            request(&escaped_query, "", &["escaped.txt"], 1007, case_sensitive),
        );
        let expected_escaped = format!("{}…", "*".repeat(2048));
        assert_eq!(
            case_rows(&escaped),
            [("escaped.txt", 1, expected_escaped.as_str())]
        );
        assert_eq!(escaped.request.query.len(), 4096);
    }
    client.close_and_wait(Duration::from_secs(5)).unwrap();
}

#[test]
#[ignore = "requires a compiled normal cedar-agent; run with CEDAR_AGENT_BIN"]
fn normal_agent_caps_rows_without_changing_requested_limit_or_ordered_masks() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "scope Ω/many.rs", &"nEeDlE\n".repeat(1105));
    let mut client = spawn(root.path());
    for case_sensitive in [false, true] {
        let query = if case_sensitive { "nEeDlE" } else { "needle" };
        for requested_limit in [1, MAX_SCOPED_SEARCH_RESULTS + 7] {
            let result = search_case(
                &mut client,
                request(
                    query,
                    "scope Ω",
                    &["*.md", "*.rs", "many.*"],
                    requested_limit,
                    case_sensitive,
                ),
            );
            let effective = requested_limit.min(MAX_SCOPED_SEARCH_RESULTS);
            assert_eq!(result.matches.len(), effective);
            assert!(result.incomplete);
            assert_eq!(result.reasons, [ScopedSearchIncompleteReason::ResultLimit]);
            for (index, row) in result.matches.iter().enumerate() {
                assert_eq!(row.path, "scope Ω/many.rs");
                assert_eq!(row.line, index + 1);
                assert_eq!(row.text, "nEeDlE");
            }
        }
    }
    client.close_and_wait(Duration::from_secs(5)).unwrap();
}

#[test]
#[ignore = "requires a compiled normal cedar-agent; run with CEDAR_AGENT_BIN"]
fn normal_agent_ordinary_backend_errors_leave_trust_off_connection_healthy() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "plain.txt", "NEEDLE needle\n");
    let mut client = spawn(root.path());
    for case_sensitive in [false, true] {
        // This is a valid wire request rejected by the real backend because
        // the scope exists as a file. It is not a local request-validation error.
        let error = client
            .request(Operation::SearchCase {
                request: request("needle", "plain.txt", &[], 500, case_sensitive),
            })
            .unwrap_err();
        assert!(error.starts_with("not_directory:"), "{error}");
        assert!(client.is_connected());
        let recovered = complete_case(&mut client, request("needle", "", &[], 500, case_sensitive));
        assert_eq!(case_rows(&recovered), [("plain.txt", 1, "NEEDLE needle")]);
    }
    let Payload::Hello {
        agent: Some(info), ..
    } = client.handshake()
    else {
        panic!("normal agent metadata missing");
    };
    let supports_tasks = info.supports("run_start");
    let error = client
        .request(Operation::RunStart {
            program: "cedar-case-search-execution-must-stay-disabled".into(),
            args: Vec::new(),
            timeout_secs: 1,
        })
        .unwrap_err();
    assert!(
        error.starts_with(if supports_tasks {
            "run_disabled:"
        } else {
            "unsupported_operation:"
        }),
        "{error}"
    );
    assert!(client.is_connected());
    let Payload::File { path, text, .. } = client
        .request(Operation::Read {
            path: "plain.txt".into(),
        })
        .unwrap()
    else {
        panic!("connection must remain usable for ordinary reads");
    };
    assert_eq!(path, "plain.txt");
    assert_eq!(text, "NEEDLE needle\n");
    let recovered = complete_case(&mut client, request("needle", "", &[], 500, false));
    assert_eq!(case_rows(&recovered), [("plain.txt", 1, "NEEDLE needle")]);
    client.close_and_wait(Duration::from_secs(5)).unwrap();
}
