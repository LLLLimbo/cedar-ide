use super::*;
use std::cell::Cell;

fn request(query: &str, scope: &str, masks: &[&str], case_sensitive: bool) -> CaseSearchRequest {
    CaseSearchRequest {
        query: query.into(),
        scope: scope.into(),
        masks: masks.iter().map(|s| (*s).into()).collect(),
        limit: 1000,
        case_sensitive,
    }
}

fn run(workspace: &Workspace, request: &CaseSearchRequest, budget: Budget) -> CaseSearchResult {
    workspace
        .search_case_with_budget(request, budget, || false, |_, _| {}, None)
        .unwrap()
}

fn fixture(root: &std::path::Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let full = root.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, text).unwrap();
    }
}

#[test]
fn case_scope_masks_exclusions_and_compile_once_precede_all_file_activity() {
    let root = tempfile::tempdir().unwrap();
    fixture(
        root.path(),
        &[
            ("outside.rs", "NEEDLE"),
            ("selected/a.rs", "NEEDLE\nneedle\nNeedle needle\n"),
            ("selected/nested/b.rs", "nEeDlE"),
            ("selected/c.RS", "NEEDLE"),
            ("selected/no.txt", "NEEDLE"),
            ("selected/target/skip.rs", "NEEDLE"),
        ],
    );
    let workspace = Workspace::open(root.path()).unwrap();
    let spec = request("needle", "selected", &["*.rs"], false);
    let mut events = Vec::new();
    let result = workspace
        .search_case_with_budget(
            &spec,
            Budget::default(),
            || false,
            |kind, path| events.push((kind, path.to_owned())),
            None,
        )
        .unwrap();
    assert_eq!(events[0].0, TestEvent::Compile);
    assert_eq!(events[1].0, TestEvent::Compiled);
    assert_eq!(
        events
            .iter()
            .filter(|(kind, _)| *kind == TestEvent::Compile)
            .count(),
        1
    );
    let mut opened: Vec<_> = events
        .iter()
        .filter(|(kind, _)| *kind == TestEvent::Open)
        .map(|(_, path)| path.as_str())
        .collect();
    opened.sort();
    assert_eq!(opened, ["selected/a.rs", "selected/nested/b.rs"]);
    assert!(events
        .iter()
        .filter(|(kind, _)| *kind == TestEvent::Directory)
        .all(|(_, p)| p.starts_with("selected")));
    assert_eq!(
        result
            .matches
            .iter()
            .map(|r| (r.path.as_str(), r.line))
            .collect::<Vec<_>>(),
        [
            ("selected/a.rs", 1),
            ("selected/a.rs", 2),
            ("selected/a.rs", 3),
            ("selected/nested/b.rs", 1)
        ]
    );
    assert_eq!(result.examined_entries, 6);
    assert_eq!(result.read_bytes, 34);
    assert!(!result.incomplete);
    result.validate_for(&spec).unwrap();
    let upper = run(
        &workspace,
        &request("needle", "selected", &["*.RS"], false),
        Budget::default(),
    );
    assert_eq!(upper.matches.len(), 1);
    assert_eq!(upper.matches[0].path, "selected/c.RS");
    assert_eq!(
        run(
            &workspace,
            &request("needle", "", &[], false),
            Budget::default()
        )
        .matches
        .len(),
        7
    );
}

#[test]
fn invalid_requests_and_matcher_limits_have_zero_traversal_or_content_opens() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), &[("sentinel", "NEEDLE")]);
    let workspace = Workspace::open(root.path()).unwrap();
    let mut invalids = Vec::new();
    for scope in ["../outside", "target", "/absolute"] {
        invalids.push(request("needle", scope, &[], false));
    }
    for query in ["", "a\nb", "a\0b"] {
        invalids.push(request(query, "", &[], false));
    }
    let mut zero = request("needle", "", &[], false);
    zero.limit = 0;
    invalids.push(zero);
    let mut long = request("needle", "", &[], false);
    long.query = "x".repeat(4097);
    invalids.push(long);
    invalids.push(request("needle", "", &["[x]"], false));
    for spec in invalids {
        let mut events = Vec::new();
        assert!(workspace
            .search_case_with_budget(
                &spec,
                Budget::default(),
                || false,
                |kind, _| events.push(kind),
                None
            )
            .is_err());
        assert!(events.is_empty());
    }
    let spec = request(&"sKΣéẞ".repeat(32), "", &[], false);
    let mut events = Vec::new();
    let error = workspace
        .search_case_with_budget(
            &spec,
            Budget::default(),
            || false,
            |kind, _| events.push(kind),
            Some(1),
        )
        .unwrap_err();
    assert_eq!(error.code, "case_search_matcher_limit");
    assert_eq!(events, [TestEvent::Compile]);
}

