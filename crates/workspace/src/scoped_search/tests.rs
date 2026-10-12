use super::*;

fn request(scope: &str, masks: &[&str]) -> ScopedSearchRequest {
    ScopedSearchRequest {
        query: "needle".into(),
        scope: scope.into(),
        masks: masks.iter().map(|m| (*m).into()).collect(),
        limit: 1000,
    }
}

fn bounded(
    workspace: &Workspace,
    request: &ScopedSearchRequest,
    budget: Budget,
) -> ScopedSearchResult {
    workspace
        .search_in_with_budget(request, budget, || false, |_, _| {})
        .unwrap()
}

#[test]
fn starts_at_scope_and_never_opens_outside_or_nonmatching_sentinels() {
    let root = tempfile::tempdir().unwrap();
    for path in [
        "outside.txt",
        "selected/one.rs",
        "selected/child/two.rs",
        "selected/not.txt",
        "selected/target/skip.rs",
    ] {
        let full = root.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, "needle\nNEEDLE\nsecond needle\n").unwrap();
    }
    let workspace = Workspace::open(root.path()).unwrap();
    let mut opened = Vec::new();
    let spec = request("selected", &["*.rs"]);
    let result = workspace
        .search_in_with_budget(
            &spec,
            Budget::default(),
            || false,
            |event, path| {
                if event == TestEvent::Open {
                    opened.push(path.to_owned());
                }
            },
        )
        .unwrap();
    opened.sort();
    assert_eq!(opened, ["selected/child/two.rs", "selected/one.rs"]);
    assert_eq!(
        result
            .matches
            .iter()
            .map(|r| (r.path.as_str(), r.line))
            .collect::<Vec<_>>(),
        [
            ("selected/child/two.rs", 1),
            ("selected/child/two.rs", 3),
            ("selected/one.rs", 1),
            ("selected/one.rs", 3)
        ]
    );
    assert_eq!(result.examined_entries, 5);
    assert!(!result.incomplete);
    assert_eq!(result.read_bytes, 56);
    result.validate_for(&spec).unwrap();
}

#[test]
fn scope_and_masks_keep_unicode_dotfiles_case_and_exact_spelling() {
    let root = tempfile::tempdir().unwrap();
    for name in [".a", "猫.rs", "猫狗.rs", "A.RS", "é.rs", "e\u{301}.rs"] {
        fs::write(root.path().join(name), "needle").unwrap();
    }
    let workspace = Workspace::open(root.path()).unwrap();
    let result = bounded(&workspace, &request("", &["?.rs", ".?"]), Budget::default());
    assert_eq!(
        result
            .matches
            .iter()
            .map(|r| r.path.as_str())
            .collect::<Vec<_>>(),
        [".a", "é.rs", "猫.rs"]
    );
    let all = bounded(&workspace, &request("", &["*"]), Budget::default());
    assert_eq!(all.matches.len(), 6);
    assert_eq!(all.request.masks, ["*"]);
}

#[test]
fn expected_binary_invalid_utf8_and_oversize_exclusions_count_actual_reads() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("binary"), b"needle\0ignored").unwrap();
    fs::write(root.path().join("invalid"), b"needle\xff").unwrap();
    File::create(root.path().join("oversize"))
        .unwrap()
        .set_len(MAX_FILE_BYTES as u64 + 1)
        .unwrap();
    fs::write(root.path().join("text"), "needle").unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let result = bounded(&workspace, &request("", &[]), Budget::default());
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].path, "text");
    assert_eq!(result.read_bytes, 14 + 7 + 6);
    assert!(!result.incomplete);
}

#[test]
fn read_budget_caps_every_actual_read_including_invalid_bytes_and_growth_probe() {
    let mut consumed = 0;
    let mut reader = io::Cursor::new(vec![255u8; 20]);
    assert!(matches!(
        read_scoped_text(&mut reader, &mut consumed, 7, &mut || false),
        FileRead::Stopped(Reason::ReadByteLimit)
    ));
    assert_eq!(consumed, 7);
    assert_eq!(reader.position(), 7);
    let mut consumed = 0;
    let mut reader = io::Cursor::new(vec![b'a'; MAX_FILE_BYTES + 50]);
    assert!(matches!(
        read_scoped_text(
            &mut reader,
            &mut consumed,
            MAX_SCOPED_SEARCH_READ_BYTES,
            &mut || false
        ),
        FileRead::Excluded
    ));
    assert_eq!(consumed, MAX_FILE_BYTES + 1);
    assert_eq!(reader.position(), (MAX_FILE_BYTES + 1) as u64);
    let mut consumed = 0;
    let mut reader = io::Cursor::new(vec![b'a'; MAX_FILE_BYTES]);
    assert!(matches!(
        read_scoped_text(
            &mut reader,
            &mut consumed,
            MAX_SCOPED_SEARCH_READ_BYTES,
            &mut || false
        ),
        FileRead::Text(_)
    ));
    assert_eq!(consumed, MAX_FILE_BYTES);
}

