//! Explicit normal-agent runtime acceptance for Linux's generic LSP route.
//! Prebuild the normal release agent and test-only mock; provide absolute
//! CEDAR_AGENT_BIN and CEDAR_MOCK_LSP_BIN paths. No listeners, SSH, Java/Maven
//! admission, user source mutation, or abrupt-agent-death claim is involved.
#![cfg(target_os = "linux")]

use cedar_client::Client;
use cedar_protocol::{
    LanguageQueryKind, Operation, Payload, JAVA_MAVEN_CAPABILITIES, JAVA_STARTUP_CAPABILITIES,
};
use std::fs::{self, File, OpenOptions};
use std::ops::{Deref, DerefMut};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const SOURCE: &[u8] = b"class Main { /* original source stays unchanged */ }\n";

struct Watchdog {
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Watchdog {
    fn start() -> Self {
        let (stop, stopped) = mpsc::channel();
        let worker = thread::spawn(move || {
            if stopped.recv_timeout(Duration::from_secs(30)).is_err() {
                eprintln!("Linux normal-agent LSP acceptance exceeded its watchdog");
                std::process::exit(126);
            }
        });
        Self {
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn binary(name: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("set {name}")));
    assert!(
        path.is_absolute() && path.is_file(),
        "{name} must name an existing absolute binary"
    );
    path
}

fn workspace() -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix("cedar linux agent language 雪 ")
        .tempdir()
        .unwrap();
    fs::write(dir.path().join("Main.java"), SOURCE).unwrap();
    dir
}

/// Always close the exact owned agent connection, including assertion-unwind
/// paths. No stored PID is used to signal an agent or its language server.
struct Agent(Option<Client>);

impl Agent {
    fn start(root: &Path, trusted: bool) -> Self {
        Self(Some(
            Client::spawn_agent(&binary("CEDAR_AGENT_BIN"), root, trusted).unwrap(),
        ))
    }

    fn close(mut self) {
        self.0
            .take()
            .unwrap()
            .close_and_wait(Duration::from_secs(4))
            .unwrap();
    }
}

impl Deref for Agent {
    type Target = Client;
    fn deref(&self) -> &Client {
        self.0.as_ref().unwrap()
    }
}

impl DerefMut for Agent {
    fn deref_mut(&mut self) -> &mut Client {
        self.0.as_mut().unwrap()
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        if let Some(client) = self.0.take() {
            let result = client.close_and_wait(Duration::from_secs(4));
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }
}

struct OwnedFixture {
    dir: PathBuf,
    // These observation-only descriptors identify the exact fixture resources.
    // A descendant's released lock does not claim this test reaped it.
    lifetimes: Vec<File>,
    root_pid: u32,
}

impl OwnedFixture {
    fn observe(dir: &Path, tree: bool) -> Self {
        let names = if tree {
            &['r', 'c', 'g'][..]
        } else {
            &['r'][..]
        };
        let lifetimes = names
            .iter()
            .map(|name| {
                let name = match name {
                    'r' => "root",
                    'c' => "child",
                    _ => "grandchild",
                };
                OpenOptions::new()
                    .write(true)
                    .open(dir.join(format!("{name}.lock")))
                    .unwrap()
            })
            .collect();
        let root_pid = fs::read_to_string(dir.join("root.pid"))
            .unwrap()
            .parse()
            .unwrap();
        let fixture = Self {
            dir: dir.to_path_buf(),
            lifetimes,
            root_pid,
        };
        fixture.assert_alive();
        fixture
    }

