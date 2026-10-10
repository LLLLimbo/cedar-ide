//! Linux cleanup guards are tested without launching a language server.
use super::*;
use cedar_language::{
    LinuxCleanupErrors, LinuxCleanupStatus, LinuxExitStatus, LinuxRootExit, LinuxShutdownOutcome,
    LinuxShutdownReason, ShutdownOutcome,
};

fn joined_outcome() -> ShutdownOutcome {
    ShutdownOutcome {
        shutdown_response_received: true,
        exit_frame_completed: true,
        windows: None,
        linux: Some(LinuxShutdownOutcome {
            reason: LinuxShutdownReason::RootExited,
            transport_failure_observed: false,
            root_exit: LinuxRootExit::BeforeTermination(LinuxExitStatus::Code(0)),
            cleanup: LinuxCleanupStatus::Joined,
            worker_joined: true,
            root_reaped: true,
            io_released: true,
            errors: LinuxCleanupErrors::default(),
            cleanup_observation_elapsed_ms: 0,
            cleanup_observation_budget_ms: 3_000,
        }),
    }
}

#[test]
fn cleanup_requires_all_three_owned_resources_and_an_observed_root() {
    assert!(!linux_cleanup_verified(ShutdownOutcome::default()));
    for worker_joined in [false, true] {
        for root_reaped in [false, true] {
            for io_released in [false, true] {
                for root_exit in [
                    LinuxRootExit::Unobserved,
                    LinuxRootExit::BeforeTermination(LinuxExitStatus::Code(0)),
                    LinuxRootExit::BeforeTermination(LinuxExitStatus::Code(7)),
                    LinuxRootExit::AfterTermination(LinuxExitStatus::Signal(9)),
                ] {
                    for cleanup in [
                        LinuxCleanupStatus::Joined,
                        LinuxCleanupStatus::JoinedWithErrors,
                        LinuxCleanupStatus::Unverified,
                    ] {
                        let mut outcome = joined_outcome();
                        let linux = outcome.linux.as_mut().unwrap();
                        linux.worker_joined = worker_joined;
                        linux.root_reaped = root_reaped;
                        linux.io_released = io_released;
                        linux.root_exit = root_exit;
                        linux.cleanup = cleanup;
                        let expected = worker_joined
                            && root_reaped
                            && io_released
                            && root_exit != LinuxRootExit::Unobserved
                            && cleanup == LinuxCleanupStatus::Joined;
                        assert_eq!(linux_cleanup_verified(outcome), expected, "{outcome:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn every_cleanup_error_blocks_even_when_resource_flags_claim_completion() {
    for errors in [
        LinuxCleanupErrors {
            observe_root: true,
            ..Default::default()
        },
        LinuxCleanupErrors {
            terminate_group: true,
            ..Default::default()
        },
        LinuxCleanupErrors {
            terminate_root: true,
            ..Default::default()
        },
        LinuxCleanupErrors {
            drain_output: true,
            ..Default::default()
        },
        LinuxCleanupErrors {
            wait_root: true,
            ..Default::default()
        },
        LinuxCleanupErrors {
            worker_panicked: true,
            ..Default::default()
        },
        LinuxCleanupErrors {
            ownership_lost: true,
            ..Default::default()
        },
        LinuxCleanupErrors {
            cleanup_deadline_expired: true,
            ..Default::default()
        },
    ] {
        let mut outcome = joined_outcome();
        outcome.linux.as_mut().unwrap().errors = errors;
        assert!(!linux_cleanup_verified(outcome), "{errors:?}");
    }
}

#[test]
fn cleanup_verification_does_not_claim_graceful_protocol_completion() {
    for reason in [
        LinuxShutdownReason::RootExited,
        LinuxShutdownReason::GraceExpired,
        LinuxShutdownReason::Aborted,
        LinuxShutdownReason::TransportFailure,
        LinuxShutdownReason::WorkerPanicked,
    ] {
        let mut outcome = joined_outcome();
        outcome.shutdown_response_received = false;
        outcome.exit_frame_completed = false;
        let linux = outcome.linux.as_mut().unwrap();
        linux.reason = reason;
        linux.transport_failure_observed = true;
        linux.root_exit = LinuxRootExit::AfterTermination(LinuxExitStatus::Signal(9));
        assert!(linux_cleanup_verified(outcome));
        assert!(!outcome.is_graceful());
    }
}

fn invalid_generic_start() -> Operation {
    Operation::LanguageStart {
        program: "must-not-be-validated\0".into(),
        args: vec![],
    }
}

#[test]
fn blocked_stop_remains_unverified_after_trust_revocation_and_repeated_stop() {
    let root = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::open(root.path()).unwrap();
    workspace.set_allow_run(true);
    workspace.block_unverified_language_restart();
    workspace.set_allow_run(false);
    for _ in 0..3 {
        assert_eq!(
            workspace.handle(Operation::LanguageStop).unwrap_err().code,
            "language_cleanup_unverified"
        );
        assert!(workspace.language_restart_cleanup_blocked());
        assert_eq!(
            workspace.handle(invalid_generic_start()).unwrap_err().code,
            "run_disabled"
        );
        assert_eq!(
            workspace
                .handle(Operation::LanguageEvents)
                .unwrap_err()
                .code,
            "run_disabled"
        );
        assert_eq!(
            workspace
                .handle(Operation::LanguageStartJavaCancel { startup_id: 1 })
                .unwrap_err()
                .code,
            "run_disabled"
        );
    }
    workspace.set_allow_run(true);
    assert_eq!(
        workspace.handle(invalid_generic_start()).unwrap_err().code,
        "language_cleanup_unverified"
    );
    assert_eq!(
        workspace.handle(Operation::LanguageStop).unwrap_err().code,
        "language_cleanup_unverified"
    );
    assert!(workspace.language.is_none());
    assert!(workspace.tasks.is_none());
    assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
}

#[test]
fn unowned_stop_does_not_gain_a_trust_exception() {
    let root = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::open(root.path()).unwrap();
    assert_eq!(
        workspace.handle(Operation::LanguageStop).unwrap_err().code,
        "run_disabled"
    );
    assert!(!workspace.language_restart_cleanup_blocked());
    workspace.set_allow_run(true);
    let Payload::Language { value } = workspace.handle(Operation::LanguageStop).unwrap() else {
        panic!("expected language acknowledgement");
    };
    assert_eq!(value, json!({"stopped": true}));
    assert!(!workspace.language_restart_cleanup_blocked());
}

fn typed_operations() -> Vec<Operation> {
    let path = "must-not-be-inspected\0".to_owned();
    vec![
        Operation::LanguageStartJava {
            java_executable: path.clone(),
            distribution: path.clone(),
            data_directory: path.clone(),
        },
        Operation::LanguageStartJavaBegin {
            java_executable: path.clone(),
            distribution: path.clone(),
            data_directory: path.clone(),
        },
        Operation::LanguageStartJavaMavenBegin {
            java_executable: path.clone(),
            distribution: path.clone(),
            data_directory: path.clone(),
            local_repository: path.clone(),
        },
        Operation::LanguageStartJavaPoll { startup_id: 1 },
        Operation::LanguageStartJavaCancel { startup_id: 1 },
        Operation::LanguageMavenModel,
        Operation::LanguageMavenDependencies {
            startup_id: 1,
            pom_sha256: path,
        },
    ]
}

#[test]
fn linux_cleanup_latch_blocks_supported_java_and_maven_start() {
    let root = tempfile::tempdir().unwrap();
    for backend in [
        cedar_tasks::BackendMode::InProcess,
        cedar_tasks::BackendMode::IsolatedAgent,
    ] {
        let mut workspace = Workspace::with_backend_mode(root.path(), backend).unwrap();
        workspace.block_unverified_language_restart();
        for op in typed_operations() {
            assert_eq!(workspace.handle(op).unwrap_err().code, "run_disabled");
        }
        workspace.set_allow_run(true);
        let java_supported = backend == cedar_tasks::BackendMode::IsolatedAgent;
        assert_eq!(java_platform_supported(backend), java_supported);
        assert_eq!(workspace.maven_platform_supported(), java_supported);
        for op in typed_operations() {
            let expected = match &op {
                Operation::LanguageStartJava { .. }
                | Operation::LanguageStartJavaBegin { .. }
                | Operation::LanguageStartJavaMavenBegin { .. }
                    if java_supported =>
                {
                    "language_cleanup_unverified"
                }
                Operation::LanguageStartJavaPoll { .. }
                | Operation::LanguageStartJavaCancel { .. }
                    if java_supported =>
                {
                    "unknown_language_startup"
                }
                Operation::LanguageMavenModel | Operation::LanguageMavenDependencies { .. }
                    if java_supported =>
                {
                    "language_not_running"
                }
                _ => "unsupported_platform",
            };
            assert_eq!(workspace.handle(op).unwrap_err().code, expected);
        }
        assert!(workspace.language.is_none());
        assert!(workspace.tasks.is_none());
        assert!(workspace.language_restart_cleanup_blocked());
    }
    assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
}

#[test]
fn missing_executable_failure_is_retryable_without_launching_a_server() {
    let root = tempfile::tempdir().unwrap();
    let program = root.path().join("absent-server");
    assert!(!program.exists());
    let mut workspace = Workspace::open(root.path()).unwrap();
    workspace.set_allow_run(true);
    for _ in 0..2 {
        let error = workspace
            .handle(Operation::LanguageStart {
                program: program.to_string_lossy().into_owned(),
                args: vec![],
            })
            .unwrap_err();
        assert_eq!(error.code, "language_error");
        assert!(workspace.language.is_none());
        assert!(!workspace.language_restart_cleanup_blocked());
        assert!(workspace.require_language_start_available().is_ok());
    }
    assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
}

#[test]
fn typed_linux_stop_reports_real_code_or_signal_and_rejects_unverified_ownership() {
    let mut observed = joined_outcome();
    let Payload::Language { value } = java_stop_payload(observed).unwrap() else {
        panic!("typed Stop");
    };
    assert_eq!(value["shutdown"]["platform"], "linux");
    assert_eq!(value["shutdown"]["status"], "graceful");
    assert_eq!(
        value["shutdown"]["root_exit"],
        json!({"kind":"code","code":0})
    );
    assert!(value["shutdown"].get("root_exit_code").is_none());
    for code in [1, 255] {
        observed.linux.as_mut().unwrap().root_exit =
            LinuxRootExit::BeforeTermination(LinuxExitStatus::Code(code));
        let Payload::Language { value } = java_stop_payload(observed).unwrap() else {
            panic!("typed Stop");
        };
        assert_eq!(value["shutdown"]["status"], "error");
        assert_eq!(value["shutdown"]["root_exit"]["code"], code);
    }
    for signal in [1, 9, 15, 64] {
        observed.linux.as_mut().unwrap().root_exit =
            LinuxRootExit::AfterTermination(LinuxExitStatus::Signal(signal));
        observed.linux.as_mut().unwrap().reason = LinuxShutdownReason::GraceExpired;
        let Payload::Language { value } = java_stop_payload(observed).unwrap() else {
            panic!("typed Stop");
        };
        assert_eq!(value["shutdown"]["status"], "forced");
        assert_eq!(
            value["shutdown"]["root_exit"],
            json!({"kind":"signal","signal":signal})
        );
        assert!(value["shutdown"].get("root_exit_code").is_none());
    }
    for exit in [
        LinuxExitStatus::Code(-1),
        LinuxExitStatus::Code(256),
        LinuxExitStatus::Signal(0),
        LinuxExitStatus::Signal(65),
    ] {
        let mut invalid = joined_outcome();
        invalid.linux.as_mut().unwrap().root_exit = LinuxRootExit::BeforeTermination(exit);
        assert_eq!(
            java_stop_payload(invalid).unwrap_err().code,
            "language_cleanup_unverified"
        );
    }
    for index in 0..6 {
        let mut invalid = joined_outcome();
        let linux = invalid.linux.as_mut().unwrap();
        match index {
            0 => linux.worker_joined = false,
            1 => linux.root_reaped = false,
            2 => linux.io_released = false,
            3 => linux.cleanup = LinuxCleanupStatus::Unverified,
            4 => linux.errors.ownership_lost = true,
            _ => linux.root_exit = LinuxRootExit::Unobserved,
        }
        assert_eq!(
            java_stop_payload(invalid).unwrap_err().code,
            "language_cleanup_unverified"
        );
    }
    let mut failed_transport = joined_outcome();
    failed_transport
        .linux
        .as_mut()
        .unwrap()
        .transport_failure_observed = true;
    let Payload::Language { value } = java_stop_payload(failed_transport).unwrap() else {
        panic!("typed Stop");
    };
    assert_eq!(value["shutdown"]["status"], "error");
}
