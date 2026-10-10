//! Actual Linux process/pipe regressions for the generic owned transport.
//! Descendants stay in the original group; escaped descendants and abrupt
//! agent death are deliberately outside these observations.
#![cfg(all(target_os = "linux", feature = "test-server"))]

use cedar_language::{
    ClientOptions, Error, LinuxCleanupErrors, LinuxCleanupStatus, LinuxExitStatus, LinuxRootExit,
    LinuxShutdownOutcome, LinuxShutdownReason, LspClient, LspEvent, ProcessConfig, RpcEvent,
    ShutdownOutcome, StdioRpc,
};
use serde_json::json;
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::sync::{mpsc, Arc, Barrier, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

// Serialize only this integration executable, so its own FD/thread comparison
// is meaningful without requiring a process-global signal/subreaper setting.
static SERIAL: Mutex<()> = Mutex::new(());
const GRACE: Duration = Duration::from_millis(300);

struct Case {
    _serial: MutexGuard<'static, ()>,
    stop: mpsc::Sender<()>,
    watchdog: Option<thread::JoinHandle<()>>,
}

impl Case {
    fn start() -> Self {
        let serial = SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (stop, stopped) = mpsc::channel();
        let watchdog = thread::spawn(move || {
            if stopped.recv_timeout(Duration::from_secs(15)).is_err() {
                eprintln!("Linux owned LSP regression exceeded its bounded watchdog");
                std::process::exit(126);
            }
        });
        Self {
            _serial: serial,
            stop,
            watchdog: Some(watchdog),
        }
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(watchdog) = self.watchdog.take() {
            let _ = watchdog.join();
        }
    }
}

/// A live descriptor for the fixture's exact lifetime file. flock observation
/// cannot signal a reused PID. Root reap is checked separately in its report;
/// descendant locks prove release, not that this test reaped descendants.
struct Lifetime {
    file: File,
}

impl Lifetime {
    fn open(dir: &Path, name: &str) -> Self {
        let lifetime = Self {
            file: OpenOptions::new()
                .write(true)
                .open(dir.join(format!("{name}.lock")))
                .unwrap(),
        };
        lifetime.assert_alive();
        lifetime
    }

    fn released(&self) -> bool {
        // SAFETY: The descriptor remains owned throughout this nonblocking
        // lock probe. A successful probe is immediately unlocked.
        let status = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if status == 0 {
            assert_eq!(
                unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) },
                0
            );
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
            !self.released(),
            "fixture released its lifetime file too early"
        );
    }

    fn assert_released(&self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !self.released() {
            assert!(
                Instant::now() < deadline,
                "owned fixture still retains its lifetime file"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
}

fn temp() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("cedar linux ownership 雪 ")
        .tempdir()
        .unwrap()
}

fn config(mode: &str, dir: &Path) -> ProcessConfig {
    let mut config = ProcessConfig::new(env!("CARGO_BIN_EXE_cedar-mock-lsp"));
    config.args = vec![mode.into(), dir.into()];
    config.working_directory = Some(dir.to_path_buf());
    config
}

fn options() -> ClientOptions {
    ClientOptions {
        request_timeout: Duration::from_millis(500),
        shutdown_timeout: GRACE,
        outbound_capacity: 2,
        event_capacity: 8,
        max_pending_requests: 8,
        ..ClientOptions::default()
    }
}

fn launch(mode: &str, dir: &Path) -> (StdioRpc, Lifetime) {
    let rpc = StdioRpc::spawn(config(mode, dir), options()).unwrap();
    assert!(matches!(rpc.next_event(Duration::from_secs(3)).unwrap(),
        Some(RpcEvent::Notification { method, .. }) if method == "mock/ready"));
    let lifetime = Lifetime::open(dir, "root");
    (rpc, lifetime)
}

fn launch_lsp(mode: &str, dir: &Path) -> (LspClient, Lifetime) {
    let client = LspClient::spawn(config(mode, dir), options()).unwrap();
    assert!(matches!(client.next_event(Duration::from_secs(3)).unwrap(),
        Some(LspEvent::Notification { method, .. }) if method == "mock/ready"));
    let lifetime = Lifetime::open(dir, "root");
    client.initialize(None, json!({})).unwrap();
    (client, lifetime)
}

fn wait_marker(path: &Path, expected: &[u8]) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if fs::read(path).is_ok_and(|bytes| bytes == expected) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "fixture marker did not arrive: {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn assert_no_watchdog(dir: &Path) {
    for name in ["root", "child", "grandchild"] {
        assert!(
            !dir.join(format!("{name}.expired")).exists(),
            "fixture watchdog ended {name}"
        );
    }
}