    fn released(file: &File) -> bool {
        // SAFETY: Each probe uses a retained live file descriptor, never a PID.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            assert_eq!(unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) }, 0);
            true
        } else {
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::EWOULDBLOCK)
            );
            false
        }
    }

    fn assert_alive(&self) {
        assert!(
            self.lifetimes.iter().all(|file| !Self::released(file)),
            "independent fixture stopped unexpectedly"
        );
    }

    fn assert_stopped(&self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.lifetimes.iter().any(|file| !Self::released(file))
            || Path::new(&format!("/proc/{}", self.root_pid)).exists()
        {
            // This is read-only root-reap evidence. PID reuse can conservatively
            // fail this check, but can never authorize a signal or false cleanup.
            assert!(
                Instant::now() < deadline,
                "agent did not release fixture resources and reap its root"
            );
            thread::sleep(Duration::from_millis(5));
        }
        for name in ["root", "child", "grandchild"] {
            assert!(
                !self.dir.join(format!("{name}.expired")).exists(),
                "fixture watchdog ended {name}"
            );
        }
    }
}

fn start_language(agent: &mut Agent, dir: &Path, mode: &str) -> OwnedFixture {
    fs::create_dir(dir).unwrap();
    language(
        agent
            .request(Operation::LanguageStart {
                program: binary("CEDAR_MOCK_LSP_BIN").to_str().unwrap().into(),
                args: vec![mode.into(), dir.to_str().unwrap().into()],
            })
            .unwrap(),
    );
    OwnedFixture::observe(dir, mode == "linux-agent-tree")
}

fn language(payload: Payload) -> serde_json::Value {
    let Payload::Language { value } = payload else {
        panic!("expected language response: {payload:?}");
    };
    value
}

fn open(agent: &mut Agent) {
    language(
        agent
            .request(Operation::LanguageOpen {
                path: "Main.java".into(),
                language_id: "java".into(),
                version: 1,
                text: String::from_utf8(SOURCE.to_vec()).unwrap(),
            })
            .unwrap(),
    );
}

fn hover(agent: &mut Agent) {
    let value = language(
        agent
            .request(Operation::LanguageQuery {
                path: "Main.java".into(),
                line: 0,
                character: 0,
                kind: LanguageQueryKind::Hover,
            })
            .unwrap(),
    );
    assert_eq!(value["contents"]["value"], "mock hover");
}

fn read_source(agent: &mut Agent) {
    let start = Instant::now();
    assert!(
        matches!(agent.request(Operation::Read { path: "Main.java".into() }).unwrap(),
        Payload::File { text, .. } if text.as_bytes() == SOURCE)
    );
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "agent file request remained blocked after language cleanup"
    );
}

fn typed_java_capabilities_keep_maven_gated(agent: &Agent) {
    let Payload::Hello {
        agent: Some(info), ..
    } = agent.handshake()
    else {
        panic!("missing agent metadata");
    };
    info.validate().unwrap();
    assert_eq!(info.os, "linux");
    assert!(info.supports("language_start"));
    for capability in [
        "language_start_java",
        "java_diagnostics_refresh",
        "language_organize_java_imports",
        "language_java_implementations",
    ] {
        assert!(info.supports(capability));
    }
    assert!(!info.supports("language_maven_dependencies"));
    for capability in JAVA_STARTUP_CAPABILITIES {
        assert!(info.supports(capability));
    }
    for capability in JAVA_MAVEN_CAPABILITIES {
        assert!(
            !info.supports(capability),
            "typed startup unexpectedly admitted: {capability}"
        );
    }
}