#[test]
fn cooperative_expiry_before_after_compile_and_after_final_match_has_no_unobserved_overrun() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), &[("text", "NEEDLE")]);
    let workspace = Workspace::open(root.path()).unwrap();
    let spec = request("needle", "", &[], false);
    let before = workspace
        .search_case_with_budget(
            &spec,
            Budget::default(),
            || true,
            |_, _| panic!("expired before compile"),
            None,
        )
        .unwrap();
    assert_eq!(before.reasons, [Reason::TimeLimit]);
    assert_eq!((before.examined_entries, before.read_bytes), (0, 0));
    let elapsed = Cell::new(false);
    let after = workspace
        .search_case_with_budget(
            &spec,
            Budget::default(),
            || elapsed.get(),
            |kind, _| {
                assert!(matches!(kind, TestEvent::Compile | TestEvent::Compiled));
                if kind == TestEvent::Compiled {
                    elapsed.set(true);
                }
            },
            None,
        )
        .unwrap();
    assert_eq!(after.reasons, [Reason::TimeLimit]);
    assert_eq!((after.examined_entries, after.read_bytes), (0, 0));
    for query in ["needle", "nomatch"] {
        elapsed.set(false);
        let last = workspace
            .search_case_with_budget(
                &request(query, "", &[], false),
                Budget::default(),
                || elapsed.get(),
                |kind, _| {
                    if kind == TestEvent::Match {
                        elapsed.set(true);
                    }
                },
                None,
            )
            .unwrap();
        assert_eq!(last.reasons, [Reason::TimeLimit]);
        assert!(last.matches.is_empty());
        assert_eq!((last.examined_entries, last.read_bytes), (1, 6));
    }
    elapsed.set(false);
    let fail = workspace
        .search_case_with_budget(
            &request(&"sKΣéẞ".repeat(32), "", &[], false),
            Budget::default(),
            || elapsed.get(),
            |kind, _| {
                if kind == TestEvent::Compile {
                    elapsed.set(true);
                }
            },
            Some(1),
        )
        .unwrap_err();
    assert_eq!(fail.code, "case_search_matcher_limit");
}

#[test]
fn original_offsets_long_lines_crlf_no_final_newline_and_one_row_per_line() {
    let root = tempfile::tempdir().unwrap();
    let prefix = "😀é".repeat(600);
    let long = format!("{prefix}K unique K\r\n\nK\nfinalK");
    fixture(root.path(), &[("text", &long)]);
    let workspace = Workspace::open(root.path()).unwrap();
    let result = run(&workspace, &request("k", "", &[], false), Budget::default());
    assert_eq!(
        result.matches.iter().map(|r| r.line).collect::<Vec<_>>(),
        [1, 3, 4]
    );
    assert!(result.matches[0].text.contains("K unique K"));
    assert!(result.matches[0].text.starts_with('…'));
    assert!(!result.matches[0].text.contains('\r'));
    assert_eq!(result.matches[1].text, "K");
    assert_eq!(result.matches[2].text, "finalK");
    assert!(result.matches.iter().all(|r| r.text.len() <= 2054));
    let query = "K".repeat(4096);
    fs::write(
        root.path().join("text"),
        format!("{}{}", "😀".repeat(700), "k".repeat(4096)),
    )
    .unwrap();
    let long_query = run(
        &workspace,
        &request(&query, "", &[], false),
        Budget::default(),
    );
    assert_eq!(long_query.matches.len(), 1);
    assert!(long_query.matches[0].text.ends_with('…'));
    assert!(!long_query.matches[0].text.contains(&"k".repeat(4096)));
}

