//! A separate integration-test executable with exactly one test: unrelated
//! parallel unit tests cannot change this process's thread/handle/fd counts.
//! These counts cover synthetic direct-child cleanup, not descendant-held pipe
//! ownership. Transport pipe readers are still detached and are not joined.
#![cfg(any(target_os = "linux", windows))]

#[allow(dead_code)]
#[path = "support/cancellation_harness.rs"]
mod harness;

use cedar_client::ConnectionCancellation;
use cedar_protocol::Payload;
use harness::*;
use std::{
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Counts {
    resources: usize,
    threads: usize,
}

#[cfg(target_os = "linux")]
fn counts() -> Counts {
    fn entries(path: &str) -> usize {
        std::fs::read_dir(path)
            .unwrap()
            .try_fold(0, |count, entry| entry.map(|_| count + 1))
            .unwrap()
    }
    Counts {
        resources: entries("/proc/self/fd"),
        threads: entries("/proc/self/task"),
    }
}

#[cfg(windows)]
fn counts() -> Counts {
    use std::mem::size_of;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetProcessHandleCount,
    };

    let mut resources = 0;
    // SAFETY: current-process pseudo handle is valid and count is writable.
    assert_ne!(
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut resources) },
        0
    );
    // SAFETY: documented snapshot call with no pointer parameters.
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    assert_ne!(raw, INVALID_HANDLE_VALUE);
    // SAFETY: the successful snapshot transfers a unique closeable handle.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    // SAFETY: live snapshot and initialized entry with its required size.
    assert_ne!(
        unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) },
        0
    );
    // SAFETY: no preconditions.
    let pid = unsafe { GetCurrentProcessId() };
    let mut threads = 0;
    loop {
        if entry.th32OwnerProcessID == pid {
            threads += 1;
        }
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        // SAFETY: same live snapshot and writable valid entry storage.
        if unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) } == 0 {
            // SAFETY: read the error immediately after the failed API call.
            assert_eq!(unsafe { GetLastError() }, ERROR_NO_MORE_FILES);
            break;
        }
    }
    assert!(threads > 0);
    Counts {
        resources: resources as usize,
        threads,
    }
}

fn sample_rss() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status
                    .lines()
                    .find(|line| line.starts_with("VmRSS:"))
                    .map(str::to_owned)
            })
    }
    #[cfg(windows)]
    {
        None
    }
}

fn settle(ceiling: Counts) -> Counts {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut previous = None;
    let mut unchanged = 0;
    loop {
        let current = counts();
        if current.resources <= ceiling.resources && current.threads <= ceiling.threads {
            unchanged = if previous == Some(current) {
                unchanged + 1
            } else {
                1
            };
            if unchanged >= 3 {
                return current;
            }
        } else {
            unchanged = 0;
        }
        assert!(
            Instant::now() < deadline,
            "resource counts did not settle: {current:?}, ceiling={ceiling:?}"
        );
        previous = Some(current);
        thread::sleep(Duration::from_millis(20));
    }
}

fn cycle() {
    // Each half waits on the exact Client-owned reaper completion. That receipt
    // is independent of the peer's EOF marker and never infers exit from a PID.
    cancelled_read_cycle();
    let fixture = Fixture::new("capability_peer");
    let mut fresh = fixture.connect(ConnectionCancellation::new());
    assert!(matches!(
        fresh.request(read()).unwrap(),
        Payload::File { .. }
    ));
    assert!(matches!(
        fresh.request(list()).unwrap(),
        Payload::Entries { .. }
    ));
    fresh.close_and_wait(CLEANUP_BOUND).unwrap();
    fixture.assert_closed(3);
}

#[test]
fn repeated_cancellation_and_reconnect_do_not_accumulate_resources() {
    let _watchdog = Watchdog::start(Duration::from_secs(25));
    for _ in 0..3 {
        cycle();
    }
    let baseline = settle(counts());
    // Fixed allowances accommodate runtime bookkeeping and bounded settling of
    // detached readers. A one-thread or one-handle/fd leak in either eight-cycle
    // batch exceeds these allowances; the second batch must also remain flat.
    let ceiling = Counts {
        resources: baseline.resources + 4,
        threads: baseline.threads + 2,
    };
    for _ in 0..8 {
        cycle();
    }
    let first = settle(ceiling);
    for _ in 0..8 {
        cycle();
    }
    let last = settle(ceiling);
    eprintln!("cancellation resources (handles on Windows, fds on Linux): baseline={baseline:?} first={first:?} last={last:?}; RSS supporting evidence={:?}", sample_rss());
    assert!(
        first.resources <= baseline.resources + 4
            && last.resources <= first.resources + 2
            && last.resources <= baseline.resources + 4
    );
    assert!(
        first.threads <= baseline.threads + 2
            && last.threads <= first.threads + 1
            && last.threads <= baseline.threads + 2
    );
}