#[test]
#[ignore = "requires explicitly supplied normal release cedar-agent and test-only mock LSP"]
fn normal_agent_stop_restart_and_orderly_close_preserve_independent_language_owner() {
    let _watchdog = Watchdog::start();
    let first_root = workspace();
    let second_root = workspace();
    let mut first = Agent::start(first_root.path(), true);
    let mut second = Agent::start(second_root.path(), true);
    typed_java_capabilities_keep_maven_gated(&first);
    typed_java_capabilities_keep_maven_gated(&second);
    let first_tree = start_language(
        &mut first,
        &first_root.path().join("tree"),
        "linux-agent-tree",
    );
    let independent = start_language(
        &mut second,
        &second_root.path().join("independent"),
        "linux-lsp-exit-eof",
    );
    open(&mut first);
    open(&mut second);
    hover(&mut first);
    hover(&mut second);
    let stopped = language(first.request(Operation::LanguageStop).unwrap());
    assert_eq!(stopped, serde_json::json!({"stopped":true}));
    first_tree.assert_stopped();
    assert_eq!(
        language(first.request(Operation::LanguageStop).unwrap()),
        stopped
    );
    independent.assert_alive();
    hover(&mut second);
    read_source(&mut first);
    let restarted = start_language(
        &mut first,
        &first_root.path().join("restarted"),
        "linux-lsp-exit-eof",
    );
    open(&mut first);
    hover(&mut first);
    first.close();
    restarted.assert_stopped();
    independent.assert_alive();
    hover(&mut second);
    second.close();
    independent.assert_stopped();
    assert_eq!(
        fs::read(first_root.path().join("Main.java")).unwrap(),
        SOURCE
    );
    assert_eq!(
        fs::read(second_root.path().join("Main.java")).unwrap(),
        SOURCE
    );
    println!(
        "\n{}",
        serde_json::json!({
            "kind":"cedar_linux_agent_language_acceptance", "schema_version":1,
            "case":"stop_restart_orderly_close", "status":"success",
            "normal_agents_started":2, "agent_close_and_reap_observed":true,
            "fixture_locks_released":true, "lsp_roots_absent":true,
            "legacy_stop_acknowledged":true, "transport_worker_join_claimed":false,
            "descendant_reaping_claimed":false, "independent_owner_preserved":true,
            "replacement_session_worked":true, "typed_java_advertised_maven_unadvertised":true,
            "agent_read_responsive":true, "source_bytes_unchanged":true
        })
    );
}

