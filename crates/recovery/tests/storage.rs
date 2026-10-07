use cedar_recovery::{
    record_id, validate_relative_path, Draft, Error, Limits, MutationOutcome, RecordId, Store,
    WorkspaceIdentity, MAX_HEADER_BYTES, MAX_OPERATION_KEYS, MAX_RECORD_BYTES, MAX_STORAGE_ENTRIES,
    MAX_TEXT_BYTES,
};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

fn private_tempdir() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    directory
}

fn workspace() -> WorkspaceIdentity {
    WorkspaceIdentity::Local {
        root: "/synthetic/workspace".into(),
    }
}

fn draft(path: &str, text: &str) -> Draft {
    Draft {
        workspace: workspace(),
        path: path.into(),
        text: text.into(),
        base_text: "original saved text\n".into(),
        base_revision: Some("original-r0".into()),
        modified_ms: 1_797_000_000_123,
    }
}

fn record_path(root: &Path, draft: &Draft) -> PathBuf {
    root.join(format!(
        "{}.draft",
        record_id(&draft.workspace, &draft.path).unwrap()
    ))
}

fn private_file(path: &Path, bytes: &[u8]) {
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).unwrap();
    file.write_all(bytes).unwrap();
}

fn change_header(path: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let bytes = fs::read(path).unwrap();
    let size = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let mut header: serde_json::Value = serde_json::from_slice(&bytes[44..44 + size]).unwrap();
    edit(&mut header);
    let new_header = serde_json::to_vec(&header).unwrap();
    let mut replacement = bytes[..8].to_vec();
    replacement.extend_from_slice(&(new_header.len() as u32).to_le_bytes());
    replacement.extend_from_slice(&Sha256::digest(&new_header));
    replacement.extend_from_slice(&new_header);
    replacement.extend_from_slice(&bytes[44 + size..]);
    private_file(path, &replacement);
}

#[test]
fn round_trip_preserves_exact_unicode_and_original_revision() {
    let root = private_tempdir();
    let mut store = Store::open(root.path()).unwrap();
    let mut original = draft("src/你好🦀.rs", "\u{feff}你好\r\n👩🏽‍💻e\u{301}\0\t\n");
    original.base_text = "原始\r\n🐻".into();
    assert_eq!(store.write(1, &original).unwrap(), MutationOutcome::Applied);
    let id = record_id(&original.workspace, &original.path).unwrap();
    assert_eq!(store.read(&id).unwrap(), original);
    let listing = store.list().unwrap();
    assert!(listing.issues.is_empty());
    assert_eq!(listing.drafts.len(), 1);
    let meta = &listing.drafts[0];
    assert_eq!(meta.id, id);
    assert_eq!(meta.path, original.path);
    assert_eq!(meta.text_bytes, original.text.len());
    assert_eq!(meta.base_text_bytes, original.base_text.len());
    assert_eq!(meta.base_revision, original.base_revision);
    drop(store);
    let store = Store::open(root.path()).unwrap();
    assert_eq!(store.read(&id).unwrap(), original);
}

#[test]
fn new_files_keep_absent_revision_and_empty_base() {
    let root = private_tempdir();
    let mut store = Store::open(root.path()).unwrap();
    let mut new_file = draft("new.txt", "new unsaved contents");
    new_file.base_revision = None;
    new_file.base_text.clear();
    store.write(1, &new_file).unwrap();
    assert_eq!(
        store
            .read(&record_id(&new_file.workspace, &new_file.path).unwrap())
            .unwrap(),
        new_file
    );
}

#[test]
fn stable_ids_separate_all_ssh_identity_fields_and_local_workspace() {
    assert_eq!(
        record_id(&workspace(), "src/main.rs").unwrap().as_str(),
        "8bc64cd34f31e40efcf30db5f55cbb51ba0dbed016ac4820e38a398fd6da92f5"
    );
    let ssh = WorkspaceIdentity::Ssh {
        host: "test@example.invalid".into(),
        port: 22,
        root: "/synthetic/workspace".into(),
        agent_path: "/usr/local/bin/cedar-agent".into(),
    };
    let mut identities = vec![workspace(), ssh.clone()];
    for index in 0..4 {
        let mut copy = ssh.clone();
        if let WorkspaceIdentity::Ssh {
            host,
            port,
            root,
            agent_path,
        } = &mut copy
        {
            match index {
                0 => *host = "other@example.invalid".into(),
                1 => *port = 2222,
                2 => *root = "/different".into(),
                _ => *agent_path = "/different-agent".into(),
            }
        }
        identities.push(copy);
    }
    let ids: std::collections::HashSet<_> = identities
        .iter()
        .map(|identity| record_id(identity, "a.txt").unwrap())
        .collect();
    assert_eq!(ids.len(), identities.len());
    assert_ne!(
        record_id(&workspace(), "e\u{301}.txt").unwrap(),
        record_id(&workspace(), "é.txt").unwrap()
    );
}