fn assert_joined(outcome: LinuxShutdownOutcome) {
    assert_eq!(outcome.cleanup, LinuxCleanupStatus::Joined, "{outcome:?}");
    assert_eq!(outcome.errors, LinuxCleanupErrors::default(), "{outcome:?}");
    assert!(
        outcome.root_reaped && outcome.io_released && outcome.worker_joined,
        "{outcome:?}"
    );
    assert_eq!(outcome.cleanup_observation_budget_ms, 3000);
    assert!(outcome.cleanup_observation_elapsed_ms < 4000, "{outcome:?}");
}

fn assert_cached(client: &LspClient, first: &(Result<(), Error>, ShutdownOutcome)) {
    let repeated = client.shutdown_with_outcome();
    assert_eq!(repeated.1, first.1);
    assert_eq!(
        repeated.0.map_err(|error| error.to_string()),
        first.0.clone().map_err(|error| error.to_string())
    );
    assert_eq!(
        client.shutdown().map_err(|error| error.to_string()),
        first.0.clone().map_err(|error| error.to_string())
    );
}

fn terminal(rpc: &StdioRpc) -> Error {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        assert!(
            Instant::now() < deadline,
            "transport did not publish termination"
        );
        match rpc.next_event(Duration::from_millis(50)) {
            Ok(Some(RpcEvent::Closed(error))) | Err(error) => return error,
            _ => {}
        }
    }
}

#[test]
fn exit_frame_then_stdin_eof_distinguishes_natural_zero_nonzero_and_signal() {
    let _case = Case::start();
    for (mode, expected, graceful) in [
        ("linux-lsp-exit-eof", LinuxExitStatus::Code(0), true),
        ("linux-lsp-nonzero", LinuxExitStatus::Code(23), false),
        (
            "linux-lsp-signal",
            LinuxExitStatus::Signal(libc::SIGTERM),
            false,
        ),
    ] {
        let dir = temp();
        let (client, root) = launch_lsp(mode, dir.path());
        let result = client.shutdown_with_outcome();
        result.0.clone().unwrap();
        assert!(result.1.shutdown_response_received && result.1.exit_frame_completed);
        let owned = result.1.linux.unwrap();
        assert_eq!(
            owned.reason,
            LinuxShutdownReason::RootExited,
            "{mode}: {owned:?}"
        );
        assert_eq!(owned.root_exit, LinuxRootExit::BeforeTermination(expected));
        assert!(!owned.transport_failure_observed);
        assert_joined(owned);
        assert_eq!(result.1.is_graceful(), graceful, "{mode}: {:?}", result.1);
        assert_cached(&client, &result);
        wait_marker(&dir.path().join("stdin-eof.ready"), b"verified");
        root.assert_released();
        drop(client);
        assert_no_watchdog(dir.path());
    }
}

