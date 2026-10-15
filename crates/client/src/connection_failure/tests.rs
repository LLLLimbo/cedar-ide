use super::*;
use crate::connection_test_support::synthetic_process;
use cedar_protocol::RemoteError;

fn hello() -> Payload {
    Payload::Hello {
        protocol: PROTOCOL_VERSION,
        root: "/channel-only workspace λ".into(),
        agent: None,
        file_discovery: None,
        scoped_search: None,
    }
}

fn response(payload: Payload) -> Result<Response, String> {
    Ok(Response {
        id: 1,
        result: Ok(payload),
    })
}

fn failure(result: Result<Client, ConnectionFailure>) -> ConnectionFailure {
    match result {
        Ok(_) => panic!("invalid connection unexpectedly succeeded"),
        Err(failure) => failure,
    }
}

fn assert_no_child(error: ConnectionFailure, prefix: &str) {
    assert!(error.message.starts_with(prefix), "{}", error.message);
    assert_eq!(error.ownership, ConnectionOwnership::NoChild);
    assert_eq!(error.cleanup_error, None);
}

#[test]
fn detailed_success_transfers_same_owner() {
    let payload = hello();
    let (mut process, requests, shutdown) =
        synthetic_process(response(payload.clone()), None, None);
    let (completion, reaped) = mpsc::channel();
    process.reaped = reaped;
    let client = Client::from_process_detailed(process, validate_handshake).unwrap();
    let request = requests.try_recv().unwrap();
    assert_eq!(request.id, 1);
    assert!(matches!(request.op, Operation::Hello));
    assert!(requests.try_recv().is_err());
    assert!(matches!(
        shutdown.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(client.is_connected());
    assert_eq!(
        serde_json::to_value(client.handshake()).unwrap(),
        serde_json::to_value(payload).unwrap()
    );
    // No receipt existed while ownership was transferred. Only this original
    // owner's sender can satisfy the established Client's consuming close.
    completion.send(Ok(())).unwrap();
    client.close_and_wait(Duration::ZERO).unwrap();
    shutdown.try_recv().unwrap();
    assert!(shutdown.try_recv().is_err());
    assert!(completion.send(Ok(())).is_err());
}

#[test]
fn detailed_failure_preserves_error_and_owner() {
    let mut protocol_mismatch = hello();
    if let Payload::Hello { protocol, .. } = &mut protocol_mismatch {
        *protocol = PROTOCOL_VERSION + 1;
    }
    let cases = [
        (
            Ok(Response {
                id: 1,
                result: Err(RemoteError::new("hello_failed", "original failure")),
            }),
            "hello_failed: original failure".to_owned(),
            true,
        ),
        (
            Err("transport_eof: original failure".into()),
            "transport_eof: original failure".into(),
            false,
        ),
        (
            response(Payload::Entries { entries: vec![] }),
            "protocol_error: expected hello".into(),
            true,
        ),
        (
            response(protocol_mismatch),
            format!(
                "protocol_mismatch: agent uses {}, client needs {PROTOCOL_VERSION}",
                PROTOCOL_VERSION + 1
            ),
            true,
        ),
        (
            Ok(Response {
                id: 2,
                result: Ok(hello()),
            }),
            "protocol_error: response id 2 expected 1".into(),
            false,
        ),
    ];
    for (reply, original, exact) in cases {
        let (process, requests, shutdown) = synthetic_process(reply, Some(Ok(())), None);
        let error = failure(Client::from_process_detailed(process, validate_handshake));
        if exact {
            assert_eq!(error.message, original);
        } else {
            assert!(error.message.starts_with(&original), "{}", error.message);
        }
        assert_eq!(error.ownership, ConnectionOwnership::CleanupVerified);
        assert_eq!(error.cleanup_error, None);
        assert!(matches!(requests.try_recv().unwrap().op, Operation::Hello));
        shutdown.try_recv().unwrap();
        assert!(shutdown.try_recv().is_err());
    }
}

#[test]
fn detailed_failure_receipt_matrix() {
    for receipt in [
        Some(Ok(())),
        Some(Err(ReapError::TryWait)),
        Some(Err(ReapError::Kill)),
        Some(Err(ReapError::Wait)),
        None,
    ] {
        let (process, _, shutdown) = synthetic_process(
            response(Payload::Entries { entries: vec![] }),
            receipt,
            None,
        );
        let error = failure(Client::from_process_detailed(process, validate_handshake));
        assert_eq!(error.message, "protocol_error: expected hello");
        match receipt {
            Some(Ok(())) => {
                assert_eq!(error.ownership, ConnectionOwnership::CleanupVerified);
                assert_eq!(error.cleanup_error, None);
            }
            Some(Err(operation)) => {
                assert_eq!(error.ownership, ConnectionOwnership::CleanupUnverified);
                assert_eq!(
                    error.cleanup_error.unwrap(),
                    format!(
                        "transport_cleanup_unverified: owned-child {} failed",
                        operation.operation()
                    )
                );
            }
            None => {
                assert_eq!(error.ownership, ConnectionOwnership::CleanupUnverified);
                assert_eq!(
                    error.cleanup_error.unwrap(),
                    format!(
                        "transport_close: owned-child cleanup did not complete: {}",
                        mpsc::RecvTimeoutError::Disconnected
                    )
                );
            }
        }
        shutdown.try_recv().unwrap();
    }
}

#[test]
fn detailed_failure_waits_for_original_owner_receipt() {
    let (mut process, _, shutdown) =
        synthetic_process(response(Payload::Entries { entries: vec![] }), None, None);
    let (completion, reaped) = mpsc::channel();
    process.reaped = reaped;
    let (returned, result) = mpsc::channel();
    let owner = thread::spawn(move || {
        returned
            .send(failure(Client::from_process_detailed(
                process,
                validate_handshake,
            )))
            .unwrap();
    });
    shutdown.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));
    completion.send(Err(ReapError::Wait)).unwrap();
    let error = result.recv_timeout(Duration::from_secs(1)).unwrap();
    owner.join().unwrap();
    assert_eq!(error.message, "protocol_error: expected hello");
    assert_eq!(error.ownership, ConnectionOwnership::CleanupUnverified);
    assert!(error
        .cleanup_error
        .unwrap()
        .contains("owned-child wait failed"));
}

