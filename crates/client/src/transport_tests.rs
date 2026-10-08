//! Real child pipes exercise transport framing, deadlines and lifecycle. No SSH
//! credentials/server, Python, shell, or platform-specific executable required.
use super::*;
use std::{fs, sync::OnceLock};
use tempfile::TempDir;

struct PeerBinary {
    _directory: TempDir,
    path: PathBuf,
}
fn peer_binary() -> &'static Path {
    static PEER: OnceLock<PeerBinary> = OnceLock::new();
    &PEER
        .get_or_init(|| {
            let directory = tempfile::tempdir().unwrap();
            let path = directory
                .path()
                .join(format!("transport-peer{}", std::env::consts::EXE_SUFFIX));
            let output = Command::new("rustc")
                .args(["--edition=2021", "--crate-name", "transport_peer"])
                .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transport_peer.rs"))
                .arg("-o")
                .arg(&path)
                .output()
                .expect("Rust compiler required for synthetic real-child tests");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            PeerBinary {
                _directory: directory,
                path,
            }
        })
        .path
}
fn spawn(mode: &str, directory: &Path) -> ProcessClient {
    let mut command = Command::new(peer_binary());
    command.arg(mode).arg(directory);
    ProcessClient::spawn_with_grace(command, Duration::from_millis(200)).unwrap()
}
fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "bounded condition did not complete"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn connected(mode: &str, dir: &Path) -> ProcessClient {
    let mut peer = spawn(mode, dir);
    assert!(matches!(
        peer.request(Operation::Hello).unwrap(),
        Payload::Hello { protocol: 4, .. }
    ));
    peer
}

fn cancellable_peer(
    mode: &str,
    directory: &Path,
    cancellation: ConnectionCancellation,
) -> ProcessClient {
    let mut command = Command::new(peer_binary());
    command.arg(mode).arg(directory);
    ProcessClient::spawn_with_grace_and_cancellation(
        command,
        Duration::from_millis(200),
        Some(cancellation),
    )
    .unwrap()
}

#[test]
fn cancellation_before_spawn_never_creates_a_child() {
    let directory = tempfile::tempdir().unwrap();
    let token = ConnectionCancellation::new();
    token.cancel();
    let mut command = Command::new(peer_binary());
    command.arg("stalled_hello").arg(directory.path());
    let result = ProcessClient::spawn(command, Some(token));
    let Err(error) = result else {
        panic!("cancelled startup spawned a child");
    };
    assert!(error.starts_with("transport_cancelled:"), "{error}");
    assert!(!directory.path().join("started").exists());
    assert!(!directory.path().join("requests").exists());
}

#[test]
fn cancelled_hello_returns_promptly_and_its_exact_owned_child_is_reaped() {
    let directory = tempfile::tempdir().unwrap();
    let token = ConnectionCancellation::new();
    let mut peer = cancellable_peer("stalled_hello", directory.path(), token.clone());
    // Retain the actual owner's completion even though failed construction
    // consumes/drops ProcessClient. PID disappearance or peer EOF is not proof.
    let reaped = std::mem::replace(&mut peer.reaped, mpsc::channel().1);
    let (finished, result) = mpsc::channel();
    let caller = thread::spawn(move || {
        let result = Client::from_process(peer);
        let _ = finished.send(result.err().expect("stalled Hello must cancel"));
    });
    wait_until(|| directory.path().join("ready-1").exists());
    assert_eq!(recorded_requests(directory.path()).len(), 1);
    let start = Instant::now();
    token.cancel();
    let error = result.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(error.starts_with("transport_cancelled:"), "{error}");
    assert!(start.elapsed() < Duration::from_secs(2));
    caller.join().unwrap();
    reaped
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert!(directory.path().join("eof").is_file());
    assert_eq!(recorded_requests(directory.path()).len(), 1);
}

#[test]
fn cancellable_poll_slices_preserve_the_original_request_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let token = ConnectionCancellation::new();
    let mut peer = cancellable_peer("stalled_read", directory.path(), token);
    peer.request(Operation::Hello).unwrap();
    let (finished, result) = mpsc::channel();
    let caller = thread::spawn(move || {
        let start = Instant::now();
        let outcome = peer.request_with_timeout(
            Operation::Read {
                path: "fixture.txt".into(),
            },
            Duration::from_millis(250),
        );
        finished.send((peer, outcome, start.elapsed())).unwrap();
    });
    wait_until(|| directory.path().join("ready-2").exists());
    let (mut peer, outcome, elapsed) = result.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(outcome.unwrap_err().starts_with("transport_timeout:"));
    assert!(
        elapsed >= Duration::from_millis(250),
        "a polling slice became the deadline: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "polling reset the deadline: {elapsed:?}"
    );
    caller.join().unwrap();
    peer.close_and_wait(Duration::from_secs(5)).unwrap();
    assert_eq!(recorded_requests(directory.path()).len(), 2);
}

#[test]
fn cancellation_during_a_mutation_keeps_its_original_timeout() {
    let directory = tempfile::tempdir().unwrap();
    let token = ConnectionCancellation::new();
    let mut peer = cancellable_peer("write_unknown", directory.path(), token.clone());
    peer.request(Operation::Hello).unwrap();
    let (finished, result) = mpsc::channel();
    let caller = thread::spawn(move || {
        let start = Instant::now();
        let outcome = peer.request_with_timeout(
            Operation::Write {
                path: "fixture.txt".into(),
                text: "saved".into(),
                expected_revision: None,
            },
            Duration::from_millis(250),
        );
        finished.send((peer, outcome, start.elapsed())).unwrap();
    });
    wait_until(|| directory.path().join("committed").exists());
    token.cancel();
    let (mut peer, outcome, elapsed) = result.recv_timeout(Duration::from_secs(2)).unwrap();
    let error = outcome.unwrap_err();
    assert!(error.starts_with("transport_timeout:"), "{error}");
    assert!(error.contains("outcome may be unknown"));
    assert!(
        elapsed >= Duration::from_millis(250),
        "mutation cancelled early: {elapsed:?}"
    );
    caller.join().unwrap();
    peer.close_and_wait(Duration::from_secs(5)).unwrap();
    assert_eq!(recorded_requests(directory.path()).len(), 2);
}

