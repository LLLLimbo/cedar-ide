//! Real child pipes exercise transport framing, deadlines and lifecycle. No SSH
//! credentials/server, Python, shell, or platform-specific executable required.
use super::*;
use cedar_protocol::{JAVA_MAVEN_DEPENDENCIES_GROUP, JAVA_MAVEN_LEAF_GROUP};
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

#[derive(Debug, PartialEq, Eq)]
enum FixturePresence {
    Present,
    Missing,
    Unavailable,
}

fn fixture_presence(path: &Path) -> FixturePresence {
    match fs::metadata(path) {
        Ok(_) => FixturePresence::Present,
        Err(error) if error.kind() == io::ErrorKind::NotFound => FixturePresence::Missing,
        Err(_) => FixturePresence::Unavailable,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct RequestFileSummary {
    bytes: usize,
    bytes_capped: bool,
    complete_lines: usize,
    lines_capped: bool,
    observed_partial_line: bool,
}

fn summarize_request_bytes(bytes: &[u8]) -> RequestFileSummary {
    let observed = &bytes[..bytes.len().min(DIAGNOSTIC_BYTE_LIMIT)];
    let lines = observed.iter().filter(|byte| **byte == b'\n').count();
    RequestFileSummary {
        bytes: observed.len(),
        bytes_capped: bytes.len() > DIAGNOSTIC_BYTE_LIMIT,
        complete_lines: lines.min(64),
        lines_capped: lines > 64,
        observed_partial_line: observed.last().is_some_and(|byte| *byte != b'\n'),
    }
}

fn request_file_summary(directory: &Path) -> Result<RequestFileSummary, FixturePresence> {
    let input = fs::File::open(directory.join("requests")).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            FixturePresence::Missing
        } else {
            FixturePresence::Unavailable
        }
    })?;
    let mut bytes = Vec::new();
    input
        .take((DIAGNOSTIC_BYTE_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| FixturePresence::Unavailable)?;
    Ok(summarize_request_bytes(&bytes))
}

fn lifecycle_snapshot(peer: &ProcessClient) -> Option<ProcessLifecycle> {
    peer.lifecycle
        .as_ref()
        .map(|lifecycle| *lifecycle.lock().unwrap_or_else(|p| p.into_inner()))
}

fn stalled_close_receipt(
    peer: &ProcessClient,
    directory: &Path,
    start: Instant,
    request_elapsed: Duration,
    reap_wait_elapsed: Option<Duration>,
) -> String {
    // Some is this exact owner's observation; None explicitly means no owner
    // observation is available (for example a channel-only synthetic client).
    // Pipe-thread fields may still be NotYetObserved after a reaped receipt.
    format!(
        "stalled_close_receipt/v1 request_timeout_ms=100 fixture_grace_ms=200 \
         caller_bound_ms=2000 reap_wait_bound_ms=5000 request_elapsed_ms={} \
         total_elapsed_ms={} reap_wait_elapsed_ms={:?} owner={:?} requests={:?} \
         eof={:?} eof_pending={:?}",
        request_elapsed.as_millis(),
        start.elapsed().as_millis(),
        reap_wait_elapsed.map(|elapsed| elapsed.as_millis()),
        lifecycle_snapshot(peer),
        request_file_summary(directory),
        fixture_presence(&directory.join("eof")),
        fixture_presence(&directory.join("eof.pending")),
    )
}

#[test]
fn lifecycle_exit_categories_have_fixed_numeric_boundaries() {
    for (code, expected) in [
        (i32::MIN, ExitObservation::Code(i32::MIN)),
        (-1, ExitObservation::Code(-1)),
        (0, ExitObservation::Success),
        (1, ExitObservation::Code(1)),
        (255, ExitObservation::Code(255)),
        (256, ExitObservation::Code(256)),
        (i32::MAX, ExitObservation::Code(i32::MAX)),
    ] {
        assert_eq!(ExitObservation::from_parts(Some(code), None), expected);
    }
    for (signal, expected) in [
        (i32::MIN, ExitObservation::SignalOutsideRange),
        (0, ExitObservation::SignalOutsideRange),
        (1, ExitObservation::Signal(1)),
        (127, ExitObservation::Signal(127)),
        (128, ExitObservation::SignalOutsideRange),
        (i32::MAX, ExitObservation::SignalOutsideRange),
    ] {
        assert_eq!(ExitObservation::from_parts(None, Some(signal)), expected);
    }
    assert_eq!(
        ExitObservation::from_parts(None, None),
        ExitObservation::Unavailable
    );
    assert_ne!(ExitObservation::default(), ExitObservation::Unavailable);
}

#[test]
fn lifecycle_stderr_counts_and_panic_classification_are_bounded() {
    let mut stderr = StderrObservation::default();
    assert_eq!(stderr.completion, StderrCompletion::NotYetObserved);
    assert_eq!(stderr.panic, PanicObservation::NotObserved);
    stderr.observe_bytes(DIAGNOSTIC_BYTE_LIMIT, b"ordinary diagnostic");
    assert_eq!(stderr.received.bytes, DIAGNOSTIC_BYTE_LIMIT);
    assert!(!stderr.received.capped);
    stderr.observe_bytes(1, b"thread 'fixture' panicked at discarded location");
    assert_eq!(stderr.panic, PanicObservation::RustPanicMarkerObserved);
    stderr.observe_bytes(usize::MAX, &vec![b'x'; STDERR_TAIL_BYTES]);
    assert_eq!(stderr.received.bytes, DIAGNOSTIC_BYTE_LIMIT);
    assert!(stderr.received.capped);
    assert_eq!(stderr.retained_bytes, STDERR_TAIL_BYTES);
    assert_eq!(stderr.panic, PanicObservation::RustPanicMarkerObserved);
    assert_eq!(stderr.completion, StderrCompletion::NotYetObserved);
}

