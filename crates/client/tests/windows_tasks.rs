//! Real Windows isolated-agent integration tests; deliberately opt-in.
//!
//! Prebuild cedar-agent and cedar-winprocess-fixture, set absolute paths in
//! CEDAR_AGENT_BIN, CEDAR_WINPROCESS_FIXTURE_BIN, and
//! CEDAR_CLIENT_BUNDLE_PROBE_BIN (the feature-gated bundle probe), then run:
//! cargo test -p cedar-client --features fixtures --test windows_tasks -- --ignored --test-threads=1
//! No compiler discovery, global environment edits, shell, credentials, or PID
//! termination. Cross-compilation does not count as Windows runtime evidence.
#![cfg(windows)]

#[path = "support/windows_task_harness.rs"]
mod harness;

use cedar_client::Client;
use cedar_protocol::{Operation, Payload, LANGUAGE_SESSION_CAPABILITIES, RUN_TASK_CAPABILITIES};
use cedar_tasks::{TaskState, MAX_COMPLETED_TASKS, MAX_OUTPUT_BYTES_PER_STREAM};
use cedar_workspace::Workspace;
use harness::*;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires real Windows, exact prebuilt agent/fixture paths; run serially"]
fn isolated_capabilities_enforce_trust_and_do_not_enable_the_in_process_host() {
    let _watchdog = Watchdog::start();
    let root = tempfile::tempdir().unwrap();
    let mut untrusted = RawAgent::new(root.path(), false);
    let info = metadata(untrusted.ok(Operation::Hello));
    assert_eq!(info.os, "windows");
    assert_eq!(info.arch, std::env::consts::ARCH);
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(
        info.capabilities,
        [
            "list",
            "read",
            "run_cancel",
            "run_poll",
            "run_start",
            "search",
            "write"
        ]
    );
    for cap in ["list", "read", "write", "search"]
        .iter()
        .chain(RUN_TASK_CAPABILITIES)
    {
        assert!(info.supports(cap), "missing isolated capability {cap}");
    }
    for cap in ["run", "git_status", "terminal"]
        .iter()
        .chain(LANGUAGE_SESSION_CAPABILITIES)
    {
        assert!(!info.supports(cap), "overstated isolated capability {cap}");
    }
    let marker = root.path().join("must-not-run");
    for op in [
        start(&["marker", &text_path(&marker)], 3),
        Operation::RunPoll { task_id: 1 },
        Operation::RunCancel { task_id: 1 },
    ] {
        assert_eq!(untrusted.request(op).unwrap_err().code, "run_disabled");
    }
    assert!(!marker.exists());
    assert_eq!(metadata(untrusted.ok(Operation::Hello)), info);

    let mut trusted = RawAgent::new(root.path(), true);
    assert_eq!(metadata(trusted.ok(Operation::Hello)), info);
    for op in [
        Operation::Run {
            program: text_path(&fixture()),
            args: vec![],
            timeout_secs: 1,
        },
        Operation::GitStatus,
        Operation::LanguageStart {
            program: text_path(&fixture()),
            args: vec![],
        },
    ] {
        assert_eq!(
            trusted.request(op).unwrap_err().code,
            "unsupported_platform"
        );
    }
    let mut in_process = Workspace::open(root.path()).unwrap();
    in_process.set_allow_run(true);
    let local = metadata(in_process.handle(Operation::Hello).unwrap());
    for cap in RUN_TASK_CAPABILITIES {
        assert!(
            !local.supports(cap),
            "ordinary host must not advertise {cap}"
        );
    }
    assert_eq!(
        in_process
            .handle(start(&["marker", &text_path(&marker)], 3))
            .unwrap_err()
            .code,
        "unsupported_platform"
    );
    assert!(!marker.exists());
    untrusted.close_cleanly();
    trusted.close_cleanly();
}

