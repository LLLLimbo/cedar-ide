//! Actual Windows runtime acceptance; compile-only or ignored results are not passes.
#![cfg(all(windows, feature = "test-server"))]
use cedar_language::{ClientOptions, Error, ProcessConfig, RpcEvent, StdioRpc};
use serde_json::json;
use std::fs::{self, OpenOptions};
use std::path::Path;
use std::sync::{mpsc, Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

fn launch(mode: &str, dir: &Path, retain_stderr: bool) -> StdioRpc {
    let mut config = ProcessConfig::new(env!("CARGO_BIN_EXE_cedar-mock-lsp"));
    config.args = vec![mode.into(), dir.into()];
    config.working_directory = Some(dir.to_path_buf());
    config.inherit_stderr = retain_stderr;
    let rpc = StdioRpc::spawn(
        config,
        ClientOptions {
            request_timeout: Duration::from_millis(500),
            shutdown_timeout: Duration::from_millis(100),
            outbound_capacity: 2,
            event_capacity: 8,
            max_pending_requests: 8,
            ..ClientOptions::default()
        },
    )
    .unwrap();
    match rpc.next_event(Duration::from_secs(4)).unwrap().unwrap() {
        RpcEvent::Notification { method, .. } => assert_eq!(method, "mock/ready"),
        other => panic!("fixture did not become ready: {other:?}"),
    }
    rpc
}

fn temp() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("cedar transport 雪 ")
        .tempdir()
        .unwrap()
}

fn assert_dead(dir: &Path, tree: bool) {
    let names = if tree {
        &['r', 'c', 'g'][..]
    } else {
        &['r'][..]
    };
    for name in names {
        let name = match name {
            'r' => "root",
            'c' => "child",
            _ => "grandchild",
        };
        assert!(
            !dir.join(format!("{name}.expired")).exists(),
            "watchdog killed {name}; cleanup did not pass"
        );
        // Fixture retains an exclusive file for its lifetime. Opening this exact
        // file proves its owner released it, without reusing a PID as identity.
        OpenOptions::new()
            .write(true)
            .open(dir.join(format!("{name}.lock")))
            .unwrap_or_else(|e| panic!("{name} still owns its lifetime file: {e}"));
    }
}

fn terminal(rpc: &StdioRpc) -> Error {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        assert!(
            Instant::now() < deadline,
            "transport did not become terminal"
        );
        match rpc.next_event(Duration::from_millis(100)) {
            Ok(Some(RpcEvent::Closed(error))) | Err(error) => return error,
            _ => {}
        }
    }
}

#[test]
#[ignore = "requires native Windows owned-process runtime"]
fn unicode_working_directory_echo_and_joined_drop() {
    let dir = temp();
    let rpc = launch("win-echo", dir.path(), true);
    assert_eq!(
        rpc.request("mock/echo", json!({"text":"雪 🦀"})).unwrap(),
        json!({"text":"雪 🦀"})
    );
    drop(rpc);
    assert_dead(dir.path(), false);
}

#[test]
#[ignore = "requires native Windows owned-process runtime"]
fn drop_terminates_owned_descendant_tree() {
    let dir = temp();
    let rpc = launch("win-tree-blocked", dir.path(), true);
    drop(rpc);
    assert_dead(dir.path(), true);
}

#[test]
#[ignore = "requires native Windows owned-process runtime"]
fn root_exit_kills_descendants_retaining_both_pipes() {
    let dir = temp();
    let rpc = launch("win-tree-exit", dir.path(), true);
    fs::write(dir.path().join("go"), b"go").unwrap();
    assert!(matches!(terminal(&rpc), Error::Closed(_)));
    drop(rpc);
    assert_dead(dir.path(), true);
}

#[test]
#[ignore = "requires native Windows owned-process runtime"]
fn stdout_eof_kills_live_root_and_stderr_holding_descendants() {
    let dir = temp();
    let rpc = launch("win-stdout-eof-tree", dir.path(), true);
    assert_eq!(
        fs::read(dir.path().join("stdout-noninheritable.ready")).unwrap(),
        b"verified"
    );
    for name in ["root", "child", "grandchild"] {
        let error = OpenOptions::new()
            .write(true)
            .open(dir.path().join(format!("{name}.lock")))
            .unwrap_err();
        assert_eq!(
            error.raw_os_error(),
            Some(windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION as i32),
            "{name} must still own its exclusive lifetime file before stdout closes"
        );
    }
    fs::write(dir.path().join("go"), b"go").unwrap();
    assert!(
        matches!(terminal(&rpc), Error::Closed(reason) if reason.contains("server stdout reached EOF"))
    );
    drop(rpc);
    assert_dead(dir.path(), true);
}