#[test]
fn lifecycle_request_summary_is_bounded_and_counts_only_complete_lines() {
    assert_eq!(
        summarize_request_bytes(b""),
        RequestFileSummary {
            bytes: 0,
            bytes_capped: false,
            complete_lines: 0,
            lines_capped: false,
            observed_partial_line: false,
        }
    );
    let partial = summarize_request_bytes(b"first\npartial");
    assert_eq!(partial.complete_lines, 1);
    assert!(partial.observed_partial_line);
    let exactly_capped = summarize_request_bytes(&vec![b'\n'; DIAGNOSTIC_BYTE_LIMIT]);
    assert_eq!(exactly_capped.bytes, DIAGNOSTIC_BYTE_LIMIT);
    assert!(!exactly_capped.bytes_capped);
    assert_eq!(exactly_capped.complete_lines, 64);
    assert!(exactly_capped.lines_capped);
    assert!(!exactly_capped.observed_partial_line);
    let over_cap = summarize_request_bytes(&vec![b'x'; DIAGNOSTIC_BYTE_LIMIT + 1]);
    assert!(over_cap.bytes_capped);
    assert!(over_cap.observed_partial_line);
    assert_eq!(over_cap.complete_lines, 0);
    for lines in [63, 64, 65] {
        let summary = summarize_request_bytes(&vec![b'\n'; lines]);
        assert_eq!(summary.complete_lines, lines.min(64));
        assert_eq!(summary.lines_capped, lines > 64);
    }
}

#[test]
fn transport_observation_registration_catches_both_publication_orders() {
    for publish_first in [false, true] {
        let observation = TransportObservation::default();
        let (wake, received) = mpsc::channel();
        if publish_first {
            observation.publish(Some("transport_eof: fixture"));
        }
        observation.register(Arc::new(move || wake.send(()).unwrap()));
        if !publish_first {
            observation.publish(Some("transport_eof: fixture"));
        }
        received.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(received.try_recv().is_err());
        assert_eq!(
            observation.begin_idle_observation().as_deref(),
            Some("transport_eof: fixture")
        );
    }
}

#[test]
fn transport_observation_coalesces_until_drained_and_retains_first_terminal() {
    let observation = TransportObservation::default();
    let (wake, received) = mpsc::channel();
    observation.register(Arc::new(move || wake.send(()).unwrap()));
    observation.publish(None);
    observation.publish(None);
    observation.publish(Some("transport_eof: first"));
    observation.publish(Some("transport_write: later"));
    received.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(received.try_recv().is_err());
    assert_eq!(
        observation.begin_idle_observation().as_deref(),
        Some("transport_eof: first")
    );
    // An event racing the owner's upcoming empty queue read must own a new
    // wake; there is no flag clear after that read that could erase it.
    observation.publish(None);
    received.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(received.try_recv().is_err());
}

#[test]
fn transport_observation_callbacks_run_outside_the_state_lock() {
    for publication_before_register in [false, true] {
        let observation = Arc::new(TransportObservation::default());
        let callback_owner = Arc::downgrade(&observation);
        let (wake, received) = mpsc::channel();
        if publication_before_register {
            observation.publish(Some("transport_eof: fixture"));
        }
        let register = thread::spawn(move || {
            observation.register(Arc::new(move || {
                let owner = callback_owner.upgrade().unwrap();
                owner.unregister();
                let _ = wake.send(owner.begin_idle_observation());
            }));
            if !publication_before_register {
                observation.publish(Some("transport_eof: fixture"));
            }
        });
        assert_eq!(
            received.recv_timeout(Duration::from_secs(1)).unwrap(),
            Some("transport_eof: fixture".into())
        );
        register.join().unwrap();
    }
}

#[test]
fn transport_observation_unregister_releases_callback_and_preserves_pending_event() {
    let observation = TransportObservation::default();
    let capture = Arc::new(());
    let retained = capture.clone();
    observation.register(Arc::new(move || {
        let _ = &retained;
    }));
    assert_eq!(Arc::strong_count(&capture), 2);
    observation.unregister();
    assert_eq!(Arc::strong_count(&capture), 1);
    observation.publish(Some("transport_eof: fixture"));
    let (wake, received) = mpsc::channel();
    observation.register(Arc::new(move || wake.send(()).unwrap()));
    received.recv_timeout(Duration::from_secs(1)).unwrap();
}

fn register_idle_wake(client: &mut Client) -> mpsc::Receiver<()> {
    // Hello may have been consumed before its producer publishes readiness.
    // Wait for that single known event, then consume its harmless stale wake.
    wait_until(|| process(client).observation.state.lock().unwrap().pending);
    let (wake, received) = mpsc::channel();
    client.set_transport_waker(move || {
        let _ = wake.send(());
    });
    received.recv_timeout(Duration::from_secs(2)).unwrap();
    client.observe_idle_transport().unwrap();
    assert!(received.try_recv().is_err());
    received
}

#[test]
fn idle_process_eof_and_read_error_wake_without_any_followup_request() {
    for (mode, expected) in [
        ("idle_eof", "transport_eof:"),
        ("idle_bad_frame", "transport_read:"),
        (
            "idle_unsolicited",
            "protocol_error: unsolicited response id 2",
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut client = Client::from_process(spawn(mode, directory.path())).unwrap();
        let wake = register_idle_wake(&mut client);
        fs::write(directory.path().join("release-1"), b"release idle event").unwrap();
        wake.recv_timeout(Duration::from_secs(2)).unwrap();
        let error = client.observe_idle_transport().unwrap_err();
        assert!(error.starts_with(expected), "{mode}: {error}");
        assert!(!error.contains("outcome may be unknown"), "{error}");
        assert!(!client.is_connected());
        // Duplicate or already queued wake delivery cannot emit a second loss.
        client.observe_idle_transport().unwrap();
        assert!(client
            .request(Operation::Read {
                path: "fixture.txt".into()
            })
            .unwrap_err()
            .starts_with("disconnected:"));
        client.close_and_wait(Duration::from_secs(5)).unwrap();
        assert_eq!(recorded_requests(directory.path()).len(), 1, "{mode}");
    }
}

#[test]
fn idle_terminal_before_registration_is_observed_and_blocks_the_next_write() {
    for register in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut client = Client::from_process(spawn("idle_eof", directory.path())).unwrap();
        fs::write(directory.path().join("release-1"), b"release EOF").unwrap();
        wait_until(|| {
            process(&mut client)
                .observation
                .state
                .lock()
                .unwrap()
                .terminal
                .is_some()
        });
        if register {
            let (wake, received) = mpsc::channel();
            client.set_transport_waker(move || {
                let _ = wake.send(());
            });
            received.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        let error = client
            .request(Operation::Write {
                path: "must-not-be-written.txt".into(),
                text: "not sent".into(),
                expected_revision: None,
            })
            .unwrap_err();
        assert!(error.starts_with("transport_eof:"), "{error}");
        assert!(!error.contains("outcome may be unknown"), "{error}");
        client.close_and_wait(Duration::from_secs(5)).unwrap();
        assert_eq!(recorded_requests(directory.path()).len(), 1);
    }
}

#[test]
fn idle_future_id_reply_cannot_be_consumed_by_a_new_request() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::from_process(spawn("idle_unsolicited", directory.path())).unwrap();
    let wake = register_idle_wake(&mut client);
    fs::write(directory.path().join("release-1"), b"release future id").unwrap();
    wake.recv_timeout(Duration::from_secs(2)).unwrap();
    // This matches the queued response's id and operation. The pre-send idle
    // check still rejects it instead of using it as this Read's response.
    let error = client
        .request(Operation::Read {
            path: "fixture.txt".into(),
        })
        .unwrap_err();
    assert!(
        error.starts_with("protocol_error: unsolicited response id 2"),
        "{error}"
    );
    client.close_and_wait(Duration::from_secs(5)).unwrap();
    assert_eq!(recorded_requests(directory.path()).len(), 1);
}

