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
        peer.reaped.recv_timeout(Duration::from_secs(5)).unwrap();
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
        peer.reaped.recv_timeout(Duration::from_secs(5)).unwrap();
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
    peer.reaped.recv_timeout(Duration::from_secs(5)).unwrap();
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
    peer.reaped.recv_timeout(Duration::from_secs(5)).unwrap();
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
    peer.reaped.recv_timeout(Duration::from_secs(5)).unwrap();
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
    peer.reaped.recv_timeout(Duration::from_secs(5)).unwrap();
    let before_reconnect = fs::read_to_string(dir.path().join("requests")).unwrap();
    assert_eq!(before_reconnect.lines().count(), 2);
    assert_eq!(before_reconnect.matches("\"type\":\"write\"").count(), 1);
    let mut fresh = connected("normal", dir.path());
    fresh.request(Operation::Hello).unwrap();
    fresh.close();
    fresh.reaped.recv_timeout(Duration::from_secs(5)).unwrap();
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
    peer.reaped.recv_timeout(Duration::from_secs(5)).unwrap();
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
    peer.reaped.recv_timeout(Duration::from_secs(5)).unwrap();
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
    peer.reaped.recv_timeout(Duration::from_secs(5)).unwrap();
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
    reaped.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(!dir.path().join("eof").exists());
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
fn advanced_operations() -> Vec<Operation> {
    vec![
        Operation::GitStatus,
        start_task(),
        Operation::RunPoll { task_id: 1 },
        Operation::RunCancel { task_id: 1 },
        Operation::Run {
            program: "never-executed".into(),
            args: vec![],
            timeout_secs: 1,
        },
        start_language(),
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