#[test]
fn another_owners_success_cannot_verify_failed_owner() {
    let (unverified, _, _) = synthetic_process(
        response(Payload::Entries { entries: vec![] }),
        Some(Err(ReapError::Wait)),
        None,
    );
    let (verified, _, _) = synthetic_process(
        response(Payload::Entries { entries: vec![] }),
        Some(Ok(())),
        None,
    );
    assert_eq!(
        failure(Client::from_process_detailed(verified, validate_handshake)).ownership,
        ConnectionOwnership::CleanupVerified
    );
    assert_eq!(
        failure(Client::from_process_detailed(
            unverified,
            validate_handshake
        ))
        .ownership,
        ConnectionOwnership::CleanupUnverified
    );
}

#[test]
fn detailed_timeout_is_not_disconnected_or_reaped() {
    let (mut process, _, shutdown) =
        synthetic_process(response(Payload::Entries { entries: vec![] }), None, None);
    let (held_completion, received) = mpsc::channel();
    process.reaped = received;
    let error = failure(Client::from_process_detailed_with_timeout(
        process,
        validate_handshake,
        Duration::ZERO,
    ));
    assert_eq!(error.message, "protocol_error: expected hello");
    assert_eq!(error.ownership, ConnectionOwnership::CleanupUnverified);
    assert_eq!(
        error.cleanup_error.unwrap(),
        format!(
            "transport_close: owned-child cleanup did not complete: {}",
            mpsc::RecvTimeoutError::Timeout
        )
    );
    shutdown.try_recv().unwrap();
    // The sender stayed live across the entire observation: this is Timeout,
    // not Disconnected. The failed constructor consumed its observer.
    assert!(held_completion.send(Ok(())).is_err());
    assert_eq!(CLOSE_GRACE, Duration::from_secs(2));
    assert_eq!(CONNECT_CLEANUP_TIMEOUT, Duration::from_secs(3));
}