#[test]
#[ignore = "requires real Windows, exact prebuilt agent/fixture paths; run serially"]
fn live_tasks_allow_file_work_cancel_idempotently_and_reconnect_without_adoption() {
    let _watchdog = Watchdog::start();
    let root = tempfile::tempdir().unwrap();
    let mut agent = RawAgent::new(root.path(), true);
    let id = agent.start(start(&["tree-live", &text_path(root.path())], 30));
    let tree = ObservedTree::ready(root.path());
    agent.ok(Operation::Write {
        path: "during.txt".into(),
        text: "while running 雪".into(),
        expected_revision: None,
    });
    assert!(
        matches!(agent.ok(Operation::Read { path: "during.txt".into() }), Payload::File { text, .. } if text == "while running 雪")
    );
    assert!(
        matches!(agent.ok(Operation::Search { query: "while running".into(), limit: 5 }), Payload::Matches { matches, .. } if matches.len() == 1)
    );
    assert_eq!(
        agent.request(start(&["exit", "0"], 3)).unwrap_err().code,
        "task_busy"
    );
    for op in [
        Operation::Run {
            program: text_path(&fixture()),
            args: vec!["exit".into(), "0".into()],
            timeout_secs: 1,
        },
        Operation::GitStatus,
        Operation::LanguageStart {
            program: text_path(&fixture()),
            args: vec![],
        },
    ] {
        assert_eq!(agent.request(op).unwrap_err().code, "unsupported_platform");
    }
    assert!(
        tree.all_alive(),
        "file operations must finish while task is live"
    );
    let cancelled = task(agent.ok(Operation::RunCancel { task_id: id }));
    assert!(matches!(
        cancelled.state,
        TaskState::Cancelling | TaskState::Cancelled
    ));
    agent.ok(Operation::RunCancel { task_id: id });
    let terminal = agent.terminal(id, "cancelled live tree");
    assert_eq!(terminal.state, TaskState::Cancelled, "{terminal:?}");
    tree.assert_already_stopped("cancelled terminal snapshot");
    agent.stable(&terminal);

    let mut ids = Vec::new();
    for index in 0..=MAX_COMPLETED_TASKS {
        let new = agent.start(start(&["exit", "0"], 3));
        assert!(new > id, "task IDs must not be reused within the agent");
        if let Some(previous) = ids.last() {
            assert!(new > *previous);
        }
        assert_eq!(
            agent.terminal(new, &format!("history {index}")).state,
            TaskState::Succeeded
        );
        ids.push(new);
    }
    for expired in [id, ids[0]] {
        for op in [
            Operation::RunPoll { task_id: expired },
            Operation::RunCancel { task_id: expired },
        ] {
            assert_eq!(agent.request(op).unwrap_err().code, "unknown_task");
        }
    }
    for retained in &ids[1..] {
        assert_eq!(
            task(agent.ok(Operation::RunPoll { task_id: *retained })).state,
            TaskState::Succeeded
        );
    }
    let last = *ids.last().unwrap();
    agent.close_cleanly();
    let mut fresh = RawAgent::new(root.path(), true);
    for op in [
        Operation::RunPoll { task_id: last },
        Operation::RunCancel { task_id: last },
    ] {
        assert_eq!(fresh.request(op).unwrap_err().code, "unknown_task");
    }
    assert!(
        matches!(fresh.ok(Operation::Read { path: "during.txt".into() }), Payload::File { text, .. } if text == "while running 雪")
    );
    let id = fresh.start(start(&["exit", "0"], 3));
    assert_eq!(
        fresh.terminal(id, "fresh session task").state,
        TaskState::Succeeded
    );
    fresh.close_cleanly();
}