#[test]
fn read_error_permission_and_deadline_keep_partial_byte_accounting() {
    struct PartialFailure {
        sent: bool,
        kind: io::ErrorKind,
    }
    impl Read for PartialFailure {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if self.sent {
                return Err(io::Error::from(self.kind));
            }
            self.sent = true;
            bytes[..3].copy_from_slice(b"abc");
            Ok(3)
        }
    }
    for (kind, reason) in [
        (io::ErrorKind::PermissionDenied, Reason::PermissionDenied),
        (io::ErrorKind::Other, Reason::IoError),
    ] {
        let mut reader = PartialFailure { sent: false, kind };
        let mut consumed = 0;
        assert!(
            matches!(read_scoped_text(&mut reader,&mut consumed,100,&mut ||false),FileRead::Failure(found) if found == reason)
        );
        assert_eq!(consumed, 3);
    }
    let mut checks = 0;
    let mut consumed = 0;
    let mut reader = io::Cursor::new(vec![b'x'; 20_000]);
    assert!(matches!(
        read_scoped_text(&mut reader, &mut consumed, 100_000, &mut || {
            checks += 1;
            checks == 2
        }),
        FileRead::Stopped(Reason::TimeLimit)
    ));
    assert_eq!(consumed, 8192);
    assert_eq!(reader.position(), 8192);
}

#[test]
fn every_traversal_budget_is_independent_and_scope_depth_starts_at_zero() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("one/two")).unwrap();
    fs::write(root.path().join("one/two/text"), "needle").unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let spec = request("", &[]);
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
        let result = bounded(&workspace, &spec, budget);
        assert_eq!(result.reasons, [reason]);
        assert!(result.incomplete);
        if reason == Reason::EntryLimit {
            assert_eq!(result.examined_entries, 1);
        }
        if reason == Reason::ReadByteLimit {
            assert_eq!(result.read_bytes, 3);
        }
    }
    let result = workspace
        .search_in_with_budget(
            &spec,
            Budget::default(),
            || true,
            |_, _| panic!("deadline must stop before filesystem hook"),
        )
        .unwrap();
    assert_eq!(result.reasons, [Reason::TimeLimit]);
    assert_eq!(result.examined_entries, 0);
    assert_eq!(result.read_bytes, 0);
    let result = bounded(
        &workspace,
        &request("one/two", &[]),
        Budget {
            depth: 0,
            ..Budget::default()
        },
    );
    assert_eq!(result.matches[0].path, "one/two/text");
    assert!(!result.incomplete);
}

#[test]
fn result_limit_and_encoded_limit_are_reported_without_best_subset_promise() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("text"), "needle\n".repeat(5)).unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let mut spec = request("", &[]);
    spec.limit = 2;
    let result = bounded(&workspace, &spec, Budget::default());
    assert_eq!(result.matches.len(), 2);
    assert_eq!(result.reasons, [Reason::ResultLimit]);
    fs::write(
        root.path().join("text"),
        format!("needle{}\n", "\"".repeat(150)).repeat(5),
    )
    .unwrap();
    spec.limit = 1000;
    let result = bounded(
        &workspace,
        &spec,
        Budget {
            result_bytes: 700,
            ..Budget::default()
        },
    );
    assert_eq!(result.reasons, [Reason::ResponseLimit]);
    assert!(result.matches.len() < 5);
    assert!(result.encoded_bytes().unwrap() <= 700);
    assert_eq!(
        result.encoded_bytes().unwrap(),
        serde_json::to_vec(&result).unwrap().len()
    );
}

#[test]
fn long_query_can_be_missing_from_cropped_preview_without_rejecting_result() {
    let root = tempfile::tempdir().unwrap();
    let mut spec = request("", &[]);
    spec.query = "猫".repeat(1000);
    fs::write(
        root.path().join("text"),
        format!("{}{}", "x".repeat(1200), spec.query),
    )
    .unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let result = bounded(&workspace, &spec, Budget::default());
    assert_eq!(result.matches.len(), 1);
    assert!(!result.matches[0].text.contains(&spec.query));
    result.validate_for(&spec).unwrap();
}

