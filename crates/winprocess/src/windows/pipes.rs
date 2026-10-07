//! Owned overlapped byte pipes, with bounded, fair polling and joined teardown.
//!
//! No output is accumulated here. Each stream retains one 8 KiB buffer, and a
//! round delivers at most four buffers per stream. The higher-level task owner
//! chooses its retention cap and finite post-exit drain policy.

use super::handles::{adopt, raw, ChildStdio};
use super::security::PipeSecurity;
use super::stdin::PendingWrite;
use crate::{CaptureProgress, Stream};
use std::cell::UnsafeCell;
use std::io;
use std::marker::PhantomPinned;
use std::mem::size_of;
use std::os::windows::io::OwnedHandle;
use std::pin::Pin;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{
    SetHandleInformation, ERROR_BROKEN_PIPE, ERROR_HANDLE_EOF, ERROR_IO_INCOMPLETE,
    ERROR_IO_PENDING, ERROR_NOT_FOUND, ERROR_OPERATION_ABORTED, ERROR_PIPE_CONNECTED, GENERIC_READ,
    GENERIC_WRITE, HANDLE_FLAG_INHERIT,
};
use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_FIRST_PIPE_INSTANCE,
    FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, PIPE_ACCESS_INBOUND,
    PIPE_ACCESS_OUTBOUND,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{CreateEventW, GetCurrentProcessId, ResetEvent};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

const READ_BYTES: usize = 8 * 1024;
const READS_PER_ROUND: usize = 4;
const PIPE_BUFFER_BYTES: u32 = 64 * 1024;

pub(crate) struct PreparedStdio {
    pub(crate) child: ChildStdio,
    pub(crate) capture: CapturePipes,
    pub(crate) stdin: Option<PendingWrite>,
}

impl PreparedStdio {
    pub(crate) fn new() -> io::Result<Self> {
        Self::prepare(false)
    }

    pub(crate) fn with_piped_stdin() -> io::Result<Self> {
        Self::prepare(true)
    }

    fn prepare(piped: bool) -> io::Result<Self> {
        let security = PipeSecurity::for_current_logon()?;
        let (stdin, child_stdin) = if piped {
            let (writer, reader) = PendingWrite::connect(&random_pipe_name()?, &security)?;
            (Some(writer), reader)
        } else {
            (None, nul_stdin()?)
        };
        let (stdout_read, stdout) = capture_pipe(&random_pipe_name()?, &security)?;
        let (stderr_read, stderr) = capture_pipe(&random_pipe_name()?, &security)?;
        Ok(Self {
            child: ChildStdio {
                stdin: child_stdin,
                stdout,
                stderr,
            },
            capture: CapturePipes {
                stdout: stdout_read,
                stderr: stderr_read,
            },
            stdin,
        })
    }
}

pub(crate) struct CapturePipes {
    stdout: PendingRead,
    stderr: PendingRead,
}

impl CapturePipes {
    pub(crate) fn capture_round(
        &mut self,
        sink: &mut dyn FnMut(Stream, &[u8]),
    ) -> io::Result<CaptureProgress> {
        // Evaluate both, even when stdout fails. A full/faulty stdout must not
        // indefinitely starve stderr, nor strand its pending I/O on an error.
        let stdout = self.stdout.capture_round(Stream::Stdout, sink);
        let stderr = self.stderr.capture_round(Stream::Stderr, sink);
        Ok(CaptureProgress {
            bytes: stdout? + stderr?,
            stdout_eof: self.stdout.state == State::Eof,
            stderr_eof: self.stderr.state == State::Eof,
        })
    }

    /// Stop issuing reads and establish completion of both outstanding reads.
    /// A last racing chunk may be discarded after the caller's finite drain.
    /// Cancellation is not transport EOF; only a real pipe-EOF result sets it.
    pub(crate) fn cancel_and_complete_pending(&mut self) -> io::Result<()> {
        // Request both cancellations before waiting for either completion.
        // Even an unexpected cancellation error does not release the storage.
        let stdout_cancel = self.stdout.request_cancel();
        let stderr_cancel = self.stderr.request_cancel();
        let stdout_complete = self.stdout.complete_cancel();
        let stderr_complete = self.stderr.complete_cancel();
        stdout_cancel
            .and(stderr_cancel)
            .and(stdout_complete)
            .and(stderr_complete)
    }
}