#[cfg(not(windows))]
#[test]
fn embedded_workspace_cancellation_prevents_the_next_synchronous_operation() {
    let directory = tempfile::tempdir().unwrap();
    let token = ConnectionCancellation::new();
    let mut client = Client::connect_with_cancellation(
        ConnectionSpec::Local {
            root: directory.path().into(),
            allow_run: false,
        },
        token.clone(),
    )
    .unwrap();
    token.cancel();
    let error = client
        .request(Operation::Write {
            path: "never-written.txt".into(),
            text: "cancelled".into(),
            expected_revision: None,
        })
        .unwrap_err();
    assert!(error.starts_with("transport_cancelled:"), "{error}");
    assert!(!directory.path().join("never-written.txt").exists());
    assert!(client
        .request(Operation::Hello)
        .unwrap_err()
        .starts_with("transport_cancelled:"));
    client.close_and_wait(Duration::from_secs(1)).unwrap();
}
#[test]
fn handshake_rejects_old_new_malformed_and_wrong_payload_peers() {
    for (mode, expected) in [
        ("old_hello", "protocol_mismatch: agent uses 3"),
        ("new_hello", "protocol_mismatch: agent uses 5"),
        ("malformed_hello", "transport_read:"),
        ("missing_hello_field", "transport_read:"),
        ("wrong_hello_payload", "protocol_error: expected hello"),
        ("wrong_id", "protocol_error: response id 2 expected 1"),
        ("bad_json", "transport_read:"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let result = Client::from_process(spawn(mode, dir.path()));
        let error = match result {
            Ok(_) => panic!("accepted {mode}"),
            Err(error) => error,
        };
        assert!(error.starts_with(expected), "{mode}: {error}");
        wait_until(|| dir.path().join("eof").exists());
    }
}
#[test]
fn rejects_truncated_and_oversized_real_pipe_frames() {
    for (mode, expected) in [
        ("truncated", "incomplete frame"),
        ("oversized", "frame too large"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut peer = spawn(mode, dir.path());
        let error = peer.request(Operation::Hello).unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert!(!peer.connected);
        peer.reaped
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
    }
}
#[test]
fn eof_before_hello_and_after_request_disconnect_without_replay() {
    for mode in [
        "eof_before_hello",
        "eof_between_requests",
        "eof_after_request",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut peer = if mode == "eof_before_hello" {
            spawn(mode, dir.path())
        } else {
            connected(mode, dir.path())
        };
        let error = peer.request(Operation::Hello).unwrap_err();
        assert!(
            error.starts_with("transport_eof:") || error.starts_with("transport_write:"),
            "{error}"
        );
        assert!(error.contains("outcome may be unknown"));
        assert!(!peer.connected);
        assert!(peer
            .request(Operation::Hello)
            .unwrap_err()
            .starts_with("disconnected:"));
        peer.reaped
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        if mode != "eof_before_hello" {
            assert_eq!(
                fs::read_to_string(dir.path().join("requests"))
                    .unwrap()
                    .lines()
                    .count(),
                if mode == "eof_after_request" { 2 } else { 1 }
            );
        }
    }
}
#[test]
fn wrong_response_id_poisoning_requires_a_new_connection() {
    let dir = tempfile::tempdir().unwrap();
    let mut peer = connected("wrong_later_id", dir.path());
    assert!(peer
        .request(Operation::Hello)
        .unwrap_err()
        .contains("response id 1 expected 2"));
    assert!(peer
        .request(Operation::Hello)
        .unwrap_err()
        .starts_with("disconnected:"));
    peer.reaped
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("requests"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}
#[test]
fn stderr_flood_is_drained_and_only_a_bounded_tail_is_retained() {
    let dir = tempfile::tempdir().unwrap();
    let mut peer = connected("stderr_flood", dir.path());
    wait_until(|| {
        peer.stderr
            .lock()
            .unwrap()
            .ends_with(b"diagnostic-tail-marker\n")
    });
    assert_eq!(peer.stderr.lock().unwrap().len(), STDERR_TAIL_BYTES);
    let error = peer.request(Operation::Hello).unwrap_err();
    assert!(error.contains("diagnostic-tail-marker"));
    assert!(!error.contains("discarded-prefix"));
    assert!(error.len() < STDERR_TAIL_BYTES + 512);
    peer.reaped
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
}
#[test]
fn stalled_response_has_a_short_internal_deadline_and_orderly_close() {
    let dir = tempfile::tempdir().unwrap();
    let mut peer = connected("stalled", dir.path());
    let start = Instant::now();
    let error = peer
        .request_with_timeout(Operation::Hello, Duration::from_millis(100))
        .unwrap_err();
    assert!(error.starts_with("transport_timeout:"));
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(!peer.connected);
    peer.reaped
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert!(dir.path().join("eof").exists());
}
#[test]
fn unknown_write_outcome_is_warned_and_never_automatically_replayed() {
    let dir = tempfile::tempdir().unwrap();
    let mut peer = connected("write_unknown", dir.path());
    let write = Operation::Write {
        path: "a.txt".into(),
        text: "changed".into(),
        expected_revision: None,
    };
    let error = peer
        .request_with_timeout(write.clone(), Duration::from_millis(150))
        .unwrap_err();
    assert!(error.contains("outcome may be unknown, reload before retrying a write"));
    assert!(dir.path().join("committed").exists());
    assert!(peer
        .request(write)
        .unwrap_err()
        .starts_with("disconnected:"));
    peer.reaped
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    let before_reconnect = fs::read_to_string(dir.path().join("requests")).unwrap();
    assert_eq!(before_reconnect.lines().count(), 2);
    assert_eq!(before_reconnect.matches("\"type\":\"write\"").count(), 1);
    let mut fresh = connected("normal", dir.path());
    fresh.request(Operation::Hello).unwrap();
    fresh.close();
    fresh
        .reaped
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    let requests = fs::read_to_string(dir.path().join("requests")).unwrap();
    assert_eq!(requests.matches("\"type\":\"write\"").count(), 1);
    let ids: Vec<_> = requests
        .lines()
        .map(|line| line.split(',').next().unwrap())
        .collect();
    assert_eq!(ids, ["{\"id\":1", "{\"id\":2", "{\"id\":1", "{\"id\":2"]);
}
#[test]
fn blocked_writer_is_bounded_and_force_closed_off_the_calling_thread() {
    let dir = tempfile::tempdir().unwrap();
    let mut peer = connected("blocked_writer", dir.path());
    let start = Instant::now();
    let error = peer
        .request_with_timeout(
            Operation::Write {
                path: "large.txt".into(),
                text: "x".repeat(1024 * 1024),
                expected_revision: None,
            },
            Duration::from_millis(100),
        )
        .unwrap_err();
    assert!(error.starts_with("transport_timeout:"), "{error}");
    assert!(start.elapsed() < Duration::from_secs(2));
    peer.reaped
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert!(!dir.path().join("eof").exists());
}
#[test]
fn drop_is_nonblocking_and_graceful_peer_observes_eof() {
    let dir = tempfile::tempdir().unwrap();
    let peer = connected("normal", dir.path());
    let start = Instant::now();
    drop(peer);
    assert!(start.elapsed() < Duration::from_millis(100));
    wait_until(|| dir.path().join("eof").exists());
}
#[test]
fn flood_cannot_hold_the_response_reader_on_a_full_queue_after_close() {
    let dir = tempfile::tempdir().unwrap();
    let mut peer = spawn("response_flood", dir.path());
    wait_until(|| dir.path().join("started").exists());
    // A malicious peer can fill the single response slot before any request.
    // Closing must drop the receiver and release the blocked reader/writer.
    peer.close();
    peer.reaped
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
}

#[test]
fn invalid_outgoing_frame_disconnects_without_putting_a_write_on_the_wire() {
    let dir = tempfile::tempdir().unwrap();
    let mut peer = connected("normal", dir.path());
    let error = peer
        .request_with_timeout(
            Operation::Write {
                path: "too-large.txt".into(),
                text: "x".repeat(cedar_protocol::MAX_FRAME_BYTES),
                expected_revision: None,
            },
            Duration::from_secs(3),
        )
        .unwrap_err();
    assert!(error.starts_with("transport_write:"), "{error}");
    assert!(error.contains("frame too large"));
    assert!(!peer.connected);
    peer.reaped
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("requests"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn dropping_an_unresponsive_peer_does_not_wait_for_the_grace_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let mut peer = connected("blocked_writer", dir.path());
    let reaped = std::mem::replace(&mut peer.reaped, mpsc::channel().1);
    let start = Instant::now();
    drop(peer);
    assert!(start.elapsed() < Duration::from_millis(100));
    reaped
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert!(!dir.path().join("eof").exists());
}

#[test]
fn explicit_close_waits_for_owned_child_cleanup_without_sending_another_request() {
    let directory = tempfile::tempdir().unwrap();
    let client = capability_client(directory.path(), Some(agent_info(&java_capabilities())));
    client.close_and_wait(Duration::from_secs(5)).unwrap();
    assert!(directory.path().join("eof").exists());
    assert_eq!(recorded_requests(directory.path()).len(), 1);
}

#[test]
fn explicit_close_timeout_is_bounded_while_the_reaper_keeps_ownership() {
    let directory = tempfile::tempdir().unwrap();
    let mut peer = connected("blocked_writer", directory.path());
    let start = Instant::now();
    let error = peer.close_and_wait(Duration::from_millis(20)).unwrap_err();
    assert!(error.starts_with("transport_close:"));
    assert!(start.elapsed() < Duration::from_millis(150));
    assert!(!peer.connected);
    peer.reaped
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert!(!directory.path().join("eof").exists());
}

#[test]
fn explicit_close_never_reports_success_for_unverified_reaper_completion() {
    for failure in [ReapError::TryWait, ReapError::Kill, ReapError::Wait] {
        let directory = tempfile::tempdir().unwrap();
        let mut client = capability_client(directory.path(), Some(agent_info(&["list", "read"])));
        let (completion, results) = mpsc::channel();
        let actual_reaper = std::mem::replace(&mut process(&mut client).reaped, results);
        completion.send(Err(failure)).unwrap();
        let error = client.close_and_wait(Duration::from_secs(5)).unwrap_err();
        assert!(
            error.starts_with("transport_cleanup_unverified:"),
            "{error}"
        );
        assert!(error.contains(failure.operation()), "{error}");
        // The injected result only tests the public boundary. The real owner
        // still closes and must independently confirm its child was reaped.
        actual_reaper
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn lost_wait_ownership_is_unverified_and_never_retried_by_drop() {
    let directory = tempfile::tempdir().unwrap();
    let child = Command::new(peer_binary())
        .arg("eof_before_hello")
        .arg(directory.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut owned = OwnedProcess {
        child,
        completion: None,
    };
    let pid = owned.child.id() as libc::pid_t;
    wait_until(|| {
        let mut status = 0;
        // Fault injection: reap only this test's exact direct child outside
        // its owner, forcing the next try_wait to observe lost wait ownership.
        // No PID lookup or signal is performed here.
        let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        assert!(
            result >= 0,
            "fixture wait failed: {}",
            io::Error::last_os_error()
        );
        result == pid
    });
    assert_eq!(owned.poll_exit(), Some(Err(ReapError::TryWait)));
    assert_eq!(owned.completion, Some(Err(ReapError::TryWait)));
    // The terminal error is sticky: the teardown path must not retry a kill or
    // wait on this PID after an external reaper may have made it reusable.
    assert_eq!(owned.terminate_and_reap(), Err(ReapError::TryWait));
    drop(owned);
}

fn agent_info(capabilities: &[&str]) -> cedar_protocol::AgentInfo {
    cedar_protocol::AgentInfo {
        schema: cedar_protocol::AGENT_INFO_SCHEMA,
        version: "fixture-agent-6".into(),
        os: "fixture_os".into(),
        arch: "fixture_arch".into(),
        capabilities: capabilities.iter().map(|name| (*name).into()).collect(),
    }
}
fn capability_peer(directory: &Path, hello: serde_json::Value) -> ProcessClient {
    fs::write(directory.join("hello.json"), hello.to_string()).unwrap();
    spawn("capability_peer", directory)
}
fn capability_client(directory: &Path, agent: Option<cedar_protocol::AgentInfo>) -> Client {
    let hello = serde_json::json!({
        "type": "hello", "protocol": PROTOCOL_VERSION, "root": "/first/工作区", "agent": agent,
    });
    Client::from_process(capability_peer(directory, hello)).unwrap()
}
fn recorded_requests(directory: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(directory.join("requests"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn read_fixture(client: &mut Client) {
    assert!(matches!(
        client.request(Operation::Read { path: "fixture.txt".into() }).unwrap(),
        Payload::File { text, .. } if text == "fixture text"
    ));
}
fn start_task() -> Operation {
    Operation::RunStart {
        program: "never-executed".into(),
        args: vec![],
        timeout_secs: 1,
    }
}
fn start_language() -> Operation {
    Operation::LanguageStart {
        program: "never-executed".into(),
        args: vec![],
    }
}
fn start_java_language() -> Operation {
    Operation::LanguageStartJava {
        java_executable: "C:\\jdk\\bin\\java.exe".into(),
        distribution: "C:\\JDT distribution 雪".into(),
        data_directory: "C:\\workspace data 雪".into(),
    }
}
fn advanced_operations() -> Vec<Operation> {
    vec![
        Operation::GitStatus,
        Operation::GitChanges {
            git_executable: "/never-executed/git".into(),
        },
        Operation::GitDiff {
            git_executable: "/never-executed/git".into(),
            path: "fixture.txt".into(),
            kind: cedar_protocol::GitDiffKind::Unstaged,
        },
        start_task(),
        Operation::RunPoll { task_id: 1 },
        Operation::RunCancel { task_id: 1 },
        Operation::Run {
            program: "never-executed".into(),
            args: vec![],
            timeout_secs: 1,
        },
        start_language(),
        start_java_language(),
        begin_java_language(),
        Operation::LanguageStartJavaPoll { startup_id: 1 },
        Operation::LanguageStartJavaCancel { startup_id: 1 },
        Operation::LanguageOpen {
            path: "fixture.txt".into(),
            language_id: "text".into(),
            version: 1,
            text: "fixture text".into(),
        },
        Operation::LanguageChange {
            path: "fixture.txt".into(),
            version: 2,
            text: "changed text".into(),
        },
        Operation::LanguageClose {
            path: "fixture.txt".into(),
        },
        Operation::LanguageQuery {
            path: "fixture.txt".into(),
            line: 0,
            character: 0,
            kind: cedar_protocol::LanguageQueryKind::Hover,
        },
        Operation::LanguageFormat {
            path: "fixture.txt".into(),
            version: 1,
            tab_size: 4,
            insert_spaces: true,
        },
        Operation::LanguageRefreshJavaDiagnostics {
            path: "fixture.txt".into(),
            version: 1,
        },
        Operation::LanguageReferences {
            path: "fixture.txt".into(),
            line: 0,
            character: 0,
            include_declaration: true,
        },
        Operation::LanguageDocumentSymbols {
            path: "fixture.txt".into(),
        },
        Operation::LanguageResolveUri {
            uri: "file:///fixture.txt".into(),
        },
        Operation::LanguageResolveCompletion {
            item: serde_json::json!({}),
        },
        Operation::LanguageEvents,
        Operation::LanguageStop,
    ]
}
#[test]
fn public_client_sends_one_hello_and_keeps_the_complete_first_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = capability_client(
        directory.path(),
        Some(agent_info(&["list", "read", "unknown_future_feature"])),
    );
    let snapshot = serde_json::to_value(client.handshake()).unwrap();
    for _ in 0..3 {
        let hello = client.request(Operation::Hello).unwrap();
        assert_eq!(serde_json::to_value(hello).unwrap(), snapshot);
    }
    assert_eq!(snapshot["root"], "/first/工作区");
    assert_eq!(snapshot["agent"]["version"], "fixture-agent-6");
    assert_eq!(snapshot["agent"]["os"], "fixture_os");
    assert_eq!(snapshot["agent"]["arch"], "fixture_arch");
    assert_eq!(recorded_requests(directory.path()).len(), 1);
    read_fixture(&mut client);
    assert_eq!(serde_json::to_value(client.handshake()).unwrap(), snapshot);
    let requests = recorded_requests(directory.path());
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["op"]["type"], "hello");
    assert_eq!(requests[1]["op"]["type"], "read");
    assert_eq!(requests[1]["id"], 2);
}
#[test]
fn legacy_peers_keep_file_operations_but_never_receive_execution_requests() {
    for explicit_null in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut hello = serde_json::json!({"type": "hello", "protocol": 4, "root": "/legacy"});
        if explicit_null {
            hello["agent"] = serde_json::Value::Null;
        }
        let mut client = Client::from_process(capability_peer(directory.path(), hello)).unwrap();
        assert!(matches!(
            client.handshake(),
            Payload::Hello { agent: None, .. }
        ));
        client
            .request(Operation::List { path: ".".into() })
            .unwrap();
        read_fixture(&mut client);
        client
            .request(Operation::Write {
                path: "fixture.txt".into(),
                text: "changed".into(),
                expected_revision: None,
            })
            .unwrap();
        client
            .request(Operation::Search {
                query: "fixture".into(),
                limit: 1,
            })
            .unwrap();
        for operation in advanced_operations() {
            let name = operation.capability_name().unwrap();
            let error = client.request(operation.clone()).unwrap_err();
            assert!(
                error.starts_with("unsupported_operation:"),
                "{name}: {error}"
            );
            assert!(error.contains(name), "{name}: {error}");
            assert!(client.is_connected());
        }
        // An allowed request after all rejections proves the stream/IDs remain aligned.
        read_fixture(&mut client);
        let requests = recorded_requests(directory.path());
        let operations: Vec<_> = requests
            .iter()
            .map(|r| r["op"]["type"].as_str().unwrap())
            .collect();
        assert_eq!(
            operations,
            ["hello", "list", "read", "write", "search", "read"]
        );
    }
}
#[test]
fn unknown_capabilities_grant_nothing_and_browsing_survives_missing_optional_features() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = capability_client(
        directory.path(),
        Some(agent_info(&[
            "list",
            "read",
            "execute",
            "run_future",
            "language_future",
            "write_future",
        ])),
    );
    let mut operations = advanced_operations();
    operations.extend([
        Operation::Write {
            path: "fixture.txt".into(),
            text: "changed".into(),
            expected_revision: None,
        },
        Operation::Search {
            query: "fixture".into(),
            limit: 1,
        },
    ]);
    for operation in operations {
        assert!(client
            .request(operation)
            .unwrap_err()
            .starts_with("unsupported_operation:"));
    }
    assert_eq!(recorded_requests(directory.path()).len(), 1);
    assert!(client.is_connected());
    read_fixture(&mut client);
    assert_eq!(recorded_requests(directory.path()).len(), 2);
}
#[test]
fn every_operation_requires_its_own_declared_capability() {
    let operations = advanced_operations();
    let capabilities: Vec<_> = ["list", "read", "write", "search"]
        .into_iter()
        .chain(
            operations
                .iter()
                .map(|operation| operation.capability_name().unwrap()),
        )
        .collect();
    for operation in &operations {
        let missing = operation.capability_name().unwrap();
        let available: Vec<_> = capabilities
            .iter()
            .copied()
            .filter(|name| *name != missing)
            .collect();
        let directory = tempfile::tempdir().unwrap();
        let mut client = capability_client(directory.path(), Some(agent_info(&available)));
        let error = client.request(operation.clone()).unwrap_err();
        assert!(
            error.starts_with("unsupported_operation:"),
            "{missing}: {error}"
        );
        assert!(error.contains(missing), "{missing}: {error}");
        assert_eq!(recorded_requests(directory.path()).len(), 1, "{missing}");
        read_fixture(&mut client);
        assert_eq!(recorded_requests(directory.path()).len(), 2, "{missing}");
    }
    let directory = tempfile::tempdir().unwrap();
    let mut client = capability_client(directory.path(), Some(agent_info(&capabilities)));
    for operation in operations {
        let startup = match &operation {
            Operation::LanguageStartJavaBegin { .. } => {
                Some(serde_json::json!({"startup_id":1,"state":"starting","process_id":null}))
            }
            Operation::LanguageStartJavaPoll { startup_id } => Some(startup_ready(*startup_id)),
            Operation::LanguageStartJavaCancel { startup_id } => Some(
                serde_json::json!({"startup_id":startup_id,"state":"cancelled","cleanup_verified":true}),
            ),
            _ => None,
        };
        if let Some(value) = startup {
            startup_result(directory.path(), &mut client, value);
        }
        client.request(operation).unwrap();
    }
    assert_eq!(
        recorded_requests(directory.path()).len(),
        capabilities.len() - 3
    );
}
#[test]
fn starts_require_the_complete_lifecycle_before_sending_any_request() {
    for (start, lifecycle) in [
        (start_task(), RUN_TASK_CAPABILITIES),
        (start_language(), LANGUAGE_SESSION_CAPABILITIES),
        (start_java_language(), JAVA_LANGUAGE_SESSION_CAPABILITIES),
    ] {
        for missing in lifecycle {
            let capabilities: Vec<_> = ["list", "read", "run"]
                .into_iter()
                .chain(lifecycle.iter().copied().filter(|name| name != missing))
                .collect();
            let directory = tempfile::tempdir().unwrap();
            let mut client = capability_client(directory.path(), Some(agent_info(&capabilities)));
            let error = client.request(start.clone()).unwrap_err();
            assert!(
                error.starts_with("unsupported_operation:"),
                "{missing}: {error}"
            );
            assert!(error.contains(missing), "{missing}: {error}");
            assert!(client.is_connected());
            assert_eq!(recorded_requests(directory.path()).len(), 1);
            read_fixture(&mut client);
            let requests = recorded_requests(directory.path());
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[1]["op"]["type"], "read");
        }
        // Optional language extensions are not prerequisites for the base session.
        let capabilities: Vec<_> = ["list", "read"]
            .into_iter()
            .chain(lifecycle.iter().copied())
            .collect();
        let directory = tempfile::tempdir().unwrap();
        let mut client = capability_client(directory.path(), Some(agent_info(&capabilities)));
        client.request(start).unwrap();
        assert_eq!(recorded_requests(directory.path()).len(), 2);
    }
}

fn java_capabilities() -> Vec<&'static str> {
    ["list", "read"]
        .into_iter()
        .chain(JAVA_LANGUAGE_SESSION_CAPABILITIES.iter().copied())
        .chain([
            "java_diagnostics_refresh",
            "language_query",
            "language_format",
            "language_references",
            "language_document_symbols",
            "language_resolve_uri",
            "language_resolve_completion",
        ])
        .collect()
}

#[test]
fn java_refresh_requires_its_optional_bridge_and_complete_typed_java_lifecycle() {
    let refresh = Operation::LanguageRefreshJavaDiagnostics {
        path: "Hello.java".into(),
        version: 7,
    };
    for missing in std::iter::once(&"java_diagnostics_refresh")
        .chain(JAVA_LANGUAGE_SESSION_CAPABILITIES.iter())
    {
        let capabilities: Vec<_> = java_capabilities()
            .into_iter()
            .chain(["language_start"])
            .filter(|capability| capability != missing)
            .collect();
        let directory = tempfile::tempdir().unwrap();
        let mut client = capability_client(directory.path(), Some(agent_info(&capabilities)));
        let error = client.request(refresh.clone()).unwrap_err();
        assert!(error.starts_with("unsupported_operation:"), "{error}");
        assert!(error.contains(missing), "{error}");
        assert_eq!(recorded_requests(directory.path()).len(), 1);
        read_fixture(&mut client);
        assert_eq!(recorded_requests(directory.path()).len(), 2);
    }
    // An older typed Java peer keeps ordinary language operations; this bridge
    // is optional and cannot become an implicit startup prerequisite.
    let capabilities: Vec<_> = java_capabilities()
        .into_iter()
        .filter(|capability| *capability != "java_diagnostics_refresh")
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let mut client = capability_client(directory.path(), Some(agent_info(&capabilities)));
    client.request(start_java_language()).unwrap();
    client.request(Operation::LanguageEvents).unwrap();
    assert!(client
        .request(refresh)
        .unwrap_err()
        .contains("java_diagnostics_refresh"));
    client.request(Operation::LanguageStop).unwrap();
    assert_eq!(recorded_requests(directory.path()).len(), 4);
}

#[test]
fn java_refresh_ack_is_returned_unchanged_without_retry_or_diagnostic_poll() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = capability_client(directory.path(), Some(agent_info(&java_capabilities())));
    client.request(start_java_language()).unwrap();
    let acknowledgment = serde_json::json!({
        "diagnostics_refresh_requested":"file:///fixture/Hello%20%23.java",
        "version":7,"notification_only":true
    });
    next_result(
        directory.path(),
        &mut client,
        serde_json::json!({
            "Ok":{"type":"language","value":acknowledgment}
        }),
    );
    let response = client
        .request(Operation::LanguageRefreshJavaDiagnostics {
            path: "Hello #.java".into(),
            version: 7,
        })
        .unwrap();
    assert!(matches!(response, Payload::Language { value } if value == acknowledgment));
    assert!(process(&mut client).java_language_session);
    let requests = recorded_requests(directory.path());
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[2]["op"],
        serde_json::json!({
            "type":"language_refresh_java_diagnostics","path":"Hello #.java","version":7
        })
    );
    read_fixture(&mut client);
    assert_eq!(recorded_requests(directory.path()).len(), 4);
}

fn process(client: &mut Client) -> &mut ProcessClient {
    match &mut client.backend {
        Backend::Process(process) => process,
        #[cfg(not(windows))]
        Backend::Local(_) => panic!("expected process transport"),
    }
}

fn next_result(directory: &Path, client: &mut Client, result: serde_json::Value) {
    let id = process(client).next_id + 1;
    fs::write(
        directory.join(format!("response-{id}.json")),
        result.to_string(),
    )
    .unwrap();
}

#[test]
fn old_generic_language_peers_never_receive_the_new_java_start() {
    let directory = tempfile::tempdir().unwrap();
    let capabilities: Vec<_> = ["list", "read"]
        .into_iter()
        .chain(LANGUAGE_SESSION_CAPABILITIES.iter().copied())
        .collect();
    let mut client = capability_client(directory.path(), Some(agent_info(&capabilities)));
    client.request(start_language()).unwrap();
    let error = client.request(start_java_language()).unwrap_err();
    assert!(
        error.contains("does not advertise language_start_java"),
        "{error}"
    );
    client.request(Operation::LanguageEvents).unwrap();
    client.request(Operation::LanguageStop).unwrap();
    read_fixture(&mut client);
    let requests = recorded_requests(directory.path());
    assert_eq!(requests.len(), 5);
    assert!(!requests
        .iter()
        .any(|r| r["op"]["type"] == "language_start_java"));
    assert_eq!(requests.last().unwrap()["id"], 5);
}

#[test]
fn java_only_peer_manages_its_complete_session_without_generic_start_permission() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = capability_client(directory.path(), Some(agent_info(&java_capabilities())));
    assert!(!process(&mut client).java_language_session);
    client.request(start_java_language()).unwrap();
    assert!(process(&mut client).java_language_session);
    assert!(client
        .request(start_language())
        .unwrap_err()
        .contains("language_start"));
    assert!(process(&mut client).java_language_session);
    for operation in advanced_operations()
        .into_iter()
        .filter(is_language_session_operation)
    {
        assert_eq!(
            process(&mut client).request_timeout(&operation),
            Duration::from_secs(75)
        );
        client.request(operation).unwrap();
    }
    assert!(!process(&mut client).java_language_session);
    assert_eq!(
        process(&mut client).request_timeout(&Operation::LanguageEvents),
        Duration::from_secs(30)
    );
    // The active-session budget is cleared, but Stop remains idempotent and
    // no-session failures still come from the Java-capable agent.
    client.request(Operation::LanguageStop).unwrap();
    next_result(
        directory.path(),
        &mut client,
        serde_json::json!({"Err":{"code":"language_not_running","message":"fixture session absent"}}),
    );
    assert!(client
        .request(Operation::LanguageEvents)
        .unwrap_err()
        .starts_with("language_not_running:"));
    read_fixture(&mut client);
    let requests = recorded_requests(directory.path());
    assert!(!requests.iter().any(|r| r["op"]["type"] == "language_start"));
    assert_eq!(requests[1]["op"]["distribution"], "C:\\JDT distribution 雪");
}

#[test]
fn language_operations_require_the_common_lifecycle_and_selected_start_route() {
    for operation in advanced_operations()
        .into_iter()
        .filter(is_language_session_operation)
    {
        let name = operation.capability_name().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut client =
            capability_client(directory.path(), Some(agent_info(&["list", "read", name])));
        assert!(client
            .request(operation)
            .unwrap_err()
            .contains("language_start"));
        assert_eq!(recorded_requests(directory.path()).len(), 1);
    }
}

#[test]
fn java_deadline_only_applies_after_successful_start_and_never_extends_other_work() {
    let directory = tempfile::tempdir().unwrap();
    let mut capabilities = java_capabilities();
    capabilities.push("language_start");
    let mut client = capability_client(directory.path(), Some(agent_info(&capabilities)));
    let language_operations: Vec<_> = advanced_operations()
        .into_iter()
        .filter(is_language_session_operation)
        .collect();
    for operation in &language_operations {
        assert_eq!(
            process(&mut client).request_timeout(operation),
            Duration::from_secs(30)
        );
    }
    assert_eq!(
        process(&mut client).request_timeout(&start_java_language()),
        Duration::from_secs(75)
    );
    next_result(
        directory.path(),
        &mut client,
        serde_json::json!({"Err":{"code":"invalid_java_profile","message":"invalid fixture path"}}),
    );
    assert!(client
        .request(start_java_language())
        .unwrap_err()
        .starts_with("invalid_java_profile:"));
    assert!(!process(&mut client).java_language_session);
    client.request(start_java_language()).unwrap();
    for operation in &language_operations {
        assert_eq!(
            process(&mut client).request_timeout(operation),
            Duration::from_secs(75)
        );
    }
    for operation in [
        Operation::Hello,
        Operation::Read {
            path: "fixture.txt".into(),
        },
        start_task(),
        Operation::RunPoll { task_id: 1 },
        Operation::RunCancel { task_id: 1 },
    ] {
        assert_eq!(
            process(&mut client).request_timeout(&operation),
            Duration::from_secs(30)
        );
    }
    for (requested, expected) in [(0, 11), (40, 50), (u64::MAX, 310)] {
        assert_eq!(
            process(&mut client).request_timeout(&Operation::Run {
                program: "never-executed".into(),
                args: vec![],
                timeout_secs: requested
            }),
            Duration::from_secs(expected)
        );
    }
    client.request(Operation::LanguageStop).unwrap();
    client.request(start_language()).unwrap();
    assert!(!process(&mut client).java_language_session);
    for operation in &language_operations {
        assert_eq!(
            process(&mut client).request_timeout(operation),
            Duration::from_secs(30)
        );
    }
}

#[test]
fn java_mode_survives_start_and_query_refusals_but_clears_on_stop_or_no_session() {
    let directory = tempfile::tempdir().unwrap();
    let mut capabilities = java_capabilities();
    capabilities.push("language_start");
    let mut client = capability_client(directory.path(), Some(agent_info(&capabilities)));
    client.request(start_java_language()).unwrap();
    for (operation, code) in [
        (start_language(), "language_running"),
        (start_java_language(), "language_running"),
        (Operation::LanguageEvents, "language_error"),
        (
            Operation::LanguageDocumentSymbols {
                path: "fixture.txt".into(),
            },
            "language_document_closed",
        ),
    ] {
        next_result(
            directory.path(),
            &mut client,
            serde_json::json!({"Err":{"code":code,"message":"fixture refusal"}}),
        );
        assert!(client.request(operation).unwrap_err().starts_with(code));
        assert!(process(&mut client).java_language_session);
        assert_eq!(
            process(&mut client).request_timeout(&Operation::LanguageStop),
            Duration::from_secs(75)
        );
    }
    next_result(
        directory.path(),
        &mut client,
        serde_json::json!({"Err":{"code":"language_cleanup_unverified","message":"fixture cleanup failed"}}),
    );
    assert!(client
        .request(Operation::LanguageStop)
        .unwrap_err()
        .starts_with("language_cleanup_unverified:"));
    assert!(!process(&mut client).java_language_session);
    assert!(client.is_connected());
    client.request(start_java_language()).unwrap();
    next_result(
        directory.path(),
        &mut client,
        serde_json::json!({"Err":{"code":"language_not_running","message":"fixture session absent"}}),
    );
    assert!(client
        .request(Operation::LanguageEvents)
        .unwrap_err()
        .starts_with("language_not_running:"));
    assert!(!process(&mut client).java_language_session);
    read_fixture(&mut client);
}

#[test]
fn java_stop_forwards_cleanup_evidence_without_inventing_graceful_success() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = capability_client(directory.path(), Some(agent_info(&java_capabilities())));
    client.request(start_java_language()).unwrap();
    let value = serde_json::json!({
        "stopped": true,
        "shutdown": {
            "status": "forced",
            "reason": "grace_expired",
            "root_exit_code": 1,
            "cleanup_joined": true,
            "shutdown_response_received": true,
            "exit_frame_completed": true
        }
    });
    next_result(
        directory.path(),
        &mut client,
        serde_json::json!({"Ok":{"type":"language","value":value}}),
    );
    let Payload::Language { value: received } = client.request(Operation::LanguageStop).unwrap()
    else {
        panic!("expected language shutdown evidence");
    };
    assert_eq!(received, value);
    assert!(!process(&mut client).java_language_session);
    assert!(client.is_connected());
}

