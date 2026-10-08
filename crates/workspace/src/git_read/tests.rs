use super::*;

fn ordinary(path: &str, xy: &str, modes: [&str; 3]) -> String {
    format!(
        "1 {xy} N... {} {} {} {} {} {path}\0",
        modes[0],
        modes[1],
        modes[2],
        "a".repeat(40),
        "b".repeat(40)
    )
}

#[test]
fn strict_status_retains_exact_unicode_spaces_globs_tabs_and_newlines() {
    let paths = [
        "-leading",
        "space name",
        "雪/é.txt",
        "[literal]*?",
        "tab\tfile",
        "line\nfile",
    ];
    let bytes = paths
        .iter()
        .map(|path| ordinary(path, "MM", ["100644"; 3]))
        .collect::<String>();
    let entries = parse_status(bytes.as_bytes()).unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        paths
    );
    assert!(entries.iter().all(|entry| entry.kind == GitChangeKind::File
        && entry.can_diff_staged
        && entry.can_diff_unstaged));
}

#[test]
fn strict_status_rejects_truncation_invalid_utf8_unknown_records_and_duplicates() {
    let valid = ordinary("file", ".M", ["100644"; 3]);
    let mut invalid_utf8 = b"? ".to_vec();
    invalid_utf8.extend([0xff, 0]);
    let malformed = [
        valid.trim_end_matches('\0').as_bytes().to_vec(),
        b"\0".to_vec(),
        b"? \0".to_vec(),
        b"# branch.head main\0".to_vec(),
        b"2 R. N... bogus\0old\0".to_vec(),
        b"! ignored\0".to_vec(),
        ordinary("file", "..", ["100644"; 3]).into_bytes(),
        ordinary("file", ".U", ["100644"; 3]).into_bytes(),
        ordinary("file", ".M", ["100600"; 3]).into_bytes(),
        valid.replace("N...", "N..!").into_bytes(),
        valid.replace(&"a".repeat(40), "short").into_bytes(),
        format!("{valid}{valid}").into_bytes(),
        invalid_utf8,
    ];
    for bytes in malformed {
        assert!(parse_status(&bytes).is_err(), "{bytes:?}");
    }
    assert!(parse_status(&[]).unwrap().is_empty());
    assert!(parse_status(format!("? {}\0", "a".repeat(MAX_PATH + 1)).as_bytes()).is_err());
    let many = (0..=MAX_ENTRIES)
        .map(|index| format!("? {index}\0"))
        .collect::<String>();
    assert_eq!(
        parse_status(many.as_bytes()).unwrap_err().code,
        "output_limit"
    );
}

#[test]
fn unsupported_modes_and_conflicts_never_offer_a_diff() {
    let conflict = format!(
        "u UU N... 100644 100644 100644 100644 {} {} {} conflict\0",
        "a".repeat(40),
        "b".repeat(40),
        "c".repeat(40)
    );
    let bytes = format!(
        "{}{}{}? untracked\0{conflict}",
        ordinary("symlink", "T.", ["100644", "120000", "120000"]),
        ordinary("submodule", "M.", ["160000"; 3]).replace("N...", "SC.."),
        ordinary("back\\slash", "M.", ["100644"; 3])
    );
    let entries = parse_status(bytes.as_bytes()).unwrap();
    assert_eq!(
        entries.iter().map(|entry| entry.kind).collect::<Vec<_>>(),
        [
            GitChangeKind::Unsupported,
            GitChangeKind::Unsupported,
            GitChangeKind::Unsupported,
            GitChangeKind::Untracked,
            GitChangeKind::Conflict
        ]
    );
    assert!(entries
        .iter()
        .all(|entry| !entry.can_diff_staged && !entry.can_diff_unstaged));
}

#[test]
fn version_accepts_release_vendor_suffixes_and_rejects_old_or_malformed() {
    for valid in [
        "git version 2.45.0\n",
        "git version 2.52.1.windows.1\n",
        "git version 2.50.1 (Apple Git-155)\n",
        "git version 3.0.0\n",
    ] {
        assert!(supported_version(valid.as_bytes()), "{valid}");
    }
    for invalid in [
        "git version 2.44.9\n",
        "git version 1.99.0\n",
        "git version 2.45\n",
        "git version 2.45.nope\n",
        "unknown option --no-lazy-fetch\n",
    ] {
        assert!(!supported_version(invalid.as_bytes()), "{invalid}");
    }
}