#[test]
#[ignore = "requires explicitly supplied normal release cedar-agent and test-only mock LSP"]
fn blocked_language_write_cleans_up_and_normal_agent_remains_responsive() {
    let _watchdog = Watchdog::start();
    let root = workspace();
    let mut agent = Agent::start(root.path(), true);
    let fixture_dir = root.path().join("blocked");
    let fixture = start_language(&mut agent, &fixture_dir, "linux-agent-blocked");
    open(&mut agent);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !fixture_dir.join("stdin-blocked.ready").is_file() {
        assert!(
            Instant::now() < deadline,
            "mock did not enter blocked-input state"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let start = Instant::now();
    let error = agent
        .request(Operation::LanguageChange {
            path: "Main.java".into(),
            version: 2,
            text: "x".repeat(900_000),
        })
        .unwrap_err();
    assert!(error.starts_with("language_error:"), "{error}");
    // Use the normal ten-second writer deadline and fixed three-second cleanup
    // observation. There is no test-only production deadline override here.
    assert!(start.elapsed() >= Duration::from_secs(9));
    assert!(start.elapsed() < Duration::from_secs(14));
    fixture.assert_stopped();
    read_source(&mut agent);
    let stop_error = agent.request(Operation::LanguageStop).unwrap_err();
    assert!(stop_error.starts_with("language_error:"), "{stop_error}");
    assert_eq!(
        language(agent.request(Operation::LanguageStop).unwrap()),
        serde_json::json!({"stopped":true})
    );
    read_source(&mut agent);
    let replacement = start_language(
        &mut agent,
        &root.path().join("replacement"),
        "linux-lsp-exit-eof",
    );
    open(&mut agent);
    hover(&mut agent);
    agent.close();
    replacement.assert_stopped();
    assert_eq!(fs::read(root.path().join("Main.java")).unwrap(), SOURCE);
    println!(
        "\n{}",
        serde_json::json!({
            "kind":"cedar_linux_agent_language_acceptance", "schema_version":1,
            "case":"blocked_write_responsiveness", "status":"success",
            "normal_agents_started":1, "agent_close_and_reap_observed":true,
            "fixture_locks_released":true, "lsp_roots_absent":true,
            "legacy_stop_acknowledged":true, "transport_worker_join_claimed":false,
            "blocked_write_deadline_observed":true, "agent_read_responsive":true,
            "replacement_session_worked":true, "source_bytes_unchanged":true
        })
    );
}

#[test]
#[ignore = "requires explicitly supplied normal release cedar-agent and test-only mock LSP"]
fn untrusted_normal_agent_rejects_generic_language_start_and_keeps_file_access() {
    let _watchdog = Watchdog::start();
    let root = workspace();
    let mut agent = Agent::start(root.path(), false);
    typed_java_capabilities_keep_maven_gated(&agent);
    let fixture = root.path().join("must-not-start");
    fs::create_dir(&fixture).unwrap();
    let error = agent
        .request(Operation::LanguageStart {
            program: binary("CEDAR_MOCK_LSP_BIN").to_str().unwrap().into(),
            args: vec![
                "linux-lsp-exit-eof".into(),
                fixture.to_str().unwrap().into(),
            ],
        })
        .unwrap_err();
    assert!(error.starts_with("run_disabled:"), "{error}");
    assert_eq!(fs::read_dir(fixture).unwrap().count(), 0);
    read_source(&mut agent);
    agent.close();
    assert_eq!(fs::read(root.path().join("Main.java")).unwrap(), SOURCE);
    println!(
        "\n{}",
        serde_json::json!({
            "kind":"cedar_linux_agent_language_acceptance", "schema_version":1,
            "case":"untrusted_start_rejected", "status":"success",
            "normal_agents_started":1, "agent_close_and_reap_observed":true,
            "fixture_process_started":false, "typed_java_advertised_maven_unadvertised":true,
            "agent_read_responsive":true, "source_bytes_unchanged":true,
            "transport_worker_join_claimed":false
        })
    );
}

#[test]
#[ignore = "requires an explicitly supplied test-only mock LSP"]
fn workspace_trust_revocation_still_allows_cleanup_of_its_owned_session() {
    use cedar_workspace::{BackendMode, Workspace};

    let _watchdog = Watchdog::start();
    for mode in [BackendMode::InProcess, BackendMode::IsolatedAgent] {
        let root = workspace();
        let dir = root.path().join("owned");
        fs::create_dir(&dir).unwrap();
        let mut workspace = Workspace::with_backend_mode(root.path(), mode).unwrap();
        workspace.set_allow_run(true);
        language(
            workspace
                .handle(Operation::LanguageStart {
                    program: binary("CEDAR_MOCK_LSP_BIN").to_str().unwrap().into(),
                    args: vec!["linux-lsp-exit-eof".into(), dir.to_str().unwrap().into()],
                })
                .unwrap(),
        );
        let owned = OwnedFixture::observe(&dir, false);
        workspace.set_allow_run(false);
        owned.assert_alive();
        assert_eq!(
            workspace
                .handle(Operation::LanguageQuery {
                    path: "Main.java".into(),
                    line: 0,
                    character: 0,
                    kind: LanguageQueryKind::Hover,
                })
                .unwrap_err()
                .code,
            "run_disabled"
        );
        assert_eq!(
            language(workspace.handle(Operation::LanguageStop).unwrap()),
            serde_json::json!({"stopped":true})
        );
        owned.assert_stopped();
        // Revocation still blocks a fresh process. This direct Workspace check
        // exercises the existing setter without inventing an agent wire toggle.
        let forbidden = root.path().join("forbidden");
        fs::create_dir(&forbidden).unwrap();
        assert_eq!(
            workspace
                .handle(Operation::LanguageStart {
                    program: binary("CEDAR_MOCK_LSP_BIN").to_str().unwrap().into(),
                    args: vec![
                        "linux-lsp-exit-eof".into(),
                        forbidden.to_str().unwrap().into()
                    ],
                })
                .unwrap_err()
                .code,
            "run_disabled"
        );
        assert_eq!(fs::read_dir(forbidden).unwrap().count(), 0);
        assert!(
            matches!(workspace.handle(Operation::Read { path: "Main.java".into() }).unwrap(),
            Payload::File { text, .. } if text.as_bytes() == SOURCE)
        );
        drop(workspace);
        assert_eq!(fs::read(root.path().join("Main.java")).unwrap(), SOURCE);
    }
}