#[test]
fn healthy_idle_observation_sends_nothing_and_close_releases_its_callback() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::from_process(spawn("normal", directory.path())).unwrap();
    let wake = register_idle_wake(&mut client);
    assert_eq!(
        wake.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    );
    client.observe_idle_transport().unwrap();
    assert!(client.is_connected());
    assert_eq!(recorded_requests(directory.path()).len(), 1);
    let capture = Arc::new(());
    let retained = capture.clone();
    client.set_transport_waker(move || {
        let _ = &retained;
    });
    assert_eq!(Arc::strong_count(&capture), 2);
    // Producers may still hold the observation Arc after close. They must not
    // retain the owner's callback while pipe cleanup finishes.
    let observation = process(&mut client).observation.clone();
    drop(client);
    assert_eq!(Arc::strong_count(&capture), 1);
    assert!(observation.state.lock().unwrap().wake.is_none());
    wait_until(|| directory.path().join("eof").exists());
}

#[test]
fn authoritative_read_and_write_replies_precede_later_idle_eof() {
    for operation in [
        Operation::Read {
            path: "fixture.txt".into(),
        },
        Operation::Write {
            path: "fixture.txt".into(),
            text: "saved".into(),
            expected_revision: None,
        },
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut client = Client::from_process(spawn("held_reply_eof", directory.path())).unwrap();
        let wake = register_idle_wake(&mut client);
        let is_write = matches!(operation, Operation::Write { .. });
        let caller = thread::spawn(move || {
            let result = client.request(operation);
            (client, result)
        });
        wait_until(|| directory.path().join("ready-2").exists());
        fs::write(directory.path().join("release-2"), b"reply then EOF").unwrap();
        let (mut client, response) = caller.join().unwrap();
        let response = response.unwrap();
        assert!(if is_write {
            matches!(response, Payload::Written { .. })
        } else {
            matches!(response, Payload::File { .. })
        });
        wake.recv_timeout(Duration::from_secs(2)).unwrap();
        wait_until(|| {
            process(&mut client)
                .observation
                .state
                .lock()
                .unwrap()
                .terminal
                .is_some()
        });
        // The worker can publish this authoritative result as connected=true
        // before the separately ordered transport-loss event.
        assert!(client.is_connected());
        assert!(client
            .observe_idle_transport()
            .unwrap_err()
            .starts_with("transport_eof:"));
        assert!(!client.is_connected());
        client.close_and_wait(Duration::from_secs(5)).unwrap();
        assert_eq!(recorded_requests(directory.path()).len(), 2);
    }
}

#[cfg(not(windows))]
#[test]
fn embedded_workspace_has_no_transport_activity_or_observer_retention() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::connect(ConnectionSpec::Local {
        root: directory.path().into(),
        allow_run: false,
    })
    .unwrap();
    let capture = Arc::new(());
    let retained = capture.clone();
    client.set_transport_waker(move || {
        let _ = &retained;
        panic!("embedded transport wake");
    });
    assert_eq!(Arc::strong_count(&capture), 1);
    client.observe_idle_transport().unwrap();
    client.clear_transport_waker();
    assert!(client.is_connected());
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
        assert!(error.contains("commands are never automatically replayed"));
        if mode == "eof_after_request" {
            assert!(error.contains("outcome may be unknown"));
        }
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
    let request_elapsed = start.elapsed();
    assert!(
        error.starts_with("transport_timeout:"),
        "{}",
        stalled_close_receipt(&peer, dir.path(), start, request_elapsed, None)
    );
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{}",
        stalled_close_receipt(&peer, dir.path(), start, request_elapsed, None)
    );
    assert!(
        !peer.connected,
        "{}",
        stalled_close_receipt(&peer, dir.path(), start, request_elapsed, None)
    );
    let reap_start = Instant::now();
    let reaped = peer.reaped.recv_timeout(Duration::from_secs(5));
    let reap_wait_elapsed = Some(reap_start.elapsed());
    if !matches!(reaped, Ok(Ok(()))) {
        eprintln!(
            "{}",
            stalled_close_receipt(&peer, dir.path(), start, request_elapsed, reap_wait_elapsed)
        );
    }
    reaped.unwrap().unwrap();
    assert!(
        dir.path().join("eof").exists(),
        "{}",
        stalled_close_receipt(&peer, dir.path(), start, request_elapsed, reap_wait_elapsed)
    );
    let lifecycle = lifecycle_snapshot(&peer).unwrap();
    // EOF can be written immediately before the grace deadline; preserve that
    // accepted case even if termination wins the subsequent exit race.
    assert!(
        matches!(
            (lifecycle.termination, lifecycle.reap),
            (
                TerminationObservation::NotAttempted,
                ReapObservation::ReapedByTryWait
            ) | (
                TerminationObservation::Succeeded,
                ReapObservation::ReapedByWait
            )
        ),
        "{lifecycle:?}"
    );
    assert!(!matches!(
        lifecycle.exit,
        ExitObservation::Pending | ExitObservation::Unavailable
    ));
    assert_eq!(lifecycle.cleanup_error, None);
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
    let lifecycle = lifecycle_snapshot(&peer).unwrap();
    assert_eq!(lifecycle.termination, TerminationObservation::Succeeded);
    assert_eq!(lifecycle.reap, ReapObservation::ReapedByWait);
    assert!(!matches!(
        lifecycle.exit,
        ExitObservation::Pending | ExitObservation::Unavailable
    ));
    assert_eq!(lifecycle.cleanup_error, None);
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
        lifecycle: Arc::new(Mutex::new(ProcessLifecycle::default())),
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
    let lifecycle = owned.lifecycle.clone();
    drop(owned);
    let lifecycle = *lifecycle.lock().unwrap();
    assert_eq!(lifecycle.termination, TerminationObservation::NotAttempted);
    assert_eq!(lifecycle.reap, ReapObservation::UnavailableTryWait);
    assert_eq!(lifecycle.exit, ExitObservation::Unavailable);
    assert_eq!(lifecycle.cleanup_error, Some(ReapError::TryWait));
}