impl Drop for CapturePipes {
    fn drop(&mut self) {
        let _ = self.cancel_and_complete_pending();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Connect,
    Read,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Pending(Operation),
    Eof,
    Stopped,
}

/// Neither the OVERLAPPED nor the buffer moves while Windows can access it.
/// UnsafeCell acknowledges kernel writes during an outstanding operation; no
/// Rust reference to either interior exists then. Pin plus the private type
/// prevents replacing/moving the allocation's contents after I/O is submitted.
struct IoStorage {
    overlapped: UnsafeCell<OVERLAPPED>,
    buffer: UnsafeCell<[u8; READ_BYTES]>,
    _pinned: PhantomPinned,
}

struct PendingRead {
    storage: Pin<Box<IoStorage>>,
    event: OwnedHandle,
    pipe: OwnedHandle,
    state: State,
}

// SAFETY: exclusive ownership of the two handles and stable allocation moves,
// never the allocation itself. Win32 overlapped operations and CancelIoEx may
// be completed/cancelled on a different thread. All access is through &mut
// self, which also excludes a callback from observing a concurrent read. This
// type is deliberately not Sync (IoStorage contains UnsafeCell).
unsafe impl Send for PendingRead {}

enum Poll {
    Pending,
    Data(usize),
    Eof,
}

impl PendingRead {
    fn new(pipe: OwnedHandle) -> io::Result<Self> {
        // SAFETY: unnamed, manual-reset, initially unsignalled, non-inheritable
        // event. Adopt occurs immediately, before any cleanup can alter error.
        let event = unsafe { adopt(CreateEventW(null(), 1, 0, null()))? };
        let overlapped = OVERLAPPED {
            hEvent: raw(&event),
            ..Default::default()
        };
        Ok(Self {
            storage: Box::pin(IoStorage {
                overlapped: UnsafeCell::new(overlapped),
                buffer: UnsafeCell::new([0; READ_BYTES]),
                _pinned: PhantomPinned,
            }),
            event,
            pipe,
            state: State::Idle,
        })
    }

    fn overlapped(&self) -> *mut OVERLAPPED {
        self.storage.overlapped.get()
    }

    fn prepare_operation(&mut self) -> io::Result<()> {
        debug_assert_eq!(self.state, State::Idle);
        // SAFETY: Idle means no kernel operation owns this storage. The event
        // remains owned, and the entire OVERLAPPED is reset only between I/Os.
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
        self.prepare_operation()?;
        self.state = State::Pending(Operation::Connect);
        // SAFETY: owned overlapped pipe, event and pinned storage outlive the
        // operation. Drop joins this operation even if opening the client fails.
        let result = unsafe { ConnectNamedPipe(raw(&self.pipe), self.overlapped()) };
        if result != 0 {
            let _ = self.poll(true)?; // establish even synchronous completion
            return Err(unexpected_client());
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error().map(|n| n as u32) {
            Some(ERROR_IO_PENDING) => Ok(()),
            Some(ERROR_PIPE_CONNECTED) => {
                self.state = State::Idle; // no request was queued
                Err(unexpected_client())
            }
            _ => {
                self.state = State::Stopped; // submission failed, no queued I/O
                Err(error)
            }
        }
    }

    fn issue_read(&mut self) -> io::Result<Poll> {
        self.prepare_operation()?;
        self.state = State::Pending(Operation::Read);
        // SAFETY: both raw pointers target the unique stable allocation. While
        // Pending, no Rust reference reads/modifies the buffer or OVERLAPPED.
        // ReadFile's async byte-count pointer is null; GetOverlappedResult owns
        // completion observation even if this ReadFile finishes synchronously.
        let result = unsafe {
            ReadFile(
                raw(&self.pipe),
                self.storage.buffer.get().cast(),
                READ_BYTES as u32,
                null_mut(),
                self.overlapped(),
            )
        };
        if result == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
                // ReadFile failed to submit, or completed with a terminal error.
                return self.terminal_error(error);
            }
        }
        self.poll(false)
    }