#[test]
#[ignore = "requires real Windows, exact prebuilt agent/fixture paths; run serially"]
fn native_argv_cwd_stdin_exit_codes_and_launch_validation_round_trip() {
    let _watchdog = Watchdog::start();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace with spaces 雪");
    let agent_cwd = directory.path().join("different agent cwd");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&agent_cwd).unwrap();
    // The agent cwd differs from the workspace, making the child-cwd assertion
    // meaningful. A valid relative executable is resolvable from either place,
    // so missing-file metadata cannot masquerade as relative-path rejection.
    fs::copy(fixture(), agent_cwd.join("cedar-winprocess-fixture.exe")).unwrap();
    let original_cwd = std::env::current_dir().unwrap();
    let mut agent = RawAgent::with_cwd(&root, true, Some(&agent_cwd));
    let arguments = [
        "",
        "plain",
        "two words",
        "\t\n",
        "雪 café 🚀",
        "\"",
        "inside\"quote",
        "\\",
        "trailing space \\",
        "slashes\\\\\\\"and quote",
        "& | < > ^ %PATH% $(never) ;",
    ];
    let args: Vec<_> = std::iter::once("inspect")
        .chain(arguments.iter().copied())
        .collect();
    let id = agent.start(start(&args, 3));
    let terminal = agent.terminal(id, "literal argv/cwd");
    assert_eq!(terminal.state, TaskState::Succeeded, "{terminal:?}");
    let mut lines = terminal.stdout.lines();
    let cwd_hex = lines.next().unwrap().strip_prefix("cwd:").unwrap();
    let decoded = (0..cwd_hex.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&cwd_hex[offset..offset + 2], 16).unwrap())
        .collect();
    let reported_cwd = PathBuf::from(String::from_utf8(decoded).unwrap());
    assert_eq!(
        reported_cwd.canonicalize().unwrap(),
        root.canonicalize().unwrap()
    );
    let expected: Vec<_> = arguments
        .iter()
        .map(|arg| format!("arg:{}", hex(arg.as_bytes())))
        .collect();
    assert_eq!(lines.collect::<Vec<_>>(), expected);
    assert_eq!(terminal.stderr, "");
    assert_eq!(std::env::current_dir().unwrap(), original_cwd);
    let id = agent.start(start(&["null-stdin"], 3));
    let terminal = agent.terminal(id, "null stdin");
    assert_eq!(terminal.state, TaskState::Succeeded, "{terminal:?}");
    assert_eq!(terminal.stdout, "stdin-eof\n");
    let id = agent.start(start(&["no-console"], 3));
    let terminal = agent.terminal(id, "stdio task has no console window");
    assert_eq!(terminal.state, TaskState::Succeeded, "{terminal:?}");
    assert_eq!(terminal.stdout, "console-window:null\n");
    assert_eq!(terminal.stderr, "console-stderr-ready\n");

    for code in [0u32, 23, 259, 0x8000_0001, 0xffff_ffff] {
        let id = agent.start(start(&["exit", &code.to_string()], 3));
        let terminal = agent.terminal(id, &format!("native exit {code:#x}"));
        assert_eq!(
            terminal.state,
            if code == 0 {
                TaskState::Succeeded
            } else {
                TaskState::Failed
            }
        );
        assert_eq!(terminal.windows_exit_code, Some(code));
        assert_eq!(terminal.exit_code, i32::try_from(code).ok());
        assert!(!terminal.truncated);
        agent.stable(&terminal);
    }

    let marker = root.join("must-not-run");
    fs::copy(fixture(), root.join("cedar-winprocess-fixture.exe")).unwrap();
    let script = root.join("script.cmd");
    fs::write(&script, format!("@echo ran>\"{}\"\r\n", marker.display())).unwrap();
    let batch = root.join("script.bat");
    fs::copy(&script, &batch).unwrap();
    let disguised = root.join("disguised.cmd");
    fs::copy(fixture(), &disguised).unwrap();
    for (label, program) in [
        ("relative exe", "cedar-winprocess-fixture.exe".into()),
        (
            "relative dot path",
            ".\\cedar-winprocess-fixture.exe".into(),
        ),
        ("cmd script", text_path(&script)),
        ("batch script", text_path(&batch)),
        ("disguised native binary", text_path(&disguised)),
    ] {
        let id = agent.start(Operation::RunStart {
            program,
            args: vec!["marker".into(), text_path(&marker)],
            timeout_secs: 3,
        });
        let terminal = agent.terminal(id, label);
        assert_eq!(
            terminal.state,
            TaskState::SpawnFailed,
            "{label}: {terminal:?}"
        );
        assert!(terminal.error.as_ref().is_some_and(|s| !s.is_empty()));
        agent.stable(&terminal);
        assert!(!marker.exists(), "{label} executed");
    }
    for program in [
        String::new(),
        "C:\\bad\0.exe".into(),
        "x".repeat(cedar_tasks::MAX_PROGRAM_BYTES + 1),
    ] {
        assert_eq!(
            agent
                .request(Operation::RunStart {
                    program,
                    args: vec![],
                    timeout_secs: 3
                })
                .unwrap_err()
                .code,
            "invalid_command"
        );
    }
    assert_eq!(
        agent
            .request(start(&["inspect", "embedded\0argument"], 3))
            .unwrap_err()
            .code,
        "invalid_command"
    );
    for seconds in [0, 301] {
        assert_eq!(
            agent
                .request(start(&["exit", "0"], seconds))
                .unwrap_err()
                .code,
            "invalid_timeout"
        );
    }
    let invalid = root.join("invalid.exe");
    fs::write(&invalid, b"not a native executable").unwrap();
    for (label, executable) in [
        ("missing", root.join("missing.exe")),
        ("invalid native image", invalid),
    ] {
        let id = agent.start(Operation::RunStart {
            program: text_path(&executable),
            args: vec!["marker".into(), text_path(&marker)],
            timeout_secs: 3,
        });
        let terminal = agent.terminal(id, label);
        assert_eq!(
            terminal.state,
            TaskState::SpawnFailed,
            "{label}: {terminal:?}"
        );
        assert!(terminal.error.as_ref().is_some_and(|s| !s.is_empty()));
        assert_eq!(terminal.windows_exit_code, None);
        agent.stable(&terminal);
    }
    assert!(!marker.exists());
    let id = agent.start(start(&["exit", "0"], 3));
    assert_eq!(
        agent
            .terminal(id, "valid task after invalid launches")
            .state,
        TaskState::Succeeded
    );
    agent.close_cleanly();
}