#[test]
fn java_transport_failure_clears_mode_without_replay_or_leaking_to_a_new_connection() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = capability_client(directory.path(), Some(agent_info(&java_capabilities())));
    client.request(start_java_language()).unwrap();
    next_result(directory.path(), &mut client, serde_json::Value::Null);
    assert!(client
        .request(Operation::LanguageEvents)
        .unwrap_err()
        .starts_with("transport_read:"));
    assert!(!client.is_connected());
    assert!(!process(&mut client).java_language_session);
    assert!(client
        .request(start_java_language())
        .unwrap_err()
        .starts_with("disconnected:"));
    process(&mut client)
        .reaped
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert_eq!(recorded_requests(directory.path()).len(), 3);
    let fresh_directory = tempfile::tempdir().unwrap();
    let mut fresh = capability_client(
        fresh_directory.path(),
        Some(agent_info(&java_capabilities())),
    );
    assert!(!process(&mut fresh).java_language_session);
    assert_eq!(
        process(&mut fresh).request_timeout(&Operation::LanguageEvents),
        Duration::from_secs(30)
    );
    fresh.request(start_java_language()).unwrap();
    process(&mut fresh).close();
    assert!(!process(&mut fresh).java_language_session);
}
#[test]
fn missing_required_workspace_capability_refuses_connection_without_fallback() {
    for capabilities in [&[][..], &["list"][..], &["read"][..], &["run"][..]] {
        let directory = tempfile::tempdir().unwrap();
        let hello = serde_json::json!({"type": "hello", "protocol": 4, "root": "/fixture", "agent": agent_info(capabilities)});
        let error = match Client::from_process(capability_peer(directory.path(), hello)) {
            Ok(_) => panic!("accepted incomplete workspace: {capabilities:?}"),
            Err(error) => error,
        };
        assert!(error.starts_with("unsupported_workspace:"), "{error}");
        wait_until(|| directory.path().join("eof").exists());
        assert_eq!(recorded_requests(directory.path()).len(), 1);
    }
}
#[test]
fn malformed_present_metadata_refuses_connection_and_never_falls_back_to_legacy() {
    let valid = serde_json::to_value(agent_info(&["list", "read"])).unwrap();
    let mut malformed = vec![
        (serde_json::json!({}), "transport_read:"),
        (serde_json::json!("legacy"), "transport_read:"),
    ];
    for (field, value, expected) in [
        ("schema", serde_json::json!(0), "invalid_agent_info:"),
        ("schema", serde_json::json!(2), "invalid_agent_info:"),
        ("schema", serde_json::json!("1"), "transport_read:"),
        ("version", serde_json::json!(""), "invalid_agent_info:"),
        (
            "version",
            serde_json::json!("v\nforged display"),
            "invalid_agent_info:",
        ),
        (
            "version",
            serde_json::json!("v".repeat(65)),
            "invalid_agent_info:",
        ),
        ("os", serde_json::json!("Linux"), "invalid_agent_info:"),
        ("arch", serde_json::json!(""), "invalid_agent_info:"),
        (
            "capabilities",
            serde_json::json!(["list", "read", "list"]),
            "invalid_agent_info:",
        ),
        (
            "capabilities",
            serde_json::json!(["list", "read", "run\n"]),
            "invalid_agent_info:",
        ),
        (
            "capabilities",
            serde_json::json!(["list", "read", 3]),
            "transport_read:",
        ),
        ("capabilities", serde_json::Value::Null, "transport_read:"),
    ] {
        let mut agent = valid.clone();
        agent[field] = value;
        malformed.push((agent, expected));
    }
    for (agent, expected) in malformed {
        let directory = tempfile::tempdir().unwrap();
        let hello =
            serde_json::json!({"type": "hello", "protocol": 4, "root": "/fixture", "agent": agent});
        let error = match Client::from_process(capability_peer(directory.path(), hello)) {
            Ok(_) => panic!("accepted invalid metadata: {agent}"),
            Err(error) => error,
        };
        assert!(error.starts_with(expected), "{agent}: {error}");
        wait_until(|| directory.path().join("eof").exists());
        assert_eq!(recorded_requests(directory.path()).len(), 1);
    }
}
#[test]
fn cached_hello_cannot_hide_an_already_detected_disconnect() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::from_process(spawn("eof_after_request", directory.path())).unwrap();
    client.request(Operation::Hello).unwrap();
    assert_eq!(recorded_requests(directory.path()).len(), 1);
    assert!(client
        .request(Operation::Read {
            path: "fixture.txt".into()
        })
        .unwrap_err()
        .starts_with("transport_eof:"));
    assert!(!client.is_connected());
    assert!(client
        .request(Operation::Hello)
        .unwrap_err()
        .starts_with("disconnected:"));
    assert!(client
        .request(start_task())
        .unwrap_err()
        .starts_with("disconnected:"));
    assert_eq!(recorded_requests(directory.path()).len(), 2);
}