#[test]
fn environment_scrubs_case_insensitive_git_and_hidden_entries_without_global_writes() {
    let inherited = [
        ("PATH", "keep"),
        ("HOME", "keep-home"),
        ("SystemRoot", "keep-system"),
        ("GIT_DIR", "elsewhere"),
        ("gIt_wOrK_tReE", "elsewhere"),
        ("git_CONFIG_PARAMETERS", "bad"),
        ("GIT_CONFIG_COUNT", "20"),
        ("GIT_TRACE", "trace-file"),
        ("GIT_SSH_COMMAND", "bad"),
        ("GIT_INDEX_FILE", "redirected"),
        ("GIT_OBJECT_DIRECTORY", "elsewhere"),
        ("GIT_EXTERNAL_DIFF", "bad"),
        ("GIT_ALLOW_PROTOCOL", "all"),
        ("GIT_CONFIG_GLOBAL", "bad"),
        ("=C:", "hidden"),
        ("lc_all", "bad"),
    ]
    .into_iter()
    .map(|(name, value)| (name.into(), value.into()));
    let result = child_environment(inherited).unwrap();
    for (name, value) in &result {
        assert!(!name.to_string_lossy().starts_with('='));
        assert!(!["elsewhere", "bad", "trace-file", "redirected", "hidden"]
            .contains(&value.to_string_lossy().as_ref()));
    }
    let find = |name: &str| {
        result
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.to_string_lossy().into_owned())
    };
    assert_eq!(find("PATH").as_deref(), Some("keep"));
    assert_eq!(find("SystemRoot").as_deref(), Some("keep-system"));
    assert_eq!(find("GIT_CONFIG_COUNT").as_deref(), Some("0"));
    assert_eq!(find("GIT_CONFIG_GLOBAL").as_deref(), Some(NULL_FILE));
    assert_eq!(find("GIT_ALLOW_PROTOCOL").as_deref(), Some(""));
    assert_eq!(find("LC_ALL").as_deref(), Some("C"));
}

#[cfg(windows)]
#[test]
fn environment_scrub_follows_native_case_rules_and_preserves_unrelated_os_strings() {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Globalization::{
        CompareStringOrdinal, CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN,
    };
    let unusual = OsString::from_wide(&[0xd800, b'X' as u16]);
    let unicode_key = OsString::from("雪_KEY");
    let candidates = vec![
        (
            OsString::from("gIt_INDEX_FILE"),
            OsString::from("ascii-redirect"),
        ),
        (
            OsString::from("GıT_INDEX_FILE"),
            OsString::from("redirected"),
        ),
        (OsString::from("GıT_TRACE"), OsString::from("trace-file")),
        (OsString::from("gıt_dır"), OsString::from("elsewhere")),
    ];
    let mut inherited = candidates.clone();
    inherited.extend([
        (unicode_key.clone(), unusual.clone()),
        (unusual.clone(), OsString::from("retained")),
    ]);
    let environment = child_environment(inherited).unwrap();
    let prefix: Vec<u16> = "GIT_".encode_utf16().collect();
    for (name, value) in &candidates {
        let units: Vec<u16> = name.encode_wide().take(4).collect();
        assert_eq!(units.len(), 4);
        // Measure the OS comparison rather than assuming a particular Unicode
        // case table. An independent native child probe checks this API against
        // GetEnvironmentVariableW; actual Git redirect/trace tests remain required.
        // SAFETY: both live buffers contain exactly four UTF-16 units.
        let comparison = unsafe { CompareStringOrdinal(units.as_ptr(), 4, prefix.as_ptr(), 4, 1) };
        assert!(matches!(
            comparison,
            CSTR_LESS_THAN | CSTR_EQUAL | CSTR_GREATER_THAN
        ));
        let matches = comparison == CSTR_EQUAL;
        assert_eq!(
            environment_key_matches(name, "GIT_", true).unwrap(),
            matches
        );
        assert_eq!(
            environment.contains(&(name.clone(), value.clone())),
            !matches
        );
    }
    assert!(!environment
        .iter()
        .any(|(_, value)| value == "ascii-redirect"));
    assert!(environment.contains(&(unicode_key, unusual.clone())));
    assert!(environment.contains(&(unusual, OsString::from("retained"))));
}