#[test]
#[ignore = "requires real Windows, exact prebuilt agent/fixture paths; run serially"]
fn output_caps_final_drain_and_native_tree_completion_are_bounded() {
    let _watchdog = Watchdog::start();
    let root = tempfile::tempdir().unwrap();
    let mut agent = RawAgent::new(root.path(), true);
    let id = agent.start(start(&["split-utf8"], 3));
    let terminal = agent.terminal(id, "split UTF-8 final drain");
    assert_eq!(terminal.state, TaskState::Succeeded, "{terminal:?}");
    assert_eq!(terminal.stdout, "雪-stdout-final\n");
    assert_eq!(terminal.stderr, "🚀-stderr-final\n");
    assert!(!terminal.truncated);
    let id = agent.start(start(&["flood", "65536", "0"], 3));
    let terminal = agent.terminal(id, "concurrent streams and final suffixes");
    assert_eq!(terminal.state, TaskState::Succeeded, "{terminal:?}");
    assert_eq!(
        terminal.stdout,
        format!("{}\nstdout-end\n", "O".repeat(65536))
    );
    assert_eq!(
        terminal.stderr,
        format!("{}\nstderr-end\n", "E".repeat(65536))
    );
    assert!(!terminal.truncated);

    let cap = MAX_OUTPUT_BYTES_PER_STREAM;
    for (label, out, err, limited) in [
        ("both streams exactly at cap", cap, cap, false),
        ("stdout one byte over", cap + 1, 0, true),
        ("stderr one byte over", 0, cap + 1, true),
        ("both streams over", cap + 8192, cap + 8192, true),
    ] {
        let id = agent.start(start(
            &["exact-streams", &out.to_string(), &err.to_string()],
            3,
        ));
        let terminal = agent.terminal(id, label);
        assert_eq!(
            terminal.state,
            if limited {
                TaskState::OutputLimit
            } else {
                TaskState::Succeeded
            },
            "{label}: {terminal:?}"
        );
        assert_eq!(terminal.truncated, limited, "{label}");
        assert!(
            terminal.stdout.len() <= cap && terminal.stderr.len() <= cap,
            "{label}"
        );
        if out > cap && err == 0 {
            assert_eq!(terminal.stdout.len(), cap, "{label}");
        }
        if err > cap && out == 0 {
            assert_eq!(terminal.stderr.len(), cap, "{label}");
        }
        if !limited {
            assert_eq!(terminal.stdout, "O".repeat(cap));
            assert_eq!(terminal.stderr, "E".repeat(cap));
        }
        agent.stable(&terminal);
    }

    let overflow_dir = root.path().join("overflow-tree");
    fs::create_dir(&overflow_dir).unwrap();
    let flood_gate = overflow_dir.join("flood-now");
    let id = agent.start(start(
        &[
            "tree-flood",
            &text_path(&overflow_dir),
            &text_path(&flood_gate),
        ],
        30,
    ));
    let tree = ObservedTree::ready(&overflow_dir);
    assert!(tree.all_alive());
    fs::write(&flood_gate, b"go").unwrap();
    let terminal = agent.terminal(id, "overflow must stop the live tree");
    assert_eq!(terminal.state, TaskState::OutputLimit, "{terminal:?}");
    assert!(terminal.truncated);
    assert!(terminal.stdout.len() <= cap && terminal.stderr.len() <= cap);
    tree.assert_already_stopped("overflow terminal snapshot");
    agent.stable(&terminal);

    for (label, timeout) in [("natural-root-exit", 30), ("timeout", 2)] {
        let tree_dir = root.path().join(label);
        fs::create_dir(&tree_dir).unwrap();
        let gate = tree_dir.join("exit-now");
        let op = if label == "natural-root-exit" {
            start(
                &["tree-exit", &text_path(&tree_dir), &text_path(&gate)],
                timeout,
            )
        } else {
            start(&["tree-live", &text_path(&tree_dir)], timeout)
        };
        let id = agent.start(op);
        let tree = ObservedTree::ready(&tree_dir);
        assert!(tree.all_alive());
        if label == "natural-root-exit" {
            fs::write(&gate, b"go").unwrap();
        }
        let terminal = agent.terminal(id, label);
        assert_eq!(
            terminal.state,
            if label == "natural-root-exit" {
                TaskState::Failed
            } else {
                TaskState::TimedOut
            },
            "{label}: {terminal:?}"
        );
        if label == "natural-root-exit" {
            assert_eq!(terminal.windows_exit_code, Some(23));
            assert_eq!(terminal.exit_code, Some(23));
        }
        assert!(terminal.stdout.contains("root-ready"));
        assert!(terminal.stdout.contains("leaf-stdout-ready"));
        assert!(terminal.stderr.contains("leaf-stderr-ready"));
        tree.assert_already_stopped(label);
        agent.stable(&terminal);
    }
    agent.close_cleanly();
}