#[test]
#[ignore = "requires native Windows owned-process runtime"]
fn stderr_eof_does_not_end_connection() {
    let dir = temp();
    let rpc = launch("win-stderr-eof", dir.path(), true);
    assert_eq!(
        rpc.request("mock/echo", json!({"n":42})).unwrap(),
        json!({"n":42})
    );
    drop(rpc);
    assert_dead(dir.path(), false);
}

#[test]
#[ignore = "requires native Windows owned-process runtime"]
fn stalled_and_trickling_frames_have_absolute_assembly_deadline() {
    for mode in [
        "win-stall-header",
        "win-trickle-header",
        "win-stall-body",
        "win-trickle-body",
    ] {
        let dir = temp();
        let rpc = launch(mode, dir.path(), true);
        fs::write(dir.path().join("go"), b"go").unwrap();
        assert!(matches!(terminal(&rpc), Error::Timeout(_)), "{mode}");
        drop(rpc);
        assert_dead(dir.path(), false);
    }
}

#[test]
#[ignore = "requires native Windows owned-process runtime"]
fn blocked_stdin_deadline_poisoning_joins_cleanup() {
    let dir = temp();
    let rpc = launch("win-blocked-stdin", dir.path(), true);
    assert!(matches!(
        rpc.notify("mock/large", json!({"data":"x".repeat(900_000)})),
        Err(Error::Timeout(_) | Error::Closed(_))
    ));
    assert!(matches!(
        rpc.request("mock/echo", json!({"n":1})),
        Err(Error::Timeout(_) | Error::Closed(_))
    ));
    drop(rpc);
    assert_dead(dir.path(), false);
}

#[test]
#[ignore = "requires native Windows owned-process runtime"]
fn abort_bypasses_full_outbound_queue_and_wakes_waiters() {
    let dir = temp();
    let rpc = Arc::new(launch("win-tree-blocked", dir.path(), true));
    let barrier = Arc::new(Barrier::new(7));
    let (completed, results) = mpsc::channel();
    let threads: Vec<_> = (0..6)
        .map(|_| {
            let rpc = Arc::clone(&rpc);
            let barrier = Arc::clone(&barrier);
            let completed = completed.clone();
            thread::spawn(move || {
                let params = json!({"data":"x".repeat(900_000)});
                barrier.wait();
                let result = rpc.request_with_timeout("mock/large", params, Duration::from_secs(5));
                completed.send(result).unwrap();
            })
        })
        .collect();
    barrier.wait();
    // A rejected enqueue proves other frames occupy the active/queued slots.
    // Fewer than eight callers excludes the pending-request limit as the cause.
    assert!(matches!(
        results.recv_timeout(Duration::from_secs(3)).unwrap(),
        Err(Error::QueueFull)
    ));
    rpc.abort(Error::Closed("test abort".into()));
    let mut aborted = 0;
    for _ in 1..6 {
        match results.recv_timeout(Duration::from_secs(2)).unwrap() {
            Err(Error::Closed(reason)) => {
                assert_eq!(reason, "test abort");
                aborted += 1;
            }
            Err(Error::QueueFull) => {}
            other => panic!("unexpected queued request outcome: {other:?}"),
        }
    }
    assert!(aborted > 0, "no pending request was woken by abort");
    for thread in threads {
        thread.join().unwrap();
    }
    drop(rpc);
    assert_dead(dir.path(), true);
}

#[test]
#[ignore = "requires native Windows owned-process runtime"]
fn stderr_flood_is_drained_and_terminal_tail_is_bounded() {
    for retain in [true, false] {
        let dir = temp();
        let rpc = launch("win-stderr-flood", dir.path(), retain);
        rpc.request("mock/floodStderr", json!(null)).unwrap();
        rpc.notify("mock/fail", json!(null)).unwrap();
        let error = terminal(&rpc).to_string();
        assert!(!error.contains("discarded-stderr-prefix"));
        assert_eq!(error.contains("retained-stderr-suffix"), retain);
        assert!(error.len() < 17_000);
        drop(rpc);
        assert_dead(dir.path(), false);
    }
}