#[test]
fn invalid_scopes_masks_and_excluded_ancestors_fail_before_any_walk() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    for scope in [
        "../x",
        "/x",
        "a//b",
        "a/./b",
        "a/../b",
        "a\\b",
        "C:/x",
        "target/child",
    ] {
        let spec = request(scope, &[]);
        assert!(workspace
            .search_in_with_budget(
                &spec,
                Budget::default(),
                || false,
                |_, _| panic!("invalid request must not walk")
            )
            .is_err());
    }
    for mask in ["", "foo;bar", "a/b", "a\\b", "[a]", "{a,b}", "!foo", "a\n"] {
        let spec = request("", &[mask]);
        assert!(workspace
            .search_in_with_budget(
                &spec,
                Budget::default(),
                || false,
                |_, _| panic!("invalid request must not walk")
            )
            .is_err());
    }
}

#[test]
fn directory_disappearance_and_leaf_replacement_report_io_or_unsafe() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("queued")).unwrap();
    fs::write(root.path().join("queued/text"), "needle").unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let result = workspace
        .search_in_with_budget(
            &request("", &[]),
            Budget::default(),
            || false,
            |event, path| {
                if event == TestEvent::Directory && path == "queued" {
                    fs::rename(root.path().join("queued"), root.path().join("moved")).unwrap();
                }
            },
        )
        .unwrap();
    assert!(result.matches.is_empty());
    assert_eq!(result.reasons, [Reason::IoError]);
    let result = workspace
        .search_in_with_budget(
            &request("moved", &[]),
            Budget::default(),
            || false,
            |event, path| {
                if event == TestEvent::FileCheck {
                    fs::rename(root.path().join(path), root.path().join("retired")).unwrap();
                    fs::create_dir(root.path().join(path)).unwrap();
                }
            },
        )
        .unwrap();
    assert!(result.matches.is_empty());
    assert_eq!(result.reasons, [Reason::UnsafePath]);
}

#[cfg(unix)]
#[test]
fn symlink_scope_ancestor_and_leaf_are_rejected_before_content_open() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("text"), "needle").unwrap();
    symlink(outside.path(), root.path().join("linked")).unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    assert_eq!(
        workspace
            .search_in(request("linked", &[]))
            .unwrap_err()
            .code,
        "invalid_path"
    );
    fs::create_dir(root.path().join("queued")).unwrap();
    let result = workspace
        .search_in_with_budget(
            &request("", &[]),
            Budget::default(),
            || false,
            |event, path| {
                if event == TestEvent::Directory && path == "queued" {
                    fs::rename(root.path().join(path), root.path().join("moved")).unwrap();
                    symlink(outside.path(), root.path().join(path)).unwrap();
                }
                assert_ne!(event, TestEvent::Open);
            },
        )
        .unwrap();
    assert_eq!(result.reasons, [Reason::UnsafePath]);
    fs::write(root.path().join("local"), "needle").unwrap();
    let result = workspace
        .search_in_with_budget(
            &request("", &["local"]),
            Budget::default(),
            || false,
            |event, path| {
                if event == TestEvent::FileCheck {
                    fs::rename(root.path().join(path), root.path().join("previous")).unwrap();
                    symlink(outside.path().join("text"), root.path().join(path)).unwrap();
                }
                assert_ne!(event, TestEvent::Open);
            },
        )
        .unwrap();
    assert!(result.matches.is_empty());
    assert_eq!(result.reasons, [Reason::UnsafePath]);
}

#[cfg(unix)]
#[test]
fn non_utf8_path_is_incomplete_without_lossy_alias_or_content_open() {
    use std::os::unix::ffi::OsStringExt;
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join(std::ffi::OsString::from_vec(vec![255])),
        "needle",
    )
    .unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let result = workspace
        .search_in_with_budget(
            &request("", &[]),
            Budget::default(),
            || false,
            |event, _| assert_ne!(event, TestEvent::Open),
        )
        .unwrap();
    assert_eq!(result.reasons, [Reason::UnrepresentablePath]);
    assert_eq!(result.read_bytes, 0);
}

#[cfg(windows)]
#[test]
fn windows_directory_junction_is_excluded_and_rejected_as_explicit_scope() {
    // Junction creation is unprivileged on ordinary Windows filesystems. This
    // fixture validates the actual reparse metadata policy without an opt-out.
    use std::process::Command;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("text"), "needle").unwrap();
    let link = root.path().join("junction");
    let output = Command::new("cmd")
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
        .search_in_with_budget(
            &request("", &[]),
            Budget::default(),
            || false,
            |event, _| assert_ne!(event, TestEvent::Open),
        )
        .unwrap();
    assert!(result.matches.is_empty());
    assert!(!result.incomplete);
    assert_eq!(result.read_bytes, 0);
    assert_eq!(
        workspace
            .search_in(request("junction", &[]))
            .unwrap_err()
            .code,
        "invalid_path"
    );
}