#[test]
fn case_true_has_exact_scoped_rows_counters_and_preview_parity() {
    let root = tempfile::tempdir().unwrap();
    let long = format!("{}needle{}", "😀é".repeat(700), "x".repeat(3000));
    fixture(
        root.path(),
        &[
            ("a.rs", "needle\nNEEDLE\nneedle needle\r\n"),
            ("nested/b.rs", &long),
            ("other.RS", "needle"),
        ],
    );
    let workspace = Workspace::open(root.path()).unwrap();
    for (scope, masks) in [
        ("", vec![]),
        ("", vec!["*.rs"]),
        ("nested", vec!["*.rs", "*.md"]),
    ] {
        let spec = request("needle", scope, &masks, true);
        let old_request = ScopedSearchRequest {
            query: spec.query.clone(),
            scope: spec.scope.clone(),
            masks: spec.masks.clone(),
            limit: spec.limit,
        };
        let old = workspace
            .search_in_with_budget(&old_request, Budget::default(), || false, |_, _| {})
            .unwrap();
        let new = run(&workspace, &spec, Budget::default());
        assert_eq!(
            serde_json::to_value(&new.matches).unwrap(),
            serde_json::to_value(&old.matches).unwrap()
        );
        assert_eq!(
            (
                new.examined_entries,
                new.read_bytes,
                new.incomplete,
                new.reasons
            ),
            (
                old.examined_entries,
                old.read_bytes,
                old.incomplete,
                old.reasons
            )
        );
    }
}

#[test]
fn each_case_traversal_budget_is_independent_and_read_exclusions_count_actual_bytes() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), &[("one/two/text", "NEEDLE\nNEEDLE")]);
    let workspace = Workspace::open(root.path()).unwrap();
    let spec = request("needle", "", &[], false);
    for (budget, reason) in [
        (
            Budget {
                entries: 1,
                ..Budget::default()
            },
            Reason::EntryLimit,
        ),
        (
            Budget {
                depth: 0,
                ..Budget::default()
            },
            Reason::DepthLimit,
        ),
        (
            Budget {
                pending_dirs: 0,
                ..Budget::default()
            },
            Reason::StackLimit,
        ),
        (
            Budget {
                pending_bytes: 2,
                ..Budget::default()
            },
            Reason::StackLimit,
        ),
        (
            Budget {
                read_bytes: 3,
                ..Budget::default()
            },
            Reason::ReadByteLimit,
        ),
    ] {
        assert!(run(&workspace, &spec, budget).reasons.contains(&reason));
    }
    let deep = run(
        &workspace,
        &request("needle", "one/two", &[], false),
        Budget {
            depth: 0,
            ..Budget::default()
        },
    );
    assert_eq!(deep.matches.len(), 2);
    assert!(!deep.incomplete);
    let mut one = request("needle", "one/two", &[], false);
    one.limit = 1;
    let limited = run(&workspace, &one, Budget::default());
    assert_eq!(limited.matches.len(), 1);
    assert_eq!(limited.reasons, [Reason::ResultLimit]);
    fs::write(root.path().join("binary"), b"NEEDLE\0ignored").unwrap();
    fs::write(root.path().join("invalid"), b"NEEDLE\xff").unwrap();
    File::create(root.path().join("oversize"))
        .unwrap()
        .set_len(MAX_FILE_BYTES as u64 + 1)
        .unwrap();
    let all = run(&workspace, &spec, Budget::default());
    assert_eq!(all.read_bytes, 13 + 14 + 7);
    assert_eq!(all.matches.len(), 2);
    assert!(!all.incomplete);
}

#[test]
fn case_response_budget_includes_exact_boolean_escaping_and_requested_limit() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), &[("text", "NEEDLE\"\\\t\nNEEDLE\"\\\t")]);
    let workspace = Workspace::open(root.path()).unwrap();
    for sensitive in [false, true] {
        let mut spec = request("NEEDLE", "", &[], sensitive);
        spec.limit = usize::MAX;
        let complete = run(&workspace, &spec, Budget::default());
        let bytes = complete.encoded_bytes().unwrap();
        assert_eq!(complete.request.limit, usize::MAX);
        let exact = run(
            &workspace,
            &spec,
            Budget {
                result_bytes: bytes,
                ..Budget::default()
            },
        );
        assert_eq!(exact.matches.len(), 2);
        assert!(!exact.incomplete);
        assert_eq!(exact.encoded_bytes().unwrap(), bytes);
        let smaller = run(
            &workspace,
            &spec,
            Budget {
                result_bytes: bytes - 1,
                ..Budget::default()
            },
        );
        assert!(smaller.matches.len() < 2);
        assert_eq!(smaller.reasons, [Reason::ResponseLimit]);
        assert!(smaller.encoded_bytes().unwrap() < bytes);
    }
}