#[test]
fn remote_record_is_frontend_local_and_never_touches_workspace() {
    let root = private_tempdir();
    let project = private_tempdir();
    fs::write(project.path().join("untouched.txt"), "workspace file").unwrap();
    let mut record = draft("untouched.txt", "private draft");
    record.workspace = WorkspaceIdentity::Ssh {
        host: "user@not-contacted.invalid".into(),
        port: 22,
        root: project.path().to_string_lossy().into_owned(),
        agent_path: "/not/executed".into(),
    };
    let mut store = Store::open(root.path()).unwrap();
    store.write(1, &record).unwrap();
    assert_eq!(
        store
            .read(&record_id(&record.workspace, &record.path).unwrap())
            .unwrap(),
        record
    );
    assert_eq!(
        fs::read_to_string(project.path().join("untouched.txt")).unwrap(),
        "workspace file"
    );
    assert_eq!(fs::read_dir(project.path()).unwrap().count(), 1);
}

#[test]
fn sequence_tombstones_reject_late_writes_and_late_deletes() {
    let root = private_tempdir();
    let mut store = Store::open(root.path()).unwrap();
    let old = draft("a.txt", "old");
    let new = draft("a.txt", "new");
    let id = record_id(&new.workspace, &new.path).unwrap();
    store.write(10, &new).unwrap();
    assert_eq!(store.write(9, &old).unwrap(), MutationOutcome::IgnoredStale);
    assert_eq!(
        store.remove(8, &new.workspace, &new.path).unwrap(),
        MutationOutcome::IgnoredStale
    );
    assert_eq!(store.read(&id).unwrap(), new);
    store.remove(11, &new.workspace, &new.path).unwrap();
    assert_eq!(
        store.write(10, &old).unwrap(),
        MutationOutcome::IgnoredStale
    );
    assert_eq!(
        store.write(11, &old).unwrap(),
        MutationOutcome::IgnoredStale
    );
    assert!(store.list().unwrap().drafts.is_empty());
    store.write(12, &new).unwrap();
    assert_eq!(
        store.remove(11, &new.workspace, &new.path).unwrap(),
        MutationOutcome::IgnoredStale
    );
    assert_eq!(store.read(&id).unwrap(), new);
    // Different keys may be coalesced and delivered in a different order.
    assert_eq!(
        store.write(1, &draft("b.txt", "independent")).unwrap(),
        MutationOutcome::Applied
    );
}

#[test]
fn failed_newer_mutation_blocks_stale_operation_and_retry_uses_new_sequence() {
    let root = private_tempdir();
    let mut store = Store::open(root.path()).unwrap();
    let old = draft("a.txt", "old");
    store.write(1, &old).unwrap();
    let mut oversized = old.clone();
    oversized.text = "x".repeat(MAX_TEXT_BYTES + 1);
    assert!(matches!(store.write(20, &oversized), Err(Error::Limit(_))));
    assert_eq!(
        store.remove(19, &old.workspace, &old.path).unwrap(),
        MutationOutcome::IgnoredStale
    );
    assert_eq!(
        store.write(20, &old).unwrap(),
        MutationOutcome::IgnoredStale
    );
    assert_eq!(
        store
            .read(&record_id(&old.workspace, &old.path).unwrap())
            .unwrap(),
        old
    );
    assert_eq!(store.write(21, &old).unwrap(), MutationOutcome::Applied);
}

#[test]
fn reopening_resets_only_process_sequence_tombstones() {
    let root = private_tempdir();
    let old = draft("a.txt", "keep base revision r0");
    let mut store = Store::open(root.path()).unwrap();
    store.write(u64::MAX, &old).unwrap();
    drop(store);
    let mut store = Store::open(root.path()).unwrap();
    let recovered = store
        .read(&record_id(&old.workspace, &old.path).unwrap())
        .unwrap();
    assert_eq!(recovered.base_revision.as_deref(), Some("original-r0"));
    assert_eq!(
        store.remove(0, &old.workspace, &old.path).unwrap(),
        MutationOutcome::Applied
    );
    drop(store);
    let mut store = Store::open(root.path()).unwrap();
    assert_eq!(store.write(0, &old).unwrap(), MutationOutcome::Applied);
}

#[test]
fn process_sequence_memory_has_a_hard_bound() {
    let root = private_tempdir();
    let mut store = Store::open(root.path()).unwrap();
    for index in 0..MAX_OPERATION_KEYS {
        store
            .remove(index as u64, &workspace(), &format!("file-{index}.txt"))
            .unwrap();
    }
    assert!(matches!(
        store.remove(999_999, &workspace(), "overflow.txt"),
        Err(Error::Limit(_))
    ));
    assert_eq!(
        store
            .write(999_999, &draft("file-0.txt", "existing key still works"))
            .unwrap(),
        MutationOutcome::Applied
    );
}