#[test]
#[ignore = "requires real Windows, exact prebuilt agent/fixture paths; run serially"]
fn session_failure_and_forced_agent_death_clean_the_exact_live_descendant_tree() {
    let _watchdog = Watchdog::start();
    for failure in [
        "eof",
        "malformed",
        "truncated",
        "oversized",
        "broken-stdout",
        "forced-agent-death",
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut agent = RawAgent::new(root.path(), true);
        agent.start(start(&["tree-live", &text_path(root.path())], 30));
        let tree = ObservedTree::ready(root.path());
        assert!(
            tree.all_alive(),
            "{failure}: fixture must be live before injection"
        );
        agent.inject_failure(failure);
        let status = agent.wait_exit(failure);
        assert_eq!(status.success(), failure == "eof", "{failure}: {status}");
        // This happens BEFORE RawAgent's cleanup guard. No outer job encloses
        // the agent: forced death must close its own task job handles itself.
        tree.assert_stopped(failure);
        if failure != "eof" && failure != "forced-agent-death" {
            assert!(
                agent.diagnostics().contains("protocol stream closed:"),
                "{failure}: {}",
                agent.diagnostics()
            );
        }
    }
    let root = tempfile::tempdir().unwrap();
    let mut client = Client::spawn_agent(&agent_binary(), root.path(), true).unwrap();
    task(
        client
            // Beyond the fixture's cap: a normal timeout cannot fake drop cleanup.
            .request(start(&["tree-live", &text_path(root.path())], 30))
            .unwrap(),
    );
    let tree = ObservedTree::ready(root.path());
    assert!(tree.all_alive());
    let before = Instant::now();
    drop(client);
    assert!(
        before.elapsed() < Duration::from_millis(500),
        "Client::drop blocked caller"
    );
    tree.assert_stopped("Client::drop");
}

