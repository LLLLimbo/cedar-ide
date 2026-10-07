//! One bounded, completion-owned overlapped stdin write. No Write trait or flush.

use super::handles::{adopt, raw};
use super::pipes::{
    create_server_pipe, open_child_endpoint, unexpected_client, verify_child_endpoint,
};
use super::security::PipeSecurity;
use crate::{StdinCancelOutcome, StdinWriteProgress, MAX_STDIN_WRITE_BYTES};
use std::cell::UnsafeCell;
use std::io;
use std::marker::PhantomPinned;
use std::os::windows::io::OwnedHandle;
use std::pin::Pin;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{
    ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_NOT_FOUND, ERROR_OPERATION_ABORTED,
    ERROR_PIPE_CONNECTED, GENERIC_READ,
};
use windows_sys::Win32::Storage::FileSystem::WriteFile;
use windows_sys::Win32::System::Pipes::ConnectNamedPipe;
use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

struct WriteStorage {
    overlapped: UnsafeCell<OVERLAPPED>,
    buffer: UnsafeCell<[u8; MAX_STDIN_WRITE_BYTES]>,
    _pinned: PhantomPinned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Connecting,
    Writing(usize),
    Stopped,
}

pub(crate) struct PendingWrite {
    storage: Pin<Box<WriteStorage>>,
    event: OwnedHandle,
    pipe: OwnedHandle,
    state: State,
}

// SAFETY: moving the exclusive owner does not move its pinned allocation. The
// kernel may read the buffer and write OVERLAPPED until completion; UnsafeCell
// prevents Sync, and all submission/inspection uses &mut self. CancelIoEx and
// GetOverlappedResult support completing I/O from a different owner thread.
unsafe impl Send for PendingWrite {}

impl PendingWrite {
    pub(crate) fn connect(
        name: &[u16],
        security: &PipeSecurity,
    ) -> io::Result<(Self, OwnedHandle)> {
        let mut write = Self::new(create_server_pipe(name, security, true)?)?;
        write.start_connect()?;
        let read = open_child_endpoint(name, GENERIC_READ)?;
        match write.complete(true)? {
            StdinWriteProgress::Idle => {}
            _ => return Err(io::Error::other("stdin pipe connection did not complete")),
        }
        verify_child_endpoint(&write.pipe, &read)?;
        Ok((write, read))
    }

    fn new(pipe: OwnedHandle) -> io::Result<Self> {
        // SAFETY: manual-reset event, unnamed and non-inheritable. It outlives
        // any I/O that refers to it, including failure and unwind cleanup.
        let event = unsafe { adopt(CreateEventW(null(), 1, 0, null()))? };
        let storage = Box::pin(WriteStorage {
            overlapped: UnsafeCell::new(OVERLAPPED {
                hEvent: raw(&event),
                ..Default::default()
            }),
            buffer: UnsafeCell::new([0; MAX_STDIN_WRITE_BYTES]),
            _pinned: PhantomPinned,
        });
        Ok(Self {
            storage,
            event,
            pipe,
            state: State::Idle,
        })
    }

    fn overlapped(&self) -> *mut OVERLAPPED {
        self.storage.overlapped.get()
    }

    fn prepare(&mut self) -> io::Result<()> {
        debug_assert_eq!(self.state, State::Idle);
        // SAFETY: Idle means no kernel operation refers to the allocation.
        unsafe {
            if ResetEvent(raw(&self.event)) == 0 {
                return Err(io::Error::last_os_error());
            }
            self.overlapped().write(OVERLAPPED {
                hEvent: raw(&self.event),
                ..Default::default()
            });
        }
        Ok(())
    }