fn agent_info(capabilities: &[&str]) -> cedar_protocol::AgentInfo {
    cedar_protocol::AgentInfo {
        schema: cedar_protocol::AGENT_INFO_SCHEMA,
        version: "fixture-agent-6".into(),
        os: "fixture_os".into(),
        arch: "fixture_arch".into(),
        capabilities: capabilities.iter().map(|name| (*name).into()).collect(),
        capability_groups: Vec::new(),
    }
}
fn grouped_agent_info(capabilities: &[&str], groups: &[&str]) -> cedar_protocol::AgentInfo {
    let mut agent = agent_info(capabilities);
    agent.capability_groups = groups.iter().map(|name| (*name).into()).collect();
    agent
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
        Operation::LanguageOrganizeJavaImports {
            path: "fixture.txt".into(),
            version: 1,
        },
        Operation::LanguageRefreshJavaDiagnostics {
            path: "fixture.txt".into(),
            version: 1,
        },
        Operation::LanguageJavaImplementations {
            path: "fixture.java".into(),
            version: 1,
            line: 0,
            character: 0,
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
        Operation::LanguageWorkspaceSymbols {
            query: "Fixture".into(),
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
        Some(grouped_agent_info(
            &["list", "read", "unknown_future_feature"],
            &[JAVA_MAVEN_LEAF_GROUP, "unknown_future_group"],
        )),
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
    assert_eq!(
        snapshot["agent"]["capability_groups"],
        serde_json::json!([JAVA_MAVEN_LEAF_GROUP, "unknown_future_group"])
    );
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
        for operation in advanced_operations().into_iter().chain([
            begin_maven_language(),
            Operation::LanguageMavenModel,
            maven_dependencies_operation(),
        ]) {
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
            "language_organize_java_imports",
            "language_java_implementations",
            "language_query",
            "language_format",
            "language_references",
            "language_document_symbols",
            "language_workspace_symbols",
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

#[test]
fn java_imports_requires_optional_bridge_and_complete_typed_java_lifecycle() {
    let organize = Operation::LanguageOrganizeJavaImports {
        path: "Hello.java".into(),
        version: 7,
    };
    for missing in std::iter::once(&"language_organize_java_imports")
        .chain(JAVA_LANGUAGE_SESSION_CAPABILITIES.iter())
    {
        let capabilities: Vec<_> = java_capabilities()
            .into_iter()
            .chain(["language_start"])
            .filter(|capability| capability != missing)
            .collect();
        let directory = tempfile::tempdir().unwrap();
        let mut client = capability_client(directory.path(), Some(agent_info(&capabilities)));
        let error = client.request(organize.clone()).unwrap_err();
        assert!(error.starts_with("unsupported_operation:"), "{error}");
        assert!(error.contains(missing), "{error}");
        assert_eq!(recorded_requests(directory.path()).len(), 1);
        read_fixture(&mut client);
        assert_eq!(recorded_requests(directory.path()).len(), 2);
    }
    let capabilities: Vec<_> = java_capabilities()
        .into_iter()
        .filter(|capability| *capability != "language_organize_java_imports")
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let mut client = capability_client(directory.path(), Some(agent_info(&capabilities)));
    client.request(start_java_language()).unwrap();
    client.request(Operation::LanguageEvents).unwrap();
    assert!(client
        .request(organize)
        .unwrap_err()
        .contains("language_organize_java_imports"));
    client.request(Operation::LanguageStop).unwrap();
    assert_eq!(recorded_requests(directory.path()).len(), 4);
}

#[test]
fn java_imports_normalized_preview_and_error_are_returned_without_retry_or_extra_requests() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = capability_client(directory.path(), Some(agent_info(&java_capabilities())));
    client.request(start_java_language()).unwrap();
    let edits = serde_json::json!([{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"newText":"import java.util.List;\n"}]);
    let organize = Operation::LanguageOrganizeJavaImports {
        path: "Hello #.java".into(),
        version: 7,
    };
    assert_eq!(
        process(&mut client).request_timeout(&organize),
        Duration::from_secs(75)
    );
    next_result(
        directory.path(),
        &mut client,
        serde_json::json!({"Ok":{"type":"language","value":edits}}),
    );
    assert!(
        matches!(client.request(organize.clone()).unwrap(), Payload::Language { value } if value == edits)
    );
    assert!(process(&mut client).java_language_session);
    let requests = recorded_requests(directory.path());
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[2]["op"],
        serde_json::json!({"type":"language_organize_java_imports","path":"Hello #.java","version":7})
    );
    next_result(
        directory.path(),
        &mut client,
        serde_json::json!({"Err":{"code":"language_imports_invalid_edit","message":"different document"}}),
    );
    assert!(client
        .request(organize)
        .unwrap_err()
        .starts_with("language_imports_invalid_edit:"));
    read_fixture(&mut client);
    assert_eq!(recorded_requests(directory.path()).len(), 5);
}

fn process(client: &mut Client) -> &mut ProcessClient {
    match &mut client.backend {
        Backend::Process(process) => process,
        #[cfg(not(windows))]
        Backend::Local(_) => panic!("expected process transport"),
    }
}

#[test]
fn java_implementations_requires_optional_bridge_and_complete_typed_java_lifecycle() {
    let implementations = Operation::LanguageJavaImplementations {
        path: "Hello.java".into(),
        version: 7,
        line: 2,
        character: 3,
    };
    for missing in std::iter::once(&"language_java_implementations")
        .chain(JAVA_LANGUAGE_SESSION_CAPABILITIES.iter())
    {
        let capabilities: Vec<_> = java_capabilities()
            .into_iter()
            .chain(["language_start"])
            .filter(|capability| capability != missing)
            .collect();
        let directory = tempfile::tempdir().unwrap();
        let mut client = capability_client(directory.path(), Some(agent_info(&capabilities)));
        let error = client.request(implementations.clone()).unwrap_err();
        assert!(error.starts_with("unsupported_operation:"), "{error}");
        assert!(error.contains(missing), "{error}");
        assert_eq!(recorded_requests(directory.path()).len(), 1);
        read_fixture(&mut client);
        assert_eq!(recorded_requests(directory.path()).len(), 2);
    }
    let capabilities: Vec<_> = java_capabilities()
        .into_iter()
        .filter(|capability| *capability != "language_java_implementations")
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let mut client = capability_client(directory.path(), Some(agent_info(&capabilities)));
    client.request(start_java_language()).unwrap();
    client.request(Operation::LanguageEvents).unwrap();
    assert!(client
        .request(implementations)
        .unwrap_err()
        .contains("language_java_implementations"));
    client.request(Operation::LanguageStop).unwrap();
    assert_eq!(recorded_requests(directory.path()).len(), 4);
}

#[test]
fn java_implementations_preserves_wire_reply_errors_and_existing_timeout_without_retry() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = capability_client(directory.path(), Some(agent_info(&java_capabilities())));
    client.request(start_java_language()).unwrap();
    let operation = Operation::LanguageJavaImplementations {
        path: "Hello #雪.java".into(),
        version: 7,
        line: 2,
        character: 3,
    };
    assert_eq!(
        process(&mut client).request_timeout(&operation),
        Duration::from_secs(75)
    );
    let locations = serde_json::json!([{"uri":"file:///C:/Hello.java","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":5}}}]);
    next_result(
        directory.path(),
        &mut client,
        serde_json::json!({"Ok":{"type":"language","value":locations}}),
    );
    assert!(
        matches!(client.request(operation.clone()).unwrap(), Payload::Language { value } if value == locations)
    );
    assert!(process(&mut client).java_language_session);
    let requests = recorded_requests(directory.path());
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[2]["op"],
        serde_json::json!({"type":"language_java_implementations","path":"Hello #雪.java","version":7,"line":2,"character":3})
    );
    next_result(
        directory.path(),
        &mut client,
        serde_json::json!({"Err":{"code":"language_error","message":"implementation provider unavailable"}}),
    );
    assert!(client
        .request(operation)
        .unwrap_err()
        .starts_with("language_error:"));
    read_fixture(&mut client);
    assert_eq!(recorded_requests(directory.path()).len(), 5);
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
        (
            "capability_groups",
            serde_json::Value::Null,
            "transport_read:",
        ),
        (
            "capability_groups",
            serde_json::json!(JAVA_MAVEN_LEAF_GROUP),
            "transport_read:",
        ),
        (
            "capability_groups",
            serde_json::json!([JAVA_MAVEN_LEAF_GROUP, 3]),
            "transport_read:",
        ),
        (
            "capability_groups",
            serde_json::json!([JAVA_MAVEN_LEAF_GROUP, JAVA_MAVEN_LEAF_GROUP]),
            "invalid_agent_info:",
        ),
        (
            "capability_groups",
            serde_json::json!([
                JAVA_MAVEN_LEAF_GROUP,
                JAVA_MAVEN_DEPENDENCIES_GROUP,
                "future_v1"
            ]),
            "invalid_agent_info:",
        ),
        (
            "capability_groups",
            serde_json::json!([""]),
            "invalid_agent_info:",
        ),
        (
            "capability_groups",
            serde_json::json!(["Java_maven_leaf_v1"]),
            "invalid_agent_info:",
        ),
        (
            "capability_groups",
            serde_json::json!(["java_maven_leaf_v1\n"]),
            "invalid_agent_info:",
        ),
        (
            "capability_groups",
            serde_json::json!(["g".repeat(65)]),
            "invalid_agent_info:",
        ),
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