#[test]
fn long_scoped_paths_and_escaped_rows_share_the_whole_encoded_result_budget() {
    let root = tempfile::tempdir().unwrap();
    // Stay below Windows MAX_PATH for this fixture while making echoed scope,
    // workspace-relative paths and escaped row text all material to the cap.
    let scope = "scope".repeat(12);
    fs::create_dir(root.path().join(&scope)).unwrap();
    for i in 0..6 {
        fs::write(
            root.path().join(&scope).join(format!("file{i}")),
            format!("needle{}", "\"".repeat(70)),
        )
        .unwrap();
    }
    let workspace = Workspace::open(root.path()).unwrap();
    let spec = request(&scope, &[]);
    let result = bounded(
        &workspace,
        &spec,
        Budget {
            result_bytes: 750,
            ..Budget::default()
        },
    );
    assert_eq!(result.reasons, [Reason::ResponseLimit]);
    assert!(!result.matches.is_empty());
    assert!(result.matches.len() < 6);
    assert!(serde_json::to_vec(&result).unwrap().len() <= 750);
    result.validate_for(&spec).unwrap();
}

#[test]
fn hello_advertises_independent_descriptor_and_legacy_search_is_unchanged() {
    use cedar_protocol::Operation;
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("text"), "needle").unwrap();
    let mut workspace = Workspace::open(root.path()).unwrap();
    assert!(
        matches!(workspace.handle(Operation::Hello).unwrap(),Payload::Hello { scoped_search:Some(descriptor), .. } if descriptor.supported())
    );
    assert!(
        matches!(workspace.handle(Operation::Search { query:"needle".into(),limit:1 }).unwrap(),Payload::Matches { matches,truncated:false } if matches.len()==1)
    );
    assert!(
        matches!(workspace.handle(Operation::SearchIn {request:request("",&["*"])}).unwrap(),Payload::ScopedMatches {result} if result.matches.len()==1 && result.request.masks==["*"])
    );
}

#[cfg(windows)]
#[test]
fn windows_queued_directory_replaced_by_junction_cannot_open_outside_content() {
    use std::process::Command;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("text"), "needle").unwrap();
    fs::create_dir(root.path().join("queued")).unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let result = workspace
        .search_in_with_budget(
            &request("", &[]),
            Budget::default(),
            || false,
            |event, path| {
                if event == TestEvent::Directory && path == "queued" {
                    let queued = root.path().join(path);
                    fs::rename(&queued, root.path().join("moved")).unwrap();
                    let output = Command::new("cmd")
                        .args(["/D", "/C", "mklink", "/J"])
                        .arg(&queued)
                        .arg(outside.path())
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "junction fixture: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                assert_ne!(event, TestEvent::Open);
            },
        )
        .unwrap();
    assert!(result.matches.is_empty());
    assert_eq!(result.read_bytes, 0);
    assert_eq!(result.reasons, [Reason::UnsafePath]);
}

#[cfg(unix)]
#[test]
fn persistent_leaf_or_ancestor_replacement_at_open_is_rechecked_before_reading() {
    use std::os::unix::fs::symlink;
    for replace_ancestor in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("text"), "needle outside").unwrap();
        fs::create_dir(root.path().join("selected")).unwrap();
        fs::write(root.path().join("selected/text"), "needle local").unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let mut opened = 0;
        let result = workspace
            .search_in_with_budget(
                &request("selected", &[]),
                Budget::default(),
                || false,
                |event, path| {
                    if event == TestEvent::Open {
                        opened += 1;
                        if replace_ancestor {
                            fs::rename(root.path().join("selected"), root.path().join("moved"))
                                .unwrap();
                            symlink(outside.path(), root.path().join("selected")).unwrap();
                        } else {
                            fs::rename(root.path().join(path), root.path().join("previous"))
                                .unwrap();
                            symlink(outside.path().join("text"), root.path().join(path)).unwrap();
                        }
                    }
                },
            )
            .unwrap();
        assert_eq!(opened, 1);
        assert_eq!(result.read_bytes, 0);
        assert!(result.matches.is_empty());
        assert_eq!(result.reasons, [Reason::UnsafePath]);
    }
}