    fn start_connect(&mut self) -> io::Result<()> {
        self.prepare()?;
        self.state = State::Connecting;
        // SAFETY: the overlapped server, event and stable storage are owned;
        // Drop cancels and joins even if opening the client endpoint fails.
        if unsafe { ConnectNamedPipe(raw(&self.pipe), self.overlapped()) } != 0 {
            let _ = self.complete(true)?;
            return Err(unexpected_client());
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error().map(|code| code as u32) {
            Some(ERROR_IO_PENDING) => Ok(()),
            Some(ERROR_PIPE_CONNECTED) => {
                self.state = State::Stopped; // no request was queued
                Err(unexpected_client())
            }
            _ => {
                self.state = State::Stopped; // synchronous submission failure
                Err(error)
            }
        }
    }

    pub(crate) fn begin(&mut self, bytes: &[u8]) -> io::Result<StdinWriteProgress> {
        match self.state {
            State::Idle => {}
            State::Stopped => return Err(closed()),
            _ => return Err(busy()),
        }
        if bytes.len() > MAX_STDIN_WRITE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stdin write exceeds 64 KiB",
            ));
        }
        // A zero-length application chunk needs no OS request; a Win32 null
        // write has device-specific behavior and must not be confused with EOF.
        if bytes.is_empty() {
            return Ok(StdinWriteProgress::Written(0));
        }
        self.prepare()?;
        // SAFETY: Idle excludes kernel access. Copy before submission, without
        // constructing a Rust reference that would survive a pending write.
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.storage.buffer.get().cast::<u8>(),
                bytes.len(),
            );
        }
        self.state = State::Writing(bytes.len());
        // SAFETY: only the owned overlapped handle is used. Both pointers and
        // event remain live and untouched until GetOverlappedResult completes.
        // The async byte-count pointer is null even for synchronous completion.
        let result = unsafe {
            WriteFile(
                raw(&self.pipe),
                self.storage.buffer.get().cast(),
                bytes.len() as u32,
                null_mut(),
                self.overlapped(),
            )
        };
        if result == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
                self.state = State::Stopped;
                return Err(error);
            }
        }
        self.complete(false)
    }

    pub(crate) fn poll(&mut self) -> io::Result<StdinWriteProgress> {
        match self.state {
            State::Idle => Ok(StdinWriteProgress::Idle),
            State::Stopped => Ok(StdinWriteProgress::Closed),
            State::Writing(_) => self.complete(false),
            State::Connecting => Err(io::Error::other("stdin pipe is not connected")),
        }
    }

    fn complete(&mut self, wait: bool) -> io::Result<StdinWriteProgress> {
        debug_assert!(self.is_pending());
        let operation = self.state;
        let mut transferred = 0;
        // SAFETY: handle and OVERLAPPED identify exactly this outstanding I/O.
        // Success or a terminal error establishes completion before state reuse.
        let result = unsafe {
            GetOverlappedResult(
                raw(&self.pipe),
                self.overlapped(),
                &mut transferred,
                wait.into(),
            )
        };
        if result == 0 {
            let error = io::Error::last_os_error();
            if matches!(
                error.raw_os_error().map(|code| code as u32),
                Some(ERROR_IO_INCOMPLETE | ERROR_IO_PENDING)
            ) {
                return Ok(StdinWriteProgress::Pending);
            }
            self.state = State::Stopped;
            return Err(error);
        }
        self.state = State::Idle;
        if let State::Writing(requested) = operation {
            match completed_bytes(requested, transferred as usize) {
                Ok(bytes) => Ok(StdinWriteProgress::Written(bytes)),
                Err(error) => {
                    self.state = State::Stopped;
                    Err(error)
                }
            }
        } else {
            Ok(StdinWriteProgress::Idle)
        }
    }

    pub(crate) fn is_pending(&self) -> bool {
        matches!(self.state, State::Connecting | State::Writing(_))
    }

    fn request_cancel(&mut self) -> io::Result<()> {
        if !self.is_pending() {
            return Ok(());
        }
        // SAFETY: CancelIoEx merely requests cancellation of our live I/O.
        // NOT_FOUND may be a completion race; it never authorizes early freeing.
        if unsafe { CancelIoEx(raw(&self.pipe), self.overlapped()) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NOT_FOUND as i32) {
                return Err(error);
            }
        }
        Ok(())
    }

    fn complete_cancel(&mut self) -> io::Result<StdinCancelOutcome> {
        while self.is_pending() {
            match self.complete(true) {
                Ok(StdinWriteProgress::Pending) => {
                    // Keep the allocation alive through exceptional driver
                    // delays. A timer does not permit freeing kernel I/O state.
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Ok(StdinWriteProgress::Written(bytes)) => {
                    self.state = State::Stopped;
                    return Ok(StdinCancelOutcome::Written(bytes));
                }
                Ok(_) => break,
                Err(error) if error.raw_os_error() == Some(ERROR_OPERATION_ABORTED as i32) => {
                    return Ok(StdinCancelOutcome::Cancelled);
                }
                Err(error) => return Err(error),
            }
        }
        self.state = State::Stopped;
        Ok(StdinCancelOutcome::Idle)
    }

    pub(crate) fn cancel_and_complete(&mut self) -> io::Result<StdinCancelOutcome> {
        let cancelled = self.request_cancel();
        // Always establish completion, even if requesting cancellation failed.
        let completed = self.complete_cancel();
        cancelled.and(completed)
    }
}

