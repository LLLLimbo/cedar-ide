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
