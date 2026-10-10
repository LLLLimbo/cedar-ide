use super::*;
use cedar_protocol::{AgentInfo, RemoteError, AGENT_INFO_SCHEMA};
use std::os::unix::{ffi::OsStringExt, fs::symlink};

fn valid_header() -> Vec<u8> {
    let mut header = vec![0u8; 64];
    header[..4].copy_from_slice(b"\x7fELF");
    header[4] = if cfg!(target_pointer_width = "64") {
        2
    } else {
        1
    };
    header[5] = if cfg!(target_endian = "little") { 1 } else { 2 };
    header[6] = 1;
    header[16..18].copy_from_slice(&2u16.to_ne_bytes());
    header[18..20].copy_from_slice(&native_elf_machine().unwrap().to_ne_bytes());
    header[20..24].copy_from_slice(&1u32.to_ne_bytes());
    header
}

fn hello(root: &str) -> Payload {
    Payload::Hello {
        protocol: PROTOCOL_VERSION,
        root: root.into(),
        agent: Some(AgentInfo {
            schema: AGENT_INFO_SCHEMA,
            version: env!("CARGO_PKG_VERSION").into(),
            os: "linux".into(),
            arch: std::env::consts::ARCH.into(),
            capabilities: vec!["list".into(), "read".into()],
            capability_groups: vec![],
        }),
    }
}

#[test]
fn resolver_derives_only_fixed_sibling_even_for_renamed_and_deps_executables() {
    for executable in [
        "/opt/cedar/cedar",
        "/opt/spaces and Unicode λ/renamed",
        "/build/deps/test-probe",
    ] {
        let executable = Path::new(executable);
        assert_eq!(
            bundled_linux_agent_path(executable).unwrap(),
            executable.parent().unwrap().join("cedar-agent")
        );
    }
    for executable in ["", "cedar", "bin/cedar", "/"] {
        assert!(bundled_linux_agent_path(Path::new(executable))
            .unwrap_err()
            .starts_with("bundled_agent_missing:"));
    }
}

#[test]
fn elf_preflight_rejects_scripts_wrong_architecture_class_endian_and_header() {
    let native = valid_header();
    assert!(is_native_linux_elf(&native));
    let mut pie = native.clone();
    pie[16..18].copy_from_slice(&3u16.to_ne_bytes());
    assert!(is_native_linux_elf(&pie));
    assert!(!is_native_linux_elf(b"#!/bin/sh\nexit 0\n"));
    assert!(!is_native_linux_elf(&native[..20]));
    for (index, value) in [
        (0, 0),
        (4, 3 - native[4]),
        (5, 3 - native[5]),
        (6, 0),
        (7, 9),
    ] {
        let mut invalid = native.clone();
        invalid[index] = value;
        assert!(!is_native_linux_elf(&invalid), "header byte {index}");
    }
    let mut wrong_arch = native.clone();
    wrong_arch[18..20].copy_from_slice(&(native_elf_machine().unwrap() + 1).to_ne_bytes());
    assert!(!is_native_linux_elf(&wrong_arch));
    let mut relocatable = native.clone();
    relocatable[16..18].copy_from_slice(&1u16.to_ne_bytes());
    assert!(!is_native_linux_elf(&relocatable));
    let mut wrong_version = native;
    wrong_version[20..24].copy_from_slice(&2u32.to_ne_bytes());
    assert!(!is_native_linux_elf(&wrong_version));
}

#[test]
fn file_preflight_requires_existing_regular_nonsymlink_executable_native_file() {
    let directory = tempfile::tempdir().unwrap();
    let agent = directory.path().join("cedar-agent");
    assert!(validate_linux_agent_file(&agent)
        .unwrap_err()
        .starts_with("bundled_agent_missing:"));
    fs::create_dir(&agent).unwrap();
    assert!(validate_linux_agent_file(&agent)
        .unwrap_err()
        .starts_with("bundled_agent_invalid:"));
    fs::remove_dir(&agent).unwrap();
    fs::write(&agent, valid_header()).unwrap();
    fs::set_permissions(&agent, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(validate_linux_agent_file(&agent)
        .unwrap_err()
        .contains("executable permission"));
    // Only test fixtures change mode. Production never repairs or chmods a file.
    fs::set_permissions(&agent, fs::Permissions::from_mode(0o700)).unwrap();
    validate_linux_agent_file(&agent).unwrap();
    let linked = directory.path().join("symlink");
    symlink(&agent, &linked).unwrap();
    assert!(validate_linux_agent_file(&linked)
        .unwrap_err()
        .contains("not a symlink"));
    fs::write(&agent, b"#!/bin/sh\nexit 0\n").unwrap();
    assert!(validate_linux_agent_file(&agent)
        .unwrap_err()
        .starts_with("bundled_agent_invalid:"));
}

#[test]
fn root_preflight_canonicalizes_utf8_and_rejects_missing_file_and_lossy_paths() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace λ");
    fs::create_dir(&root).unwrap();
    let alias = directory.path().join("workspace-alias");
    symlink(&root, &alias).unwrap();
    assert_eq!(
        canonical_linux_root(&alias).unwrap(),
        root.canonicalize().unwrap().to_str().unwrap()
    );
    assert!(canonical_linux_root(&directory.path().join("absent"))
        .unwrap_err()
        .starts_with("invalid_root:"));
    let file = directory.path().join("file");
    fs::write(&file, b"not a root").unwrap();
    assert!(canonical_linux_root(&file)
        .unwrap_err()
        .contains("must be a directory"));
    let lossy = directory
        .path()
        .join(std::ffi::OsString::from_vec(b"workspace-\xff".to_vec()));
    fs::create_dir(&lossy).unwrap();
    assert!(canonical_linux_root(&lossy)
        .unwrap_err()
        .contains("must be valid UTF-8"));
}