#[test]
fn metadata_listing_does_not_read_draft_payloads_but_restore_checks_them() {
    let root = private_tempdir();
    let mut store = Store::open(root.path()).unwrap();
    let value = draft("a.txt", "payload");
    store.write(1, &value).unwrap();
    let path = record_path(root.path(), &value);
    let mut bytes = fs::read(&path).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    private_file(&path, &bytes);
    let listing = store.list().unwrap();
    assert_eq!(listing.drafts.len(), 1);
    assert!(listing.issues.is_empty());
    assert!(matches!(
        store.read(&listing.drafts[0].id),
        Err(Error::Damaged(_))
    ));
    assert!(store.write(2, &value).is_err());
    assert!(store.remove(3, &value.workspace, &value.path).is_err());
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn damaged_records_do_not_hide_healthy_ones_and_are_never_erased() {
    let root = private_tempdir();
    let mut store = Store::open(root.path()).unwrap();
    let bad = draft("bad.txt", "bad");
    let good = draft("good.txt", "healthy");
    store.write(1, &bad).unwrap();
    store.write(2, &good).unwrap();
    let bad_path = record_path(root.path(), &bad);
    private_file(&bad_path, b"damaged record evidence");
    let listing = store.list().unwrap();
    assert_eq!(listing.drafts.len(), 1);
    assert_eq!(listing.drafts[0].path, "good.txt");
    assert_eq!(listing.issues.len(), 1);
    assert_eq!(store.read(&listing.drafts[0].id).unwrap(), good);
    assert!(store.write(3, &bad).is_err());
    assert!(store.remove(4, &bad.workspace, &bad.path).is_err());
    assert_eq!(fs::read(&bad_path).unwrap(), b"damaged record evidence");
    // A damaged ordinary file does not prevent healthy, quota-fitting writes.
    store
        .write(5, &draft("third.txt", "another healthy draft"))
        .unwrap();
}

#[test]
fn unknown_versions_fields_and_mismatched_record_names_fail_closed() {
    for mutation in 0..3 {
        let root = private_tempdir();
        let mut store = Store::open(root.path()).unwrap();
        let value = draft("a.txt", "data");
        store.write(1, &value).unwrap();
        let path = record_path(root.path(), &value);
        change_header(&path, |header| match mutation {
            0 => header["version"] = 999.into(),
            1 => header["allow_run"] = true.into(),
            _ => header["path"] = "other.txt".into(),
        });
        assert!(store
            .read(&record_id(&value.workspace, &value.path).unwrap())
            .is_err());
        assert_eq!(store.list().unwrap().issues.len(), 1);
        assert!(path.exists());
    }
}

#[test]
fn invalid_utf8_is_rejected_even_when_checksum_matches() {
    let root = private_tempdir();
    let mut store = Store::open(root.path()).unwrap();
    let mut value = draft("a.txt", "x");
    value.base_text.clear();
    store.write(1, &value).unwrap();
    let path = record_path(root.path(), &value);
    change_header(&path, |header| {
        header["text_sha256"] = format!("{:x}", Sha256::digest([0xff])).into()
    });
    let mut bytes = fs::read(&path).unwrap();
    *bytes.last_mut().unwrap() = 0xff;
    private_file(&path, &bytes);
    assert!(
        matches!(store.read(&record_id(&value.workspace, &value.path).unwrap()), Err(Error::Damaged(message)) if message.contains("UTF-8"))
    );
}

#[test]
fn malicious_lengths_truncation_and_trailing_data_are_bounded() {
    for mutation in 0..5 {
        let root = private_tempdir();
        let mut store = Store::open(root.path()).unwrap();
        let value = draft("a.txt", "data");
        store.write(1, &value).unwrap();
        let path = record_path(root.path(), &value);
        match mutation {
            0 => change_header(&path, |header| header["text_bytes"] = u64::MAX.into()),
            1 => {
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(&path)
                    .unwrap();
                file.write_all(b"CEDARDR1").unwrap();
                file.write_all(&((MAX_HEADER_BYTES + 1) as u32).to_le_bytes())
                    .unwrap();
            }
            2 => File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_len(MAX_RECORD_BYTES + 1)
                .unwrap(),
            3 => File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_len(20)
                .unwrap(),
            _ => File::options()
                .append(true)
                .open(&path)
                .unwrap()
                .write_all(b"trailing")
                .unwrap(),
        }
        assert!(store
            .read(&record_id(&value.workspace, &value.path).unwrap())
            .is_err());
        assert_eq!(store.list().unwrap().issues.len(), 1);
    }
}

#[test]
fn exact_text_limit_works_without_json_expansion_and_oversize_is_not_truncated() {
    let root = private_tempdir();
    let mut store = Store::open(root.path()).unwrap();
    let mut value = draft("a.txt", &"\0".repeat(MAX_TEXT_BYTES));
    value.base_text = "🦀".repeat(MAX_TEXT_BYTES / 4);
    store.write(1, &value).unwrap();
    let id = record_id(&value.workspace, &value.path).unwrap();
    assert_eq!(store.read(&id).unwrap(), value);
    let old_bytes = fs::read(record_path(root.path(), &value)).unwrap();
    value.text.push('x');
    assert!(matches!(store.write(2, &value), Err(Error::Limit(_))));
    assert_eq!(
        fs::read(record_path(root.path(), &value)).unwrap(),
        old_bytes
    );
    value.text.pop();
    value.base_text.push('x');
    assert!(matches!(store.write(3, &value), Err(Error::Limit(_))));
    assert_eq!(
        fs::read(record_path(root.path(), &value)).unwrap(),
        old_bytes
    );
}

#[test]
fn record_and_total_bounds_fail_without_eviction_or_false_acknowledgement() {
    let root = private_tempdir();
    let first = draft("a.txt", "preserve this draft");
    let mut store = Store::open_with_limits(
        root.path(),
        Limits {
            max_records: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    store.write(1, &first).unwrap();
    assert!(matches!(
        store.write(2, &draft("b.txt", "second")),
        Err(Error::Limit(_))
    ));
    let id = record_id(&first.workspace, &first.path).unwrap();
    assert_eq!(store.read(&id).unwrap(), first);
    let size = fs::metadata(record_path(root.path(), &first))
        .unwrap()
        .len();
    drop(store);
    // A same-size replacement needs room for both old and temporary records.
    let mut store = Store::open_with_limits(
        root.path(),
        Limits {
            max_total_bytes: 2 * size - 1,
            ..Limits::default()
        },
    )
    .unwrap();
    assert!(matches!(store.write(3, &first), Err(Error::Limit(_))));
    assert_eq!(store.read(&id).unwrap(), first);
    store.remove(4, &first.workspace, &first.path).unwrap();
    assert_eq!(store.write(5, &first).unwrap(), MutationOutcome::Applied);
    drop(store);
    let mut store = Store::open_with_limits(
        root.path(),
        Limits {
            max_record_bytes: size - 1,
            ..Limits::default()
        },
    )
    .unwrap();
    assert!(matches!(store.write(6, &first), Err(Error::Limit(_))));
    assert!(record_path(root.path(), &first).exists());
}

#[test]
fn unknown_files_and_interrupted_temporaries_count_toward_storage_and_are_retained() {
    let root = private_tempdir();
    let mut store = Store::open_with_limits(
        root.path(),
        Limits {
            max_total_bytes: 1024,
            ..Limits::default()
        },
    )
    .unwrap();
    let evidence = root.path().join(".cedar-write-crashed");
    private_file(&evidence, &vec![42; 1024]);
    assert!(matches!(
        store.write(1, &draft("a.txt", "data")),
        Err(Error::Limit(_))
    ));
    assert_eq!(fs::read(&evidence).unwrap(), vec![42; 1024]);
    let listing = store.list().unwrap();
    assert!(listing.drafts.is_empty());
    assert_eq!(listing.issues.len(), 1);
}

#[test]
fn directory_scan_and_metadata_results_have_hard_count_limits() {
    let root = private_tempdir();
    let mut store = Store::open(root.path()).unwrap();
    for i in 0..MAX_STORAGE_ENTRIES + 10 {
        private_file(&root.path().join(format!("unknown-{i}")), b"");
    }
    assert!(matches!(
        store.write(1, &draft("a.txt", "data")),
        Err(Error::Limit(_))
    ));
    let listing = store.list().unwrap();
    assert!(listing.issues.len() <= MAX_STORAGE_ENTRIES + 1);
    assert!(listing
        .issues
        .iter()
        .any(|issue| issue.message.contains("incomplete")));
    assert_eq!(
        fs::read_dir(root.path()).unwrap().count(),
        MAX_STORAGE_ENTRIES + 11
    );
}

#[test]
fn invalid_limits_are_rejected_before_creating_storage() {
    let parent = private_tempdir();
    for (index, limits) in [
        Limits {
            max_records: 0,
            ..Limits::default()
        },
        Limits {
            max_text_bytes: MAX_TEXT_BYTES + 1,
            ..Limits::default()
        },
        Limits {
            max_record_bytes: u64::MAX,
            ..Limits::default()
        },
        Limits {
            max_total_bytes: u64::MAX,
            ..Limits::default()
        },
    ]
    .into_iter()
    .enumerate()
    {
        let path = parent.path().join(index.to_string());
        assert!(Store::open_with_limits(&path, limits).is_err());
        assert!(!path.exists());
    }
}

#[test]
fn traversal_windows_aliases_and_unbounded_fields_are_rejected() {
    for path in [
        "",
        "/x",
        "../x",
        "a/../b",
        "a/./b",
        "a//b",
        "a/",
        "C:/x",
        "C:x",
        "a\\b",
        "\\\\server\\file",
        "x:stream",
        "x\0",
        "x\n",
        "NUL",
        "CON.txt",
        "con .txt",
        "COM1.log",
        "LPT9",
        "COM¹",
        "CONIN$",
        "CONOUT$",
        "a.",
        "a ",
        "a?b",
        "a*b",
        "a<b",
    ] {
        assert!(validate_relative_path(path).is_err(), "accepted {path:?}");
    }
    assert!(validate_relative_path(&"a".repeat(256)).is_err());
    assert!(validate_relative_path(&"a/".repeat(2049)).is_err());
    for path in [
        "src/你好.rs",
        "read me.md",
        ".gitignore",
        "a...b",
        "COM10.txt",
        "under_score/x-y",
    ] {
        assert!(validate_relative_path(path).is_ok(), "rejected {path:?}");
    }
    assert!(RecordId::parse("../escape").is_err());
    assert!(RecordId::parse(&"A".repeat(64)).is_err());
    assert!(record_id(
        &WorkspaceIdentity::Local {
            root: "x".repeat(4097)
        },
        "a"
    )
    .is_err());
    let too_long_host = WorkspaceIdentity::Ssh {
        host: "a".repeat(1025),
        port: 22,
        root: "/root".into(),
        agent_path: "agent".into(),
    };
    assert!(record_id(&too_long_host, "a").is_err());
    let bad_port = WorkspaceIdentity::Ssh {
        host: "host".into(),
        port: 0,
        root: "/root".into(),
        agent_path: "agent".into(),
    };
    assert!(record_id(&bad_port, "a").is_err());
    let root = private_tempdir();
    let mut store = Store::open(root.path()).unwrap();
    let mut value = draft("a.txt", "draft");
    value.base_revision = Some("r".repeat(257));
    assert!(store.write(1, &value).is_err());
    assert!(store.list().unwrap().drafts.is_empty());
}

#[test]
fn lock_contention_is_explicit_and_drop_releases_it() {
    let root = private_tempdir();
    let store = Store::open(root.path()).unwrap();
    assert!(matches!(Store::open(root.path()), Err(Error::Locked)));
    drop(store);
    Store::open(root.path()).unwrap();
}

#[test]
fn atomic_replacement_readers_never_observe_a_partial_record() {
    let root = private_tempdir();
    let mut store = Store::open(root.path()).unwrap();
    let a = draft("a.txt", &"A🦀".repeat(1000));
    let b = draft("a.txt", &"B你".repeat(1000));
    store.write(1, &a).unwrap();
    let path = record_path(root.path(), &a);
    let a_bytes = fs::read(&path).unwrap();
    store.write(2, &b).unwrap();
    let b_bytes = fs::read(&path).unwrap();
    let running = Arc::new(AtomicBool::new(true));
    let reads = Arc::new(AtomicUsize::new(0));
    let active = running.clone();
    let reader_count = reads.clone();
    let (started_sender, started_receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut started = Some(started_sender);
        while active.load(Ordering::Acquire) {
            let bytes = fs::read(&path).unwrap();
            assert!(
                bytes == a_bytes || bytes == b_bytes,
                "partial/interleaved record"
            );
            reader_count.fetch_add(1, Ordering::Relaxed);
            if let Some(sender) = started.take() {
                sender.send(()).unwrap();
            }
        }
    });
    started_receiver
        .recv_timeout(Duration::from_secs(15))
        .unwrap();
    let mut write_result = Ok(MutationOutcome::Applied);
    for sequence in 3..43 {
        write_result = store.write(sequence, if sequence % 2 == 0 { &a } else { &b });
        if write_result.is_err() {
            break;
        }
    }
    // Always stop/join before propagating a failure, so cleanup cannot tear
    // down the directory underneath a still-running reader.
    running.store(false, Ordering::Release);
    reader.join().unwrap();
    write_result.expect("atomic replacement must succeed with a delete-sharing reader");
    assert!(reads.load(Ordering::Relaxed) > 0);
    assert_eq!(
        fs::read_dir(root.path()).unwrap().count(),
        2,
        "only lock and complete record remain"
    );
}

#[cfg(unix)]
#[test]
fn unix_new_storage_is_private_and_existing_nonprivate_storage_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let parent = private_tempdir();
    let root = parent.path().join("new/cedar/recovery");
    let mut store = Store::open(&root).unwrap();
    for path in [
        &root,
        &parent.path().join("new"),
        &parent.path().join("new/cedar"),
    ] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    let value = draft("a.txt", "secret draft");
    store.write(1, &value).unwrap();
    for entry in fs::read_dir(&root).unwrap() {
        assert_eq!(
            entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let path = record_path(&root, &value);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store
        .read(&record_id(&value.workspace, &value.path).unwrap())
        .is_err());
    assert!(store.write(2, &value).is_err());
    assert!(store.remove(3, &value.workspace, &value.path).is_err());
    assert_eq!(store.list().unwrap().issues.len(), 1);
    drop(store);
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(Store::open(&root), Err(Error::UnsafeStorage(_))));
    assert_eq!(
        fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[cfg(unix)]
#[test]
fn symlink_directories_lock_files_drafts_and_hardlinks_are_never_followed() {
    use std::os::unix::fs::symlink;
    let parent = private_tempdir();
    let real = parent.path().join("real");
    let store = Store::open(&real).unwrap();
    drop(store);
    let alias = parent.path().join("alias");
    symlink(&real, &alias).unwrap();
    assert!(Store::open(&alias).is_err());
    assert!(Store::open(alias.join("child")).is_err());
    assert!(!real.join("child").exists());
    fs::remove_file(real.join(".cedar-lock")).unwrap();
    let outside = parent.path().join("outside");
    private_file(&outside, b"must not touch");
    symlink(&outside, real.join(".cedar-lock")).unwrap();
    assert!(Store::open(&real).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"must not touch");
    fs::remove_file(real.join(".cedar-lock")).unwrap();
    let mut store = Store::open(&real).unwrap();
    let value = draft("a.txt", "draft");
    let destination = record_path(&real, &value);
    symlink(&outside, &destination).unwrap();
    assert!(store.write(1, &value).is_err());
    assert!(store
        .read(&record_id(&value.workspace, &value.path).unwrap())
        .is_err());
    assert!(store.remove(2, &value.workspace, &value.path).is_err());
    assert!(fs::symlink_metadata(&destination)
        .unwrap()
        .file_type()
        .is_symlink());
    fs::remove_file(&destination).unwrap();
    fs::hard_link(&outside, &destination).unwrap();
    assert!(store.write(3, &value).is_err());
    assert!(store.remove(4, &value.workspace, &value.path).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"must not touch");
}

#[cfg(unix)]
#[test]
fn special_files_are_rejected_without_blocking_and_healthy_records_remain_visible() {
    use std::os::unix::ffi::OsStrExt;
    let root = private_tempdir();
    let mut store = Store::open(root.path()).unwrap();
    let healthy = draft("healthy.txt", "healthy");
    store.write(1, &healthy).unwrap();
    let fifo = draft("fifo.txt", "never written");
    let path = record_path(root.path(), &fifo);
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    let start = std::time::Instant::now();
    assert!(store
        .read(&record_id(&fifo.workspace, &fifo.path).unwrap())
        .is_err());
    assert!(store.write(2, &fifo).is_err());
    assert!(store.remove(3, &fifo.workspace, &fifo.path).is_err());
    assert!(start.elapsed() < Duration::from_secs(1));
    let listing = store.list().unwrap();
    assert_eq!(listing.drafts.len(), 1);
    assert_eq!(listing.issues.len(), 1);
}

#[cfg(unix)]
#[test]
fn root_or_lock_replacement_is_detected_before_mutation() {
    for replace_lock in [false, true] {
        let parent = private_tempdir();
        let root = parent.path().join("root");
        let mut store = Store::open(&root).unwrap();
        if replace_lock {
            fs::rename(root.join(".cedar-lock"), root.join("old-lock")).unwrap();
            private_file(&root.join(".cedar-lock"), b"");
        } else {
            fs::rename(&root, parent.path().join("moved")).unwrap();
            let other = Store::open(&root).unwrap();
            drop(other);
        }
        assert!(matches!(
            store.write(1, &draft("a.txt", "never persisted")),
            Err(Error::UnsafeStorage(_))
        ));
        assert!(store.list().is_err());
    }
}

fn child_command(mode: &str, root: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "subprocess_entry", "--nocapture"])
        .env("CEDAR_RECOVERY_TEST_MODE", mode)
        .env("CEDAR_RECOVERY_TEST_ROOT", root)
        .stdin(Stdio::null());
    command
}

#[test]
fn subprocess_entry() {
    let Ok(mode) = std::env::var("CEDAR_RECOVERY_TEST_MODE") else {
        return;
    };
    let root = PathBuf::from(std::env::var_os("CEDAR_RECOVERY_TEST_ROOT").unwrap());
    match mode.as_str() {
        "noop" => {}
        "lock" => {
            if matches!(Store::open(root), Err(Error::Locked)) {
                std::process::exit(0);
            }
            std::process::exit(17);
        }
        "path" => {
            let expected = std::env::var_os("CEDAR_RECOVERY_TEST_EXPECTED").unwrap();
            let result = cedar_recovery::default_store_path();
            if expected == "ERROR" {
                assert!(result.is_err());
            } else {
                assert_eq!(result.unwrap(), PathBuf::from(expected));
            }
        }
        "crash" => {
            let mut store = Store::open(root).unwrap();
            let value = draft("crash.txt", "durable unsaved draft 你好🦀\r\n");
            assert_eq!(store.write(100, &value).unwrap(), MutationOutcome::Applied);
            println!("CEDAR_DURABLE_ACK");
            std::io::stdout().flush().unwrap();
            loop {
                std::thread::sleep(Duration::from_secs(60));
            }
        }
        _ => panic!("unknown child mode"),
    }
}

#[test]
fn independent_process_cannot_take_writer_lock() {
    let root = private_tempdir();
    let _store = Store::open(root.path()).unwrap();
    let status = child_command("lock", root.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn acknowledged_draft_survives_actual_subprocess_kill_and_reopen() {
    let root = private_tempdir();
    let mut child = child_command("crash", root.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if line.unwrap().contains("CEDAR_DURABLE_ACK") {
                let _ = sender.send(());
                return;
            }
        }
    });
    let acknowledged = receiver.recv_timeout(Duration::from_secs(15));
    let _ = child.kill();
    let status = child.wait().unwrap();
    reader.join().unwrap();
    assert!(
        acknowledged.is_ok(),
        "child never durably acknowledged a draft"
    );
    assert!(!status.success());
    let mut store = Store::open(root.path()).unwrap();
    let original = draft("crash.txt", "durable unsaved draft 你好🦀\r\n");
    assert_eq!(
        store
            .read(&record_id(&original.workspace, &original.path).unwrap())
            .unwrap(),
        original
    );
    assert_eq!(store.list().unwrap().drafts.len(), 1);
    assert_eq!(store.write(1, &original).unwrap(), MutationOutcome::Applied);
}

#[test]
fn default_paths_are_frontend_local_and_explicit_override_requires_absolute_path() {
    let root = private_tempdir();
    let check = |settings: &[(&str, &std::ffi::OsStr)], expected: &std::ffi::OsStr| {
        let mut command = child_command("path", root.path());
        for key in [
            "CEDAR_RECOVERY_DIR",
            "XDG_DATA_HOME",
            "HOME",
            "LOCALAPPDATA",
        ] {
            command.env_remove(key);
        }
        for (key, value) in settings {
            command.env(key, value);
        }
        let output = command
            .env("CEDAR_RECOVERY_TEST_EXPECTED", expected)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    };
    check(
        &[("CEDAR_RECOVERY_DIR", root.path().as_os_str())],
        root.path().as_os_str(),
    );
    check(
        &[("CEDAR_RECOVERY_DIR", std::ffi::OsStr::new("relative"))],
        std::ffi::OsStr::new("ERROR"),
    );
    check(&[], std::ffi::OsStr::new("ERROR"));
    #[cfg(target_os = "windows")]
    check(
        &[("LOCALAPPDATA", root.path().as_os_str())],
        root.path().join("Cedar/recovery").as_os_str(),
    );
    #[cfg(target_os = "macos")]
    check(
        &[("HOME", root.path().as_os_str())],
        root.path()
            .join("Library/Application Support/Cedar/recovery")
            .as_os_str(),
    );
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        check(
            &[("XDG_DATA_HOME", root.path().as_os_str())],
            root.path().join("cedar/recovery").as_os_str(),
        );
        check(
            &[("HOME", root.path().as_os_str())],
            root.path().join(".local/share/cedar/recovery").as_os_str(),
        );
        check(
            &[
                ("HOME", root.path().as_os_str()),
                ("XDG_DATA_HOME", std::ffi::OsStr::new("relative")),
            ],
            root.path().join(".local/share/cedar/recovery").as_os_str(),
        );
    }
}