fn begin_java_language() -> Operation {
    Operation::LanguageStartJavaBegin {
        java_executable: "/synthetic/java.exe".into(),
        distribution: "/synthetic/jdt".into(),
        data_directory: "/synthetic/data".into(),
    }
}
fn async_java_capabilities() -> Vec<&'static str> {
    java_capabilities()
        .into_iter()
        .chain(JAVA_STARTUP_CAPABILITIES.iter().copied())
        .collect()
}
fn startup_result(root: &std::path::Path, client: &mut Client, value: serde_json::Value) {
    next_result(
        root,
        client,
        serde_json::json!({"Ok":{"type":"language","value":value}}),
    );
}
fn startup_ready(id: u64) -> serde_json::Value {
    serde_json::json!({"startup_id":id,"state":"ready","language":{
        "started":true,"initialize":{"capabilities":{}},"root_uri":"file:///fixture","process_id":42
    }})
}

#[test]
fn async_java_requires_complete_optional_and_legacy_lifecycle_before_any_wire_request() {
    for missing in JAVA_STARTUP_CAPABILITIES
        .iter()
        .chain(JAVA_LANGUAGE_SESSION_CAPABILITIES)
    {
        let directory = tempfile::tempdir().unwrap();
        let caps: Vec<_> = async_java_capabilities()
            .into_iter()
            .filter(|name| name != missing)
            .collect();
        let mut client = capability_client(directory.path(), Some(agent_info(&caps)));
        for operation in [
            begin_java_language(),
            Operation::LanguageStartJavaPoll { startup_id: 1 },
            Operation::LanguageStartJavaCancel { startup_id: 1 },
        ] {
            let error = client.request(operation).unwrap_err();
            assert!(error.contains(missing), "{missing}: {error}");
        }
        assert_eq!(recorded_requests(directory.path()).len(), 1);
        read_fixture(&mut client);
    }
    let directory = tempfile::tempdir().unwrap();
    let mut legacy = capability_client(directory.path(), Some(agent_info(&java_capabilities())));
    legacy.request(start_java_language()).unwrap();
    assert!(process(&mut legacy).java_language_session);
    assert!(legacy
        .request(begin_java_language())
        .unwrap_err()
        .contains("language_start_java_begin"));
}