fn begin_maven_language() -> Operation {
    Operation::LanguageStartJavaMavenBegin {
        java_executable: "/synthetic/java.exe".into(),
        distribution: "/synthetic/jdt 雪".into(),
        data_directory: "/synthetic/data".into(),
        local_repository: "/synthetic/cache 雪".into(),
    }
}
fn maven_capabilities() -> Vec<&'static str> {
    async_java_capabilities()
        .into_iter()
        .chain(JAVA_MAVEN_CAPABILITIES.iter().copied())
        .collect()
}

// Both representations use the same controlled, side-effect-free peer: the
// frozen Windows 0.37 direct claims and Linux's current grouped representation.
fn maven_agent_variants(include_dependencies: bool) -> [cedar_protocol::AgentInfo; 2] {
    let mut direct = agent_info(&maven_capabilities());
    direct.version = "0.37.0".into();
    direct.os = "windows".into();
    direct.arch = "x86_64".into();
    let mut grouped = grouped_agent_info(&async_java_capabilities(), &[JAVA_MAVEN_LEAF_GROUP]);
    if include_dependencies {
        direct
            .capabilities
            .push("language_maven_dependencies".into());
        grouped
            .capability_groups
            .push(JAVA_MAVEN_DEPENDENCIES_GROUP.into());
    }
    [direct, grouped]
}

#[test]
fn maven_requires_separate_capabilities_and_full_owned_java_lifecycle_without_fallback() {
    for missing in JAVA_MAVEN_CAPABILITIES
        .iter()
        .chain(JAVA_STARTUP_CAPABILITIES)
        .chain(JAVA_LANGUAGE_SESSION_CAPABILITIES)
    {
        let root = tempfile::tempdir().unwrap();
        let caps: Vec<_> = maven_capabilities()
            .into_iter()
            .filter(|cap| cap != missing)
            .collect();
        let mut client = capability_client(root.path(), Some(agent_info(&caps)));
        for op in [begin_maven_language(), Operation::LanguageMavenModel] {
            let failure = client.request(op).unwrap_err();
            assert!(failure.starts_with("unsupported_operation:"), "{failure}");
            assert!(failure.contains(missing), "{missing}: {failure}");
        }
        assert_eq!(recorded_requests(root.path()).len(), 1);
        read_fixture(&mut client);
    }
    for metadata in [
        None,
        Some(agent_info(&java_capabilities())),
        Some(agent_info(&async_java_capabilities())),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut client = capability_client(root.path(), metadata);
        assert!(client
            .request(begin_maven_language())
            .unwrap_err()
            .contains("language_start_java_maven_begin"));
        assert_eq!(recorded_requests(root.path()).len(), 1);
    }
}