#[test]
fn trust_and_host_mode_precede_all_git_program_and_repository_inspection() {
    let root = tempfile::tempdir().unwrap();
    for mode in [BackendMode::InProcess, BackendMode::IsolatedAgent] {
        let mut workspace = Workspace::with_backend_mode(root.path(), mode).unwrap();
        for operation in [
            Operation::GitChanges {
                git_executable: "missing".into(),
            },
            Operation::GitDiff {
                git_executable: "missing".into(),
                path: "../bad".into(),
                kind: GitDiffKind::Staged,
            },
        ] {
            assert_eq!(
                workspace.handle(operation).unwrap_err().code,
                "run_disabled"
            );
        }
        workspace.set_allow_run(true);
        if !platform_supported(mode) {
            assert_eq!(
                workspace
                    .handle(Operation::GitChanges {
                        git_executable: "missing".into()
                    })
                    .unwrap_err()
                    .code,
                "unsupported_platform"
            );
        }
        assert!(workspace.tasks.is_none());
        assert!(workspace.language.is_none());
    }
}

#[test]
fn deleted_parent_paths_are_lexical_and_existing_directories_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("directory")).unwrap();
    for valid in ["deleted/parent/file", "-file", "space name", "雪"] {
        validate_file_path(root.path(), valid).unwrap();
    }
    for invalid in [
        "",
        ".",
        "./file",
        "a//b",
        "../file",
        "a/../file",
        "/file",
        ".git/index",
        "a/.GIT/config",
        "directory",
        "directory/",
        "C:drive",
        "back\\slash",
    ] {
        assert!(
            validate_file_path(root.path(), invalid).is_err(),
            "{invalid}"
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.path().join("missing"), root.path().join("link")).unwrap();
        assert!(validate_file_path(root.path(), "link/child").is_err());
    }
}