#[test]
fn hostile_header_identity_and_revision_bounds_fail_before_payload_allocation() {
    for mutation in 0..4 {
        let root = private_tempdir();
        let mut store = Store::open(root.path()).unwrap();
        let value = draft("a.txt", "data");
        store.write(1, &value).unwrap();
        let path = record_path(root.path(), &value);
        change_header(&path, |header| match mutation {
            0 => header["workspace"]["root"] = "x".repeat(4097).into(),
            1 => header["path"] = "../outside".into(),
            2 => header["base_revision"] = "r".repeat(257).into(),
            _ => header["text_sha256"] = "not-a-hash".into(),
        });
        assert!(store
            .read(&record_id(&value.workspace, &value.path).unwrap())
            .is_err());
        assert_eq!(store.list().unwrap().issues.len(), 1);
    }
}

#[test]
fn parse_valid_revision_and_timestamp_damage_is_detected_and_preserved() {
    for field in ["original-r0", "1797000000123"] {
        let root = private_tempdir();
        let mut store = Store::open(root.path()).unwrap();
        let value = draft("a.txt", "unchanged text");
        store.write(1, &value).unwrap();
        let path = record_path(root.path(), &value);
        let mut damaged = fs::read(&path).unwrap();
        let position = damaged
            .windows(field.len())
            .position(|part| part == field.as_bytes())
            .unwrap();
        let last = position + field.len() - 1;
        damaged[last] += 1; // Still valid JSON and an equally sized valid field.
        private_file(&path, &damaged);
        let id = record_id(&value.workspace, &value.path).unwrap();
        assert!(
            matches!(store.read(&id), Err(Error::Damaged(message)) if message.contains("metadata checksum"))
        );
        assert_eq!(store.list().unwrap().issues.len(), 1);
        assert!(store.write(2, &value).is_err());
        assert!(store.remove(3, &value.workspace, &value.path).is_err());
        assert_eq!(fs::read(&path).unwrap(), damaged);
    }
}