#[test]
fn stdout_eof_during_grace_preserves_live_root_until_natural_release() {
    let _case = Case::start();
    let dir = temp();
    let (client, root) = launch_lsp("linux-lsp-grace-release", dir.path());
    let client = Arc::new(client);
    let shutting_down = Arc::clone(&client);
    let (done, result) = mpsc::channel();
    let worker = thread::spawn(move || {
        done.send(shutting_down.shutdown_with_outcome()).unwrap();
    });
    wait_marker(&dir.path().join("stdout-closed.ready"), b"closed");
    assert!(matches!(
        result.recv_timeout(Duration::from_millis(30)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    root.assert_alive();
    fs::write(dir.path().join("release"), b"release").unwrap();
    let result = result.recv_timeout(Duration::from_secs(4)).unwrap();
    result.0.clone().unwrap();
    assert!(result.1.is_graceful(), "{:?}", result.1);
    assert_joined(result.1.linux.unwrap());
    assert_cached(&client, &result);
    worker.join().unwrap();
    root.assert_released();
    assert_no_watchdog(dir.path());
}

#[test]
fn grace_expiry_is_forced_signal_cleanup_and_never_graceful() {
    let _case = Case::start();
    let dir = temp();
    let (client, root) = launch_lsp("linux-lsp-grace-stalled", dir.path());
    let start = Instant::now();
    let result = client.shutdown_with_outcome();
    result.0.clone().unwrap(); // Preserve the existing protocol result semantics.
    assert!(start.elapsed() >= GRACE);
    assert!(start.elapsed() < Duration::from_secs(4));
    assert!(!result.1.is_graceful());
    assert!(result.1.shutdown_response_received && result.1.exit_frame_completed);
    let owned = result.1.linux.unwrap();
    assert_eq!(owned.reason, LinuxShutdownReason::GraceExpired);
    assert_eq!(
        owned.root_exit,
        LinuxRootExit::AfterTermination(LinuxExitStatus::Signal(libc::SIGKILL))
    );
    assert!(!owned.transport_failure_observed);
    assert_joined(owned);
    assert_cached(&client, &result);
    root.assert_released();
    assert_no_watchdog(dir.path());
}

#[test]
fn malformed_or_truncated_final_output_cannot_upgrade_to_graceful() {
    let _case = Case::start();
    for mode in ["linux-lsp-final-truncated", "linux-lsp-final-malformed"] {
        let dir = temp();
        let (client, root) = launch_lsp(mode, dir.path());
        let result = client.shutdown_with_outcome();
        assert!(result.1.shutdown_response_received);
        let owned = result.1.linux.unwrap();
        assert!(owned.transport_failure_observed, "{mode}: {owned:?}");
        assert!(!result.1.is_graceful(), "{mode}: {:?}", result.1);
        assert!(matches!(
            owned.reason,
            LinuxShutdownReason::RootExited | LinuxShutdownReason::TransportFailure
        ));
        assert_joined(owned);
        assert_cached(&client, &result);
        root.assert_released();
        assert_no_watchdog(dir.path());
    }
}

#[test]
fn premature_stdout_eof_cleans_live_root() {
    let _case = Case::start();
    let dir = temp();
    let (rpc, root) = launch("linux-premature-eof", dir.path());
    fs::write(dir.path().join("go"), b"go").unwrap();
    assert!(matches!(terminal(&rpc), Error::Closed(_)));
    rpc.finish_process().unwrap();
    let owned = rpc.linux_shutdown_outcome().unwrap();
    assert_eq!(owned.reason, LinuxShutdownReason::TransportFailure);
    assert!(owned.transport_failure_observed);
    assert_eq!(
        owned.root_exit,
        LinuxRootExit::AfterTermination(LinuxExitStatus::Signal(libc::SIGKILL))
    );
    assert_joined(owned);
    root.assert_released();
    assert_no_watchdog(dir.path());
}

#[test]
fn final_reply_is_delivered_before_root_eof() {
    let _case = Case::start();
    let dir = temp();
    let (rpc, root) = launch("linux-final-response", dir.path());
    // Exceed one bounded stdout capture round, so final draining must preserve
    // a complete buffered reply rather than only one small pipe read.
    let payload = json!({"text":"雪 🦀", "data":"x".repeat(96 * 1024)});
    assert_eq!(rpc.request("mock/echo", payload.clone()).unwrap(), payload);
    assert!(matches!(terminal(&rpc), Error::Closed(_)));
    rpc.finish_process().unwrap();
    let owned = rpc.linux_shutdown_outcome().unwrap();
    // Raw RPC has not armed graceful shutdown. Kernel stdout EOF may precede
    // waitable root exit, so the owner may terminate that still-running root.
    // The LSP shutdown test above checks natural-exit classification precisely.
    assert!(matches!(
        owned.root_exit,
        LinuxRootExit::BeforeTermination(LinuxExitStatus::Code(0))
            | LinuxRootExit::AfterTermination(
                LinuxExitStatus::Code(0) | LinuxExitStatus::Signal(libc::SIGKILL)
            )
    ));
    assert_joined(owned);
    root.assert_released();
    assert_no_watchdog(dir.path());
}

#[test]
fn root_exit_cleans_descendants_holding_both_pipes_in_original_group() {
    let _case = Case::start();
    let dir = temp();
    let (rpc, root) = launch("linux-tree-exit", dir.path());
    let child = Lifetime::open(dir.path(), "child");
    let grandchild = Lifetime::open(dir.path(), "grandchild");
    fs::write(dir.path().join("go"), b"go").unwrap();
    assert!(matches!(terminal(&rpc), Error::Closed(_)));
    rpc.finish_process().unwrap();
    let owned = rpc.linux_shutdown_outcome().unwrap();
    assert_eq!(
        owned.root_exit,
        LinuxRootExit::BeforeTermination(LinuxExitStatus::Code(0))
    );
    assert_joined(owned);
    root.assert_released();
    child.assert_released();
    grandchild.assert_released();
    assert_no_watchdog(dir.path());
}

#[test]
fn blocked_stdin_deadline_cancels_nonblocking_writer_and_joins_owner() {
    let _case = Case::start();
    let dir = temp();
    let (rpc, root) = launch("linux-blocked-stdin", dir.path());
    let start = Instant::now();
    assert!(matches!(
        rpc.notify("mock/large", json!({"data":"x".repeat(900_000)})),
        Err(Error::Timeout(_) | Error::Closed(_))
    ));
    rpc.finish_process().unwrap();
    assert!(start.elapsed() < Duration::from_secs(4));
    let owned = rpc.linux_shutdown_outcome().unwrap();
    assert!(owned.transport_failure_observed);
    assert_joined(owned);
    root.assert_released();
    assert_no_watchdog(dir.path());
}

#[test]
fn abort_bypasses_full_outbound_queue_and_wakes_every_waiter() {
    let _case = Case::start();
    let dir = temp();
    let (rpc, root) = launch("linux-blocked-stdin", dir.path());
    let rpc = Arc::new(rpc);
    let barrier = Arc::new(Barrier::new(7));
    let (completed, results) = mpsc::channel();
    let workers: Vec<_> = (0..6)
        .map(|_| {
            let rpc = Arc::clone(&rpc);
            let barrier = Arc::clone(&barrier);
            let completed = completed.clone();
            thread::spawn(move || {
                let params = json!({"data":"x".repeat(900_000)});
                barrier.wait();
                completed
                    .send(rpc.request_with_timeout("mock/large", params, Duration::from_secs(6)))
                    .unwrap();
            })
        })
        .collect();
    barrier.wait();
    // Six callers cannot exhaust the eight pending slots. QueueFull is evidence
    // of an occupied writer/queue, not an inferred sleep-based readiness check.
    assert!(matches!(
        results.recv_timeout(Duration::from_secs(2)).unwrap(),
        Err(Error::QueueFull)
    ));
    let start = Instant::now();
    rpc.abort(Error::Closed("owned test abort".into()));
    assert!(start.elapsed() < Duration::from_secs(4));
    let owned = rpc.linux_shutdown_outcome().unwrap();
    assert_eq!(owned.reason, LinuxShutdownReason::Aborted);
    assert_eq!(
        owned.root_exit,
        LinuxRootExit::AfterTermination(LinuxExitStatus::Signal(libc::SIGKILL))
    );
    assert_joined(owned);
    rpc.abort(Error::Closed("repeated owned test abort".into()));
    rpc.finish_process().unwrap();
    assert_eq!(rpc.linux_shutdown_outcome(), Some(owned));
    let mut aborted = 0;
    for _ in 1..6 {
        match results.recv_timeout(Duration::from_secs(2)).unwrap() {
            Err(Error::Closed(_)) => aborted += 1,
            Err(Error::QueueFull) => {}
            other => panic!("queued call did not end with abort or QueueFull: {other:?}"),
        }
    }
    assert!(aborted > 0);
    for worker in workers {
        worker.join().unwrap();
    }
    root.assert_released();
    assert_no_watchdog(dir.path());
}

#[test]
fn startup_abort_interrupts_partially_written_initialize() {
    let _case = Case::start();
    let dir = temp();
    let client = Arc::new(
        LspClient::spawn(config("linux-initialize-partial", dir.path()), options()).unwrap(),
    );
    assert!(matches!(client.next_event(Duration::from_secs(3)).unwrap(),
        Some(LspEvent::Notification { method, .. }) if method == "mock/ready"));
    let root = Lifetime::open(dir.path(), "root");
    let abort = client.abort_handle();
    let initializer = Arc::clone(&client);
    let worker = thread::spawn(move || {
        initializer.initialize_with_deadline(
            None,
            json!({"data":"x".repeat(900_000)}),
            Duration::from_secs(60),
            Instant::now() + Duration::from_secs(75),
        )
    });
    assert!(matches!(client.next_event(Duration::from_secs(3)).unwrap(),
        Some(LspEvent::Notification { method, .. }) if method == "mock/initializePartial"));
    abort.signal();
    assert!(matches!(worker.join().unwrap(), Err(Error::Closed(_))));
    let owned = Arc::try_unwrap(client)
        .ok()
        .unwrap()
        .abort_and_join()
        .linux
        .unwrap();
    assert_eq!(owned.reason, LinuxShutdownReason::Aborted);
    assert!(!owned.transport_failure_observed);
    assert_joined(owned);
    root.assert_released();
    abort.signal(); // Retained cancellation handles remain harmless after join.
    assert_no_watchdog(dir.path());
}

fn resources() -> (usize, usize) {
    let owner_threads = fs::read_dir("/proc/self/task")
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            fs::read_to_string(entry.path().join("comm"))
                .is_ok_and(|name| name.trim() == "cedar-lsp-owner")
        })
        .count();
    // Count the named owned workers rather than unrelated libtest threads,
    // which can finish or begin while another test waits on SERIAL.
    (
        fs::read_dir("/proc/self/fd").unwrap().count(),
        owner_threads,
    )
}

#[test]
fn repeated_shutdown_and_drop_release_fds_threads_and_preserve_unrelated_owner() {
    let _case = Case::start();
    let unrelated_dir = temp();
    let (unrelated, unrelated_root) = launch("linux-echo", unrelated_dir.path());
    let baseline = resources();
    for iteration in 0..8 {
        let dir = temp();
        let source = dir.path().join("Unchanged.java");
        let bytes = b"class Unchanged { String value = \"original\"; }\n";
        fs::write(&source, bytes).unwrap();
        let (client, root) = launch_lsp("linux-lsp-exit-eof", dir.path());
        if iteration % 2 == 0 {
            let result = client.shutdown_with_outcome();
            assert!(result.1.is_graceful(), "{:?}", result.1);
            assert_joined(result.1.linux.unwrap());
            assert_cached(&client, &result);
        }
        drop(client);
        root.assert_released();
        drop(root);
        assert_eq!(fs::read(source).unwrap(), bytes);
        assert_no_watchdog(dir.path());
        unrelated_root.assert_alive();
        assert_eq!(
            unrelated
                .request("mock/echo", json!({"iteration":iteration}))
                .unwrap(),
            json!({"iteration":iteration})
        );
        assert_eq!(
            resources(),
            baseline,
            "resources accumulated after cycle {iteration}"
        );
    }
    drop(unrelated);
    unrelated_root.assert_released();
    assert_no_watchdog(unrelated_dir.path());
}