#[test]
fn strict_hello_requires_metadata_exact_platform_version_and_root() {
    validate_bundled_linux_handshake(&hello("/workspace"), "/workspace").unwrap();
    assert!(
        validate_bundled_linux_handshake(&Payload::Entries { entries: vec![] }, "/workspace")
            .unwrap_err()
            .starts_with("protocol_error:")
    );
    let mut legacy = hello("/workspace");
    if let Payload::Hello { agent, .. } = &mut legacy {
        *agent = None;
    }
    assert!(validate_bundled_linux_handshake(&legacy, "/workspace")
        .unwrap_err()
        .starts_with("invalid_agent_info:"));
    for key in [
        "old_protocol",
        "new_protocol",
        "schema",
        "version",
        "os",
        "arch",
        "capabilities",
        "root",
    ] {
        let mut invalid = hello("/workspace");
        let Payload::Hello {
            protocol,
            root,
            agent: Some(agent),
        } = &mut invalid
        else {
            unreachable!();
        };
        let expected = match key {
            "old_protocol" => {
                *protocol = 3;
                "protocol_mismatch:"
            }
            "new_protocol" => {
                *protocol = 5;
                "protocol_mismatch:"
            }
            "schema" => {
                agent.schema = 0;
                "invalid_agent_info:"
            }
            "version" => {
                agent.version.push_str("-different");
                "bundled_agent_mismatch:"
            }
            "os" => {
                agent.os = "windows".into();
                "bundled_agent_mismatch:"
            }
            "arch" => {
                agent.arch = "other".into();
                "bundled_agent_mismatch:"
            }
            "capabilities" => {
                agent.capabilities.retain(|cap| cap != "read");
                "unsupported_workspace:"
            }
            "root" => {
                root.push_str("/other");
                "bundled_agent_root_mismatch:"
            }
            _ => unreachable!(),
        };
        let error = validate_bundled_linux_handshake(&invalid, "/workspace").unwrap_err();
        assert!(error.starts_with(expected), "{key}: {error}");
    }
}