#[test]
fn typed_maven_startup_uses_the_same_owned_id_and_forwards_one_fixed_model_operation() {
    for agent in maven_agent_variants(false) {
        let root = tempfile::tempdir().unwrap();
        let serialized = serde_json::to_value(&agent).unwrap();
        if agent.os == "windows" {
            assert!(serialized.get("capability_groups").is_none());
        }
        let mut client = capability_client(root.path(), Some(agent));
        assert!(client
            .request(maven_dependencies_operation())
            .unwrap_err()
            .contains("language_maven_dependencies"));
        assert_eq!(recorded_requests(root.path()).len(), 1);
        startup_result(
            root.path(),
            &mut client,
            serde_json::json!({"startup_id":1,"state":"starting","process_id":null}),
        );
        client.request(begin_maven_language()).unwrap();
        assert_eq!(process(&mut client).java_startup.pending, Some(1));
        assert!(!process(&mut client).java_language_session);
        startup_result(root.path(), &mut client, startup_ready(1));
        client
            .request(Operation::LanguageStartJavaPoll { startup_id: 1 })
            .unwrap();
        assert_eq!(process(&mut client).java_startup.active, Some(1));
        assert!(process(&mut client).java_language_session);
        let result =
            serde_json::json!({"profile":"maven_leaf","status":"unresolved","pom_path":"pom.xml"});
        startup_result(root.path(), &mut client, result.clone());
        assert!(
            matches!(client.request(Operation::LanguageMavenModel).unwrap(), Payload::Language { value } if value == result)
        );
        let requests = recorded_requests(root.path());
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[1]["op"]["type"], "language_start_java_maven_begin");
        assert_eq!(
            requests[3]["op"],
            serde_json::json!({"type":"language_maven_model"})
        );
        startup_result(
            root.path(),
            &mut client,
            serde_json::json!({"startup_id":1,"state":"cancelled","cleanup_verified":true}),
        );
        client
            .request(Operation::LanguageStartJavaCancel { startup_id: 1 })
            .unwrap();
        assert!(!process(&mut client).java_language_session);
        assert_eq!(process(&mut client).java_startup.active, None);
    }
}

fn maven_dependencies_operation() -> Operation {
    Operation::LanguageMavenDependencies {
        startup_id: 7,
        pom_sha256: "a".repeat(64),
    }
}

#[test]
fn maven_dependencies_require_optional_and_complete_lifecycle_without_wire_fallback() {
    let capability = "language_maven_dependencies";
    let complete: Vec<_> = maven_capabilities()
        .into_iter()
        .chain([capability])
        .collect();
    for missing in std::iter::once(&capability)
        .chain(JAVA_MAVEN_CAPABILITIES)
        .chain(JAVA_STARTUP_CAPABILITIES)
        .chain(JAVA_LANGUAGE_SESSION_CAPABILITIES)
    {
        let root = tempfile::tempdir().unwrap();
        let capabilities: Vec<_> = complete
            .iter()
            .copied()
            .filter(|name| name != missing)
            .collect();
        let mut client = capability_client(root.path(), Some(agent_info(&capabilities)));
        let error = client.request(maven_dependencies_operation()).unwrap_err();
        assert!(error.starts_with("unsupported_operation:"), "{error}");
        assert!(error.contains(missing), "{missing}: {error}");
        assert_eq!(recorded_requests(root.path()).len(), 1);
        read_fixture(&mut client);
    }
    let root = tempfile::tempdir().unwrap();
    let mut legacy = capability_client(root.path(), None);
    assert!(legacy
        .request(maven_dependencies_operation())
        .unwrap_err()
        .contains(capability));
    assert_eq!(recorded_requests(root.path()).len(), 1);
}

#[test]
fn maven_dependencies_forward_exact_identity_and_typed_snapshot_once() {
    for agent in maven_agent_variants(true) {
        let root = tempfile::tempdir().unwrap();
        let serialized = serde_json::to_value(&agent).unwrap();
        if agent.os == "windows" {
            assert!(serialized.get("capability_groups").is_none());
        }
        let mut client = capability_client(root.path(), Some(agent));
        let snapshot = serde_json::json!({
            "schema":1,"profile":"maven_leaf","startup_id":7,"pom_path":"pom.xml",
            "pom_sha256":"a".repeat(64),"declarations":[],
            "observation":{"status":"unavailable","reason":"model_unavailable"}
        });
        next_result(
            root.path(),
            &mut client,
            serde_json::json!({"Ok": {
                "type":"maven_dependencies", "snapshot":snapshot
            }}),
        );
        let Payload::MavenDependencies { snapshot: actual } =
            client.request(maven_dependencies_operation()).unwrap()
        else {
            panic!("typed dependency snapshot expected");
        };
        assert_eq!(serde_json::to_value(actual).unwrap(), snapshot);
        let requests = recorded_requests(root.path());
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1]["op"],
            serde_json::json!({
                "type":"language_maven_dependencies","startup_id":7,"pom_sha256":"a".repeat(64)
            })
        );
        assert!(is_language_session_operation(
            &maven_dependencies_operation()
        ));
        process(&mut client).java_language_session = true;
        assert_eq!(
            process(&mut client).request_timeout(&maven_dependencies_operation()),
            JAVA_LANGUAGE_REQUEST_TIMEOUT
        );
    }
}