    fn terminal_error(&mut self, error: io::Error) -> io::Result<Poll> {
        if matches!(
            error.raw_os_error().map(|n| n as u32),
            Some(ERROR_BROKEN_PIPE | ERROR_HANDLE_EOF)
        ) {
            self.state = State::Eof;
            Ok(Poll::Eof)
        } else {
            self.state = State::Stopped;
            Err(error)
        }
    }

    fn poll(&mut self, wait: bool) -> io::Result<Poll> {
        debug_assert!(matches!(self.state, State::Pending(_)));
        let operation = self.state;
        let mut transferred = 0;
        // SAFETY: these are exactly the handle/OVERLAPPED of the outstanding
        // operation. Windows synchronizes completion before reporting results.
        if unsafe {
            GetOverlappedResult(
                raw(&self.pipe),
                self.overlapped(),
                &mut transferred,
                wait.into(),
            )
        } != 0
        {
            self.state = State::Idle;
            if transferred as usize > READ_BYTES && operation == State::Pending(Operation::Read) {
                self.state = State::Stopped;
                return Err(io::Error::other("pipe read returned an invalid byte count"));
            }
            return Ok(Poll::Data(transferred as usize));
        }
        let error = io::Error::last_os_error();
        if matches!(
            error.raw_os_error().map(|n| n as u32),
            Some(ERROR_IO_INCOMPLETE | ERROR_IO_PENDING)
        ) {
            return Ok(Poll::Pending);
        }
        self.terminal_error(error)
    }

    fn capture_round(
        &mut self,
        stream: Stream,
        sink: &mut dyn FnMut(Stream, &[u8]),
    ) -> io::Result<usize> {
        let mut bytes = 0;
        for _ in 0..READS_PER_ROUND {
            let result = match self.state {
                State::Idle => self.issue_read()?,
                State::Pending(Operation::Read) => self.poll(false)?,
                State::Eof | State::Stopped => break,
                State::Pending(Operation::Connect) => {
                    return Err(io::Error::other("capture pipe is not connected"))
                }
            };
            match result {
                Poll::Pending | Poll::Eof => break,
                Poll::Data(len) => {
                    if len != 0 {
                        // SAFETY: completion was established and state is Idle;
                        // no read starts until the callback returns. The slice
                        // is borrowed for the callback only, never stored here.
                        let chunk = unsafe {
                            std::slice::from_raw_parts(self.storage.buffer.get().cast::<u8>(), len)
                        };
                        sink(stream, chunk);
                        bytes += len;
                    }
                    // A successful zero-byte pipe read can mean a zero-byte
                    // write. It is NOT EOF; the fixed loop still bounds work.
                }
            }
        }
        Ok(bytes)
    }

    fn request_cancel(&mut self) -> io::Result<()> {
        if !matches!(self.state, State::Pending(_)) {
            if self.state != State::Eof {
                self.state = State::Stopped;
            }
            return Ok(());
        }
        // SAFETY: the handle/storage remain owned and pending. CancelIoEx only
        // requests cancellation; neither its success nor NOT_FOUND frees them.
        if unsafe { CancelIoEx(raw(&self.pipe), self.overlapped()) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NOT_FOUND as i32) {
                return Err(error);
            }
        }
        Ok(())
    }

    fn complete_cancel(&mut self) -> io::Result<()> {
        while matches!(self.state, State::Pending(_)) {
            match self.poll(true) {
                Ok(Poll::Pending) => {
                    // A conforming pipe driver completes promptly after cancel.
                    // If it does not, preserve ownership and keep joining; a
                    // deadline is not permission to free kernel-visible memory.
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Ok(Poll::Data(_)) => self.state = State::Stopped,
                Ok(Poll::Eof) => {}
                Err(error) if error.raw_os_error() == Some(ERROR_OPERATION_ABORTED as i32) => {}
                Err(error) => return Err(error),
            }
        }
        if self.state != State::Eof {
            self.state = State::Stopped;
        }
        Ok(())
    }
}

