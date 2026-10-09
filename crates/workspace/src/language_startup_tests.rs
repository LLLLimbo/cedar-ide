//! These fixtures bypass only launch selection, privately in this unit module.
//! Production Begin remains trusted, Windows-only, and isolated-agent-only.
use super::*;
use cedar_language::{
    ClientOptions, ProcessConfig, WindowsCleanupErrors, WindowsShutdownOutcome,
    WindowsShutdownReason,
};
use cedar_protocol::Operation;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{mpsc, OnceLock},
};
use tempfile::TempDir;

struct Peer {
    _root: TempDir,
    path: PathBuf,
}
fn peer() -> &'static Path {
    static PEER: OnceLock<Peer> = OnceLock::new();
    &PEER
        .get_or_init(|| {
            let root = tempfile::tempdir().unwrap();
            let path = root
                .path()
                .join(format!("java-startup-peer{}", std::env::consts::EXE_SUFFIX));
            let output = Command::new("rustc")
                .args(["--edition=2021", "--crate-name", "java_startup_peer"])
                .arg(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/fixtures/java_startup_peer.rs"),
                )
                .arg("-o")
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            Peer { _root: root, path }
        })
        .path
}
fn workspace() -> (TempDir, Workspace) {
    let root = tempfile::tempdir().unwrap();
    let mut workspace =
        Workspace::with_backend_mode(root.path(), crate::BackendMode::IsolatedAgent).unwrap();
    workspace.set_allow_run(true);
    (root, workspace)
}
fn launch(directory: &Path, mode: &str) -> JavaLaunch {
    let mut config = ProcessConfig::new(peer());
    config.args = vec![directory.into(), mode.into()];
    config.inherit_stderr = false;
    JavaLaunch {
        config,
        options: ClientOptions {
            request_timeout: Duration::from_secs(2),
            shutdown_timeout: Duration::from_millis(100),
            ..ClientOptions::default()
        },
        initialization_options: Value::Null,
        maven: None,
    }
}
fn value(payload: Payload) -> Value {
    let Payload::Language { value } = payload else {
        panic!("language payload required")
    };
    value
}
fn start(workspace: &mut Workspace, mode: &str, budget: Duration) -> u64 {
    let launch = launch(workspace.root(), mode);
    let initial = value(
        workspace
            .begin_java_startup_worker(move || Ok(launch), budget)
            .unwrap(),
    );
    assert_eq!(initial["state"], "starting");
    assert!(workspace.language.is_none());
    initial["startup_id"].as_u64().unwrap()
}
fn wait_for(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "fixture exceeded its finite deadline"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn terminal(workspace: &mut Workspace, id: u64) -> Value {
    let mut result = Value::Null;
    wait_for(|| {
        result = value(workspace.poll_java_startup(id).unwrap());
        matches!(
            result["state"].as_str(),
            Some("ready" | "cancelled" | "failed")
        )
    });
    wait_for(|| {
        workspace.poll_java_startup(id).unwrap();
        workspace
            .java_startup
            .record
            .as_ref()
            .unwrap()
            .worker
            .is_none()
    });
    result
}
fn cancellation_terminal(result: &Value) {
    if cfg!(windows) {
        assert_eq!(result["state"], "cancelled", "{result}");
        assert_eq!(result["cleanup_verified"], true);
    } else {
        // The portable transport owns its direct child but cannot claim joined
        // Windows process-tree ownership. It must never manufacture that proof.
        assert_eq!(result["state"], "failed", "{result}");
        assert_eq!(result["cleanup_verified"], false);
    }
}
#[cfg(unix)]
fn assert_dead(pid: u32) {
    assert_eq!(
        unsafe { libc::kill(pid as i32, 0) },
        -1,
        "fixture child {pid} survived owned cleanup"
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}
#[cfg(windows)]
fn assert_dead(pid: u32) {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::{ERROR_INVALID_PARAMETER, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
    };
    // Used only when a deliberately panicking worker has already cleaned up
    // before the observer can open a handle. Access denial is never exit proof.
    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if raw.is_null() {
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(ERROR_INVALID_PARAMETER as i32),
            "cannot verify fixture PID {pid}"
        );
        return;
    }
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    assert_eq!(
        unsafe { WaitForSingleObject(process.as_raw_handle(), 0) },
        WAIT_OBJECT_0,
        "fixture child {pid} survived owned cleanup"
    );
}

// Capture the process object before cancellation. The same held Windows handle
// proves exit even if the numeric PID becomes reusable after the owner reaps it.
struct ObservedProcess {
    pid: u32,
    #[cfg(windows)]
    handle: std::os::windows::io::OwnedHandle,
    #[cfg(windows)]
    creation_time: u64,
}
impl ObservedProcess {
    fn open(pid: u32) -> Self {
        #[cfg(windows)]
        let (handle, creation_time) = {
            use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
            use windows_sys::Win32::System::Threading::{
                GetProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            };
            let raw = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    pid,
                )
            };
            assert!(
                !raw.is_null(),
                "observe live fixture PID {pid}: {}",
                std::io::Error::last_os_error()
            );
            let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
            assert_eq!(unsafe { GetProcessId(handle.as_raw_handle()) }, pid);
            let creation_time = process_creation_time(&handle);
            (handle, creation_time)
        };
        let observed = Self {
            pid,
            #[cfg(windows)]
            handle,
            #[cfg(windows)]
            creation_time,
        };
        observed.assert_alive();
        observed
    }
    fn assert_alive(&self) {
        #[cfg(unix)]
        assert_eq!(
            unsafe { libc::kill(self.pid as i32, 0) },
            0,
            "fixture {} is not alive",
            self.pid
        );
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::{
                Foundation::WAIT_TIMEOUT, System::Threading::WaitForSingleObject,
            };
            assert_eq!(
                unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) },
                WAIT_TIMEOUT,
                "fixture {} is not alive",
                self.pid
            );
            assert_eq!(process_creation_time(&self.handle), self.creation_time);
        }
    }
    fn assert_dead(&self) {
        #[cfg(unix)]
        assert_dead(self.pid);
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::{
                Foundation::WAIT_OBJECT_0,
                System::Threading::{GetProcessId, WaitForSingleObject},
            };
            assert_eq!(
                unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 1500) },
                WAIT_OBJECT_0,
                "fixture {} survived owned cleanup",
                self.pid
            );
            assert_eq!(
                unsafe { GetProcessId(self.handle.as_raw_handle()) },
                self.pid
            );
            assert_eq!(process_creation_time(&self.handle), self.creation_time);
        }
    }
}
#[cfg(windows)]
fn process_creation_time(handle: &std::os::windows::io::OwnedHandle) -> u64 {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{Foundation::FILETIME, System::Threading::GetProcessTimes};
    let mut creation = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut exit = creation;
    let mut kernel = creation;
    let mut user = creation;
    assert_ne!(
        unsafe {
            GetProcessTimes(
                handle.as_raw_handle(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        },
        0,
        "process creation identity: {}",
        std::io::Error::last_os_error()
    );
    (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime)
}

#[test]
fn async_public_operations_gate_trust_and_platform_before_allocating_an_owner() {
    let root = tempfile::tempdir().unwrap();
    for backend in [
        crate::BackendMode::InProcess,
        crate::BackendMode::IsolatedAgent,
    ] {
        let mut workspace = Workspace::with_backend_mode(root.path(), backend).unwrap();
        let begin = || Operation::LanguageStartJavaBegin {
            java_executable: "must-not-be-inspected\0".into(),
            distribution: "must-not-be-inspected\0".into(),
            data_directory: "must-not-be-inspected\0".into(),
        };
        for operation in [
            begin(),
            Operation::LanguageStartJavaPoll { startup_id: 1 },
            Operation::LanguageStartJavaCancel { startup_id: 1 },
        ] {
            assert_eq!(
                workspace.handle(operation).unwrap_err().code,
                "run_disabled"
            );
            assert!(workspace.java_startup.record.is_none());
        }
        workspace.set_allow_run(true);
        if !super::super::java_platform_supported(backend) {
            for operation in [
                begin(),
                Operation::LanguageStartJavaPoll { startup_id: 1 },
                Operation::LanguageStartJavaCancel { startup_id: 1 },
            ] {
                assert_eq!(
                    workspace.handle(operation).unwrap_err().code,
                    "unsupported_platform"
                );
                assert!(workspace.java_startup.record.is_none());
            }
        } else {
            let id = value(workspace.handle(begin()).unwrap())["startup_id"]
                .as_u64()
                .unwrap();
            let result = terminal(&mut workspace, id);
            assert_eq!(result["state"], "failed");
            assert_eq!(result["cleanup_verified"], true);
            assert_eq!(result["error"]["code"], "invalid_java_launch");
        }
        assert!(workspace.language.is_none());
        assert!(workspace.tasks.is_none());
    }
    assert!(fs::read_dir(root.path()).unwrap().next().is_none());
}

#[test]
fn pending_files_saves_and_independent_tasks_work_and_cancel_does_not_join() {
    let (root, mut workspace) = workspace();
    let binary = peer().to_path_buf();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let id = value(
        workspace
            .begin_java_startup_worker(
                move || {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                    Err(error("fixture_preflight", "no child created"))
                },
                Duration::from_secs(5),
            )
            .unwrap(),
    )["startup_id"]
        .as_u64()
        .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let first = workspace
        .handle(Operation::Write {
            path: "draft.java".into(),
            text: "first".into(),
            expected_revision: None,
        })
        .unwrap();
    let Payload::Written { revision } = first else {
        panic!("exact save acknowledgement required")
    };
    let task = workspace
        .handle(Operation::RunStart {
            program: binary.to_string_lossy().into_owned(),
            args: vec![root.path().to_string_lossy().into_owned(), "task".into()],
            timeout_secs: 20,
        })
        .unwrap();
    let Payload::RunTask { snapshot } = task else {
        panic!("task payload")
    };
    let task_id = snapshot["id"].as_u64().unwrap();
    wait_for(|| root.path().join("task.pid").exists());
    for duplicate in [
        Operation::LanguageStart {
            program: "must-not-run".into(),
            args: vec![],
        },
        Operation::LanguageStop,
    ] {
        assert_eq!(
            workspace.handle(duplicate).unwrap_err().code,
            "language_start_in_progress"
        );
    }
    assert_eq!(
        workspace
            .begin_java_startup_worker(|| unreachable!(), Duration::from_secs(1))
            .unwrap_err()
            .code,
        "language_start_in_progress"
    );
    let before = Instant::now();
    assert_eq!(
        value(workspace.cancel_java_startup(id).unwrap())["state"],
        "cancelling"
    );
    assert!(before.elapsed() < Duration::from_millis(250));
    assert_eq!(
        value(workspace.cancel_java_startup(id).unwrap())["state"],
        "cancelling"
    );
    assert!(matches!(
        workspace
            .handle(Operation::Write {
                path: "draft.java".into(),
                text: "second".into(),
                expected_revision: Some(revision.clone())
            })
            .unwrap(),
        Payload::Written { .. }
    ));
    assert_eq!(
        workspace
            .handle(Operation::Write {
                path: "draft.java".into(),
                text: "stale".into(),
                expected_revision: Some(revision)
            })
            .unwrap_err()
            .code,
        "conflict"
    );
    assert!(
        matches!(workspace.handle(Operation::Read { path: "draft.java".into() }).unwrap(), Payload::File { text, .. } if text == "second")
    );
    workspace
        .handle(Operation::List { path: ".".into() })
        .unwrap();
    workspace
        .handle(Operation::Search {
            query: "second".into(),
            limit: 10,
        })
        .unwrap();
    release_tx.send(()).unwrap();
    let result = terminal(&mut workspace, id);
    assert_eq!(result["state"], "cancelled");
    assert_eq!(result["cleanup_verified"], true);
    let Payload::RunTask { snapshot } = workspace.handle(Operation::RunPoll { task_id }).unwrap()
    else {
        unreachable!()
    };
    assert_eq!(
        snapshot["state"], "running",
        "language cancel affected independent task: {snapshot}"
    );
    workspace.handle(Operation::RunCancel { task_id }).unwrap();
    wait_for(
        || matches!(workspace.handle(Operation::RunPoll { task_id }).unwrap(), Payload::RunTask { snapshot } if snapshot["state"] == "cancelled"),
    );
}

#[test]
fn cancellation_before_worker_preparation_never_launches() {
    let control = Arc::new(StartupControl {
        cancelled: AtomicBool::new(true),
        prepared: AtomicBool::new(false),
        worker_failed: AtomicBool::new(false),
        panic_after_publish: AtomicBool::new(false),
        panic_after_adoption: AtomicBool::new(false),
        adopted: AtomicBool::new(false),
        transfer: Mutex::new(None),
        wake: OnceLock::new(),
        published: Mutex::new(None),
        deadline: Instant::now() + Duration::from_secs(1),
    });
    let result = startup_worker(control, "file:///fixture/".into(), || {
        panic!("cancelled startup ran preparation")
    });
    assert!(matches!(
        result,
        StartupResult::Finished {
            cancelled: true,
            cleanup_verified: true,
            error: None
        }
    ));
}

#[test]
fn cancel_during_initialize_is_idempotent_and_unknown_ids_cannot_signal() {
    let (root, mut workspace) = workspace();
    let id = start(&mut workspace, "wait_init", Duration::from_secs(5));
    wait_for(|| root.path().join("initialize.received").exists());
    let pid = workspace
        .java_startup_record(id)
        .unwrap()
        .control
        .process_id()
        .unwrap();
    let observed = ObservedProcess::open(pid);
    for wrong in [0, id + 1, u64::MAX] {
        assert_eq!(
            workspace.poll_java_startup(wrong).unwrap_err().code,
            "unknown_language_startup"
        );
        assert_eq!(
            workspace.cancel_java_startup(wrong).unwrap_err().code,
            "unknown_language_startup"
        );
    }
    assert!(!workspace
        .java_startup_record(id)
        .unwrap()
        .control
        .is_cancelled());
    for _ in 0..3 {
        assert_eq!(
            value(workspace.cancel_java_startup(id).unwrap())["state"],
            "cancelling"
        );
    }
    let result = terminal(&mut workspace, id);
    cancellation_terminal(&result);
    observed.assert_dead();
    assert_eq!(value(workspace.poll_java_startup(id).unwrap()), result);
    assert_eq!(value(workspace.cancel_java_startup(id).unwrap()), result);
    assert!(workspace.language.is_none());
}

#[test]
fn original_deadline_survives_repeated_polls() {
    let (root, mut workspace) = workspace();
    let before = Instant::now();
    let id = start(&mut workspace, "wait_init", Duration::from_millis(180));
    let deadline = workspace.java_startup_record(id).unwrap().control.deadline;
    wait_for(|| root.path().join("initialize.received").exists());
    for _ in 0..5 {
        workspace.poll_java_startup(id).unwrap();
        assert_eq!(
            workspace.java_startup_record(id).unwrap().control.deadline,
            deadline
        );
        thread::sleep(Duration::from_millis(15));
    }
    let result = terminal(&mut workspace, id);
    assert_eq!(result["state"], "failed");
    if cfg!(windows) {
        assert_eq!(result["cleanup_verified"], true);
        assert_eq!(result["error"]["code"], "language_startup_timeout");
    } else {
        assert_eq!(result["cleanup_verified"], false);
    }
    assert!(
        before.elapsed() < Duration::from_secs(3),
        "poll extended the original startup deadline"
    );
    assert!(workspace.language.is_none());
}

#[test]
fn cancel_wins_over_completed_unadopted_ready_and_never_exposes_a_session() {
    let (_root, mut workspace) = workspace();
    let id = start(&mut workspace, "ready", Duration::from_secs(5));
    wait_for(|| {
        workspace
            .java_startup_record(id)
            .unwrap()
            .control
            .prepared
            .load(Ordering::SeqCst)
    });
    let pid = workspace
        .java_startup_record(id)
        .unwrap()
        .control
        .process_id()
        .unwrap();
    let observed = ObservedProcess::open(pid);
    assert!(workspace.language.is_none());
    assert_eq!(
        value(workspace.cancel_java_startup(id).unwrap())["state"],
        "cancelling"
    );
    let result = terminal(&mut workspace, id);
    cancellation_terminal(&result);
    observed.assert_dead();
    assert!(workspace.language.is_none());
}

#[test]
fn ready_is_adopted_once_and_its_id_scoped_cancel_closes_only_that_session() {
    let (_root, mut workspace) = workspace();
    let id = start(&mut workspace, "ready", Duration::from_secs(5));
    let ready = terminal(&mut workspace, id);
    assert_eq!(ready["state"], "ready");
    assert_eq!(ready["language"]["started"], true);
    assert_eq!(
        ready["language"]["initialize"]["cedar_java_diagnostics_refresh"],
        true
    );
    assert_eq!(
        ready["language"]["initialize"]["cedar_java_organize_imports"],
        true
    );
    assert!(workspace.language.as_ref().unwrap().java_organize_imports);
    assert_eq!(
        ready["language"]["initialize"]["cedar_java_maven_model"],
        false
    );
    assert!(ready["language"]["initialize"]
        .get("cedar_java_profile")
        .is_none());
    assert!(ready["language"]["initialize"]
        .get("cedar_java_maven_pom_sha256")
        .is_none());
    assert!(workspace.language.as_ref().unwrap().java_maven.is_none());
    assert_eq!(workspace.language.as_ref().unwrap().startup_id, Some(id));
    let pid = ready["language"]["process_id"].as_u64().unwrap() as u32;
    let observed = ObservedProcess::open(pid);
    assert_eq!(value(workspace.poll_java_startup(id).unwrap()), ready);
    assert_eq!(
        value(workspace.cancel_java_startup(id).unwrap())["state"],
        "cancelling"
    );
    assert!(workspace.language.is_none());
    cancellation_terminal(&terminal(&mut workspace, id));
    observed.assert_dead();
}

#[test]
fn bounded_terminal_retention_and_monotonic_ids_never_reactivate_old_success() {
    let (_root, mut workspace) = workspace();
    let failed = |workspace: &mut Workspace| {
        value(
            workspace
                .begin_java_startup_worker(
                    || Err(error("fixture", "preflight refused")),
                    Duration::from_secs(1),
                )
                .unwrap(),
        )["startup_id"]
            .as_u64()
            .unwrap()
    };
    let first = failed(&mut workspace);
    let terminal_first = terminal(&mut workspace, first);
    assert_eq!(terminal_first["cleanup_verified"], true);
    let second = failed(&mut workspace);
    assert!(second > first);
    assert_eq!(
        workspace.poll_java_startup(first).unwrap_err().code,
        "unknown_language_startup"
    );
    assert_eq!(
        workspace.cancel_java_startup(first).unwrap_err().code,
        "unknown_language_startup"
    );
    assert_eq!(terminal(&mut workspace, second)["cleanup_verified"], true);
    workspace.java_startup.last_id = u64::MAX;
    assert_eq!(
        workspace
            .begin_java_startup_worker(|| unreachable!(), Duration::from_secs(1))
            .unwrap_err()
            .code,
        "language_startup_exhausted"
    );
}

#[test]
fn unverified_spawn_or_owner_failure_blocks_every_future_start_until_reconnect() {
    let (_root, mut workspace) = workspace();
    let missing = JavaLaunch {
        config: ProcessConfig::new("cedar-fixture-executable-does-not-exist"),
        options: ClientOptions::default(),
        initialization_options: Value::Null,
        maven: None,
    };
    let id = value(
        workspace
            .begin_java_startup_worker(move || Ok(missing), Duration::from_secs(1))
            .unwrap(),
    )["startup_id"]
        .as_u64()
        .unwrap();
    let result = terminal(&mut workspace, id);
    assert_eq!(result["state"], "failed");
    assert_eq!(result["cleanup_verified"], false);
    assert!(workspace.java_startup.restart_blocked);
    assert_eq!(
        workspace
            .begin_java_startup_worker(|| unreachable!(), Duration::from_secs(1))
            .unwrap_err()
            .code,
        "language_cleanup_unverified"
    );
    assert_eq!(
        workspace
            .handle(Operation::LanguageStart {
                program: "must-not-run".into(),
                args: vec![]
            })
            .unwrap_err()
            .code,
        "language_cleanup_unverified"
    );
}

#[test]
fn workspace_drop_joins_both_pending_and_completed_unadopted_owners() {
    for mode in ["wait_init", "ready"] {
        let (root, mut workspace) = workspace();
        let id = start(&mut workspace, mode, Duration::from_secs(5));
        wait_for(|| root.path().join("initialize.received").exists());
        if mode == "ready" {
            wait_for(|| {
                workspace
                    .java_startup_record(id)
                    .unwrap()
                    .control
                    .prepared
                    .load(Ordering::SeqCst)
            });
        }
        let pid = workspace
            .java_startup_record(id)
            .unwrap()
            .control
            .process_id()
            .unwrap();
        let observed = ObservedProcess::open(pid);
        let before = Instant::now();
        drop(workspace);
        assert!(before.elapsed() < Duration::from_secs(3));
        observed.assert_dead();
    }
}

#[test]
fn cancellation_never_claims_cleanup_without_joined_owned_root_evidence() {
    let good = WindowsShutdownOutcome {
        reason: WindowsShutdownReason::Aborted,
        transport_failure_observed: false,
        root_exit: WindowsRootExit::AfterTermination(1067),
        cleanup: WindowsCleanupStatus::Joined,
        errors: WindowsCleanupErrors::default(),
    };
    assert!(cleanup_verified(ShutdownOutcome {
        windows: Some(good),
        ..ShutdownOutcome::default()
    }));
    assert!(!cleanup_verified(ShutdownOutcome::default()));
    for windows in [
        WindowsShutdownOutcome {
            root_exit: WindowsRootExit::Unobserved,
            ..good
        },
        WindowsShutdownOutcome {
            cleanup: WindowsCleanupStatus::Unverified,
            ..good
        },
        WindowsShutdownOutcome {
            cleanup: WindowsCleanupStatus::JoinedWithErrors,
            ..good
        },
        WindowsShutdownOutcome {
            errors: WindowsCleanupErrors {
                terminate_tree: true,
                ..WindowsCleanupErrors::default()
            },
            ..good
        },
    ] {
        assert!(!cleanup_verified(ShutdownOutcome {
            windows: Some(windows),
            ..ShutdownOutcome::default()
        }));
    }
}

#[test]
fn prepared_success_expires_without_polling_and_cannot_be_adopted_late() {
    let (_root, mut workspace) = workspace();
    let id = start(&mut workspace, "ready", Duration::from_millis(400));
    wait_for(|| {
        workspace
            .java_startup_record(id)
            .unwrap()
            .control
            .prepared
            .load(Ordering::SeqCst)
    });
    let pid = workspace
        .java_startup_record(id)
        .unwrap()
        .control
        .process_id()
        .unwrap();
    let observed = ObservedProcess::open(pid);
    // No Poll requests adoption. The worker itself must enforce its deadline.
    wait_for(|| {
        workspace
            .java_startup_record(id)
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .is_finished()
    });
    let result = terminal(&mut workspace, id);
    assert_eq!(result["state"], "failed");
    assert!(workspace.language.is_none());
    observed.assert_dead();
}

#[test]
fn one_poll_immediately_adopts_ready_without_an_unconsumed_success_handoff() {
    let (_root, mut workspace) = workspace();
    let id = start(&mut workspace, "ready", Duration::from_millis(400));
    let control = workspace.java_startup_record(id).unwrap().control.clone();
    wait_for(|| control.prepared.load(Ordering::SeqCst));
    let ready = value(workspace.poll_java_startup(id).unwrap());
    assert_eq!(ready["state"], "ready");
    assert!(control.transfer.lock().unwrap().is_none());
    assert!(control.adopted.load(Ordering::SeqCst));
    assert_eq!(workspace.language.as_ref().unwrap().startup_id, Some(id));
    // No second poll is needed to complete the ownership transfer. Cancel can
    // immediately take both this exact session and any predecessor join handle.
    let pid = control.process_id().unwrap();
    let observed = ObservedProcess::open(pid);
    assert_eq!(
        value(workspace.cancel_java_startup(id).unwrap())["state"],
        "cancelling"
    );
    cancellation_terminal(&terminal(&mut workspace, id));
    observed.assert_dead();
}

#[test]
fn panicked_owner_fails_closed_and_never_detaches_a_result() {
    let (_root, mut workspace) = workspace();
    let id = value(
        workspace
            .begin_java_startup_worker(
                || panic!("synthetic preparation failure"),
                Duration::from_secs(1),
            )
            .unwrap(),
    )["startup_id"]
        .as_u64()
        .unwrap();
    let result = terminal(&mut workspace, id);
    assert_eq!(result["state"], "failed");
    assert_eq!(result["cleanup_verified"], false);
    assert!(workspace.java_startup.restart_blocked);
    assert!(workspace.language.is_none());
}

#[cfg(windows)]
#[test]
fn legacy_stop_retires_ready_id_and_stale_cancel_never_touches_a_later_session() {
    let (_root, mut workspace) = workspace();
    let first = start(&mut workspace, "ready", Duration::from_secs(5));
    assert_eq!(terminal(&mut workspace, first)["state"], "ready");
    workspace.handle(Operation::LanguageStop).unwrap();
    let retired = value(workspace.poll_java_startup(first).unwrap());
    assert_eq!(retired["state"], "cancelled");
    let unrelated = launch(workspace.root(), "ready");
    workspace
        .start_language_session(unrelated.config, unrelated.options, Value::Null, false)
        .unwrap();
    let pid = workspace.language.as_ref().unwrap().client.process_id();
    assert_eq!(
        value(workspace.cancel_java_startup(first).unwrap()),
        retired
    );
    assert_eq!(value(workspace.poll_java_startup(first).unwrap()), retired);
    assert_eq!(
        workspace.language.as_ref().unwrap().client.process_id(),
        pid
    );
    workspace
        .language
        .as_ref()
        .unwrap()
        .client
        .request("fixture/barrier", Value::Null)
        .unwrap();
    workspace.handle(Operation::LanguageStop).unwrap();
    let second = start(&mut workspace, "ready", Duration::from_secs(5));
    assert!(second > first);
    assert_eq!(
        workspace.cancel_java_startup(first).unwrap_err().code,
        "unknown_language_startup"
    );
    assert_eq!(terminal(&mut workspace, second)["state"], "ready");
    workspace.cancel_java_startup(second).unwrap();
    cancellation_terminal(&terminal(&mut workspace, second));
}

fn start_with_panic(workspace: &mut Workspace, after_adoption: bool) -> u64 {
    let launch = launch(workspace.root(), "ready");
    let (release_tx, release_rx) = mpsc::channel();
    let id = value(
        workspace
            .begin_java_startup_worker(
                move || {
                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                    Ok(launch)
                },
                Duration::from_secs(5),
            )
            .unwrap(),
    )["startup_id"]
        .as_u64()
        .unwrap();
    let control = &workspace.java_startup_record(id).unwrap().control;
    if after_adoption {
        control.panic_after_adoption.store(true, Ordering::SeqCst);
    } else {
        control.panic_after_publish.store(true, Ordering::SeqCst);
    }
    release_tx.send(()).unwrap();
    id
}

#[test]
fn panic_after_prepared_publication_cleans_client_before_failure_is_observed() {
    let (_root, mut workspace) = workspace();
    let id = start_with_panic(&mut workspace, false);
    wait_for(|| {
        workspace
            .java_startup_record(id)
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .is_finished()
    });
    let control = workspace.java_startup_record(id).unwrap().control.clone();
    let pid = control.process_id().unwrap();
    assert!(control.worker_failed.load(Ordering::SeqCst));
    assert!(control.transfer.lock().unwrap().is_none());
    assert_dead(pid);
    let result = terminal(&mut workspace, id);
    assert_eq!(result["state"], "failed");
    assert_eq!(result["cleanup_verified"], false);
    assert!(workspace.language.is_none());
    assert!(workspace.java_startup.restart_blocked);
}

#[test]
fn panic_after_atomic_adoption_preserves_the_authoritative_owned_ready_session() {
    let (_root, mut workspace) = workspace();
    let id = start_with_panic(&mut workspace, true);
    let control = workspace.java_startup_record(id).unwrap().control.clone();
    wait_for(|| control.prepared.load(Ordering::SeqCst));
    let ready = value(workspace.poll_java_startup(id).unwrap());
    assert_eq!(ready["state"], "ready");
    wait_for(|| {
        workspace
            .java_startup_record(id)
            .unwrap()
            .worker
            .as_ref()
            .is_none_or(JoinHandle::is_finished)
    });
    assert_eq!(value(workspace.poll_java_startup(id).unwrap()), ready);
    assert_eq!(workspace.language.as_ref().unwrap().startup_id, Some(id));
    assert!(control.transfer.lock().unwrap().is_none());
    workspace
        .language
        .as_ref()
        .unwrap()
        .client
        .request("fixture/barrier", Value::Null)
        .unwrap();
    assert!(!workspace.java_startup.restart_blocked);
    let observed = ObservedProcess::open(control.process_id().unwrap());
    workspace.cancel_java_startup(id).unwrap();
    cancellation_terminal(&terminal(&mut workspace, id));
    observed.assert_dead();
}

#[cfg(windows)]
#[test]
fn revoking_trust_preserves_only_existing_id_poll_and_cancel() {
    let (root, mut workspace) = workspace();
    let id = start(&mut workspace, "wait_init", Duration::from_secs(5));
    wait_for(|| root.path().join("initialize.received").exists());
    workspace.set_allow_run(false);
    assert_eq!(
        value(
            workspace
                .handle(Operation::LanguageStartJavaPoll { startup_id: id })
                .unwrap()
        )["state"],
        "starting"
    );
    assert_eq!(
        value(
            workspace
                .handle(Operation::LanguageStartJavaCancel { startup_id: id })
                .unwrap()
        )["state"],
        "cancelling"
    );
    for operation in [
        Operation::LanguageStartJavaPoll { startup_id: id + 1 },
        Operation::LanguageStartJavaCancel { startup_id: id + 1 },
        Operation::LanguageStartJavaBegin {
            java_executable: "must-not-be-read".into(),
            distribution: "must-not-be-read".into(),
            data_directory: "must-not-be-read".into(),
        },
    ] {
        assert_eq!(
            workspace.handle(operation).unwrap_err().code,
            "run_disabled"
        );
    }
    cancellation_terminal(&terminal(&mut workspace, id));
}

#[test]
fn late_marker_panic_cannot_downgrade_verified_stop_or_block_reuse() {
    let (_root, mut workspace) = workspace();
    // Construct a retired record without a process. Its terminal cleanup proof
    // has already been published by Stop; only a resource-free adoption marker
    // remains to reap. That marker cannot revoke the independent cleanup proof.
    let id = value(
        workspace
            .begin_java_startup_worker(|| Err(error("fixture", "no child")), Duration::from_secs(1))
            .unwrap(),
    )["startup_id"]
        .as_u64()
        .unwrap();
    terminal(&mut workspace, id);
    let verified = json!({"startup_id":id,"state":"cancelled","cleanup_verified":true});
    let record = workspace.java_startup.record.as_mut().unwrap();
    record.control.adopted.store(true, Ordering::SeqCst);
    record.terminal = Some(verified.clone());
    record.worker = Some(thread::spawn(|| {
        panic!("late resource-free adoption marker")
    }));
    wait_for(|| {
        workspace
            .java_startup_record(id)
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .is_finished()
    });
    assert_eq!(value(workspace.poll_java_startup(id).unwrap()), verified);
    assert!(!workspace.java_startup.restart_blocked);
    let next = value(
        workspace
            .begin_java_startup_worker(|| Err(error("fixture", "no child")), Duration::from_secs(1))
            .unwrap(),
    )["startup_id"]
        .as_u64()
        .unwrap();
    assert!(next > id);
    assert_eq!(terminal(&mut workspace, next)["cleanup_verified"], true);
}

fn fixture_pid(directory: &Path, name: &str) -> u32 {
    let mut pid = None;
    wait_for(|| {
        pid = fs::read_to_string(directory.join(name))
            .ok()
            .and_then(|text| text.parse::<u32>().ok())
            .filter(|pid| *pid != 0);
        pid.is_some()
    });
    pid.unwrap()
}

fn start_observed_task(workspace: &mut Workspace, directory: &Path) -> (u64, ObservedProcess) {
    let Payload::RunTask { snapshot } = workspace
        .handle(Operation::RunStart {
            program: peer().to_string_lossy().into_owned(),
            args: vec![directory.to_string_lossy().into_owned(), "task".into()],
            timeout_secs: 20,
        })
        .unwrap()
    else {
        panic!("task payload")
    };
    let task_id = snapshot["id"].as_u64().unwrap();
    let observed = ObservedProcess::open(fixture_pid(directory, "task.pid"));
    wait_for(
        || matches!(workspace.handle(Operation::RunPoll { task_id }).unwrap(), Payload::RunTask { snapshot } if snapshot["state"] == "running"),
    );
    (task_id, observed)
}

#[test]
fn stalled_initialize_serves_files_saves_and_task_controls_and_cancel_keeps_task_alive() {
    let (root, mut workspace) = workspace();
    // Windows additionally creates a descendant before the initialize marker.
    // Held observation handles prove that both owned processes exit while the
    // independent command task remains alive in its separate owner.
    let mode = if cfg!(windows) {
        "wait_init_tree"
    } else {
        "wait_init"
    };
    let id = start(&mut workspace, mode, Duration::from_secs(10));
    wait_for(|| root.path().join("initialize.received").exists());
    let language = ObservedProcess::open(
        workspace
            .java_startup_record(id)
            .unwrap()
            .control
            .process_id()
            .unwrap(),
    );
    #[cfg(windows)]
    let descendant = ObservedProcess::open(fixture_pid(root.path(), "descendant.pid"));
    assert_eq!(
        value(workspace.poll_java_startup(id).unwrap())["state"],
        "starting"
    );
    assert!(!workspace
        .java_startup_record(id)
        .unwrap()
        .control
        .prepared
        .load(Ordering::SeqCst));

    let Payload::Written { revision: first } = workspace
        .handle(Operation::Write {
            path: "during-initialize.java".into(),
            text: "first draft".into(),
            expected_revision: None,
        })
        .unwrap()
    else {
        panic!("exact save acknowledgement required")
    };
    assert!(
        matches!(workspace.handle(Operation::Read { path: "during-initialize.java".into() }).unwrap(), Payload::File { text, revision, .. } if text == "first draft" && revision == first)
    );
    workspace
        .handle(Operation::List { path: ".".into() })
        .unwrap();
    workspace
        .handle(Operation::Search {
            query: "first draft".into(),
            limit: 10,
        })
        .unwrap();
    let Payload::Written { revision: second } = workspace
        .handle(Operation::Write {
            path: "during-initialize.java".into(),
            text: "second draft".into(),
            expected_revision: Some(first.clone()),
        })
        .unwrap()
    else {
        panic!("revision acknowledgement required")
    };
    assert_eq!(
        workspace
            .handle(Operation::Write {
                path: "during-initialize.java".into(),
                text: "stale".into(),
                expected_revision: Some(first)
            })
            .unwrap_err()
            .code,
        "conflict"
    );

    let (first_task, first_process) = start_observed_task(&mut workspace, root.path());
    workspace
        .handle(Operation::RunCancel {
            task_id: first_task,
        })
        .unwrap();
    wait_for(
        || matches!(workspace.handle(Operation::RunPoll { task_id: first_task }).unwrap(), Payload::RunTask { snapshot } if snapshot["state"] == "cancelled"),
    );
    first_process.assert_dead();
    language.assert_alive();
    fs::remove_file(root.path().join("task.pid")).unwrap();
    let (task_id, task) = start_observed_task(&mut workspace, root.path());

    let audit: Vec<Value> = fs::read_to_string(root.path().join("audit.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        audit.len(),
        1,
        "initialize unexpectedly completed during workspace operations"
    );
    assert_eq!(audit[0]["method"], "initialize");
    assert!(!root.path().join("initialize.release").exists());
    assert!(workspace.language.is_none());
    let before = Instant::now();
    assert_eq!(
        value(workspace.cancel_java_startup(id).unwrap())["state"],
        "cancelling"
    );
    assert!(before.elapsed() < Duration::from_millis(250));
    assert!(matches!(
        workspace
            .handle(Operation::Write {
                path: "during-initialize.java".into(),
                text: "saved while cancelling".into(),
                expected_revision: Some(second)
            })
            .unwrap(),
        Payload::Written { .. }
    ));
    assert!(
        matches!(workspace.handle(Operation::Read { path: "during-initialize.java".into() }).unwrap(), Payload::File { text, .. } if text == "saved while cancelling")
    );
    let Payload::RunTask { snapshot } = workspace.handle(Operation::RunPoll { task_id }).unwrap()
    else {
        unreachable!()
    };
    assert_eq!(snapshot["state"], "running");
    cancellation_terminal(&terminal(&mut workspace, id));
    language.assert_dead();
    #[cfg(windows)]
    descendant.assert_dead();
    task.assert_alive();
    let Payload::RunTask { snapshot } = workspace.handle(Operation::RunPoll { task_id }).unwrap()
    else {
        unreachable!()
    };
    assert_eq!(
        snapshot["state"], "running",
        "Java cancellation affected independent task ownership"
    );
    workspace.handle(Operation::RunCancel { task_id }).unwrap();
    wait_for(
        || matches!(workspace.handle(Operation::RunPoll { task_id }).unwrap(), Payload::RunTask { snapshot } if snapshot["state"] == "cancelled"),
    );
    task.assert_dead();
}

#[test]
fn cleanup_thread_allocation_failure_retains_client_and_predecessor_until_drop() {
    let (_root, mut workspace) = workspace();
    let id = start(&mut workspace, "ready", Duration::from_secs(5));
    assert_eq!(terminal(&mut workspace, id)["state"], "ready");
    let pid = workspace.language.as_ref().unwrap().client.process_id();
    let observed = ObservedProcess::open(pid);
    // The real startup predecessor was joined by terminal(). Replace only its
    // resource-free marker with a deterministic blocked marker, so accidental
    // synchronous joining on Cancel cannot pass unnoticed.
    let (release_tx, release_rx) = mpsc::channel();
    let completed = Arc::new(AtomicBool::new(false));
    let worker_completed = completed.clone();
    let predecessor = thread::spawn(move || {
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        worker_completed.store(true, Ordering::SeqCst);
        StartupResult::Adopted
    });
    let predecessor_id = predecessor.thread().id();
    workspace.java_startup.record.as_mut().unwrap().worker = Some(predecessor);
    workspace.java_startup.fail_cleanup_spawn = true;
    let before = Instant::now();
    let failed = value(workspace.cancel_java_startup(id).unwrap());
    assert!(
        before.elapsed() < Duration::from_millis(250),
        "Cancel joined its blocked predecessor"
    );
    assert_eq!(failed["state"], "failed");
    assert_eq!(failed["cleanup_verified"], false);
    assert!(workspace.language.is_none());
    assert!(workspace.java_startup.restart_blocked);
    let fallback = workspace.java_startup.cleanup_fallback.as_ref().unwrap();
    assert_eq!(fallback.client.process_id(), pid);
    let retained = fallback.predecessor.as_ref().unwrap();
    assert_eq!(retained.thread().id(), predecessor_id);
    assert!(!retained.is_finished());
    assert!(!completed.load(Ordering::SeqCst));
    assert_eq!(value(workspace.poll_java_startup(id).unwrap()), failed);
    assert_eq!(value(workspace.cancel_java_startup(id).unwrap()), failed);
    assert!(workspace.java_startup.cleanup_fallback.is_some());
    assert_eq!(
        workspace
            .begin_java_startup_worker(|| unreachable!(), Duration::from_secs(1))
            .unwrap_err()
            .code,
        "language_cleanup_unverified"
    );
    release_tx.send(()).unwrap();
    drop(workspace);
    assert!(
        completed.load(Ordering::SeqCst),
        "Drop did not join the retained predecessor"
    );
    observed.assert_dead();
}

#[test]
fn async_java_adoption_overwrites_spoofed_import_support_without_command_advertisement() {
    let (_root, mut workspace) = workspace();
    let id = start(
        &mut workspace,
        "imports_unsupported",
        Duration::from_secs(5),
    );
    let ready = terminal(&mut workspace, id);
    assert_eq!(ready["state"], "ready");
    assert_eq!(
        ready["language"]["initialize"]["cedar_java_organize_imports"],
        false
    );
    assert!(!workspace.language.as_ref().unwrap().java_organize_imports);
    let pid = ready["language"]["process_id"].as_u64().unwrap() as u32;
    let observed = ObservedProcess::open(pid);
    workspace.cancel_java_startup(id).unwrap();
    cancellation_terminal(&terminal(&mut workspace, id));
    observed.assert_dead();
}

#[test]
fn typed_maven_metadata_and_model_access_follow_existing_startup_ownership() {
    use sha2::{Digest, Sha256};
    let (_root, mut workspace) = workspace();
    let root = crate::java_launch::ordinary_local_path(workspace.root()).unwrap();
    fs::write(root.join("pom.xml"), b"<project/>").unwrap();
    let digest = format!("{:x}", Sha256::digest(b"<project/>"));
    let mut launch = launch(workspace.root(), "ready");
    launch.maven = Some(crate::java_maven::MavenSession {
        root: root.clone(),
        local_repository: root.clone(),
        pom_sha256: digest.clone(),
        pom_uri: url::Url::from_file_path(root.join("pom.xml"))
            .unwrap()
            .into(),
        declared_dependencies: Vec::new(),
        source_paths: Vec::new(),
    });
    let id = value(
        workspace
            .begin_java_startup_worker(move || Ok(launch), Duration::from_secs(5))
            .unwrap(),
    )["startup_id"]
        .as_u64()
        .unwrap();
    let ready = terminal(&mut workspace, id);
    assert_eq!(ready["state"], "ready");
    assert_eq!(
        ready["language"]["initialize"]["cedar_java_profile"],
        "maven_leaf"
    );
    assert_eq!(
        ready["language"]["initialize"]["cedar_java_maven_pom_sha256"],
        digest
    );
    assert_eq!(
        ready["language"]["initialize"]["cedar_java_maven_model"],
        true
    );
    assert_eq!(workspace.language.as_ref().unwrap().startup_id, Some(id));
    assert!(workspace.language.as_ref().unwrap().java_maven_model);
    let model = value(workspace.maven_model().unwrap());
    assert_eq!(model["status"], "unavailable");
    assert_eq!(model["pom_sha256"], digest);
    fs::write(root.join("pom.xml"), b"changed").unwrap();
    assert_eq!(
        workspace.maven_model().unwrap_err().code,
        "language_maven_restart_required"
    );
    let pid = ready["language"]["process_id"].as_u64().unwrap() as u32;
    let observed = ObservedProcess::open(pid);
    workspace.set_allow_run(false);
    assert_eq!(
        value(workspace.cancel_java_startup(id).unwrap())["state"],
        "cancelling"
    );
    assert!(workspace.language.is_none());
    cancellation_terminal(&terminal(&mut workspace, id));
    observed.assert_dead();
    assert_eq!(
        workspace.maven_model().unwrap_err().code,
        "language_not_running"
    );
}