#[test]
fn grouped_maven_claims_still_require_every_owned_java_and_maven_prerequisite_before_wire() {
    for missing in JAVA_MAVEN_CAPABILITIES
        .iter()
        .chain(JAVA_STARTUP_CAPABILITIES)
        .chain(JAVA_LANGUAGE_SESSION_CAPABILITIES)
    {
        let root = tempfile::tempdir().unwrap();
        let mut agent = maven_agent_variants(true)[1].clone();
        agent.capabilities.retain(|name| name.as_str() != *missing);
        if JAVA_MAVEN_CAPABILITIES.contains(missing) {
            // Removing one core operation requires a partial direct claim:
            // the leaf group is indivisible and must not hide the missing half.
            agent
                .capability_groups
                .retain(|name| name != JAVA_MAVEN_LEAF_GROUP);
            agent.capabilities.extend(
                JAVA_MAVEN_CAPABILITIES
                    .iter()
                    .filter(|name| *name != missing)
                    .map(|name| (*name).to_owned()),
            );
        }
        let mut client = capability_client(root.path(), Some(agent));
        for operation in [
            begin_maven_language(),
            Operation::LanguageMavenModel,
            maven_dependencies_operation(),
        ] {
            let error = client.request(operation).unwrap_err();
            assert!(
                error.starts_with("unsupported_operation:"),
                "{missing}: {error}"
            );
            assert!(error.contains(missing), "{missing}: {error}");
            assert!(client.is_connected());
        }
        assert_eq!(recorded_requests(root.path()).len(), 1, "{missing}");
        assert_eq!(process(&mut client).next_id, 1, "{missing}");
        assert_eq!(process(&mut client).java_startup.pending, None);
        assert!(!process(&mut client).java_language_session);
        read_fixture(&mut client);
        let requests = recorded_requests(root.path());
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["op"]["type"], "read");
        assert_eq!(requests[1]["id"], 2);
    }
}

#[test]
fn dependency_only_and_unknown_groups_cannot_launch_or_expand_other_operation_families() {
    for groups in [
        vec![JAVA_MAVEN_DEPENDENCIES_GROUP],
        vec!["java_maven_leaf_v2", "java_maven_dependencies_v2"],
        vec!["java_maven_leaf_v1_extra", "unknown_future_group"],
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut client = capability_client(
            root.path(),
            Some(grouped_agent_info(&async_java_capabilities(), &groups)),
        );
        for operation in [
            begin_maven_language(),
            Operation::LanguageMavenModel,
            maven_dependencies_operation(),
            start_language(),
            start_task(),
        ] {
            let error = client.request(operation).unwrap_err();
            assert!(
                error.starts_with("unsupported_operation:"),
                "{groups:?}: {error}"
            );
        }
        assert_eq!(recorded_requests(root.path()).len(), 1);
        assert!(client.is_connected());
        read_fixture(&mut client);
        assert_eq!(recorded_requests(root.path()).len(), 2);
    }
    // Recognized groups still supply only their narrow Maven operations.
    let root = tempfile::tempdir().unwrap();
    let mut client = capability_client(
        root.path(),
        Some(grouped_agent_info(
            &["list", "read"],
            &[JAVA_MAVEN_LEAF_GROUP, JAVA_MAVEN_DEPENDENCIES_GROUP],
        )),
    );
    for operation in advanced_operations().into_iter().chain([
        begin_maven_language(),
        Operation::LanguageMavenModel,
        maven_dependencies_operation(),
    ]) {
        assert!(client
            .request(operation)
            .unwrap_err()
            .starts_with("unsupported_operation:"));
    }
    assert_eq!(recorded_requests(root.path()).len(), 1);
    read_fixture(&mut client);
}

#[test]
fn late_grouped_hello_reply_never_replaces_the_first_capability_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let mut client = capability_client(root.path(), Some(agent_info(&async_java_capabilities())));
    let snapshot = serde_json::to_value(client.handshake()).unwrap();
    let late = serde_json::json!({
        "type": "hello", "protocol": 4, "root": "/changed-after-connect",
        "agent": maven_agent_variants(true)[1],
    });
    next_result(root.path(), &mut client, serde_json::json!({"Ok": late}));
    assert!(matches!(
        client
            .request(Operation::Read {
                path: "fixture.txt".into()
            })
            .unwrap(),
        Payload::Hello { .. }
    ));
    for operation in [
        begin_maven_language(),
        Operation::LanguageMavenModel,
        maven_dependencies_operation(),
    ] {
        assert!(client
            .request(operation)
            .unwrap_err()
            .starts_with("unsupported_operation:"));
    }
    assert_eq!(serde_json::to_value(client.handshake()).unwrap(), snapshot);
    assert_eq!(
        serde_json::to_value(client.request(Operation::Hello).unwrap()).unwrap(),
        snapshot
    );
    assert_eq!(recorded_requests(root.path()).len(), 2);
    read_fixture(&mut client);
    let requests = recorded_requests(root.path());
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0]["op"]["type"], "hello");
    assert_eq!(requests[1]["op"]["type"], "read");
    assert_eq!(requests[2]["op"]["type"], "read");
}

#[test]
fn unsolicited_grouped_hello_closes_without_discovery_or_capability_upgrade() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("hello.json"),
        serde_json::json!({
            "type": "hello", "protocol": 4, "root": "/first",
            "agent": agent_info(&async_java_capabilities()),
        })
        .to_string(),
    )
    .unwrap();
    let mut client = Client::from_process(spawn("idle_unsolicited", root.path())).unwrap();
    let snapshot = serde_json::to_value(client.handshake()).unwrap();
    let wake = register_idle_wake(&mut client);
    next_result(
        root.path(),
        &mut client,
        serde_json::json!({"Ok": {
            "type": "hello", "protocol": 4, "root": "/late",
            "agent": maven_agent_variants(true)[1],
        }}),
    );
    fs::write(root.path().join("release-1"), b"release unsolicited Hello").unwrap();
    wake.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(client
        .observe_idle_transport()
        .unwrap_err()
        .starts_with("protocol_error: unsolicited response id 2"));
    assert_eq!(serde_json::to_value(client.handshake()).unwrap(), snapshot);
    for operation in [
        Operation::Hello,
        begin_maven_language(),
        maven_dependencies_operation(),
    ] {
        assert!(client
            .request(operation)
            .unwrap_err()
            .starts_with("disconnected:"));
    }
    client.close_and_wait(Duration::from_secs(5)).unwrap();
    let requests = recorded_requests(root.path());
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["op"]["type"], "hello");
}

