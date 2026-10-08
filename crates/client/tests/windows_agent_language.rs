//! Nonshipping generic agent/Workspace integration acceptance. Production Java
//! has a separate scoped route; these fixtures never grant arbitrary LSP startup.
//! Prebuild the validation agent, normal agent, mock LSP and process fixture.
//! Set CEDAR_AGENT_LANGUAGE_VALIDATION_BIN, CEDAR_AGENT_BIN,
//! CEDAR_MOCK_LSP_BIN and CEDAR_WINPROCESS_FIXTURE_BIN to their absolute paths.
//! cargo test -p cedar-client --features windows-language-validation --test windows_agent_language -- --ignored --test-threads=1
//! These tests exercise the real cedar_agent::serve and Workspace bridge. They
//! require native Windows; cross-target compilation is not a runtime pass.
#![cfg(windows)]

// Shared with the broader task suite, whose bundle/output helpers are unused here.
#[allow(dead_code)]
#[path = "support/windows_task_harness.rs"]
mod harness;

use cedar_protocol::{LanguageQueryKind, Operation, Payload, JAVA_LANGUAGE_SESSION_CAPABILITIES};
use cedar_tasks::TaskState;
use cedar_workspace::{BackendMode, Workspace};
use harness::*;
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::Command,
};

fn synthetic_root() -> tempfile::TempDir {
    let root = tempfile::Builder::new()
        .prefix("cedar agent language 雪 ")
        .tempdir()
        .unwrap();
    fs::write(
        root.path().join(".cedar-windows-language-validation"),
        b"cedar-windows-language-validation-v1\n",
    )
    .unwrap();
    root
}

fn validation_agent(root: &Path, allow_run: bool) -> RawAgent {
    let mut command = Command::new(binary("CEDAR_AGENT_LANGUAGE_VALIDATION_BIN"));
    command.arg("--synthetic-root").arg(root);
    if allow_run {
        command.arg("--allow-run");
    }
    RawAgent::from_command(command)
}

fn language_start(dir: &Path, task_ready: Option<&Path>) -> Operation {
    let mut args = vec!["win-agent-tree".into(), text_path(dir)];
    if let Some(ready) = task_ready {
        args.push(text_path(ready));
    }
    Operation::LanguageStart {
        program: text_path(&binary("CEDAR_MOCK_LSP_BIN")),
        args,
    }
}

fn language(payload: Payload) -> serde_json::Value {
    let Payload::Language { value } = payload else {
        panic!("expected language response, got {payload:?}")
    };
    value
}

struct LanguageTree {
    observed: ObservedTree,
    dir: PathBuf,
}
impl LanguageTree {
    fn ready(dir: &Path) -> Self {
        assert!(
            dir.join("root.ready").is_file(),
            "initialize preceded fixture readiness"
        );
        let tree = Self {
            observed: ObservedTree::observe(dir, &["root.pid", "child.pid", "grandchild.pid"]),
            dir: dir.into(),
        };
        tree.assert_alive();
        tree
    }
    fn assert_alive(&self) {
        assert!(
            self.observed.all_alive(),
            "an independently owned LSP process stopped"
        );
        for name in ["root", "child", "grandchild"] {
            let error = OpenOptions::new()
                .write(true)
                .open(self.dir.join(format!("{name}.lock")))
                .unwrap_err();
            assert_eq!(
                error.raw_os_error(),
                Some(32),
                "{name} must hold its exact lifetime file exclusively"
            );
        }
    }
    fn assert_stopped(&self, context: &str) {
        self.observed.assert_stopped(context);
        for name in ["root", "child", "grandchild"] {
            OpenOptions::new()
                .write(true)
                .open(self.dir.join(format!("{name}.lock")))
                .unwrap_or_else(|error| {
                    panic!("{context}: {name} retained lifetime file: {error}")
                });
            assert!(
                !self.dir.join(format!("{name}.expired")).exists(),
                "{context}: {name} reached its safety cap"
            );
        }
    }
}

fn hover(agent: &mut RawAgent) {
    let value = language(agent.ok(Operation::LanguageQuery {
        path: "Main.java".into(),
        line: 0,
        character: 0,
        kind: LanguageQueryKind::Hover,
    }));
    assert_eq!(value["contents"]["value"], "mock hover");
}

fn open_document(agent: &mut RawAgent) {
    agent.ok(Operation::LanguageOpen {
        path: "Main.java".into(),
        language_id: "java".into(),
        version: 1,
        text: "class Main { /* 雪 */ }".into(),
    });
    hover(agent);
}

fn start_both(
    agent: &mut RawAgent,
    root: &Path,
    wave: usize,
    task_first: bool,
) -> (u64, ObservedTree, LanguageTree) {
    let task_dir = root.join(format!("task {wave}"));
    let language_dir = root.join(format!("language {wave}"));
    fs::create_dir(&task_dir).unwrap();
    fs::create_dir(&language_dir).unwrap();
    let id;
    if task_first {
        // RunStart admits asynchronously. Do not poll readiness before sending
        // LanguageStart: the synthetic task waits for the LSP and the LSP waits
        // for the task tree, proving both startups are in progress together.
        id = agent.start(start(
            &[
                "tree-coexist",
                &text_path(&task_dir),
                &text_path(&language_dir.join("root.ready")),
            ],
            30,
        ));
        assert_eq!(
            language(agent.ok(language_start(
                &language_dir,
                Some(&task_dir.join("tree.ready"))
            )))["started"],
            true
        );
        assert!(task_dir.join("startup.waiting").is_file());
    } else {
        assert_eq!(
            language(agent.ok(language_start(&language_dir, None)))["started"],
            true
        );
        id = agent.start(start(&["tree-live", &text_path(&task_dir)], 30));
    }
    let task_tree = ObservedTree::ready(&task_dir);
    let language_tree = LanguageTree::ready(&language_dir);
    assert_eq!(
        task(agent.ok(Operation::RunPoll { task_id: id })).state,
        TaskState::Running
    );
    open_document(agent);
    assert!(task_tree.all_alive());
    language_tree.assert_alive();
    (id, task_tree, language_tree)
}