impl Drop for PendingRead {
    fn drop(&mut self) {
        // Also covers partially constructed pipes, failed connections, and
        // unwinding through a sink callback. All fields outlive this join.
        let _ = self.request_cancel();
        let _ = self.complete_cancel();
    }
}

pub(super) fn unexpected_client() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "unexpected client occupied the private stdio pipe",
    )
}

pub(super) fn random_pipe_name() -> io::Result<Vec<u16>> {
    let mut nonce = [0u8; 16];
    // SAFETY: nonce is writable, and system-preferred RNG accepts a null
    // algorithm handle. This produces no saved key, token or credential.
    let status = unsafe {
        BCryptGenRandom(
            null_mut(),
            nonce.as_mut_ptr(),
            nonce.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 {
        return Err(io::Error::other(format!(
            "capture pipe randomness failed (NTSTATUS {status:#010x})"
        )));
    }
    use std::fmt::Write;
    let mut name = String::from(r"\\.\pipe\cedar-capture-");
    for byte in nonce {
        write!(name, "{byte:02x}").expect("writing into String cannot fail");
    }
    // Names resist accidental collision, not unauthorized clients. The DACL,
    // FIRST_PIPE_INSTANCE, local-only mode and PID check do the access work.
    Ok(name.encode_utf16().chain([0]).collect())
}

fn capture_pipe(name: &[u16], security: &PipeSecurity) -> io::Result<(PendingRead, OwnedHandle)> {
    let mut read = create_server(name, security)?;
    read.start_connect()?;
    // SAFETY: opens only this just-created name. No retry/wait can attach to an
    // unexpectedly occupied pipe. Null attributes keep writer non-inheritable
    // until the server has verified that this process connected it.
    let write = open_child_endpoint(name, GENERIC_WRITE)?;
    match read.poll(true)? {
        Poll::Data(_) => {}
        _ => return Err(io::Error::other("capture pipe connection did not complete")),
    }
    verify_child_endpoint(&read.pipe, &write)?;
    Ok((read, write))
}

pub(super) fn open_child_endpoint(name: &[u16], access: u32) -> io::Result<OwnedHandle> {
    // SAFETY: callers supply our terminated server name. The synchronous child
    // endpoint stays non-inheritable until its server verifies the client PID.
    unsafe {
        adopt(CreateFileW(
            name.as_ptr(),
            access,
            0,
            null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        ))
    }
}

pub(super) fn verify_child_endpoint(server: &OwnedHandle, child: &OwnedHandle) -> io::Result<()> {
    let mut client_pid = 0;
    // SAFETY: server owns the connected instance; pid output is writable.
    if unsafe { GetNamedPipeClientProcessId(raw(server), &mut client_pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: GetCurrentProcessId has no pointer arguments or ownership effect.
    if client_pid != unsafe { GetCurrentProcessId() } {
        return Err(unexpected_client());
    }
    // SAFETY: only our verified child endpoint becomes inheritable. The worker
    // must still name it explicitly in its exact three-handle HANDLE_LIST.
    if unsafe { SetHandleInformation(raw(child), HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn create_server(name: &[u16], security: &PipeSecurity) -> io::Result<PendingRead> {
    PendingRead::new(create_server_pipe(name, security, false)?)
}

pub(super) fn create_server_pipe(
    name: &[u16],
    security: &PipeSecurity,
    outbound: bool,
) -> io::Result<OwnedHandle> {
    if name.last() != Some(&0) || name[..name.len() - 1].contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pipe name must have exactly one terminal NUL",
        ));
    }
    let attributes = security.attributes();
    // SAFETY: internal name is terminated and security is alive during create.
    // One first instance only: reject collisions, never connect an old server.
    let access = if outbound {
        PIPE_ACCESS_OUTBOUND
    } else {
        PIPE_ACCESS_INBOUND
    };
    unsafe {
        adopt(CreateNamedPipeW(
            name.as_ptr(),
            access | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            if outbound { PIPE_BUFFER_BYTES } else { 0 },
            if outbound { 0 } else { PIPE_BUFFER_BYTES },
            0,
            &attributes,
        ))
    }
}

fn nul_stdin() -> io::Result<OwnedHandle> {
    let name = [b'N' as u16, b'U' as u16, b'L' as u16, 0];
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    // SAFETY: opens existing NUL read-only; attributes affect only this returned
    // handle's inheritance, never the device descriptor or global OS settings.
    unsafe {
        adopt(CreateFileW(
            name.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &attributes,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{
        GetHandleInformation, ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Storage::FileSystem::WriteFile;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    fn is_inheritable(handle: &OwnedHandle) -> bool {
        let mut flags = 0;
        // SAFETY: handle is borrowed alive; flags is writable.
        assert_ne!(unsafe { GetHandleInformation(raw(handle), &mut flags) }, 0);
        flags & HANDLE_FLAG_INHERIT != 0
    }

    fn write_pipe(handle: &OwnedHandle, bytes: &[u8]) {
        // Tests write less than the pipe's buffer capacity before draining.
        assert!(bytes.len() < PIPE_BUFFER_BYTES as usize);
        let mut count = 0;
        // SAFETY: this client is a synchronous owned handle. The slice and
        // byte-count output remain live throughout the synchronous call.
        assert_ne!(
            unsafe {
                WriteFile(
                    raw(handle),
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    &mut count,
                    null_mut(),
                )
            },
            0,
            "{}",
            io::Error::last_os_error()
        );
        assert_eq!(count as usize, bytes.len());
    }

    fn await_read_event(read: &PendingRead) {
        // SAFETY: event is owned and remains alive while waiting.
        assert_eq!(
            unsafe { WaitForSingleObject(raw(&read.event), 5_000) },
            WAIT_OBJECT_0
        );
    }

    #[test]
    fn only_child_standard_handles_are_inheritable() {
        let prepared = PreparedStdio::new().unwrap();
        for handle in [
            &prepared.child.stdin,
            &prepared.child.stdout,
            &prepared.child.stderr,
        ] {
            assert!(is_inheritable(handle));
        }
        for handle in [
            &prepared.capture.stdout.pipe,
            &prepared.capture.stdout.event,
            &prepared.capture.stderr.pipe,
            &prepared.capture.stderr.event,
        ] {
            assert!(!is_inheritable(handle));
        }
    }

    #[test]
    fn each_round_bounds_both_streams_and_preserves_tail_and_eof() {
        let PreparedStdio {
            child, mut capture, ..
        } = PreparedStdio::new().unwrap();
        let expected_stdout = vec![b'o'; READ_BYTES * 5];
        let expected_stderr = vec![b'e'; READ_BYTES * 5];
        write_pipe(&child.stdout, &expected_stdout);
        write_pipe(&child.stderr, &expected_stderr);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut stdout_calls = 0;
        let mut stderr_calls = 0;
        let progress = capture
            .capture_round(&mut |stream, bytes| {
                assert!(bytes.len() <= READ_BYTES);
                match stream {
                    Stream::Stdout => {
                        stdout_calls += 1;
                        stdout.extend_from_slice(bytes);
                    }
                    Stream::Stderr => {
                        stderr_calls += 1;
                        stderr.extend_from_slice(bytes);
                    }
                }
            })
            .unwrap();
        assert!((1..=READS_PER_ROUND).contains(&stdout_calls));
        assert!((1..=READS_PER_ROUND).contains(&stderr_calls));
        assert!(stdout.len() <= READS_PER_ROUND * READ_BYTES);
        assert!(stderr.len() <= READS_PER_ROUND * READ_BYTES);
        assert_eq!(progress.bytes, stdout.len() + stderr.len());
        assert!(!progress.stdout_eof && !progress.stderr_eof);
        drop(child);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let progress = capture
                .capture_round(&mut |stream, bytes| match stream {
                    Stream::Stdout => stdout.extend_from_slice(bytes),
                    Stream::Stderr => stderr.extend_from_slice(bytes),
                })
                .unwrap();
            if progress.stdout_eof && progress.stderr_eof {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(stdout, expected_stdout);
        assert_eq!(stderr, expected_stderr);
        capture.cancel_and_complete_pending().unwrap();
        let progress = capture
            .capture_round(&mut |_, _| panic!("read after EOF"))
            .unwrap();
        assert!(progress.stdout_eof && progress.stderr_eof);
    }

    #[test]
    fn pending_read_cancellation_is_prompt_and_is_not_transport_eof() {
        let PreparedStdio {
            child, mut capture, ..
        } = PreparedStdio::new().unwrap();
        let progress = capture
            .capture_round(&mut |_, _| panic!("unexpected data"))
            .unwrap();
        assert_eq!(progress, CaptureProgress::default());
        assert_eq!(capture.stdout.state, State::Pending(Operation::Read));
        assert_eq!(capture.stderr.state, State::Pending(Operation::Read));
        let started = Instant::now();
        capture.cancel_and_complete_pending().unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(capture.stdout.state, State::Stopped);
        assert_eq!(capture.stderr.state, State::Stopped);
        assert_eq!(
            capture
                .capture_round(&mut |_, _| panic!("read after cancellation"))
                .unwrap(),
            CaptureProgress::default()
        );
        // Both writers remain alive: cancellation did not manufacture closure.
        assert!(is_inheritable(&child.stdout) && is_inheritable(&child.stderr));
        capture.cancel_and_complete_pending().unwrap(); // idempotent
    }

    #[test]
    fn completion_winning_cancel_race_is_joined_and_not_reported_as_eof() {
        let PreparedStdio {
            child, mut capture, ..
        } = PreparedStdio::new().unwrap();
        capture
            .capture_round(&mut |_, _| panic!("unexpected data"))
            .unwrap();
        write_pipe(&child.stdout, b"completed before cancellation");
        await_read_event(&capture.stdout);
        // State deliberately still records pending until GetOverlappedResult.
        // CancelIoEx can now return NOT_FOUND; completion must still be joined.
        assert_eq!(capture.stdout.state, State::Pending(Operation::Read));
        capture.cancel_and_complete_pending().unwrap();
        assert_eq!(
            capture
                .capture_round(&mut |_, _| panic!("discarded racing chunk"))
                .unwrap(),
            CaptureProgress::default()
        );
    }

    #[test]
    fn pending_capture_can_move_to_a_joined_cancellation_thread() {
        let PreparedStdio {
            child, mut capture, ..
        } = PreparedStdio::new().unwrap();
        capture.capture_round(&mut |_, _| {}).unwrap();
        assert_eq!(capture.stdout.state, State::Pending(Operation::Read));
        assert_eq!(capture.stderr.state, State::Pending(Operation::Read));
        // The production primitive spawns no threads. A scoped test thread
        // proves the Send contract with already-pending kernel I/O and always
        // joins, including assertion failure/unwinding.
        let progress = std::thread::scope(|scope| {
            scope
                .spawn(move || {
                    capture.cancel_and_complete_pending().unwrap();
                    capture
                        .capture_round(&mut |_, _| panic!("data after cancellation"))
                        .unwrap()
                })
                .join()
                .unwrap()
        });
        assert_eq!(progress, CaptureProgress::default());
        assert!(is_inheritable(&child.stdout));
    }

    #[test]
    fn occupied_name_is_rejected_without_attaching_to_an_existing_pipe() {
        let security = PipeSecurity::for_current_logon().unwrap();
        let name = random_pipe_name().unwrap();
        let (mut read, write) = capture_pipe(&name, &security).unwrap();
        let before = b"original bytes buffered before collision";
        let after = b"original writer still connected after collision";
        write_pipe(&write, before);

        // Check the server-only path too: rejection must happen before any
        // ConnectNamedPipe/CreateFileW call can attach to the occupied name.
        let server_error = match create_server(&name, &security) {
            Ok(_) => panic!("existing pipe accepted a second server"),
            Err(error) => error,
        };
        let capture_error = match capture_pipe(&name, &security) {
            Ok(_) => panic!("existing pipe was accepted"),
            Err(error) => error,
        };
        // FIRST_PIPE_INSTANCE documents ACCESS_DENIED, but this fixture also
        // occupies its sole allowed instance (nMaxInstances = 1). Windows can
        // reject that exhausted instance count with PIPE_BUSY first. Accept
        // only these collision errors, never an arbitrary construction failure.
        for error in [server_error, capture_error] {
            assert!(
                matches!(
                    error.raw_os_error().map(|code| code as u32),
                    Some(ERROR_ACCESS_DENIED | ERROR_PIPE_BUSY)
                ),
                "unexpected occupied-name error: {error}"
            );
        }

        let mut client_pid = 0;
        // SAFETY: the original server is still owned; pid output is writable.
        assert_ne!(
            unsafe { GetNamedPipeClientProcessId(raw(&read.pipe), &mut client_pid) },
            0
        );
        // SAFETY: GetCurrentProcessId has no pointer or ownership arguments.
        assert_eq!(client_pid, unsafe { GetCurrentProcessId() });
        write_pipe(&write, after);
        drop(write);

        // Neither buffered bytes nor the live connection may be stolen. EOF
        // also proves a failed attempt did not retain an extra writer handle.
        let mut captured = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            read.capture_round(Stream::Stdout, &mut |_, bytes| {
                captured.extend_from_slice(bytes)
            })
            .unwrap();
            if read.state == State::Eof {
                break;
            }
            assert!(Instant::now() < deadline, "original pipe did not reach EOF");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(captured, [before.as_slice(), after.as_slice()].concat());
    }

    #[test]
    fn stdout_read_error_still_drains_stderr_in_the_same_round() {
        let security = PipeSecurity::for_current_logon().unwrap();
        let (_unused_reader, write_only) =
            capture_pipe(&random_pipe_name().unwrap(), &security).unwrap();
        let (stderr, writer) = capture_pipe(&random_pipe_name().unwrap(), &security).unwrap();
        // Deliberately supply a valid, owned write-only handle as a read source.
        // ReadFile must reject access synchronously; no handle is invalidated
        // and no pending allocation is modified to inject this failure.
        let mut capture = CapturePipes {
            stdout: PendingRead::new(write_only).unwrap(),
            stderr,
        };
        write_pipe(&writer, b"stderr survives stdout error");
        let mut observed = Vec::new();
        let error = capture
            .capture_round(&mut |stream, bytes| {
                assert_eq!(stream, Stream::Stderr);
                observed.extend_from_slice(bytes);
            })
            .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(ERROR_ACCESS_DENIED as i32));
        assert_eq!(observed, b"stderr survives stdout error");
        assert_eq!(capture.stdout.state, State::Stopped);
        capture.cancel_and_complete_pending().unwrap();
        assert_eq!(capture.stderr.state, State::Stopped);
    }

    #[test]
    fn unexpected_early_client_is_rejected_even_in_the_same_process() {
        let security = PipeSecurity::for_current_logon().unwrap();
        let name = random_pipe_name().unwrap();
        let mut read = create_server(&name, &security).unwrap();
        // SAFETY: deterministic local test client before the expected connect.
        let writer = unsafe {
            adopt(CreateFileW(
                name.as_ptr(),
                GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            ))
        }
        .unwrap();
        assert!(!is_inheritable(&writer));
        assert_eq!(
            read.start_connect().unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn failure_before_client_creation_joins_pending_connect() {
        let security = PipeSecurity::for_current_logon().unwrap();
        let mut read = create_server(&random_pipe_name().unwrap(), &security).unwrap();
        read.start_connect().unwrap();
        assert_eq!(read.state, State::Pending(Operation::Connect));
        let started = Instant::now();
        drop(read);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn panicking_output_sink_joins_other_stream_without_detached_threads() {
        let PreparedStdio {
            child, mut capture, ..
        } = PreparedStdio::new().unwrap();
        capture.capture_round(&mut |_, _| {}).unwrap();
        write_pipe(&child.stdout, b"panic in sink");
        await_read_event(&capture.stdout);
        assert_eq!(capture.stderr.state, State::Pending(Operation::Read));
        let started = Instant::now();
        let unwind = catch_unwind(AssertUnwindSafe(move || {
            let _ = capture.capture_round(&mut |_, _| panic!("intentional callback failure"));
        }));
        assert!(unwind.is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(is_inheritable(&child.stderr));
    }
}