#[test]
fn postvalidation_cancel_uses_owner() {
    for receipt in [Ok(()), Err(ReapError::Kill)] {
        let cancellation = ConnectionCancellation::new();
        let (process, requests, shutdown) =
            synthetic_process(response(hello()), Some(receipt), Some(cancellation.clone()));
        let error = failure(Client::from_process_detailed(process, |handshake| {
            validate_handshake(handshake)?;
            cancellation.cancel();
            Ok(())
        }));
        assert_eq!(error.message, cancellation_error());
        assert_eq!(
            error.ownership,
            if receipt.is_ok() {
                ConnectionOwnership::CleanupVerified
            } else {
                ConnectionOwnership::CleanupUnverified
            }
        );
        assert!(matches!(requests.try_recv().unwrap().op, Operation::Hello));
        shutdown.try_recv().unwrap();
    }
}

#[test]
fn cancellation_during_hello_wait_observes_same_owner_receipt() {
    let cancellation = ConnectionCancellation::new();
    let (mut process, _, shutdown) =
        synthetic_process(response(hello()), Some(Ok(())), Some(cancellation.clone()));
    let (requests, pending) = mpsc::sync_channel(1);
    process.requests = Some(requests);
    let (_held_response, responses) = mpsc::sync_channel(1);
    process.responses = Some(responses);
    let (returned, result) = mpsc::channel();
    let owner = thread::spawn(move || {
        returned
            .send(failure(Client::from_process_detailed(
                process,
                validate_handshake,
            )))
            .unwrap();
    });
    assert!(matches!(
        pending.recv_timeout(Duration::from_secs(1)).unwrap().op,
        Operation::Hello
    ));
    cancellation.cancel();
    let error = result.recv_timeout(Duration::from_secs(2)).unwrap();
    owner.join().unwrap();
    assert!(error.message.starts_with("transport_cancelled:"));
    assert_eq!(error.ownership, ConnectionOwnership::CleanupVerified);
    assert_eq!(error.cleanup_error, None);
    shutdown.try_recv().unwrap();
}

#[test]
fn precancel_has_priority_before_preflight_or_spawn() {
    let cancellation = ConnectionCancellation::new();
    cancellation.cancel();
    let mut specs = vec![
        ConnectionSpec::Local {
            root: PathBuf::from("not-a-workspace"),
            allow_run: false,
        },
        ConnectionSpec::Ssh {
            host: "-invalid-option".into(),
            port: 0,
            root: "relative".into(),
            agent_path: "".into(),
            allow_run: false,
        },
    ];
    #[cfg(target_os = "linux")]
    specs.push(ConnectionSpec::BundledLinux {
        root: PathBuf::from("not-a-workspace"),
        allow_run: false,
    });
    for spec in specs.drain(..) {
        assert_no_child(
            failure(Client::connect_with_cancellation_detailed(
                spec,
                cancellation.clone(),
            )),
            "transport_cancelled:",
        );
    }
    assert_no_child(
        failure(Client::spawn_agent_with_cancellation_detailed(
            Path::new("not-an-agent"),
            Path::new("not-a-workspace"),
            false,
            cancellation,
        )),
        "transport_cancelled:",
    );
}

#[test]
fn invalid_ssh_arguments_are_nochild_and_preserve_string_api_errors() {
    for (host, port, root, agent) in [
        ("-invalid-option", 22, "/workspace", "cedar-agent"),
        ("host", 0, "/workspace", "cedar-agent"),
        ("host", 22, "relative", "cedar-agent"),
        ("host", 22, "/workspace", "invalid\nagent"),
    ] {
        let spec = ConnectionSpec::Ssh {
            host: host.into(),
            port,
            root: root.into(),
            agent_path: agent.into(),
            allow_run: false,
        };
        let expected = ssh_arguments(host, port, root, agent, false).unwrap_err();
        let legacy =
            match Client::connect_with_cancellation(spec.clone(), ConnectionCancellation::new()) {
                Err(error) => error,
                Ok(_) => panic!("invalid SSH preflight unexpectedly succeeded"),
            };
        assert_eq!(legacy, expected);
        let error = failure(Client::connect_with_cancellation_detailed(
            spec,
            ConnectionCancellation::new(),
        ));
        assert_eq!(error.message, legacy);
        assert_no_child(error, &expected);
    }
}