#[cfg(unix)]
#[test]
fn owner_drop_unlocks_while_an_actual_fork_child_holds_inherited_descriptor() {
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;

    let root = private_tempdir();
    let store = Store::open(root.path()).unwrap();
    let (mut parent_signal, child_signal) = UnixStream::pair().unwrap();
    child_signal
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    child_signal
        .set_write_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    parent_signal
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    parent_signal
        .set_write_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    let mut command = child_command("noop", root.path());
    command.stdout(Stdio::null()).stderr(Stdio::null());
    // A pre_exec callback forces the actual fork/exec path. Only raw, async-
    // signal-safe syscalls run in the child, before CLOEXEC closes the Store FD.
    unsafe {
        command.pre_exec(move || {
            let descriptor = child_signal.as_raw_fd();
            let ready = b'R';
            loop {
                let result = libc::write(descriptor, (&ready as *const u8).cast(), 1);
                if result == 1 {
                    break;
                }
                let error = std::io::Error::last_os_error();
                if result < 0 && error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(std::io::Error::from_raw_os_error(libc::EIO));
            }
            let mut release = 0u8;
            loop {
                let result = libc::read(descriptor, (&mut release as *mut u8).cast(), 1);
                if result == 1 {
                    return Ok(());
                }
                let error = std::io::Error::last_os_error();
                if result < 0 && error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(std::io::Error::from_raw_os_error(libc::EIO));
            }
        });
    }
    // spawn waits for exec, so the parent fork/exec launch runs concurrently
    // while this thread deterministically exercises the inherited-lock window.
    let spawning = std::thread::spawn(move || command.spawn());
    let mut ready = [0u8];
    let ready_result = parent_signal.read_exact(&mut ready);
    drop(store);
    let reopened = Store::open(root.path());
    // Release and reap even if the assertion below will fail. No sleeps/retries.
    let release_result = parent_signal.write_all(b"G");
    drop(parent_signal);
    let status = spawning.join().unwrap().unwrap().wait().unwrap();
    ready_result.unwrap();
    release_result.unwrap();
    assert_eq!(ready, [b'R']);
    assert!(status.success());
    let reopened =
        reopened.expect("inherited pre-exec descriptor must not retain dropped owner lock");
    assert!(matches!(Store::open(root.path()), Err(Error::Locked)));
    drop(reopened);
    Store::open(root.path()).unwrap();
}