impl Drop for PendingWrite {
    fn drop(&mut self) {
        let _ = self.cancel_and_complete();
        // CloseHandle only after completion. Never DisconnectNamedPipe (which
        // discards unread data), nor FlushFileBuffers (waits for consumption).
    }
}

fn completed_bytes(requested: usize, written: usize) -> io::Result<usize> {
    if written > requested {
        Err(io::Error::other(
            "stdin write returned an invalid byte count",
        ))
    } else if written == 0 {
        Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "stdin write made no progress",
        ))
    } else {
        Ok(written)
    }
}

pub(super) fn closed() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "stdin is closed or configured as NUL",
    )
}

pub(super) fn busy() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "stdin write completion is still outstanding",
    )
}

#[cfg(test)]
mod tests {
    use super::super::pipes::random_pipe_name;
    use super::*;
    use std::fs::File;
    use std::io::Read;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{
        GetHandleInformation, ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, HANDLE_FLAG_INHERIT,
        WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    fn pipe() -> (PendingWrite, OwnedHandle) {
        PendingWrite::connect(
            &random_pipe_name().unwrap(),
            &PipeSecurity::for_current_logon().unwrap(),
        )
        .unwrap()
    }

    // Named-pipe buffer sizes are advisory. Bound this setup while requiring
    // an actual kernel-pending write, not a synthetic blocked-state assertion.
    fn fill(write: &mut PendingWrite, bytes: &[u8]) -> usize {
        assert_eq!(bytes.len(), MAX_STDIN_WRITE_BYTES);
        let mut accepted = 0;
        for _ in 0..16 {
            match write.begin(bytes).unwrap() {
                StdinWriteProgress::Written(n) => accepted += n,
                StdinWriteProgress::Pending => return accepted,
                other => panic!("unexpected fill result: {other:?}"),
            }
        }
        panic!("nonreading pipe did not produce a pending write within 1 MiB");
    }

    #[test]
    fn stdin_inherits_only_the_verified_synchronous_reader() {
        let (write, read) = pipe();
        for (handle, inherited) in [(&read, true), (&write.pipe, false), (&write.event, false)] {
            let mut flags = 0;
            // SAFETY: each owned handle remains alive; flags is writable.
            assert_ne!(unsafe { GetHandleInformation(raw(handle), &mut flags) }, 0);
            assert_eq!(flags & HANDLE_FLAG_INHERIT != 0, inherited);
        }
    }

    #[test]
    fn completion_counts_accept_short_writes_and_reject_zero_or_excess() {
        assert_eq!(completed_bytes(10, 4).unwrap(), 4);
        assert_eq!(completed_bytes(10, 10).unwrap(), 10);
        assert_eq!(
            completed_bytes(10, 0).unwrap_err().kind(),
            io::ErrorKind::WriteZero
        );
        assert!(completed_bytes(10, 11).is_err());
    }

    #[test]
    fn already_closed_reader_rejects_submission_and_stops_future_writes() {
        let (mut write, read) = pipe();
        drop(read);
        let error = write.begin(b"undeliverable").unwrap_err();
        use windows_sys::Win32::Foundation::{
            ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_NOT_CONNECTED,
        };
        assert!(
            matches!(
                error.raw_os_error().map(|code| code as u32),
                Some(ERROR_BROKEN_PIPE | ERROR_NO_DATA | ERROR_PIPE_NOT_CONNECTED)
            ),
            "{error}"
        );
        assert!(!write.is_pending());
        assert_eq!(write.poll().unwrap(), StdinWriteProgress::Closed);
        assert_eq!(
            write.begin(b"no replay").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(
            write.cancel_and_complete().unwrap(),
            StdinCancelOutcome::Idle
        );
    }

    #[test]
    fn bounded_submission_and_empty_chunk_do_not_change_pipe_state() {
        let (mut write, read) = pipe();
        assert_eq!(write.begin(&[]).unwrap(), StdinWriteProgress::Written(0));
        assert_eq!(write.poll().unwrap(), StdinWriteProgress::Idle);
        assert_eq!(
            write
                .begin(&vec![0; MAX_STDIN_WRITE_BYTES + 1])
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        let mut source = b"owned bytes before EOF".to_vec();
        let progress = write.begin(&source).unwrap();
        source.fill(b'x');
        assert_eq!(progress, StdinWriteProgress::Written(source.len()));
        assert_eq!(write.poll().unwrap(), StdinWriteProgress::Idle);
        drop(write); // CloseHandle, not FlushFileBuffers or DisconnectNamedPipe
        let mut input = Vec::new();
        File::from(read).read_to_end(&mut input).unwrap();
        assert_eq!(input, b"owned bytes before EOF");
    }

    #[test]
    fn full_pipe_cancel_joins_live_io_without_waiting_for_child_read() {
        let (mut write, read) = pipe();
        fill(&mut write, &[b'f'; MAX_STDIN_WRITE_BYTES]);
        assert_eq!(write.poll().unwrap(), StdinWriteProgress::Pending);
        assert_eq!(
            write.begin(b"cannot replace").unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            write.begin(&[]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        let started = Instant::now();
        assert_eq!(
            write.cancel_and_complete().unwrap(),
            StdinCancelOutcome::Cancelled
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(!write.is_pending());
        assert_eq!(write.poll().unwrap(), StdinWriteProgress::Closed);
        assert_eq!(
            write.begin(b"no replay").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(
            write.cancel_and_complete().unwrap(),
            StdinCancelOutcome::Idle
        );
        drop(read); // reader was open, and never read, throughout cancellation
    }

    #[test]
    fn pending_write_owns_source_and_completed_write_wins_cancel_race() {
        let (mut write, read) = pipe();
        let mut source = vec![b'f'; MAX_STDIN_WRITE_BYTES];
        let accepted = fill(&mut write, &source);
        assert_eq!(write.poll().unwrap(), StdinWriteProgress::Pending);
        source.fill(b'x');
        assert_eq!(write.poll().unwrap(), StdinWriteProgress::Pending);
        drop(source);
        // This exercises caller-buffer mutation and destruction during a real
        // pending operation, then checks exact submitted bytes after draining.
        // It does not control when Windows internally copies submitted data.
        let mut input = vec![0; accepted + MAX_STDIN_WRITE_BYTES];
        File::from(read).read_exact(&mut input).unwrap();
        assert!(input.iter().all(|byte| *byte == b'f'));
        // SAFETY: the owned manual-reset event remains alive while waiting.
        assert_eq!(
            unsafe { WaitForSingleObject(raw(&write.event), 5_000) },
            WAIT_OBJECT_0
        );
        assert!(write.is_pending()); // still unobserved by GetOverlappedResult
        assert_eq!(
            write.cancel_and_complete().unwrap(),
            StdinCancelOutcome::Written(MAX_STDIN_WRITE_BYTES)
        );
        assert_eq!(write.poll().unwrap(), StdinWriteProgress::Closed);
    }

    #[test]
    fn pending_write_can_move_to_a_joined_cancellation_thread() {
        let (mut write, read) = pipe();
        fill(&mut write, &[b'f'; MAX_STDIN_WRITE_BYTES]);
        let outcome = std::thread::scope(|scope| {
            scope
                .spawn(move || write.cancel_and_complete())
                .join()
                .unwrap()
                .unwrap()
        });
        assert_eq!(outcome, StdinCancelOutcome::Cancelled);
        drop(read);
    }

    #[test]
    fn outbound_name_collision_fails_before_connecting_to_existing_pipe() {
        let security = PipeSecurity::for_current_logon().unwrap();
        let name = random_pipe_name().unwrap();
        let (mut write, read) = PendingWrite::connect(&name, &security).unwrap();
        let error = match PendingWrite::connect(&name, &security) {
            Ok(_) => panic!("second stdin server accepted"),
            Err(error) => error,
        };
        assert!(matches!(
            error.raw_os_error().map(|n| n as u32),
            Some(ERROR_ACCESS_DENIED | ERROR_PIPE_BUSY)
        ));
        assert_eq!(
            write.begin(b"original").unwrap(),
            StdinWriteProgress::Written(8)
        );
        drop(write);
        let mut input = Vec::new();
        File::from(read).read_to_end(&mut input).unwrap();
        assert_eq!(input, b"original");
    }

    #[test]
    fn early_stdin_client_is_rejected_and_failed_connect_is_joined() {
        let security = PipeSecurity::for_current_logon().unwrap();
        let name = random_pipe_name().unwrap();
        let mut write =
            PendingWrite::new(create_server_pipe(&name, &security, true).unwrap()).unwrap();
        let _read = open_child_endpoint(&name, GENERIC_READ).unwrap();
        assert_eq!(
            write.start_connect().unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        let mut pending = PendingWrite::new(
            create_server_pipe(&random_pipe_name().unwrap(), &security, true).unwrap(),
        )
        .unwrap();
        pending.start_connect().unwrap();
        assert_eq!(pending.state, State::Connecting);
        let started = Instant::now();
        drop(pending);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
