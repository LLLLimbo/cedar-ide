use super::*;
use cedar_protocol::{Operation, MAX_FILE_BYTES};

fn find(workspace: &Workspace, query: &str, scope: &str, limit: usize) -> FileDiscoveryResult {
    let Payload::FilePaths { result } = workspace.find_files(query, scope, limit).unwrap() else {
        panic!("discovery payload");
    };
    result
}

fn bounded(workspace: &Workspace, budget: Budget) -> FileDiscoveryResult {
    workspace
        .find_files_with_budget("match", "", 100, budget, || false, |_| {})
        .unwrap()
}

#[test]
fn discovers_untouched_paths_and_ranks_exact_prefix_substring_then_path() {
    let root = tempfile::tempdir().unwrap();
    for path in [
        "z/match",
        "a/MATCH",
        "prefix/match_more",
        "b/hasmatch",
        "matchdir/other",
        "无关/你好.txt",
    ] {
        fs::create_dir_all(root.path().join(path).parent().unwrap()).unwrap();
        fs::write(root.path().join(path), b"not searched").unwrap();
    }
    let workspace = Workspace::open(root.path()).unwrap();
    let result = find(&workspace, "MaTcH", "", 100);
    assert_eq!(
        result.paths,
        [
            "a/MATCH",
            "z/match",
            "prefix/match_more",
            "b/hasmatch",
            "matchdir/other"
        ]
    );
    assert!(!result.incomplete);
    assert!(result.reasons.is_empty());
    assert_eq!(find(&workspace, "你好", "", 100).paths, ["无关/你好.txt"]);
    assert_eq!(find(&workspace, "match", "z", 100).paths, ["z/match"]);
    assert_eq!(find(&workspace, "missing", "", 100).paths.len(), 0);
}

#[test]
fn discovers_binary_and_large_files_while_read_stays_authoritative() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("match.bin"), [0, 1, 255]).unwrap();
    fs::File::create(root.path().join("match.large"))
        .unwrap()
        .set_len(MAX_FILE_BYTES as u64 + 1)
        .unwrap();
    let mut workspace = Workspace::open(root.path()).unwrap();
    assert_eq!(
        find(&workspace, "match", "", 100).paths,
        ["match.bin", "match.large"]
    );
    assert!(workspace
        .handle(Operation::Read {
            path: "match.bin".into()
        })
        .is_err());
    assert_eq!(
        workspace
            .handle(Operation::Read {
                path: "match.large".into()
            })
            .unwrap_err()
            .code,
        "file_too_large"
    );
    // The workspace was never granted execution trust.
    assert_eq!(
        workspace
            .handle(Operation::Run {
                program: "never".into(),
                args: vec![],
                timeout_secs: 1
            })
            .unwrap_err()
            .code,
        "run_disabled"
    );
}

#[test]
fn exclusions_apply_to_nested_directories_and_explicit_scope_ancestors() {
    let root = tempfile::tempdir().unwrap();
    for excluded in FILE_DISCOVERY_EXCLUDED_DIRECTORIES {
        fs::create_dir_all(root.path().join("src").join(excluded).join("nested")).unwrap();
        fs::write(
            root.path().join("src").join(excluded).join("nested/match"),
            "",
        )
        .unwrap();
    }
    fs::write(root.path().join("match"), "").unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let result = find(&workspace, "match", "", 100);
    assert_eq!(result.paths, ["match"]);
    assert!(
        !result.incomplete,
        "declared exclusions do not falsely claim a failed scan"
    );
    for excluded in FILE_DISCOVERY_EXCLUDED_DIRECTORIES {
        assert_eq!(
            workspace
                .find_files("match", &format!("src/{excluded}/nested"), 100)
                .unwrap_err()
                .code,
            "excluded_scope"
        );
    }
}

#[test]
fn result_limit_keeps_the_best_observed_candidates_instead_of_stopping() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..12 {
        fs::write(root.path().join(format!("hasmatch_{index:02}")), "").unwrap();
    }
    fs::create_dir(root.path().join("last")).unwrap();
    fs::write(root.path().join("last/match"), "").unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let result = find(&workspace, "match", "", 1);
    assert_eq!(result.paths, ["last/match"]);
    assert_eq!(result.examined_entries, 14);
    assert_eq!(result.reasons, [Reason::ResultLimit]);
    assert!(result.incomplete);
}

#[test]
fn entry_depth_stack_and_deadline_caps_are_independent_and_explicit() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("one/two")).unwrap();
    fs::write(root.path().join("one/two/match"), "").unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let entries = bounded(
        &workspace,
        Budget {
            entries: 1,
            ..Budget::default()
        },
    );
    assert_eq!(entries.examined_entries, 1);
    assert_eq!(entries.reasons, [Reason::EntryLimit]);
    let depth = bounded(
        &workspace,
        Budget {
            depth: 0,
            ..Budget::default()
        },
    );
    assert_eq!(depth.reasons, [Reason::DepthLimit]);
    let count = bounded(
        &workspace,
        Budget {
            pending_dirs: 0,
            ..Budget::default()
        },
    );
    assert_eq!(count.reasons, [Reason::StackLimit]);
    let bytes = bounded(
        &workspace,
        Budget {
            pending_bytes: 2,
            ..Budget::default()
        },
    );
    assert_eq!(bytes.reasons, [Reason::StackLimit]);
    let time = workspace
        .find_files_with_budget("match", "", 100, Budget::default(), || true, |_| {})
        .unwrap();
    assert_eq!(time.examined_entries, 0);
    assert_eq!(time.reasons, [Reason::TimeLimit]);
    assert!(
        time.paths.is_empty() && time.incomplete,
        "incomplete no-match is distinct from an exhaustive miss"
    );
    assert_eq!(
        workspace
            .find_files_with_budget(
                "match",
                "one/two",
                100,
                Budget {
                    depth: 0,
                    ..Budget::default()
                },
                || false,
                |_| {}
            )
            .unwrap()
            .paths,
        ["one/two/match"]
    );
}