#[test]
#[ignore = "requires native Windows owned-process runtime"]
fn simultaneous_connections_stop_independently() {
    let a = temp();
    let b = temp();
    let (left, right) = thread::scope(|scope| {
        let left = scope.spawn(|| launch("win-tree-blocked", a.path(), true));
        let right = scope.spawn(|| launch("win-echo", b.path(), true));
        (left.join().unwrap(), right.join().unwrap())
    });
    drop(left);
    assert_dead(a.path(), true);
    assert_eq!(
        right
            .request("mock/echo", json!({"text":"still alive"}))
            .unwrap(),
        json!({"text":"still alive"})
    );
    drop(right);
    assert_dead(b.path(), false);
}

#[test]
#[ignore = "requires native Windows owned-process runtime"]
fn final_response_larger_than_capture_round_is_not_discarded_on_root_exit() {
    let dir = temp();
    let rpc = launch("win-final-response", dir.path(), true);
    let result = rpc
        .request("mock/echo", json!({"data":"x".repeat(128_000)}))
        .unwrap();
    assert_eq!(result["data"].as_str().unwrap().len(), 128_000);
    drop(rpc);
    assert_dead(dir.path(), false);
}

fn current_handle_count() -> u32 {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
    let mut count = 0;
    // SAFETY: current-process pseudo handle is valid; count is writable.
    assert_ne!(
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) },
        0
    );
    count
}

fn current_thread_count() -> usize {
    use std::mem::size_of;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    // SAFETY: documented snapshot call with no pointer parameters.
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    assert_ne!(raw, INVALID_HANDLE_VALUE);
    // SAFETY: successful snapshot returns a uniquely owned closeable handle.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    // SAFETY: snapshot is live and entry has the required size and writable storage.
    assert_ne!(
        unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) },
        0
    );
    // SAFETY: no preconditions.
    let pid = unsafe { GetCurrentProcessId() };
    let mut count = 0;
    loop {
        if entry.th32OwnerProcessID == pid {
            count += 1;
        }
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        // SAFETY: same live snapshot and valid entry storage.
        if unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) } == 0 {
            // SAFETY: read error immediately after the failed call.
            assert_eq!(unsafe { GetLastError() }, ERROR_NO_MORE_FILES);
            break;
        }
    }
    assert!(count > 0);
    count
}

#[test]
#[ignore = "requires serial native Windows owned-process runtime"]
fn repeated_transport_cleanup_does_not_accumulate_host_handles_or_threads() {
    fn cycle(index: usize) {
        let dir = temp();
        let tree = index % 4 == 2;
        let mode = if tree {
            "win-tree-blocked"
        } else if index % 4 == 3 {
            "win-final-response"
        } else {
            "win-echo"
        };
        let rpc = launch(mode, dir.path(), true);
        if !tree {
            assert_eq!(
                rpc.request("mock/echo", json!({"n":index})).unwrap(),
                json!({"n":index})
            );
        }
        if index % 4 == 1 {
            rpc.abort(Error::Closed("repeat cleanup".into()));
        }
        drop(rpc); // joins the worker and all process cleanup before measurement
        assert_dead(dir.path(), tree);
    }
    for i in 0..4 {
        cycle(i);
    } // warm runtime/library caches before baseline
    let baseline = (current_handle_count(), current_thread_count());
    for i in 0..8 {
        cycle(i);
    }
    let first = (current_handle_count(), current_thread_count());
    for i in 0..8 {
        cycle(i);
    }
    let last = (current_handle_count(), current_thread_count());
    eprintln!("transport resource counts (handles, threads): baseline={baseline:?} first={first:?} final={last:?}");
    // Small fixed allowance for runtime bookkeeping; any per-session handle or
    // worker-thread leak in either eight-session batch exceeds these margins.
    assert!(first.0 <= baseline.0 + 4 && last.0 <= first.0 + 2 && last.0 <= baseline.0 + 4);
    assert!(first.1 <= baseline.1 + 2 && last.1 <= first.1 + 1 && last.1 <= baseline.1 + 2);
}