#[test]
fn async_java_tracks_authoritative_ready_and_cancellation_without_replay_or_polling() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = capability_client(
        directory.path(),
        Some(agent_info(&async_java_capabilities())),
    );
    startup_result(
        directory.path(),
        &mut client,
        serde_json::json!({"startup_id":1,"state":"starting","process_id":null}),
    );
    client.request(begin_java_language()).unwrap();
    assert!(!process(&mut client).java_language_session);
    assert_eq!(process(&mut client).java_startup.pending, Some(1));
    assert_eq!(recorded_requests(directory.path()).len(), 2);
    read_fixture(&mut client);
    startup_result(directory.path(), &mut client, startup_ready(1));
    client
        .request(Operation::LanguageStartJavaPoll { startup_id: 1 })
        .unwrap();
    assert!(process(&mut client).java_language_session);
    assert_eq!(process(&mut client).java_startup.active, Some(1));
    assert_eq!(
        process(&mut client).request_timeout(&Operation::LanguageEvents),
        Duration::from_secs(75)
    );
    for operation in [
        begin_java_language(),
        Operation::LanguageStartJavaPoll { startup_id: 1 },
        Operation::LanguageStartJavaCancel { startup_id: 1 },
    ] {
        assert_eq!(
            process(&mut client).request_timeout(&operation),
            Duration::from_secs(30)
        );
    }
    startup_result(
        directory.path(),
        &mut client,
        serde_json::json!({"startup_id":1,"state":"cancelling","process_id":42}),
    );
    client
        .request(Operation::LanguageStartJavaCancel { startup_id: 1 })
        .unwrap();
    assert!(!process(&mut client).java_language_session);
    assert_eq!(process(&mut client).java_startup.cancelling, Some(1));
    read_fixture(&mut client);
    startup_result(
        directory.path(),
        &mut client,
        serde_json::json!({"startup_id":1,"state":"cancelled","cleanup_verified":true}),
    );
    client
        .request(Operation::LanguageStartJavaPoll { startup_id: 1 })
        .unwrap();
    assert_eq!(process(&mut client).java_startup.active, None);
    assert_eq!(process(&mut client).java_startup.pending, None);
    assert_eq!(recorded_requests(directory.path()).len(), 7);
    assert!(client.is_connected());
}