#[test]
#[ignore = "requires native Windows and exact prebuilt fixture paths; run serially"]
fn fixture_trust_is_explicit_and_normal_all_feature_hosts_remain_gated() {
    let _watchdog = Watchdog::start();
    let root = synthetic_root();
    let lsp_dir = root.path().join("must not launch");
    fs::create_dir(&lsp_dir).unwrap();
    let mut normal = RawAgent::new(root.path(), true);
    let info = metadata(normal.ok(Operation::Hello));
    assert!(!info.supports("language_start"));
    for cap in JAVA_LANGUAGE_SESSION_CAPABILITIES {
        assert!(info.supports(cap), "standard isolated binary omitted {cap}");
    }
    assert_eq!(
        normal
            .request(language_start(&lsp_dir, None))
            .unwrap_err()
            .code,
        "unsupported_platform"
    );
    normal.close_cleanly();
    for backend_mode in [BackendMode::InProcess, BackendMode::IsolatedAgent] {
        let mut workspace = Workspace::with_backend_mode(root.path(), backend_mode).unwrap();
        workspace.set_allow_run(true);
        assert_eq!(
            workspace
                .handle(language_start(&lsp_dir, None))
                .unwrap_err()
                .code,
            "unsupported_platform"
        );
        let info = metadata(workspace.handle(Operation::Hello).unwrap());
        assert!(!info.supports("language_start"));
        for cap in JAVA_LANGUAGE_SESSION_CAPABILITIES {
            assert_eq!(
                info.supports(cap),
                backend_mode == BackendMode::IsolatedAgent,
                "{cap}"
            );
        }
    }
    let mut untrusted = validation_agent(root.path(), false);
    assert_eq!(metadata(untrusted.ok(Operation::Hello)), info);
    assert_eq!(
        untrusted
            .request(language_start(&lsp_dir, None))
            .unwrap_err()
            .code,
        "run_disabled"
    );
    assert_eq!(
        untrusted
            .request(start(
                &["marker", &text_path(&root.path().join("must-not-run"))],
                1
            ))
            .unwrap_err()
            .code,
        "run_disabled"
    );
    assert!(!lsp_dir.join("root.lock").exists());
    assert!(!root.path().join("must-not-run").exists());
    untrusted.close_cleanly();
}

#[test]
#[ignore = "requires native Windows and exact prebuilt fixture paths; run serially"]
fn repeated_overlapping_startups_stop_each_owner_independently() {
    let _watchdog = Watchdog::start();
    let root = synthetic_root();
    let mut agent = validation_agent(root.path(), true);
    let info = metadata(agent.ok(Operation::Hello));
    for wave in 0..4 {
        let (id, task_tree, language_tree) = start_both(&mut agent, root.path(), wave, wave != 3);
        agent.ok(Operation::Write {
            path: format!("during-{wave}.txt"),
            text: "both trees live 雪".into(),
            expected_revision: None,
        });
        assert!(
            matches!(agent.ok(Operation::Read { path: format!("during-{wave}.txt") }), Payload::File { text, .. } if text == "both trees live 雪")
        );
        assert_eq!(metadata(agent.ok(Operation::Hello)), info);
        if wave % 2 == 0 {
            assert_eq!(language(agent.ok(Operation::LanguageStop))["stopped"], true);
            language_tree.assert_stopped("LanguageStop joined its entire Job");
            assert!(task_tree.all_alive(), "LanguageStop killed the task Job");
            assert_eq!(
                task(agent.ok(Operation::RunPoll { task_id: id })).state,
                TaskState::Running
            );
            agent.ok(Operation::LanguageStop); // idempotent stop
            agent.ok(Operation::RunCancel { task_id: id });
        } else {
            agent.ok(Operation::RunCancel { task_id: id });
            assert_eq!(
                agent.terminal(id, "cancel while LSP lives").state,
                TaskState::Cancelled
            );
            task_tree.assert_already_stopped("cancelled task before checking live LSP");
            language_tree.assert_alive();
            hover(&mut agent); // successful protocol request, not merely a live PID
            agent.ok(Operation::LanguageStop);
            language_tree.assert_stopped("LSP stop after task cancellation");
        }
        let terminal = agent.terminal(id, "independent owner cancellation");
        assert_eq!(terminal.state, TaskState::Cancelled);
        task_tree.assert_already_stopped("terminal task snapshot");
        agent.stable(&terminal);
    }
    agent.close_cleanly();
}

#[test]
#[ignore = "requires native Windows and exact prebuilt fixture paths; run serially"]
fn peer_eof_and_forced_agent_death_clean_both_live_jobs() {
    let _watchdog = Watchdog::start();
    for failure in ["eof", "forced-agent-death"] {
        let root = synthetic_root();
        let mut agent = validation_agent(root.path(), true);
        let (_, task_tree, language_tree) = start_both(&mut agent, root.path(), 0, true);
        agent.inject_failure(failure);
        let status = agent.wait_exit(failure);
        assert_eq!(
            status.success(),
            failure == "eof",
            "{}",
            agent.diagnostics()
        );
        // RawAgent has no test-owned surrounding Job. Exact observation handles
        // and lifetime files, opened while live, cannot clean anything up.
        task_tree.assert_stopped(failure);
        language_tree.assert_stopped(failure);
    }
}