#[test]
fn production_encoded_cap_counts_large_escaped_rows_and_keeps_a_valid_prefix() {
    let root = tempfile::tempdir().unwrap();
    let text = format!("NEEDLE{}\n", "\"".repeat(800)).repeat(900);
    fixture(root.path(), &[("escaped", &text)]);
    let workspace = Workspace::open(root.path()).unwrap();
    for sensitive in [false, true] {
        let spec = request("NEEDLE", "", &[], sensitive);
        let result = run(&workspace, &spec, Budget::default());
        assert_eq!(result.reasons, [Reason::ResponseLimit]);
        assert!(!result.matches.is_empty() && result.matches.len() < 900);
        assert_eq!(result.read_bytes, text.len());
        assert_eq!(
            result.encoded_bytes().unwrap(),
            serde_json::to_vec(&result).unwrap().len()
        );
        assert!(result.encoded_bytes().unwrap() <= MAX_SCOPED_SEARCH_RESULT_BYTES);
        result.validate_for(&spec).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn case_symlink_scope_queued_directory_and_leaf_replacements_never_open_sentinels() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fixture(outside.path(), &[("sentinel", "NEEDLE")]);
    symlink(outside.path(), root.path().join("linked")).unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let failure = workspace
        .search_case_with_budget(
            &request("needle", "linked", &[], false),
            Budget::default(),
            || false,
            |kind, _| assert_ne!(kind, TestEvent::Open),
            None,
        )
        .unwrap_err();
    assert_eq!(failure.code, "invalid_path");
    fs::create_dir(root.path().join("queued")).unwrap();
    let queued = workspace
        .search_case_with_budget(
            &request("needle", "", &[], false),
            Budget::default(),
            || false,
            |kind, path| {
                if kind == TestEvent::Directory && path == "queued" {
                    fs::rename(root.path().join(path), root.path().join("moved")).unwrap();
                    symlink(outside.path(), root.path().join(path)).unwrap();
                }
                assert_ne!(kind, TestEvent::Open);
            },
            None,
        )
        .unwrap();
    assert_eq!(queued.reasons, [Reason::UnsafePath]);
    assert_eq!(queued.read_bytes, 0);
    fixture(root.path(), &[("local", "NEEDLE")]);
    let leaf = workspace
        .search_case_with_budget(
            &request("needle", "", &["local"], false),
            Budget::default(),
            || false,
            |kind, path| {
                if kind == TestEvent::FileCheck {
                    fs::rename(root.path().join(path), root.path().join("retired")).unwrap();
                    symlink(outside.path().join("sentinel"), root.path().join(path)).unwrap();
                }
                assert_ne!(kind, TestEvent::Open);
            },
            None,
        )
        .unwrap();
    assert_eq!(leaf.reasons, [Reason::UnsafePath]);
    assert_eq!(leaf.read_bytes, 0);
}

#[cfg(unix)]
#[test]
fn case_non_utf8_path_is_incomplete_without_lossy_alias_or_content_open() {
    use std::os::unix::ffi::OsStringExt;
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join(std::ffi::OsString::from_vec(vec![255])),
        "NEEDLE",
    )
    .unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let result = workspace
        .search_case_with_budget(
            &request("needle", "", &[], false),
            Budget::default(),
            || false,
            |kind, _| assert_ne!(kind, TestEvent::Open),
            None,
        )
        .unwrap();
    assert_eq!(result.reasons, [Reason::UnrepresentablePath]);
    assert_eq!(result.read_bytes, 0);
}

#[cfg(windows)]
#[test]
fn case_windows_junction_is_excluded_and_rejected_as_explicit_scope() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fixture(outside.path(), &[("sentinel", "NEEDLE")]);
    let link = root.path().join("junction");
    let output = std::process::Command::new("cmd")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&link)
        .arg(outside.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "junction fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(is_link_or_reparse(&fs::symlink_metadata(&link).unwrap()));
    let workspace = Workspace::open(root.path()).unwrap();
    let result = workspace
        .search_case_with_budget(
            &request("needle", "", &[], false),
            Budget::default(),
            || false,
            |kind, _| assert_ne!(kind, TestEvent::Open),
            None,
        )
        .unwrap();
    assert!(result.matches.is_empty());
    assert!(!result.incomplete);
    assert_eq!(result.read_bytes, 0);
    assert_eq!(
        workspace
            .search_case(request("needle", "junction", &[], false))
            .unwrap_err()
            .code,
        "invalid_path"
    );
}