#[test]
fn async_java_pending_stop_refusal_and_old_snapshots_do_not_clear_new_owner() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = capability_client(
        directory.path(),
        Some(agent_info(&async_java_capabilities())),
    );
    startup_result(
        directory.path(),
        &mut client,
        serde_json::json!({"startup_id":2,"state":"starting","process_id":null}),
    );
    client.request(begin_java_language()).unwrap();
    next_result(
        directory.path(),
        &mut client,
        serde_json::json!({"Err":{"code":"language_start_in_progress","message":"cancel startup first"}}),
    );
    assert!(client
        .request(Operation::LanguageStop)
        .unwrap_err()
        .contains("language_start_in_progress"));
    assert_eq!(process(&mut client).java_startup.pending, Some(2));
    startup_result(directory.path(), &mut client, startup_ready(2));
    client
        .request(Operation::LanguageStartJavaPoll { startup_id: 2 })
        .unwrap();
    startup_result(
        directory.path(),
        &mut client,
        serde_json::json!({"startup_id":1,"state":"cancelled","cleanup_verified":true}),
    );
    client
        .request(Operation::LanguageStartJavaCancel { startup_id: 1 })
        .unwrap();
    assert!(process(&mut client).java_language_session);
    assert_eq!(process(&mut client).java_startup.active, Some(2));
    startup_result(directory.path(), &mut client, startup_ready(1));
    client
        .request(Operation::LanguageStartJavaPoll { startup_id: 1 })
        .unwrap();
    assert_eq!(process(&mut client).java_startup.active, Some(2));
    client.request(Operation::LanguageStop).unwrap();
    assert!(!process(&mut client).java_language_session);
    assert_eq!(process(&mut client).java_startup.active, None);
    startup_result(directory.path(), &mut client, startup_ready(2));
    client
        .request(Operation::LanguageStartJavaPoll { startup_id: 2 })
        .unwrap();
    assert!(!process(&mut client).java_language_session);
}