#[test]
#[ignore = "requires real Windows, exact prebuilt agent/fixture paths; run serially"]
fn windows_local_uses_only_the_current_executable_sibling_agent() {
    let _watchdog = Watchdog::start();
    let probe_source = binary("CEDAR_CLIENT_BUNDLE_PROBE_BIN");
    assert!(probe_source.is_file(), "prebuilt bundle probe is missing");
    for case in ["success", "missing", "invalid"] {
        let directory = tempfile::tempdir().unwrap();
        let bundle = directory.path().join("isolated bundle 雪");
        let cwd_decoy = directory.path().join("cwd-decoy");
        let path_decoy = directory.path().join("path-decoy");
        let root = directory.path().join("workspace");
        for path in [&bundle, &cwd_decoy, &path_decoy, &root] {
            fs::create_dir(path).unwrap();
        }
        let probe = bundle.join("cedar-client-bundle-probe.exe");
        fs::copy(&probe_source, &probe).unwrap();
        // Valid fallback binaries make an accidental PATH/cwd/env lookup
        // connect successfully, which the negative probe explicitly rejects.
        fs::copy(agent_binary(), cwd_decoy.join("cedar-agent.exe")).unwrap();
        let env_decoy = path_decoy.join("cedar-agent.exe");
        fs::copy(agent_binary(), &env_decoy).unwrap();
        match case {
            "success" => {
                fs::copy(agent_binary(), bundle.join("cedar-agent.exe")).unwrap();
            }
            "invalid" => {
                fs::write(bundle.join("cedar-agent.exe"), b"invalid native image").unwrap()
            }
            "missing" => {}
            _ => unreachable!(),
        }
        let mut command = Command::new(&probe);
        command
            .arg(case)
            .arg(&root)
            .arg(fixture())
            .current_dir(&cwd_decoy)
            .env("PATH", &path_decoy)
            .env("CEDAR_AGENT_BIN", &env_decoy)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let result = run_probe(command, case);
        assert!(
            result.status.success(),
            "{case}: status={} stdout={} stderr={}",
            result.status,
            result.stdout,
            result.stderr
        );
        assert!(
            result.stdout.contains(if case == "success" {
                "bundle-ok:"
            } else {
                "expected-error:"
            }),
            "{case}: {}",
            result.stdout
        );
    }
}

#[test]
#[ignore = "requires real Windows, exact prebuilt agent/fixture paths; run serially"]
fn completed_tasks_release_owned_agent_handles_after_repeated_launches() {
    let _watchdog = Watchdog::start();
    let root = tempfile::tempdir().unwrap();
    let mut agent = RawAgent::new(root.path(), true);
    // Warm the supervisor, named-pipe machinery and retained history first.
    for _ in 0..3 {
        let id = agent.start(start(&["exit", "0"], 3));
        assert_eq!(
            agent.terminal(id, "handle-count warmup").state,
            TaskState::Succeeded
        );
    }
    let baseline = agent.handle_count();
    for index in 0..24 {
        let id = agent.start(start(&["exit", "0"], 3));
        assert_eq!(
            agent
                .terminal(id, &format!("handle-count cycle {index}"))
                .state,
            TaskState::Succeeded
        );
    }
    let after = agent.handle_count();
    assert!(
        after <= baseline + 2,
        "completed tasks leaked agent handles: baseline={baseline}, after={after}"
    );
    agent.close_cleanly();
}