// Channel-only seam: no executable override, process creation, or runtime
// fixture dependency is added to the production sibling selector.
fn synthetic_process(
    reply: Result<Response, String>,
    reaped: Option<ReapResult>,
    cancellation: Option<ConnectionCancellation>,
) -> (ProcessClient, mpsc::Receiver<Request>, mpsc::Receiver<()>) {
    let (requests, request_rx) = mpsc::sync_channel(1);
    let (responses, response_rx) = mpsc::sync_channel(1);
    let (observed, observed_rx) = mpsc::channel();
    thread::spawn(move || {
        if let Ok(request) = request_rx.recv() {
            let _ = observed.send(request);
            let _ = responses.send(reply);
        }
    });
    let (shutdown, shutdown_rx) = mpsc::channel();
    let (completion, completion_rx) = mpsc::channel();
    if let Some(result) = reaped {
        completion.send(result).unwrap();
    }
    (
        ProcessClient {
            cancellation,
            requests: Some(requests),
            responses: Some(response_rx),
            observation: Arc::new(TransportObservation::default()),
            shutdown: Some(shutdown),
            reaped: completion_rx,
            stderr: Arc::new(Mutex::new(Vec::new())),
            next_id: 0,
            connected: true,
            java_language_session: false,
            java_startup: JavaStartupMode::default(),
        },
        observed_rx,
        shutdown_rx,
    )
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

#[test]
fn successful_strict_hello_transfers_owner_without_closing_it() {
    let (process, requests, shutdown) =
        synthetic_process(response(hello("/workspace")), Some(Ok(())), None);
    let client = Client::from_bundled_linux_process(process, "/workspace").unwrap();
    assert!(matches!(requests.try_recv().unwrap().op, Operation::Hello));
    assert!(matches!(
        shutdown.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(client.is_connected());
    client.close_and_wait(Duration::ZERO).unwrap();
    shutdown.try_recv().unwrap();
}

#[test]
fn every_postspawn_failure_retains_original_error_and_closes_owned_process() {
    let cancelled = ConnectionCancellation::new();
    cancelled.cancel();
    let cases = [
        (
            response(hello("/wrong-root")),
            None,
            "bundled_agent_root_mismatch:",
        ),
        (Err("transport_eof: fixture".into()), None, "transport_eof:"),
        (
            Ok(Response {
                id: 1,
                result: Err(RemoteError::new("hello_failed", "fixture")),
            }),
            None,
            "hello_failed:",
        ),
        (
            response(hello("/workspace")),
            Some(cancelled),
            "transport_cancelled:",
        ),
    ];
    for (reply, cancellation, expected) in cases {
        let (process, _requests, shutdown) = synthetic_process(reply, Some(Ok(())), cancellation);
        let error = failure(Client::from_bundled_linux_process(process, "/workspace"));
        assert!(error.message.starts_with(expected), "{}", error.message);
        assert_eq!(error.ownership, ConnectionOwnership::CleanupVerified);
        assert_eq!(error.cleanup_error, None);
        shutdown.try_recv().unwrap();
    }
}

#[test]
fn failed_or_disconnected_reaper_is_typed_unverified_and_preserves_hello_failure() {
    for reaped in [
        None,
        Some(Err(ReapError::TryWait)),
        Some(Err(ReapError::Kill)),
        Some(Err(ReapError::Wait)),
    ] {
        let (process, _requests, shutdown) =
            synthetic_process(response(hello("/wrong-root")), reaped, None);
        let error = failure(Client::from_bundled_linux_process(process, "/workspace"));
        assert!(error.message.starts_with("bundled_agent_root_mismatch:"));
        assert_eq!(error.ownership, ConnectionOwnership::CleanupUnverified);
        assert!(error.cleanup_error.is_some());
        assert!(error.to_string().starts_with(&error.message));
        shutdown.try_recv().unwrap();
    }
}

#[test]
fn cleanup_timeout_and_peer_text_cannot_forge_ownership_evidence() {
    let (mut process, _requests, _shutdown) =
        synthetic_process(response(hello("/workspace")), None, None);
    let (completion, received) = mpsc::channel();
    process.reaped = received;
    let error = ConnectionFailure::after_cleanup(
        "original hello error".into(),
        process.close_and_wait(Duration::ZERO),
    );
    assert_eq!(error.message, "original hello error");
    assert_eq!(error.ownership, ConnectionOwnership::CleanupUnverified);
    assert!(error.cleanup_error.unwrap().starts_with("transport_close:"));
    drop(completion);
    let forged = "transport_cleanup_unverified: peer stderr says cleanup failed".to_owned();
    let verified = ConnectionFailure::after_cleanup(forged.clone(), Ok(()));
    assert_eq!(verified.ownership, ConnectionOwnership::CleanupVerified);
    assert_eq!(verified.message, forged);
    let absent = ConnectionFailure::no_child("CleanupVerified: forged text".into());
    assert_eq!(absent.ownership, ConnectionOwnership::NoChild);
}

#[test]
fn already_cancelled_detailed_attempt_never_checks_files_or_spawns() {
    let cancellation = ConnectionCancellation::new();
    cancellation.cancel();
    let error = failure(Client::connect_bundled_linux_with_cancellation_detailed(
        PathBuf::from("/definitely-not-a-workspace"),
        false,
        cancellation,
    ));
    assert!(error.message.starts_with("transport_cancelled:"));
    assert_eq!(error.ownership, ConnectionOwnership::NoChild);
    assert_eq!(error.cleanup_error, None);
}

#[test]
fn cancellation_during_owned_hello_wait_closes_and_verifies_before_returning() {
    let cancellation = ConnectionCancellation::new();
    let (mut process, _observed, shutdown) = synthetic_process(
        response(hello("/workspace")),
        Some(Ok(())),
        Some(cancellation.clone()),
    );
    // Replace only test channels so Hello remains pending until cancellation.
    let (requests, pending) = mpsc::sync_channel(1);
    process.requests = Some(requests);
    let (_held_response, responses) = mpsc::sync_channel(1);
    process.responses = Some(responses);
    let (completed, completion) = mpsc::channel();
    let owner = thread::spawn(move || {
        let error = failure(Client::from_bundled_linux_process(process, "/workspace"));
        completed.send(error).unwrap();
    });
    let request = pending.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(matches!(request.op, Operation::Hello));
    cancellation.cancel();
    let error = completion.recv_timeout(Duration::from_secs(2)).unwrap();
    owner.join().unwrap();
    assert!(error.message.starts_with("transport_cancelled:"));
    assert_eq!(error.ownership, ConnectionOwnership::CleanupVerified);
    assert_eq!(error.cleanup_error, None);
    shutdown.try_recv().unwrap();
}