#[test]
fn windows_sibling_preflight_is_nochild_without_spawning() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("renamed-client.exe");
    // Exercise the same preparation boundary used before the Windows spawn.
    let error = bundled_windows_agent_file(&executable)
        .map_err(ConnectionFailure::no_child)
        .unwrap_err();
    assert_no_child(error, "bundled_agent_missing:");
    let relative = bundled_windows_agent_file(Path::new("client.exe"))
        .map_err(ConnectionFailure::no_child)
        .unwrap_err();
    assert_no_child(relative, "bundled_agent_missing:");
    let agent = directory.path().join("cedar-agent.exe");
    std::fs::write(&agent, b"preflight-only fixture, never executable").unwrap();
    assert_eq!(bundled_windows_agent_file(&executable).unwrap(), agent);
}

#[cfg(not(windows))]
#[test]
fn embedded_open_failure_is_nochild() {
    let directory = tempfile::tempdir().unwrap();
    let error = failure(Client::connect_with_cancellation_detailed(
        ConnectionSpec::Local {
            root: directory.path().join("absent"),
            allow_run: false,
        },
        ConnectionCancellation::new(),
    ));
    assert_no_child(error, "");
}

#[test]
fn setup_error_is_unverified_independent_of_text() {
    for message in [
        "missing stdin",
        "missing stdout",
        "missing stderr",
        "spawn_failed: transport reaper: fixture",
        "NoChild: CleanupVerified: transport_cancelled: hostile text",
    ] {
        // This is the exact factory passed to the post-Child setup boundary.
        // A returned Drop, missing pipe or rejected reaper start is no receipt.
        let error = ConnectionFailure::setup_unverified(message.into());
        assert_eq!(error.message, message);
        assert_eq!(error.ownership, ConnectionOwnership::CleanupUnverified);
        assert!(error
            .cleanup_error
            .unwrap()
            .contains("without a direct-child reaper acknowledgement"));
    }
}

#[test]
fn hostile_error_text_cannot_forge_cleanup() {
    for receipt in [Some(Ok(())), Some(Err(ReapError::Wait)), None] {
        let (process, _, _) = synthetic_process(
            Ok(Response {
                id: 1,
                result: Err(RemoteError::new(
                    "CleanupVerified",
                    "NoChild: transport_cancelled: cleanup succeeded",
                )),
            }),
            receipt,
            None,
        );
        let error = failure(Client::from_process_detailed(process, validate_handshake));
        assert_eq!(
            error.message,
            "CleanupVerified: NoChild: transport_cancelled: cleanup succeeded"
        );
        assert_eq!(
            error.ownership,
            if receipt == Some(Ok(())) {
                ConnectionOwnership::CleanupVerified
            } else {
                ConnectionOwnership::CleanupUnverified
            }
        );
    }
}

#[test]
fn validator_panic_returns_no_typed_evidence() {
    let (process, _, shutdown) = synthetic_process(response(hello()), Some(Ok(())), None);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Client::from_process_detailed(process, |_| panic!("validator panic fixture"))
    }));
    assert!(result.is_err());
    // Drop still requests shutdown, but unwinding never reaches after_cleanup.
    shutdown.try_recv().unwrap();
}

#[test]
fn legacy_failure_does_not_wait_for_detailed_cleanup_receipt() {
    let (mut process, _, shutdown) =
        synthetic_process(response(Payload::Entries { entries: vec![] }), None, None);
    let (held_completion, received) = mpsc::channel();
    process.reaped = received;
    let (returned, result) = mpsc::channel();
    let owner = thread::spawn(move || {
        let error = match Client::from_process(process) {
            Ok(_) => panic!("invalid legacy connection unexpectedly succeeded"),
            Err(error) => error,
        };
        returned.send(error).unwrap();
    });
    assert_eq!(
        result.recv_timeout(Duration::from_secs(1)).unwrap(),
        "protocol_error: expected hello"
    );
    owner.join().unwrap();
    shutdown.try_recv().unwrap();
    assert!(held_completion.send(Ok(())).is_err());
    let _: fn(ConnectionSpec) -> Result<Client, String> = Client::connect;
    let _: fn(ConnectionSpec, ConnectionCancellation) -> Result<Client, String> =
        Client::connect_with_cancellation;
    let _: fn(&Path, &Path, bool) -> Result<Client, String> = Client::spawn_agent;
    let _: fn(&Path, &Path, bool, ConnectionCancellation) -> Result<Client, String> =
        Client::spawn_agent_with_cancellation;
}