#[cfg(unix)]
#[test]
fn immediate_reopen_survives_parallel_subprocess_fork_exec_stress() {
    use std::os::unix::process::CommandExt;
    let root = private_tempdir();
    let barrier = Arc::new(std::sync::Barrier::new(5));
    let mut store = Some(Store::open(root.path()).unwrap());
    let mut spawning = Vec::new();
    for _ in 0..4 {
        let root = root.path().to_owned();
        let start = Arc::clone(&barrier);
        spawning.push(std::thread::spawn(move || {
            start.wait();
            for _ in 0..20 {
                let mut command = child_command("noop", &root);
                command.stdout(Stdio::null()).stderr(Stdio::null());
                // Force fork rather than a platform-dependent posix_spawn path.
                unsafe {
                    command.pre_exec(|| Ok(()));
                }
                assert!(command.status().unwrap().success());
            }
        }));
    }
    barrier.wait();
    let mut failure = None;
    for iteration in 0..250 {
        drop(store.take());
        match Store::open(root.path()) {
            Ok(reopened) => store = Some(reopened),
            Err(error) => {
                failure = Some(format!("immediate reopen {iteration} failed: {error}"));
                break;
            }
        }
    }
    for thread in spawning {
        thread.join().unwrap();
    }
    assert!(failure.is_none(), "{}", failure.unwrap_or_default());
    assert!(matches!(Store::open(root.path()), Err(Error::Locked)));
    drop(store);
    Store::open(root.path()).unwrap();
}