#[test]
fn reconnect_drops_grouped_support_and_owned_startup_identity_without_replay() {
    let root = tempfile::tempdir().unwrap();
    let mut client = capability_client(root.path(), Some(maven_agent_variants(true)[1].clone()));
    startup_result(
        root.path(),
        &mut client,
        serde_json::json!({
            "startup_id": 7, "state": "starting", "process_id": null,
        }),
    );
    client.request(begin_maven_language()).unwrap();
    startup_result(root.path(), &mut client, startup_ready(7));
    client
        .request(Operation::LanguageStartJavaPoll { startup_id: 7 })
        .unwrap();
    assert_eq!(process(&mut client).java_startup.active, Some(7));
    next_result(root.path(), &mut client, serde_json::Value::Null);
    assert!(client
        .request(Operation::LanguageEvents)
        .unwrap_err()
        .starts_with("transport_read:"));
    assert!(!client.is_connected());
    assert!(!process(&mut client).java_language_session);
    assert_eq!(process(&mut client).java_startup.pending, None);
    assert_eq!(process(&mut client).java_startup.active, None);
    client.close_and_wait(Duration::from_secs(5)).unwrap();
    assert_eq!(recorded_requests(root.path()).len(), 4);

    let fresh_root = tempfile::tempdir().unwrap();
    let mut fresh = capability_client(
        fresh_root.path(),
        Some(agent_info(&async_java_capabilities())),
    );
    assert!(!process(&mut fresh).java_language_session);
    assert_eq!(process(&mut fresh).java_startup.pending, None);
    assert_eq!(process(&mut fresh).java_startup.active, None);
    assert_eq!(
        process(&mut fresh).request_timeout(&Operation::LanguageEvents),
        Duration::from_secs(30)
    );
    let Payload::Hello {
        agent: Some(agent), ..
    } = fresh.handshake()
    else {
        panic!("new connection metadata expected");
    };
    assert!(agent.capability_groups.is_empty());
    for operation in [
        begin_maven_language(),
        Operation::LanguageMavenModel,
        maven_dependencies_operation(),
    ] {
        assert!(fresh
            .request(operation)
            .unwrap_err()
            .starts_with("unsupported_operation:"));
    }
    assert_eq!(recorded_requests(fresh_root.path()).len(), 1);
    read_fixture(&mut fresh);
    let requests = recorded_requests(fresh_root.path());
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["id"], 1);
    assert_eq!(requests[1]["id"], 2);
    assert_eq!(requests[1]["op"]["type"], "read");
}

#[test]
fn old_linux_flat_inventory_keeps_typed_java_without_admitting_maven_or_discovery() {
    // Frozen 0.37 Linux isolated-agent wire inventory, independent of current
    // inventory generation. An empty groups field is equivalent to omission.
    let capabilities = [
        "list",
        "read",
        "write",
        "search",
        "git_status",
        "run",
        "git_changes",
        "git_diff",
        "run_start",
        "run_poll",
        "run_cancel",
        "language_start",
        "language_start_java",
        "language_start_java_begin",
        "language_start_java_poll",
        "language_start_java_cancel",
        "java_diagnostics_refresh",
        "language_organize_java_imports",
        "language_java_implementations",
        "language_open",
        "language_change",
        "language_close",
        "language_query",
        "language_format",
        "language_references",
        "language_document_symbols",
        "language_workspace_symbols",
        "language_resolve_uri",
        "language_resolve_completion",
        "language_events",
        "language_stop",
    ];
    assert_eq!(capabilities.len(), 31);
    for explicit_empty in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut hello = serde_json::json!({
            "type": "hello", "protocol": 4, "root": "/old-linux",
            "agent": {
                "schema": 1, "version": "0.37.0", "os": "linux", "arch": "x86_64",
                "capabilities": capabilities,
            },
        });
        if explicit_empty {
            hello["agent"]["capability_groups"] = serde_json::json!([]);
        }
        let mut client = Client::from_process(capability_peer(root.path(), hello)).unwrap();
        let snapshot = serde_json::to_value(client.handshake()).unwrap();
        let Payload::Hello {
            agent: Some(agent), ..
        } = client.handshake()
        else {
            panic!("old Linux metadata expected");
        };
        assert_eq!(agent.capabilities.len(), 31);
        assert!(agent.capability_groups.is_empty());
        for capability in capabilities {
            assert!(agent.supports(capability), "{capability}");
        }
        for operation in [
            begin_maven_language(),
            Operation::LanguageMavenModel,
            maven_dependencies_operation(),
        ] {
            assert!(client
                .request(operation)
                .unwrap_err()
                .starts_with("unsupported_operation:"));
        }
        assert_eq!(recorded_requests(root.path()).len(), 1);
        startup_result(
            root.path(),
            &mut client,
            serde_json::json!({
                "startup_id": 1, "state": "starting", "process_id": null,
            }),
        );
        client.request(begin_java_language()).unwrap();
        read_fixture(&mut client);
        startup_result(
            root.path(),
            &mut client,
            serde_json::json!({
                "startup_id": 1, "state": "cancelled", "cleanup_verified": true,
            }),
        );
        client
            .request(Operation::LanguageStartJavaCancel { startup_id: 1 })
            .unwrap();
        assert_eq!(
            serde_json::to_value(client.request(Operation::Hello).unwrap()).unwrap(),
            snapshot
        );
        let requests = recorded_requests(root.path());
        let operations: Vec<_> = requests
            .iter()
            .map(|request| request["op"]["type"].as_str().unwrap())
            .collect();
        assert_eq!(
            operations,
            [
                "hello",
                "language_start_java_begin",
                "read",
                "language_start_java_cancel"
            ]
        );
    }
}

#[test]
fn duplicate_group_fields_are_rejected_on_the_wire_without_legacy_fallback() {
    let root = tempfile::tempdir().unwrap();
    let agent = serde_json::to_string(&agent_info(&["list", "read"])).unwrap();
    let agent = agent.strip_suffix('}').unwrap();
    let hello = format!(
        "{{\"type\":\"hello\",\"protocol\":4,\"root\":\"/fixture\",\"agent\":{agent},\"capability_groups\":[\"{JAVA_MAVEN_LEAF_GROUP}\"],\"capability_groups\":[]}}}}"
    );
    fs::write(root.path().join("hello.json"), hello).unwrap();
    let error = match Client::from_process(spawn("capability_peer", root.path())) {
        Ok(_) => panic!("accepted duplicate capability_groups fields"),
        Err(error) => error,
    };
    assert!(error.starts_with("transport_read:"), "{error}");
    wait_until(|| root.path().join("eof").exists());
    assert_eq!(recorded_requests(root.path()).len(), 1);
}