#[test]
fn response_budget_counts_escaping_and_added_reason_metadata() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..8 {
        fs::write(
            root.path()
                .join(format!("match_{index}_{}", "long".repeat(20))),
            "",
        )
        .unwrap();
    }
    let workspace = Workspace::open(root.path()).unwrap();
    let result = bounded(
        &workspace,
        Budget {
            result_bytes: 350,
            ..Budget::default()
        },
    );
    assert_eq!(result.reasons, [Reason::ResponseLimit]);
    assert!(result.paths.len() < 8);
    assert!(serde_json::to_vec(&result).unwrap().len() <= 350);
    assert_eq!(
        result.encoded_bytes().unwrap(),
        serde_json::to_vec(&result).unwrap().len()
    );
}

#[test]
fn one_deadline_is_observed_mid_ancestor_resolution_before_more_io() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("one/two/three")).unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let mut checks = 0;
    let failure = workspace
        .resolve_discovery_path("one/two/three", &mut || {
            checks += 1;
            checks >= 4
        })
        .unwrap_err();
    assert_eq!(failure.code, "discovery_time_limit");
    assert_eq!(checks, 4, "no renewed deadline or additional ancestor work");
    let mut checks = 0;
    let result = workspace
        .find_files_with_budget(
            "match",
            "one/two/three",
            100,
            Budget::default(),
            || {
                checks += 1;
                checks >= 5
            },
            |_| {},
        )
        .unwrap();
    assert_eq!(result.reasons, [Reason::TimeLimit]);
    assert_eq!(result.examined_entries, 0);
}

#[cfg(unix)]
#[test]
fn escaped_filename_bytes_participate_in_the_response_budget() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..5 {
        fs::write(
            root.path().join(format!("match{index}{}", "\"".repeat(80))),
            "",
        )
        .unwrap();
    }
    let workspace = Workspace::open(root.path()).unwrap();
    let result = bounded(
        &workspace,
        Budget {
            result_bytes: 350,
            ..Budget::default()
        },
    );
    assert_eq!(result.reasons, [Reason::ResponseLimit]);
    assert_eq!(result.paths.len(), 1);
    assert!(result.encoded_bytes().unwrap() <= 350);
    assert!(result.paths[0].contains('"'));
}

#[test]
fn queued_directory_is_revalidated_and_failures_are_bounded_categories() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("queued")).unwrap();
    fs::write(root.path().join("queued/match"), "").unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let result = workspace
        .find_files_with_budget(
            "match",
            "",
            100,
            Budget::default(),
            || false,
            |path| {
                if path == "queued" {
                    fs::rename(root.path().join("queued"), root.path().join("moved")).unwrap();
                }
            },
        )
        .unwrap();
    assert!(result.paths.is_empty());
    assert_eq!(result.reasons, [Reason::IoError]);
    assert_eq!(
        reason_for_io(&io::Error::from(io::ErrorKind::PermissionDenied)),
        Reason::PermissionDenied
    );
    assert_eq!(
        reason_for_error(&error("invalid_path", "fixture")),
        Reason::UnsafePath
    );
}

#[test]
fn invalid_queries_scopes_and_limits_do_not_walk() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    for query in ["", "a\nb", "a\0b", "a\u{2028}b", &"a".repeat(257)] {
        assert_eq!(
            workspace.find_files(query, "", 100).unwrap_err().code,
            "invalid_query"
        );
    }
    for scope in [
        "../escape",
        "/absolute",
        "a//b",
        "a/./b",
        "C:/drive",
        "a\\b",
    ] {
        assert_eq!(
            workspace.find_files("match", scope, 100).unwrap_err().code,
            "invalid_path"
        );
    }
    assert_eq!(
        workspace.find_files("match", "", 0).unwrap_err().code,
        "invalid_limit"
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_names_are_incomplete_and_links_are_never_followed() {
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("match"), "private").unwrap();
    symlink(outside.path(), root.path().join("linked")).unwrap();
    symlink(outside.path().join("match"), root.path().join("match-link")).unwrap();
    fs::write(
        root.path().join(std::ffi::OsString::from_vec(vec![255])),
        "",
    )
    .unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let result = find(&workspace, "match", "", 100);
    assert!(result.paths.is_empty());
    assert_eq!(result.reasons, [Reason::UnrepresentablePath]);
    assert_eq!(
        workspace
            .find_files("match", "linked", 100)
            .unwrap_err()
            .code,
        "invalid_path"
    );
}

#[cfg(unix)]
#[test]
fn replacing_a_queued_ancestor_with_a_link_cannot_escape() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("queued")).unwrap();
    fs::write(outside.path().join("match"), "private").unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let result = workspace
        .find_files_with_budget(
            "match",
            "",
            100,
            Budget::default(),
            || false,
            |path| {
                if path == "queued" {
                    fs::remove_dir(root.path().join("queued")).unwrap();
                    symlink(outside.path(), root.path().join("queued")).unwrap();
                }
            },
        )
        .unwrap();
    assert!(result.paths.is_empty());
    assert_eq!(result.reasons, [Reason::UnsafePath]);
}