#[test]
fn atomic_replacement_keeps_an_open_reader_on_the_complete_original_record() {
    let root = private_tempdir();
    let mut store = Store::open(root.path()).unwrap();
    let old = draft("held-reader.txt", "original complete record 你好");
    let new = draft("held-reader.txt", "replacement complete record 🦀");
    store.write(1, &old).unwrap();
    let path = record_path(root.path(), &old);
    let original_bytes = fs::read(&path).unwrap();
    let mut held_reader = File::open(&path).unwrap();
    assert_eq!(store.write(2, &new).unwrap(), MutationOutcome::Applied);
    let mut held_bytes = Vec::new();
    held_reader.read_to_end(&mut held_bytes).unwrap();
    assert_eq!(held_bytes, original_bytes);
    assert_eq!(
        store
            .read(&record_id(&new.workspace, &new.path).unwrap())
            .unwrap(),
        new
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        assert_eq!(
            fs::metadata(&path).unwrap().file_attributes() & 0x100,
            0,
            "persisted file must not retain FILE_ATTRIBUTE_TEMPORARY"
        );
    }
    drop(held_reader);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
}

#[cfg(windows)]
#[test]
fn deny_delete_sharing_returns_error_preserves_old_record_and_allows_newer_retry() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = private_tempdir();
    let mut store = Store::open(root.path()).unwrap();
    let old = draft("blocked.txt", "original safe record");
    let new = draft("blocked.txt", "new private draft");
    store.write(1, &old).unwrap();
    let path = record_path(root.path(), &old);
    let original_bytes = fs::read(&path).unwrap();
    let held_reader = File::options()
        .read(true)
        .share_mode(0x1 | 0x2)
        .open(&path)
        .unwrap();
    let result = store.write(2, &new);
    assert!(
        matches!(result, Err(Error::Io(error)) if matches!(error.raw_os_error(), Some(5 | 32))),
        "an explicit delete-sharing restriction must fail, never acknowledge persistence"
    );
    assert_eq!(fs::read(&path).unwrap(), original_bytes);
    assert_eq!(
        fs::read_dir(root.path()).unwrap().count(),
        2,
        "failed temporary is cleaned up"
    );
    drop(held_reader);
    assert_eq!(store.write(2, &new).unwrap(), MutationOutcome::IgnoredStale);
    assert_eq!(store.write(3, &new).unwrap(), MutationOutcome::Applied);
    assert_eq!(
        store
            .read(&record_id(&new.workspace, &new.path).unwrap())
            .unwrap(),
        new
    );
}