#[test]
fn async_java_malformed_snapshots_disconnect_without_automatic_cancel_or_restart() {
    for malformed in [
        serde_json::json!({"startup_id":1,"state":"starting"}),
        serde_json::json!({"startup_id":1,"state":"ready","language":{"started":true,"initialize":{},"root_uri":"file:///fixture","process_id":42}}),
        serde_json::json!({"startup_id":2,"state":"starting","process_id":null}),
        serde_json::json!({"startup_id":1,"state":"ready","language":{"started":true}}),
        serde_json::json!({"startup_id":1,"state":"cancelled","cleanup_verified":false}),
        serde_json::json!({"startup_id":1,"state":"failed","cleanup_verified":"yes","error":{}}),
        serde_json::json!({"startup_id":1,"state":"starting","process_id":-1}),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut client = capability_client(
            directory.path(),
            Some(agent_info(&async_java_capabilities())),
        );
        startup_result(
            directory.path(),
            &mut client,
            serde_json::json!({"startup_id":1,"state":"starting","process_id":null}),
        );
        client.request(begin_java_language()).unwrap();
        startup_result(directory.path(), &mut client, malformed);
        assert!(client
            .request(Operation::LanguageStartJavaPoll { startup_id: 1 })
            .unwrap_err()
            .starts_with("protocol_error:"));
        assert!(!client.is_connected());
        assert!(!process(&mut client).java_language_session);
        assert_eq!(process(&mut client).java_startup.pending, None);
        process(&mut client)
            .reaped
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!(recorded_requests(directory.path()).len(), 3);
    }
}
