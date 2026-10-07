use cedar_protocol::{Operation, Payload, MAX_FILE_BYTES};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use cedar_workspace::MAX_COMMAND_OUTPUT_BYTES;
use cedar_workspace::{Workspace, MAX_COMMAND_TIMEOUT_SECS};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn workspace() -> (TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::open(dir.path()).unwrap();
    (dir, ws)
}
fn read(ws: &mut Workspace, path: &str) -> (String, String) {
    match ws.handle(Operation::Read { path: path.into() }).unwrap() {
        Payload::File { text, revision, .. } => (text, revision),
        other => panic!("{other:?}"),
    }
}
fn write(
    ws: &mut Workspace,
    path: &str,
    text: &str,
    expected: Option<String>,
) -> Result<Payload, cedar_protocol::RemoteError> {
    ws.handle(Operation::Write {
        path: path.into(),
        text: text.into(),
        expected_revision: expected,
    })
}

fn written_revision(payload: Payload, text: &str) -> String {
    match payload {
        Payload::Written { revision } => {
            assert_eq!(revision, format!("{:x}", Sha256::digest(text.as_bytes())));
            revision
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn opens_only_existing_directories() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("file"), "x").unwrap();
    assert_eq!(
        Workspace::open(dir.path().join("file")).unwrap_err().code,
        "not_directory"
    );
    assert_eq!(
        Workspace::open(dir.path().join("missing"))
            .unwrap_err()
            .code,
        "not_found"
    );
}

#[test]
fn hello_and_sorted_listing() {
    let (dir, mut ws) = workspace();
    fs::create_dir(dir.path().join("zdir")).unwrap();
    fs::write(dir.path().join("beta"), "").unwrap();
    fs::write(dir.path().join("alpha"), "").unwrap();
    match ws.handle(Operation::Hello).unwrap() {
        Payload::Hello { protocol, root } => {
            assert_eq!(protocol, cedar_protocol::PROTOCOL_VERSION);
            assert_eq!(root, dir.path().canonicalize().unwrap().to_string_lossy());
        }
        other => panic!("{other:?}"),
    }
    for path in ["", ".", "./"] {
        match ws.handle(Operation::List { path: path.into() }).unwrap() {
            Payload::Entries { entries } => {
                assert_eq!(
                    entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
                    ["zdir", "alpha", "beta"]
                );
                assert!(entries[0].is_dir);
                assert_eq!(entries[1].path, "alpha");
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn rejects_all_traversal_and_platform_prefix_forms() {
    let (_dir, mut ws) = workspace();
    for path in [
        "../outside",
        "a/../../outside",
        "a/../b",
        "/etc/passwd",
        "C:/Windows",
        "C:relative",
        "\\\\server\\share",
        "a\\b",
        "a\0b",
        "foo:stream",
    ] {
        for op in [
            Operation::Read { path: path.into() },
            Operation::List { path: path.into() },
            Operation::Write {
                path: path.into(),
                text: "x".into(),
                expected_revision: None,
            },
        ] {
            assert_eq!(ws.handle(op).unwrap_err().code, "invalid_path", "{path:?}");
        }
    }
}

#[test]
fn reads_utf8_and_sha256_revisions_and_round_trips_new_files() {
    let (dir, mut ws) = workspace();
    fs::create_dir(dir.path().join("src")).unwrap();
    write(&mut ws, "src/你好.rs", "你好 🌲\r\n", None).unwrap();
    let (text, revision) = read(&mut ws, "src/你好.rs");
    assert_eq!(text, "你好 🌲\r\n");
    assert_eq!(revision.len(), 64);
    assert_eq!(
        fs::read_to_string(dir.path().join("src/你好.rs")).unwrap(),
        text
    );
    write(&mut ws, "hash", "abc", None).unwrap();
    assert_eq!(
        read(&mut ws, "hash").1,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn writes_require_fresh_revision_and_never_blindly_overwrite() {
    let (dir, mut ws) = workspace();
    write(&mut ws, "file", "first", None).unwrap();
    let (_, first_revision) = read(&mut ws, "file");
    assert_eq!(
        write(&mut ws, "file", "clobber", None).unwrap_err().code,
        "conflict"
    );
    assert_eq!(
        write(&mut ws, "file", "stale", Some("bogus".into()))
            .unwrap_err()
            .code,
        "conflict"
    );
    fs::write(dir.path().join("file"), "external edit").unwrap();
    assert_eq!(
        write(&mut ws, "file", "clobber", Some(first_revision))
            .unwrap_err()
            .code,
        "conflict"
    );
    assert_eq!(read(&mut ws, "file").0, "external edit");
    let (_, revision) = read(&mut ws, "file");
    write(&mut ws, "file", "saved", Some(revision)).unwrap();
    assert_eq!(read(&mut ws, "file").0, "saved");
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        1,
        "temporary files cleaned up"
    );
}

#[test]
fn removed_file_is_a_conflict_not_implicit_recreation() {
    let (dir, mut ws) = workspace();
    write(&mut ws, "file", "before", None).unwrap();
    let (_, rev) = read(&mut ws, "file");
    fs::remove_file(dir.path().join("file")).unwrap();
    assert_eq!(
        write(&mut ws, "file", "after", Some(rev)).unwrap_err().code,
        "conflict"
    );
    assert!(!dir.path().join("file").exists());
    assert_eq!(
        write(&mut ws, "missing/child", "x", None).unwrap_err().code,
        "not_found"
    );
}

#[test]
fn replacement_keeps_held_reader_on_old_bytes_and_new_opens_on_new_bytes() {
    let (dir, mut ws) = workspace();
    let path = dir.path().join("held.txt");
    let old = "original complete file 你好\r\n";
    let new = "replacement complete file 🦀\n";
    let old_revision = written_revision(write(&mut ws, "held.txt", old, None).unwrap(), old);
    let mut held_reader = File::open(&path).unwrap();
    let new_revision = written_revision(
        write(&mut ws, "held.txt", new, Some(old_revision)).unwrap(),
        new,
    );
    let mut held_bytes = Vec::new();
    held_reader.read_to_end(&mut held_bytes).unwrap();
    assert_eq!(held_bytes, old.as_bytes());
    assert_eq!(fs::read(&path).unwrap(), new.as_bytes());
    assert_eq!(read(&mut ws, "held.txt"), (new.into(), new_revision));
    drop(held_reader);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn concurrent_readers_never_observe_partial_or_missing_files_and_every_save_succeeds() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    let (dir, mut ws) = workspace();
    let a = "A🦀\r\n".repeat(4000);
    let b = "B你好\n".repeat(5000);
    let mut revision = written_revision(write(&mut ws, "file", &a, None).unwrap(), &a);
    let path = dir.path().join("file");
    let a_bytes = a.as_bytes().to_owned();
    let b_bytes = b.as_bytes().to_owned();
    let running = Arc::new(AtomicBool::new(true));
    let reads = Arc::new(AtomicUsize::new(0));
    let active = running.clone();
    let reader_count = reads.clone();
    let (started_sender, started_receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut started = Some(started_sender);
        while active.load(Ordering::Acquire) {
            let bytes = fs::read(&path).expect("a reader must never observe a missing file");
            assert!(
                bytes == a_bytes || bytes == b_bytes,
                "partial/interleaved file"
            );
            reader_count.fetch_add(1, Ordering::Relaxed);
            if let Some(sender) = started.take() {
                sender.send(()).unwrap();
            }
        }
    });
    let started = started_receiver.recv_timeout(Duration::from_secs(15));
    let mut failure = None;
    let mut saved = 0;
    if started.is_ok() {
        for sequence in 0..64 {
            let contents = if sequence % 2 == 0 { &b } else { &a };
            match write(&mut ws, "file", contents, Some(revision.clone())) {
                Ok(payload) => {
                    // Defer assertions until the reader is stopped and joined.
                    if let Payload::Written { revision: next } = payload {
                        if next != format!("{:x}", Sha256::digest(contents.as_bytes())) {
                            failure = Some("save returned the wrong SHA-256".to_owned());
                            break;
                        }
                        revision = next;
                        saved += 1;
                    } else {
                        failure = Some(format!("unexpected save response: {payload:?}"));
                        break;
                    }
                }
                Err(error) => {
                    failure = Some(format!("save failed with an ordinary reader: {error:?}"));
                    break;
                }
            }
        }
    }
    // Stop and join even on failure, before assertions can drop the directory.
    running.store(false, Ordering::Release);
    reader.join().unwrap();
    started.unwrap();
    assert!(failure.is_none(), "{}", failure.unwrap_or_default());
    assert_eq!(saved, 64);
    assert!(reads.load(Ordering::Relaxed) > 0);
    assert_eq!(read(&mut ws, "file"), (a, revision));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn deny_delete_sharing_preserves_old_file_cleans_temporary_and_retry_succeeds() {
    use std::os::windows::fs::OpenOptionsExt;

    let (dir, mut ws) = workspace();
    let path = dir.path().join("blocked.txt");
    let old = "original safe file 你好";
    let new = "new complete file 🦀";
    let revision = written_revision(write(&mut ws, "blocked.txt", old, None).unwrap(), old);
    let mut held_reader = File::options()
        .read(true)
        .share_mode(0x1 | 0x2) // FILE_SHARE_READ | FILE_SHARE_WRITE, no DELETE
        .open(&path)
        .unwrap();
    let failure = write(&mut ws, "blocked.txt", new, Some(revision.clone())).unwrap_err();
    assert!(matches!(
        failure.code.as_str(),
        "permission_denied" | "io_error"
    ));
    assert_eq!(fs::read(&path).unwrap(), old.as_bytes());
    assert_eq!(read(&mut ws, "blocked.txt"), (old.into(), revision.clone()));
    let mut held_bytes = Vec::new();
    held_reader.read_to_end(&mut held_bytes).unwrap();
    assert_eq!(held_bytes, old.as_bytes());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    drop(held_reader);
    let saved = written_revision(
        write(&mut ws, "blocked.txt", new, Some(revision)).unwrap(),
        new,
    );
    assert_eq!(read(&mut ws, "blocked.txt"), (new.into(), saved));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn new_and_replaced_files_do_not_keep_the_windows_temporary_attribute() {
    use std::os::windows::fs::MetadataExt;

    let (dir, mut ws) = workspace();
    let mut revision = None;
    for contents in ["first", "replacement", "another replacement"] {
        revision = Some(written_revision(
            write(&mut ws, "file", contents, revision).unwrap(),
            contents,
        ));
        assert_eq!(
            fs::metadata(dir.path().join("file"))
                .unwrap()
                .file_attributes()
                & 0x100,
            0,
            "persisted file must not retain FILE_ATTRIBUTE_TEMPORARY"
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[cfg(windows)]
#[test]
fn read_only_windows_file_is_denied_and_unchanged() {
    let (dir, mut ws) = workspace();
    let path = dir.path().join("file");
    let revision = written_revision(write(&mut ws, "file", "before", None).unwrap(), "before");
    let original_permissions = fs::metadata(&path).unwrap().permissions();
    let mut read_only = original_permissions.clone();
    read_only.set_readonly(true);
    fs::set_permissions(&path, read_only).unwrap();
    let result = write(&mut ws, "file", "after", Some(revision.clone()));
    let actual = fs::read(&path).unwrap();
    let permissions = fs::metadata(&path).unwrap().permissions();
    // Restore before asserting so even a regression leaves a removable fixture.
    fs::set_permissions(&path, original_permissions).unwrap();
    assert_eq!(result.unwrap_err().code, "permission_denied");
    assert_eq!(actual, b"before");
    assert!(permissions.readonly());
    assert_eq!(read(&mut ws, "file"), ("before".into(), revision));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn rejects_binary_invalid_utf8_and_oversized_files() {
    let (dir, mut ws) = workspace();
    fs::write(dir.path().join("bytes"), [0xff, 0xfe]).unwrap();
    fs::write(dir.path().join("binary"), b"hello\0world").unwrap();
    fs::write(dir.path().join("large"), vec![b'a'; MAX_FILE_BYTES + 1]).unwrap();
    for (path, code) in [
        ("bytes", "invalid_utf8"),
        ("binary", "binary_file"),
        ("large", "file_too_large"),
    ] {
        assert_eq!(
            ws.handle(Operation::Read { path: path.into() })
                .unwrap_err()
                .code,
            code
        );
    }
    assert_eq!(
        write(&mut ws, "new", &"a".repeat(MAX_FILE_BYTES + 1), None)
            .unwrap_err()
            .code,
        "file_too_large"
    );
    assert_eq!(
        write(&mut ws, "new", "a\0b", None).unwrap_err().code,
        "binary_file"
    );
    assert!(!dir.path().join("new").exists());
    write(&mut ws, "boundary", &"a".repeat(MAX_FILE_BYTES), None).unwrap();
    assert_eq!(read(&mut ws, "boundary").0.len(), MAX_FILE_BYTES);
}

#[test]
fn rejects_directories_and_reports_missing_files() {
    let (_dir, mut ws) = workspace();
    assert_eq!(
        ws.handle(Operation::Read { path: ".".into() })
            .unwrap_err()
            .code,
        "invalid_path"
    );
    assert_eq!(
        write(&mut ws, ".", "x", None).unwrap_err().code,
        "invalid_path"
    );
    assert_eq!(
        ws.handle(Operation::Read {
            path: "gone".into()
        })
        .unwrap_err()
        .code,
        "not_found"
    );
}

#[test]
fn search_is_literal_case_sensitive_numbered_and_skips_generated_dirs() {
    let (dir, mut ws) = workspace();
    fs::write(
        dir.path().join("source.rs"),
        "first\nneedle one\nNeedle two\nneedle three\n",
    )
    .unwrap();
    fs::write(dir.path().join("invalid"), [0xff, 0xff]).unwrap();
    fs::write(dir.path().join("binary"), b"needle\0").unwrap();
    for skipped in [".git", "target", "build", "node_modules", "dist"] {
        fs::create_dir(dir.path().join(skipped)).unwrap();
        fs::write(dir.path().join(skipped).join("ignored"), "needle").unwrap();
    }
    match ws
        .handle(Operation::Search {
            query: "needle".into(),
            limit: 50,
        })
        .unwrap()
    {
        Payload::Matches { matches, truncated } => {
            assert!(!truncated);
            assert_eq!(matches.len(), 2);
            assert_eq!(matches[0].line, 2);
            assert_eq!(matches[1].line, 4);
            assert!(matches.iter().all(|m| m.path == "source.rs"));
        }
        other => panic!("{other:?}"),
    }
    match ws
        .handle(Operation::Search {
            query: "needle".into(),
            limit: 1,
        })
        .unwrap()
    {
        Payload::Matches { matches, truncated } => {
            assert_eq!(matches.len(), 1);
            assert!(truncated);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn search_limits_long_unicode_lines_and_reports_invalid_queries() {
    let (dir, mut ws) = workspace();
    fs::write(
        dir.path().join("long"),
        format!("{}needle{}", "🌲".repeat(1000), "🌲".repeat(1000)),
    )
    .unwrap();
    match ws
        .handle(Operation::Search {
            query: "needle".into(),
            limit: usize::MAX,
        })
        .unwrap()
    {
        Payload::Matches { matches, truncated } => {
            assert!(!truncated);
            assert_eq!(matches.len(), 1);
            assert!(matches[0].text.contains("needle"));
            assert!(matches[0].text.len() < 2100);
        }
        other => panic!("{other:?}"),
    }
    for query in ["", "a\nb", "a\rb", "a\0b"] {
        assert_eq!(
            ws.handle(Operation::Search {
                query: query.into(),
                limit: 1
            })
            .unwrap_err()
            .code,
            "invalid_query"
        );
    }
    assert_eq!(
        ws.handle(Operation::Search {
            query: "needle".into(),
            limit: 0
        })
        .unwrap_err()
        .code,
        "invalid_limit"
    );
}

#[test]
fn commands_are_explicitly_opt_in_and_arguments_are_bounded() {
    let (_dir, mut ws) = workspace();
    assert_eq!(
        ws.handle(Operation::Run {
            program: "anything".into(),
            args: vec![],
            timeout_secs: 1
        })
        .unwrap_err()
        .code,
        "run_disabled"
    );
    ws.set_allow_run(true);
    for timeout in [0, MAX_COMMAND_TIMEOUT_SECS + 1] {
        assert_eq!(
            ws.handle(Operation::Run {
                program: "anything".into(),
                args: vec![],
                timeout_secs: timeout
            })
            .unwrap_err()
            .code,
            "invalid_timeout"
        );
    }
    for (program, args) in [
        ("", vec![]),
        ("x\0y", vec![]),
        ("x", vec!["a\0b".into()]),
        ("x", vec!["a".into(); 257]),
    ] {
        assert_eq!(
            ws.handle(Operation::Run {
                program: program.into(),
                args,
                timeout_secs: 1
            })
            .unwrap_err()
            .code,
            "invalid_command"
        );
    }
    assert_eq!(
        ws.handle(Operation::Run {
            program: "cedar-nonexistent-program-7eb26ba8".into(),
            args: vec![],
            timeout_secs: 1
        })
        .unwrap_err()
        .code,
        if cfg!(any(target_os = "linux", target_os = "macos")) {
            "command_failed"
        } else {
            "unsupported_platform"
        }
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn symlinks_to_inside_outside_and_missing_targets_are_rejected() {
        let (dir, mut ws) = workspace();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret"), "secret").unwrap();
        fs::write(dir.path().join("real"), "inside").unwrap();
        symlink(outside.path(), dir.path().join("escape")).unwrap();
        symlink("real", dir.path().join("inside_link")).unwrap();
        symlink(outside.path().join("missing"), dir.path().join("dangling")).unwrap();
        for path in ["escape/secret", "escape/new", "inside_link", "dangling"] {
            assert_eq!(
                ws.handle(Operation::Read { path: path.into() })
                    .unwrap_err()
                    .code,
                "invalid_path"
            );
            assert_eq!(
                write(&mut ws, path, "overwrite", None).unwrap_err().code,
                "invalid_path"
            );
        }
        match ws.handle(Operation::List { path: "".into() }).unwrap() {
            Payload::Entries { entries } => assert_eq!(entries.len(), 1),
            other => panic!("{other:?}"),
        }
        match ws
            .handle(Operation::Search {
                query: "secret".into(),
                limit: 50,
            })
            .unwrap()
        {
            Payload::Matches { matches, .. } => assert!(matches.is_empty()),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            fs::read_to_string(outside.path().join("secret")).unwrap(),
            "secret"
        );
        assert!(!outside.path().join("missing").exists());
    }

    #[test]
    fn atomic_replacement_preserves_mode_and_does_not_follow_hardlink() {
        let (dir, mut ws) = workspace();
        fs::write(dir.path().join("file"), "before").unwrap();
        fs::set_permissions(dir.path().join("file"), fs::Permissions::from_mode(0o751)).unwrap();
        fs::hard_link(dir.path().join("file"), dir.path().join("other")).unwrap();
        let (_, rev) = read(&mut ws, "file");
        write(&mut ws, "file", "after", Some(rev)).unwrap();
        assert_eq!(
            fs::metadata(dir.path().join("file"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o751
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("other")).unwrap(),
            "before"
        );
        assert_eq!(read(&mut ws, "file").0, "after");
    }

    #[test]
    fn read_only_files_are_not_replaced_even_if_parent_is_writable() {
        let (dir, mut ws) = workspace();
        fs::write(dir.path().join("file"), "before").unwrap();
        fs::set_permissions(dir.path().join("file"), fs::Permissions::from_mode(0o444)).unwrap();
        let (_, rev) = read(&mut ws, "file");
        assert_eq!(
            write(&mut ws, "file", "after", Some(rev)).unwrap_err().code,
            "permission_denied"
        );
        assert_eq!(read(&mut ws, "file").0, "before");
    }

    fn run(ws: &mut Workspace, program: &str, args: &[&str], timeout_secs: u64) -> Payload {
        ws.set_allow_run(true);
        ws.handle(Operation::Run {
            program: program.into(),
            args: args.iter().map(|s| (*s).into()).collect(),
            timeout_secs,
        })
        .unwrap()
    }

    #[test]
    fn commands_keep_arguments_literal_and_have_workspace_cwd() {
        let (dir, mut ws) = workspace();
        match run(
            &mut ws,
            "/bin/echo",
            &["$(touch SHOULD_NOT_EXIST); && literal"],
            2,
        ) {
            Payload::Run {
                stdout,
                exit_code,
                timed_out,
                truncated,
                ..
            } => {
                assert_eq!(stdout, "$(touch SHOULD_NOT_EXIST); && literal\n");
                assert_eq!(exit_code, Some(0));
                assert!(!timed_out);
                assert!(!truncated);
            }
            other => panic!("{other:?}"),
        }
        assert!(!dir.path().join("SHOULD_NOT_EXIST").exists());
        match run(&mut ws, "/bin/pwd", &[], 2) {
            Payload::Run { stdout, .. } => assert_eq!(
                stdout.trim(),
                dir.path().canonicalize().unwrap().to_string_lossy()
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn command_captures_both_streams_and_nonzero_exit() {
        let (_dir, mut ws) = workspace();
        match run(
            &mut ws,
            "/bin/sh",
            &["-c", "printf out; printf err >&2; exit 7"],
            2,
        ) {
            Payload::Run {
                stdout,
                stderr,
                exit_code,
                timed_out,
                truncated,
            } => {
                assert_eq!(stdout, "out");
                assert_eq!(stderr, "err");
                assert_eq!(exit_code, Some(7));
                assert!(!timed_out);
                assert!(!truncated);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn timeout_kills_process_group_without_waiting_for_inherited_pipes() {
        let (dir, mut ws) = workspace();
        let start = Instant::now();
        match run(
            &mut ws,
            "/bin/sh",
            &["-c", "(sleep 2; touch escaped) & wait"],
            1,
        ) {
            Payload::Run { timed_out, .. } => assert!(timed_out),
            other => panic!("{other:?}"),
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(1300));
        assert!(
            !dir.path().join("escaped").exists(),
            "descendant survived group termination"
        );
    }

    #[test]
    fn completed_commands_do_not_leave_background_descendants() {
        let (dir, mut ws) = workspace();
        let start = Instant::now();
        match run(
            &mut ws,
            "/bin/sh",
            &["-c", "(sleep 1; touch escaped) & exit 0"],
            3,
        ) {
            Payload::Run {
                timed_out,
                exit_code,
                ..
            } => {
                assert!(!timed_out);
                assert_eq!(exit_code, Some(0));
            }
            other => panic!("{other:?}"),
        }
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(1200));
        assert!(!dir.path().join("escaped").exists());
    }

    #[test]
    fn runaway_stdout_is_capped_and_command_terminated_early() {
        let (_dir, mut ws) = workspace();
        let start = Instant::now();
        match run(
            &mut ws,
            "/bin/sh",
            &[
                "-c",
                "while :; do printf '1234567890123456789012345678901234567890'; done",
            ],
            5,
        ) {
            Payload::Run {
                stdout,
                truncated,
                timed_out,
                ..
            } => {
                assert_eq!(stdout.len(), MAX_COMMAND_OUTPUT_BYTES);
                assert!(truncated);
                assert!(!timed_out);
            }
            other => panic!("{other:?}"),
        }
        assert!(start.elapsed() < Duration::from_secs(4));
    }
}

#[test]
fn simultaneous_new_file_creation_has_exactly_one_winner() {
    use std::sync::{Arc, Barrier};
    for _ in 0..12 {
        let dir = tempfile::tempdir().unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();
        for contents in ["left", "right"] {
            let path = dir.path().to_owned();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                let mut ws = Workspace::open(path).unwrap();
                barrier.wait();
                (contents, write(&mut ws, "new", contents, None))
            }));
        }
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(
            results.iter().filter(|(_, result)| result.is_ok()).count(),
            1
        );
        let disk = fs::read_to_string(dir.path().join("new")).unwrap();
        for (contents, result) in results {
            match result {
                Ok(payload) => {
                    assert_eq!(disk, contents, "disk must contain the acknowledged winner");
                    written_revision(payload, contents);
                }
                Err(error) => assert_eq!(error.code, "conflict"),
            }
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn heavily_escaped_search_results_fit_protocol_frames() {
    let (dir, mut ws) = workspace();
    let line = format!("needle{}\n", "\u{1}".repeat(2000));
    fs::write(dir.path().join("a"), line.repeat(400)).unwrap();
    let payload = ws
        .handle(Operation::Search {
            query: "needle".into(),
            limit: 1000,
        })
        .unwrap();
    assert!(matches!(
        payload,
        Payload::Matches {
            truncated: true,
            ..
        }
    ));
    let mut bytes = Vec::new();
    cedar_protocol::write_frame(
        &mut bytes,
        &cedar_protocol::Response {
            id: 1,
            result: Ok(payload),
        },
    )
    .unwrap();
    assert!(bytes.len() < cedar_protocol::MAX_FRAME_BYTES);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn explicit_git_status_requires_command_execution_opt_in() {
    let (dir, mut ws) = workspace();
    let init = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .arg(dir.path())
        .status();
    if matches!(&init, Err(e) if e.kind() == std::io::ErrorKind::NotFound) {
        eprintln!("git unavailable: skipping git integration test");
        return;
    }
    assert!(init.unwrap().success());
    fs::write(dir.path().join("untracked.txt"), "text").unwrap();
    assert_eq!(
        ws.handle(Operation::GitStatus).unwrap_err().code,
        "run_disabled"
    );
    ws.set_allow_run(true);
    match ws.handle(Operation::GitStatus).unwrap() {
        Payload::GitStatus { text } => assert!(text.contains("?? untracked.txt")),
        other => panic!("{other:?}"),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn git_status_cannot_execute_repository_clean_filters_without_trust() {
    let (dir, mut ws) = workspace();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .current_dir(dir.path())
            .args(args)
            .status()
    };
    let init = git(&["init", "--quiet"]);
    if matches!(&init, Err(e) if e.kind() == std::io::ErrorKind::NotFound) {
        eprintln!("git unavailable: skipping clean-filter regression test");
        return;
    }
    assert!(init.unwrap().success());
    fs::write(
        dir.path().join(".gitattributes"),
        "tracked.txt filter=review\n",
    )
    .unwrap();
    fs::write(dir.path().join("tracked.txt"), "before\n").unwrap();
    assert!(git(&["add", "--", ".gitattributes", "tracked.txt"])
        .unwrap()
        .success());
    assert!(git(&[
        "config",
        "filter.review.clean",
        "touch filter-executed; cat"
    ])
    .unwrap()
    .success());
    // Same-size changes exercise Git's refresh path, which invokes the clean filter.
    fs::write(dir.path().join("tracked.txt"), "after!\n").unwrap();
    assert_eq!(
        ws.handle(Operation::GitStatus).unwrap_err().code,
        "run_disabled"
    );
    assert!(!dir.path().join("filter-executed").exists());

    // Verify that this fixture really invokes code once the workspace is trusted.
    ws.set_allow_run(true);
    assert!(matches!(
        ws.handle(Operation::GitStatus).unwrap(),
        Payload::GitStatus { .. }
    ));
    assert!(dir.path().join("filter-executed").exists());
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[test]
fn unsupported_platform_refuses_local_commands_and_git_even_with_execution_trust() {
    let (_dir, mut ws) = workspace();
    ws.set_allow_run(true);
    for operation in [
        Operation::Run {
            program: "cmd.exe".into(),
            args: vec!["/C".into(), "echo unsafe".into()],
            timeout_secs: 1,
        },
        Operation::GitStatus,
    ] {
        let failure = ws.handle(operation).unwrap_err();
        assert_eq!(failure.code, "unsupported_platform");
        assert!(failure.message.contains("SSH"));
    }
}