#[cfg(windows)]
#[test]
fn windows_deleted_suffix_rejects_device_and_ambiguous_names_without_unicode_panics() {
    let root = tempfile::tempdir().unwrap();
    for suffix in [
        "COM1.txt",
        "COM1 .txt",
        "CON .txt",
        "NUL .txt",
        "COM¹.txt",
        "LPT³",
        "NUL.anything",
        "CONIN$",
        "trailing.",
        "space ",
        "glob*",
        "question?",
        "bad|name",
        "a\n",
    ] {
        assert!(
            validate_file_path(root.path(), &format!("missing/{suffix}")).is_err(),
            "{suffix}"
        );
    }
    for suffix in ["a雪", "雪a", "éab", "COM雪", "ordinary.txt"] {
        validate_file_path(root.path(), &format!("missing/{suffix}")).unwrap();
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod repositories {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    // PATH discovery is a test-fixture convenience only. Production always
    // receives an explicit absolute executable and never searches PATH.
    fn test_git() -> PathBuf {
        if let Some(path) = std::env::var_os("CEDAR_TEST_GIT") {
            return PathBuf::from(path);
        }
        let name = if cfg!(windows) { "git.exe" } else { "git" };
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|directory| directory.join(name))
            .find(|path| path.is_file())
            .expect("Git >= 2.45 is required for generated-repository tests (set CEDAR_TEST_GIT)")
    }

    struct Repo {
        directory: TempDir,
        git: PathBuf,
        workspace: Workspace,
    }
    impl Repo {
        fn new() -> Self {
            let directory = tempfile::Builder::new()
                .prefix("cedar-git-read-雪-")
                .tempdir()
                .unwrap();
            let git = test_git();
            let mut workspace =
                Workspace::with_backend_mode(directory.path(), BackendMode::IsolatedAgent).unwrap();
            workspace.set_allow_run(true);
            let repo = Self {
                directory,
                git,
                workspace,
            };
            repo.git(&["init", "--initial-branch=main"]);
            repo.git(&["config", "user.name", "Cedar Synthetic Fixture"]);
            repo.git(&["config", "user.email", "cedar-fixture@example.invalid"]);
            repo.git(&["config", "commit.gpgsign", "false"]);
            repo
        }
        fn path(&self, path: &str) -> PathBuf {
            self.directory.path().join(path)
        }
        fn command(&self, args: &[&str]) -> Command {
            let mut command = Command::new(&self.git);
            command
                .args(args)
                .current_dir(self.directory.path())
                .env_clear()
                .envs(child_environment(std::env::vars_os()).unwrap());
            command
        }
        fn git(&self, args: &[&str]) {
            let output = self.command(args).output().unwrap();
            assert!(
                output.status.success(),
                "synthetic Git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        fn write(&self, path: &str, text: &str) {
            if let Some(parent) = self.path(path).parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(self.path(path), text).unwrap();
        }
        fn commit(&self) {
            self.git(&["add", "--all"]);
            self.git(&["commit", "-m", "synthetic fixture"]);
        }
        fn changes(&mut self) -> Vec<GitChange> {
            let payload = self
                .workspace
                .handle(Operation::GitChanges {
                    git_executable: self.git.to_str().unwrap().into(),
                })
                .unwrap();
            let Payload::GitChanges { entries } = payload else {
                panic!("wrong payload")
            };
            entries
        }
        fn diff_result(&mut self, path: &str, kind: GitDiffKind) -> Result<Payload, RemoteError> {
            self.workspace.handle(Operation::GitDiff {
                git_executable: self.git.to_str().unwrap().into(),
                path: path.into(),
                kind,
            })
        }
        fn diff(&mut self, path: &str, kind: GitDiffKind) -> String {
            let Payload::GitDiff {
                path: received,
                kind: received_kind,
                text,
            } = self.diff_result(path, kind).unwrap()
            else {
                panic!("wrong payload")
            };
            assert_eq!(received, path);
            assert_eq!(received_kind, kind);
            text
        }
        fn recipe(&self) -> Recipe {
            Recipe::new(
                self.directory.path(),
                self.git.to_str().unwrap(),
                Instant::now() + REQUEST_TIMEOUT,
            )
            .unwrap()
        }
    }

    #[test]
    fn clean_staged_unstaged_double_change_and_unborn_without_index_writes() {
        let mut repo = Repo::new();
        assert!(repo.changes().is_empty());
        repo.write("file.txt", "first\n");
        assert_eq!(repo.changes()[0].kind, GitChangeKind::Untracked);
        assert_eq!(
            repo.diff_result("file.txt", GitDiffKind::Unstaged)
                .unwrap_err()
                .code,
            "git_diff_unavailable"
        );
        repo.git(&["add", "--", "file.txt"]);
        let index = fs::read(repo.path(".git/index")).unwrap();
        let entry = repo.changes().remove(0);
        assert_eq!((entry.index, entry.worktree), ('A', '.'));
        assert!(repo
            .diff("file.txt", GitDiffKind::Staged)
            .contains("+first"));
        assert_eq!(fs::read(repo.path(".git/index")).unwrap(), index);
        repo.commit();
        assert!(repo.changes().is_empty());
        repo.write("file.txt", "second longer\n");
        let entry = repo.changes().remove(0);
        assert_eq!((entry.index, entry.worktree), ('.', 'M'));
        assert!(repo
            .diff("file.txt", GitDiffKind::Unstaged)
            .contains("+second longer"));
        repo.git(&["add", "--", "file.txt"]);
        repo.write("file.txt", "third even longer\n");
        let index = fs::read(repo.path(".git/index")).unwrap();
        let entry = repo.changes().remove(0);
        assert_eq!((entry.index, entry.worktree), ('M', 'M'));
        assert!(repo
            .diff("file.txt", GitDiffKind::Staged)
            .contains("+second longer"));
        assert!(repo
            .diff("file.txt", GitDiffKind::Unstaged)
            .contains("+third even longer"));
        assert_eq!(fs::read(repo.path(".git/index")).unwrap(), index);
        assert_eq!(
            fs::read_to_string(repo.path("file.txt")).unwrap(),
            "third even longer\n"
        );
        assert!(repo.workspace.tasks.is_none());
        assert!(repo.workspace.language.is_none());
    }

    #[test]
    fn deleted_parent_rename_add_delete_ignored_and_nested_untracked() {
        let mut repo = Repo::new();
        repo.write("gone/parent/file", "removed\n");
        repo.write("old", "renamed\n");
        repo.write(".gitignore", "ignored\n");
        repo.commit();
        fs::remove_dir_all(repo.path("gone")).unwrap();
        repo.git(&["mv", "old", "new"]);
        repo.write("nested/new/file", "untracked\n");
        repo.write("ignored", "ignored\n");
        let entries = repo.changes();
        assert!(entries
            .iter()
            .any(|entry| entry.path == "gone/parent/file" && entry.worktree == 'D'));
        assert!(entries
            .iter()
            .any(|entry| entry.path == "old" && entry.index == 'D'));
        assert!(entries
            .iter()
            .any(|entry| entry.path == "new" && entry.index == 'A'));
        assert!(
            entries
                .iter()
                .any(|entry| entry.path == "nested/new/file"
                    && entry.kind == GitChangeKind::Untracked)
        );
        assert!(!entries.iter().any(|entry| entry.path == "ignored"));
        assert!(repo
            .diff("gone/parent/file", GitDiffKind::Unstaged)
            .contains("-removed"));
        repo.git(&["add", "--all"]);
        assert!(repo
            .diff("gone/parent/file", GitDiffKind::Staged)
            .contains("-removed"));
        assert!(repo
            .diff("old", GitDiffKind::Staged)
            .contains("deleted file"));
        assert!(repo.diff("new", GitDiffKind::Staged).contains("new file"));
    }

    #[test]
    fn literal_paths_select_exactly_one_file_and_stale_selection_is_rejected() {
        let mut repo = Repo::new();
        let paths = [
            "-leading.txt",
            "snow 雪 é.txt",
            "bracket[1].txt",
            #[cfg(unix)]
            "star*.txt",
            #[cfg(unix)]
            "question?.txt",
            #[cfg(unix)]
            "tab\tfile",
            #[cfg(unix)]
            "line\nfile",
        ];
        for (index, path) in paths.iter().enumerate() {
            repo.write(path, &format!("before {index}\n"));
        }
        repo.commit();
        for (index, path) in paths.iter().enumerate() {
            repo.write(path, &format!("after distinct {index}\n"));
        }
        for (index, path) in paths.iter().enumerate() {
            let text = repo.diff(path, GitDiffKind::Unstaged);
            assert!(text.contains(&format!("+after distinct {index}")));
            assert_eq!(text.matches("diff --git ").count(), 1);
        }
        repo.write(paths[0], "before 0\n");
        assert_eq!(
            repo.diff_result(paths[0], GitDiffKind::Unstaged)
                .unwrap_err()
                .code,
            "git_diff_unavailable"
        );
        assert_eq!(
            repo.diff_result(paths[1], GitDiffKind::Staged)
                .unwrap_err()
                .code,
            "git_diff_unavailable"
        );
        for path in ["../file", ".git/config", ".", "snow 雪 é.txt/"] {
            assert_eq!(
                repo.diff_result(path, GitDiffKind::Unstaged)
                    .unwrap_err()
                    .code,
                "invalid_path"
            );
        }
    }

    #[test]
    fn conflicts_remain_status_only() {
        let mut repo = Repo::new();
        repo.write("conflict", "base\n");
        repo.commit();
        repo.git(&["checkout", "-b", "other"]);
        repo.write("conflict", "other\n");
        repo.commit();
        repo.git(&["checkout", "main"]);
        repo.write("conflict", "main\n");
        repo.commit();
        assert!(!repo
            .command(&["merge", "other"])
            .output()
            .unwrap()
            .status
            .success());
        let entry = repo
            .changes()
            .into_iter()
            .find(|entry| entry.path == "conflict")
            .unwrap();
        assert_eq!(entry.kind, GitChangeKind::Conflict);
        for kind in [GitDiffKind::Staged, GitDiffKind::Unstaged] {
            assert_eq!(
                repo.diff_result("conflict", kind).unwrap_err().code,
                "git_diff_unavailable"
            );
        }
    }

    #[test]
    fn ignored_gitlinks_and_file_replaced_by_directory_are_not_diffable() {
        let mut repo = Repo::new();
        repo.write("file", "before\n");
        repo.commit();
        let head = repo.command(&["rev-parse", "HEAD"]).output().unwrap();
        assert!(head.status.success());
        let object = String::from_utf8(head.stdout).unwrap();
        fs::create_dir(repo.path("module")).unwrap();
        repo.git(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},module", object.trim()),
        ]);
        fs::remove_file(repo.path("file")).unwrap();
        fs::create_dir(repo.path("file")).unwrap();
        let entries = repo.changes();
        // --ignore-submodules=all can suppress even a staged gitlink; any
        // reported nonregular mode is independently covered by the parser test.
        assert!(!entries.iter().any(|entry| entry.path == "module"));
        let entry = entries.iter().find(|entry| entry.path == "file").unwrap();
        assert_eq!(entry.kind, GitChangeKind::Unsupported);
        assert!(!entry.can_diff_staged && !entry.can_diff_unstaged);
        for path in ["module", "file"] {
            for kind in [GitDiffKind::Staged, GitDiffKind::Unstaged] {
                assert_eq!(
                    repo.diff_result(path, kind).unwrap_err().code,
                    "invalid_path"
                );
            }
        }
    }

    #[test]
    fn binary_summary_invalid_utf8_and_large_patch_are_never_lossy_success() {
        let mut repo = Repo::new();
        fs::write(repo.path("binary"), b"first\0binary").unwrap();
        repo.write("text", "before\n");
        repo.write("large", "before\n");
        repo.commit();
        fs::write(repo.path("binary"), b"second\0binary").unwrap();
        assert!(repo
            .diff("binary", GitDiffKind::Unstaged)
            .contains("Binary files"));
        fs::write(repo.path("text"), [0xff, b'\n']).unwrap();
        assert_eq!(
            repo.diff_result("text", GitDiffKind::Unstaged)
                .unwrap_err()
                .code,
            "invalid_utf8"
        );
        repo.write(
            "large",
            &"many changed lines\n".repeat(MAX_COMMAND_OUTPUT_BYTES / 4),
        );
        assert_eq!(
            repo.diff_result("large", GitDiffKind::Unstaged)
                .unwrap_err()
                .code,
            "output_limit"
        );
    }

    #[test]
    fn bare_gitfile_shared_and_alternate_layouts_are_rejected() {
        let mut repo = Repo::new();
        repo.git(&["config", "core.bare", "true"]);
        assert_eq!(
            repo.workspace
                .handle(Operation::GitChanges {
                    git_executable: repo.git.to_str().unwrap().into()
                })
                .unwrap_err()
                .code,
            "git_repository_unsupported"
        );
        repo.git(&["config", "core.bare", "false"]);
        for path in [
            "commondir",
            "gitdir",
            "objects/info/alternates",
            "objects/info/http-alternates",
        ] {
            repo.write(&format!(".git/{path}"), "elsewhere\n");
            assert!(Recipe::new(
                repo.directory.path(),
                repo.git.to_str().unwrap(),
                Instant::now() + REQUEST_TIMEOUT
            )
            .is_err());
            fs::remove_file(repo.path(&format!(".git/{path}"))).unwrap();
        }
        let outer = tempfile::tempdir().unwrap();
        fs::write(outer.path().join(".git"), "gitdir: elsewhere\n").unwrap();
        assert!(Recipe::new(
            outer.path(),
            repo.git.to_str().unwrap(),
            Instant::now() + REQUEST_TIMEOUT
        )
        .is_err());
        fs::create_dir(repo.path("child")).unwrap();
        assert!(Recipe::new(
            &repo.path("child"),
            repo.git.to_str().unwrap(),
            Instant::now() + REQUEST_TIMEOUT
        )
        .is_err());
    }

    #[test]
    fn hostile_inherited_git_routing_and_trace_are_removed_in_the_actual_recipe() {
        let repo = Repo::new();
        repo.write("file", "before\n");
        repo.commit();
        repo.write("file", "after\n");
        let external = tempfile::tempdir().unwrap();
        let mut inherited: Vec<_> = std::env::vars_os().collect();
        inherited.extend([
            (OsString::from("GIT_DIR"), external.path().into()),
            (OsString::from("gIt_wOrK_tReE"), external.path().into()),
            (
                OsString::from("GIT_INDEX_FILE"),
                external.path().join("redirected-index").into(),
            ),
            (
                OsString::from("GIT_TRACE"),
                external.path().join("trace").into(),
            ),
            (
                OsString::from("GIT_TRACE_SETUP"),
                external.path().join("trace-setup").into(),
            ),
            (OsString::from("GIT_CONFIG_COUNT"), OsString::from("1")),
            (
                OsString::from("GIT_CONFIG_KEY_0"),
                OsString::from("core.bare"),
            ),
            (OsString::from("GIT_CONFIG_VALUE_0"), OsString::from("true")),
        ]);
        let mut recipe = repo.recipe();
        recipe.environment = child_environment(inherited).unwrap();
        let index = fs::read(repo.path(".git/index")).unwrap();
        recipe.require_version().unwrap();
        recipe.require_nonbare().unwrap();
        assert_eq!(recipe.changes().unwrap()[0].path, "file");
        assert!(recipe
            .diff("file", GitDiffKind::Unstaged)
            .unwrap()
            .contains("+after"));
        assert_eq!(fs::read(repo.path(".git/index")).unwrap(), index);
        assert_eq!(fs::read_dir(external.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::os::unix::fs::{symlink, PermissionsExt};
        fn script(repo: &Repo, name: &str, source: &str) -> PathBuf {
            let path = repo.path(name);
            fs::write(&path, source).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            path
        }

        #[test]
        fn symlink_files_ancestors_and_metadata_are_unsupported() {
            let mut repo = Repo::new();
            repo.write("regular", "before\n");
            repo.commit();
            symlink("regular", repo.path("untracked-link")).unwrap();
            symlink("regular", repo.path("tracked-link")).unwrap();
            repo.git(&["add", "--", "tracked-link"]);
            let entries = repo.changes();
            assert!(entries
                .iter()
                .filter(|entry| entry.path.ends_with("link"))
                .all(|entry| entry.kind == GitChangeKind::Unsupported));
            assert_eq!(
                repo.diff_result("tracked-link", GitDiffKind::Staged)
                    .unwrap_err()
                    .code,
                "invalid_path"
            );
            let outside = tempfile::tempdir().unwrap();
            symlink(outside.path(), repo.path("link-parent")).unwrap();
            assert!(validate_file_path(repo.directory.path(), "link-parent/missing").is_err());
            symlink(outside.path(), repo.path(".git/refs/escape")).unwrap();
            assert!(Recipe::new(
                repo.directory.path(),
                repo.git.to_str().unwrap(),
                Instant::now() + REQUEST_TIMEOUT
            )
            .is_err());
        }

        #[test]
        fn non_utf8_filename_is_an_error_not_an_omitted_or_lossy_entry() {
            use std::os::unix::ffi::OsStringExt;
            let mut repo = Repo::new();
            fs::write(
                repo.directory.path().join(OsString::from_vec(vec![0xff])),
                "x",
            )
            .unwrap();
            assert_eq!(
                repo.workspace
                    .handle(Operation::GitChanges {
                        git_executable: repo.git.to_str().unwrap().into()
                    })
                    .unwrap_err()
                    .code,
                "invalid_utf8"
            );
        }

        #[test]
        fn old_missing_timeout_and_flooding_executables_fail_closed() {
            let repo = Repo::new();
            let old = script(
                &repo,
                "old-git",
                "#!/bin/sh\nprintf 'git version 2.44.9\\n'\n",
            );
            let recipe = Recipe::new(
                repo.directory.path(),
                old.to_str().unwrap(),
                Instant::now() + REQUEST_TIMEOUT,
            )
            .unwrap();
            assert_eq!(
                recipe.require_version().unwrap_err().code,
                "unsupported_git_version"
            );
            let rejected_option = script(
                &repo,
                "old-option-git",
                "#!/bin/sh\nprintf 'unknown option: --no-lazy-fetch\\n' >&2\nexit 129\n",
            );
            let recipe = Recipe::new(
                repo.directory.path(),
                rejected_option.to_str().unwrap(),
                Instant::now() + REQUEST_TIMEOUT,
            )
            .unwrap();
            assert_eq!(
                recipe.require_version().unwrap_err().code,
                "unsupported_git_version"
            );
            assert!(validate_executable("git").is_err());
            assert!(validate_executable(repo.path("absent").to_str().unwrap()).is_err());
            let sleeping = script(&repo, "sleeping-git", "#!/bin/sh\nsleep 30\n");
            let recipe = Recipe::new(
                repo.directory.path(),
                sleeping.to_str().unwrap(),
                Instant::now() + Duration::from_millis(100),
            )
            .unwrap();
            assert_eq!(
                recipe.require_version().unwrap_err().code,
                "command_timeout"
            );
            let flooding = script(&repo, "flooding-git", "#!/bin/sh\nwhile :; do printf 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'; done\n");
            let recipe = Recipe::new(
                repo.directory.path(),
                flooding.to_str().unwrap(),
                Instant::now() + REQUEST_TIMEOUT,
            )
            .unwrap();
            assert_eq!(recipe.require_version().unwrap_err().code, "output_limit");
        }

        #[test]
        fn configured_diff_textconv_fsmonitor_and_global_config_do_not_execute() {
            let mut repo = Repo::new();
            repo.write("file.txt", "before\n");
            repo.commit();
            let hostile = script(&repo, "hostile", "#!/bin/sh\ntouch hostile-ran\nexit 1\n");
            let command = format!("'{}'", hostile.display());
            for key in ["diff.external", "diff.evil.textconv", "core.fsmonitor"] {
                repo.git(&["config", key, &command]);
            }
            repo.write(".gitattributes", "*.txt diff=evil\n");
            repo.write("file.txt", "after longer\n");
            let index = fs::read(repo.path(".git/index")).unwrap();
            assert!(repo
                .diff("file.txt", GitDiffKind::Unstaged)
                .contains("+after longer"));
            assert!(!repo.path("hostile-ran").exists());
            assert_eq!(fs::read(repo.path(".git/index")).unwrap(), index);
            let global = repo.path("hostile-global");
            fs::write(&global, "[core]\n bare = true\n").unwrap();
            let mut inherited: Vec<_> = std::env::vars_os().collect();
            inherited.push(("GIT_CONFIG_GLOBAL".into(), global.into()));
            let mut recipe = repo.recipe();
            recipe.environment = child_environment(inherited).unwrap();
            recipe.require_nonbare().unwrap();
            assert!(!recipe.changes().unwrap().is_empty());
        }

        #[test]
        fn git_cleanup_does_not_cancel_an_independent_run_task() {
            let mut repo = Repo::new();
            repo.write("file", "before\n");
            repo.commit();
            repo.write("file", "after\n");
            let Payload::RunTask { snapshot } = repo
                .workspace
                .handle(Operation::RunStart {
                    program: "/bin/sh".into(),
                    args: vec!["-c".into(), "sleep 30".into()],
                    timeout_secs: 40,
                })
                .unwrap()
            else {
                panic!("wrong task response")
            };
            let snapshot: cedar_tasks::TaskSnapshot = serde_json::from_value(snapshot).unwrap();
            let task_id = snapshot.id;
            assert!(!repo.changes().is_empty());
            assert!(repo.diff("file", GitDiffKind::Unstaged).contains("+after"));
            let Payload::RunTask { snapshot } = repo
                .workspace
                .handle(Operation::RunPoll { task_id })
                .unwrap()
            else {
                panic!("wrong task response")
            };
            let snapshot: cedar_tasks::TaskSnapshot = serde_json::from_value(snapshot).unwrap();
            assert!(!snapshot.state.is_terminal());
            repo.workspace
                .handle(Operation::RunCancel { task_id })
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let Payload::RunTask { snapshot } = repo
                    .workspace
                    .handle(Operation::RunPoll { task_id })
                    .unwrap()
                else {
                    panic!("wrong task response")
                };
                let snapshot: cedar_tasks::TaskSnapshot = serde_json::from_value(snapshot).unwrap();
                if snapshot.state.is_terminal() {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "task cancellation did not finish"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        #[test]
        fn explicit_trust_still_allows_repository_clean_filters() {
            let mut repo = Repo::new();
            repo.write("file.txt", "before\n");
            repo.commit();
            let filter = script(
                &repo,
                "trusted-filter",
                "#!/bin/sh\ntouch trusted-filter-ran\ncat\n",
            );
            repo.git(&[
                "config",
                "filter.cedar.clean",
                &format!("'{}'", filter.display()),
            ]);
            repo.write(".gitattributes", "*.txt filter=cedar\n");
            repo.write("file.txt", "after!\n");
            let index = fs::read(repo.path(".git/index")).unwrap();
            assert!(repo.changes().iter().any(|entry| entry.path == "file.txt"));
            assert!(
                repo.path("trusted-filter-ran").exists(),
                "read-only Git is not a sandbox against trusted filters"
            );
            assert_eq!(fs::read(repo.path(".git/index")).unwrap(), index);
            assert_eq!(
                fs::read_to_string(repo.path("file.txt")).unwrap(),
                "after!\n"
            );
        }
    }
}
