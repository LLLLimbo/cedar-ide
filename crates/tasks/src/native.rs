use super::*;
use std::collections::VecDeque;
use std::fs;
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Instant;

const CHECK_INTERVAL: Duration = Duration::from_millis(10);
const DRAIN_TIMEOUT: Duration = Duration::from_millis(250);
const READ_CHUNK: usize = 8192;
const READS_PER_ROUND: usize = 4;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
struct Request {
    program: String,
    args: Vec<String>,
    timeout: Duration,
    accepted: Instant,
}

#[derive(Debug)]
struct Record {
    id: TaskId,
    state: TaskState,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_code: Option<i32>,
    truncated: bool,
    error: Option<String>,
    cancel_requested: bool,
    request: Option<Request>,
}

impl Record {
    fn snapshot(&self) -> TaskSnapshot {
        TaskSnapshot {
            id: self.id,
            state: self.state,
            stdout: String::from_utf8_lossy(&self.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&self.stderr).into_owned(),
            exit_code: self.exit_code,
            truncated: self.truncated,
            error: self.error.clone(),
        }
    }
}

#[derive(Debug, Default)]
struct Store {
    active: Option<Record>,
    completed: VecDeque<Record>,
    shutdown: bool,
}

#[derive(Debug, Default)]
struct Shared {
    store: Mutex<Store>,
    wake: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn finish(&self, outcome: Outcome) {
        let mut store = self.lock();
        if let Some(mut record) = store.active.take() {
            record.state = outcome.state;
            record.exit_code = outcome.status.and_then(|s| s.code());
            record.error = outcome.error.map(bounded_error);
            record.truncated |= outcome.truncated;
            record.request = None;
            if store.completed.len() == MAX_COMPLETED_TASKS {
                store.completed.pop_front();
            }
            store.completed.push_back(record);
        }
    }

    fn cancelled(&self) -> bool {
        let store = self.lock();
        store.shutdown || store.active.as_ref().is_some_and(|r| r.cancel_requested)
    }

    fn wait_tick(&self) {
        let store = self.lock();
        // Check under the same lock as cancel/notify, preventing a lost wake.
        if !store.shutdown && !store.active.as_ref().is_some_and(|r| r.cancel_requested) {
            let _ = self
                .wake
                .wait_timeout(store, CHECK_INTERVAL)
                .unwrap_or_else(|e| e.into_inner());
        }
    }
}

#[derive(Debug)]
pub(super) struct Manager {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl Manager {
    pub(super) fn new(root: &Path) -> Result<Self, TaskError> {
        let root = fs::canonicalize(root)
            .map_err(|e| TaskError::InvalidRoot(bounded_error(e.to_string())))?;
        if !root.is_dir() {
            return Err(TaskError::InvalidRoot("not a directory".into()));
        }
        let shared = Arc::new(Shared::default());
        let worker_shared = shared.clone();
        let worker = thread::Builder::new()
            .name("cedar-command-tasks".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    supervise(root, &worker_shared);
                }));
                if result.is_err() {
                    // OwnedChild's drop guard has already cleaned up. Do not
                    // restart a command or leave the task forever pending.
                    worker_shared.lock().shutdown = true;
                    worker_shared.finish(Outcome::failed("Command supervisor panicked"));
                }
            })
            .map_err(|e| TaskError::WorkerUnavailable(bounded_error(e.to_string())))?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    pub(super) fn start(
        &self,
        program: String,
        args: Vec<String>,
        timeout: Duration,
    ) -> Result<TaskId, TaskError> {
        validate(&program, &args, timeout)?;
        let mut store = self.shared.lock();
        if store.shutdown {
            return Err(TaskError::WorkerUnavailable("supervisor closed".into()));
        }
        if let Some(record) = &store.active {
            return Err(TaskError::Busy { id: record.id });
        }
        let id = NEXT_ID
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| TaskError::Capacity)?;
        store.active = Some(Record {
            id,
            state: TaskState::Starting,
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: None,
            truncated: false,
            error: None,
            cancel_requested: false,
            request: Some(Request {
                program,
                args,
                timeout,
                accepted: Instant::now(),
            }),
        });
        self.shared.wake.notify_one();
        Ok(id)
    }

    pub(super) fn poll(&self, id: TaskId) -> Result<TaskSnapshot, TaskError> {
        let store = self.shared.lock();
        store
            .active
            .iter()
            .chain(store.completed.iter())
            .find(|r| r.id == id)
            .map(Record::snapshot)
            .ok_or(TaskError::UnknownTask { id })
    }

    pub(super) fn cancel(&self, id: TaskId) -> Result<TaskSnapshot, TaskError> {
        let mut store = self.shared.lock();
        if let Some(record) = store.active.as_mut().filter(|r| r.id == id) {
            record.cancel_requested = true;
            record.state = TaskState::Cancelling;
            let snapshot = record.snapshot();
            self.shared.wake.notify_one();
            return Ok(snapshot);
        }
        store
            .completed
            .iter()
            .find(|r| r.id == id)
            .map(Record::snapshot)
            .ok_or(TaskError::UnknownTask { id })
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        {
            let mut store = self.shared.lock();
            store.shutdown = true;
            if let Some(record) = store.active.as_mut() {
                record.cancel_requested = true;
            }
            self.shared.wake.notify_one();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn validate(program: &str, args: &[String], timeout: Duration) -> Result<(), TaskError> {
    if program.is_empty()
        || program.len() > MAX_PROGRAM_BYTES
        || program.contains('\0')
        || args.len() > MAX_ARGUMENTS
        || args.iter().any(|s| s.contains('\0'))
        || args
            .iter()
            .try_fold(0usize, |sum, arg| sum.checked_add(arg.len()))
            .is_none_or(|n| n > MAX_ARGUMENT_BYTES)
    {
        return Err(TaskError::InvalidCommand);
    }
    if timeout.is_zero() || timeout > MAX_TIMEOUT {
        return Err(TaskError::InvalidTimeout);
    }
    Ok(())
}

fn bounded_error(mut message: String) -> String {
    if message.len() > MAX_ERROR_BYTES {
        let mut boundary = MAX_ERROR_BYTES;
        while !message.is_char_boundary(boundary) {
            boundary -= 1;
        }
        message.truncate(boundary);
    }
    message
}

fn supervise(root: PathBuf, shared: &Shared) {
    loop {
        let request = {
            let mut store = shared.lock();
            loop {
                if let Some(request) = store.active.as_mut().and_then(|r| r.request.take()) {
                    break request;
                }
                if store.shutdown {
                    return;
                }
                store = shared.wake.wait(store).unwrap_or_else(|e| e.into_inner());
            }
        };
        let outcome = run(&root, request, shared);
        shared.finish(outcome);
    }
}

struct Outcome {
    state: TaskState,
    status: Option<ExitStatus>,
    error: Option<String>,
    truncated: bool,
}

impl Outcome {
    fn new(state: TaskState) -> Self {
        Self {
            state,
            status: None,
            error: None,
            truncated: false,
        }
    }

    fn failed(error: impl ToString) -> Self {
        Self {
            error: Some(bounded_error(error.to_string())),
            ..Self::new(TaskState::Failed)
        }
    }
}

fn run(root: &Path, request: Request, shared: &Shared) -> Outcome {
    if shared.cancelled() {
        return Outcome::new(TaskState::Cancelled);
    }
    if request.accepted.elapsed() >= request.timeout {
        return Outcome::new(TaskState::TimedOut);
    }
    let spawned = Command::new(&request.program)
        .args(&request.args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn();
    let mut owned = match spawned {
        Ok(child) => OwnedChild::new(child),
        Err(error) => {
            return Outcome {
                error: Some(bounded_error(error.to_string())),
                ..Outcome::new(TaskState::SpawnFailed)
            }
        }
    };
    let mut pipes = match Pipes::take(&mut owned.child) {
        Ok(pipes) => pipes,
        Err(error) => {
            let mut outcome = Outcome::failed(error);
            outcome.status = owned.signal_and_reap().ok();
            outcome.truncated = true;
            return outcome;
        }
    };
    {
        let mut store = shared.lock();
        if let Some(record) = store.active.as_mut() {
            if !record.cancel_requested {
                record.state = TaskState::Running;
            }
        }
    }
    let mut outcome = loop {
        pipes.capture(shared);
        // Always establish continued exclusive wait ownership before signaling.
        // A competing reaper invalidates cached PID/group identity: fail closed.
        let exited = match owned.exited() {
            Ok(exited) => exited,
            Err(error) => break Outcome::failed(error),
        };
        if pipes.output_limit {
            break Outcome::new(TaskState::OutputLimit);
        }
        if let Some(error) = &pipes.read_error {
            break Outcome::failed(error);
        }
        // Natural exit observed here wins a simultaneous cancel/timeout. Once
        // a termination cause is chosen below, later cancels cannot change it.
        if exited {
            break Outcome::new(TaskState::Succeeded);
        }
        if shared.cancelled() {
            break Outcome::new(TaskState::Cancelled);
        }
        if request.accepted.elapsed() >= request.timeout {
            break Outcome::new(TaskState::TimedOut);
        }
        shared.wait_tick();
    };
    // Even natural leader exit cleans up its remaining process group *before*
    // waiting. The unreaped leader reserves the process/group identifier.
    if owned.wait_owned {
        match owned.signal_and_reap() {
            Ok(status) => {
                if outcome.state == TaskState::Succeeded && !status.success() {
                    outcome.state = TaskState::Failed;
                }
                outcome.status = Some(status);
            }
            Err(error) => {
                outcome.state = TaskState::Failed;
                outcome.error = Some(bounded_error(error.to_string()));
            }
        }
    }
    pipes.drain(shared);
    outcome.truncated = !pipes.done() || pipes.output_limit || pipes.read_error.is_some();
    if let Some(error) = pipes.read_error {
        if outcome.error.is_none() {
            outcome.error = Some(error);
        }
        if matches!(outcome.state, TaskState::Succeeded | TaskState::Failed) {
            outcome.state = TaskState::Failed;
        }
    }
    if pipes.output_limit && matches!(outcome.state, TaskState::Succeeded | TaskState::Failed) {
        outcome.state = TaskState::OutputLimit;
    }
    outcome
}

/// Owns one wait state. No cached PID may be signaled after a successful wait,
/// nor after waitid reports that ownership is no longer established.
struct OwnedChild {
    child: Child,
    wait_owned: bool,
}

impl OwnedChild {
    fn new(child: Child) -> Self {
        Self {
            child,
            wait_owned: true,
        }
    }

    fn exited(&mut self) -> io::Result<bool> {
        match exited_without_reaping(&self.child) {
            Ok(exited) => Ok(exited),
            Err(error) => {
                // Never signal an uncertain/reaped child, including from Drop.
                self.wait_owned = false;
                Err(error)
            }
        }
    }

    fn signal_and_reap(&mut self) -> io::Result<ExitStatus> {
        if !self.wait_owned {
            return Err(io::Error::other("Child wait ownership is unavailable"));
        }
        self.exited()?;
        // SAFETY: this is our private process group, whose leader has not been
        // reaped. Exclusive wait ownership reserves its PID until child.wait().
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        // Also target the owned leader if a program somehow changed its group.
        // No signal is sent after wait, even when waiting returns an error.
        let _ = self.child.kill();
        let status = self.child.wait();
        self.wait_owned = false;
        status
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.wait_owned {
            let _ = self.signal_and_reap();
        }
    }
}

fn exited_without_reaping(child: &Child) -> io::Result<bool> {
    loop {
        // SAFETY: siginfo_t's storage is zero initialized, waitid writes within
        // it, and WNOWAIT leaves the selected owned child unreaped.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
            )
        };
        if result == 0 {
            // SAFETY: valid after successful waitid; zero covers WNOHANG with
            // no waitable status on platforms not writing siginfo in that case.
            return Ok(unsafe { info.si_pid() } != 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn nonblocking(pipe: &impl AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    // SAFETY: live pipe owns fd; fcntl reads and updates descriptor flags only.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

struct Pipes {
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    output_limit: bool,
    read_error: Option<String>,
}

impl Pipes {
    fn take(child: &mut Child) -> io::Result<Self> {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("Missing stdout pipe"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("Missing stderr pipe"))?;
        nonblocking(&stdout)?;
        nonblocking(&stderr)?;
        Ok(Self {
            stdout: Some(stdout),
            stderr: Some(stderr),
            output_limit: false,
            read_error: None,
        })
    }

    fn done(&self) -> bool {
        self.stdout.is_none() && self.stderr.is_none()
    }

    fn drain(&mut self, shared: &Shared) {
        let deadline = Instant::now() + DRAIN_TIMEOUT;
        while !self.done() && Instant::now() < deadline {
            self.capture(shared);
            if !self.done() {
                // Cancellation is already handled. Do not spin on its flag.
                thread::sleep(CHECK_INTERVAL);
            }
        }
    }

    fn capture(&mut self, shared: &Shared) {
        let mut buffer = [0u8; READ_CHUNK];
        for stream in [Stream::Stdout, Stream::Stderr] {
            for _ in 0..READS_PER_ROUND {
                let result = match stream {
                    Stream::Stdout => self.stdout.as_mut().map(|p| read_chunk(p, &mut buffer)),
                    Stream::Stderr => self.stderr.as_mut().map(|p| read_chunk(p, &mut buffer)),
                };
                let close = match result {
                    Some(Ok(Chunk::Bytes(count))) => {
                        let mut store = shared.lock();
                        let Some(record) = store.active.as_mut() else {
                            return;
                        };
                        let target = match stream {
                            Stream::Stdout => &mut record.stdout,
                            Stream::Stderr => &mut record.stderr,
                        };
                        let available = MAX_OUTPUT_BYTES_PER_STREAM - target.len();
                        target.extend_from_slice(&buffer[..count.min(available)]);
                        if count > available {
                            self.output_limit = true;
                            record.truncated = true;
                            true
                        } else {
                            false
                        }
                    }
                    Some(Ok(Chunk::Eof)) => true,
                    Some(Ok(Chunk::Pending)) | None => break,
                    Some(Err(error)) => {
                        self.read_error =
                            Some(bounded_error(format!("{stream:?} capture: {error}")));
                        true
                    }
                };
                if close {
                    match stream {
                        Stream::Stdout => self.stdout = None,
                        Stream::Stderr => self.stderr = None,
                    }
                    break;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

enum Chunk {
    Bytes(usize),
    Eof,
    Pending,
}

fn read_chunk(reader: &mut impl Read, buffer: &mut [u8]) -> io::Result<Chunk> {
    // Interrupted syscalls count against a bounded round too. A signal storm
    // cannot keep this loop from returning to timeout/cancellation checks.
    match reader.read(buffer) {
        Ok(0) => Ok(Chunk::Eof),
        Ok(count) => Ok(Chunk::Bytes(count)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            Ok(Chunk::Pending)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;

    fn active_shared() -> Shared {
        Shared {
            store: Mutex::new(Store {
                active: Some(Record {
                    id: 1,
                    state: TaskState::Running,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    exit_code: None,
                    truncated: false,
                    error: None,
                    cancel_requested: false,
                    request: None,
                }),
                ..Store::default()
            }),
            wake: Condvar::new(),
        }
    }

    fn pipe_pair() -> (ChildStdout, UnixStream) {
        let (reader, writer) = UnixStream::pair().unwrap();
        let reader: OwnedFd = reader.into();
        let reader = ChildStdout::from(reader);
        nonblocking(&reader).unwrap();
        (reader, writer)
    }

    fn stdout_only(stdout: ChildStdout) -> Pipes {
        Pipes {
            stdout: Some(stdout),
            stderr: None,
            output_limit: false,
            read_error: None,
        }
    }

    #[test]
    fn inherited_pipe_with_a_live_writer_cannot_block_final_drain() {
        let shared = active_shared();
        let (reader, mut writer) = pipe_pair();
        writer.write_all(b"before").unwrap();
        let mut pipes = stdout_only(reader);
        let started = Instant::now();
        pipes.drain(&shared);
        assert!(started.elapsed() >= DRAIN_TIMEOUT);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(!pipes.done());
        assert_eq!(shared.lock().active.as_ref().unwrap().stdout, b"before");
        drop(pipes);
        assert!(writer.write_all(b"after").is_err());
    }

    #[test]
    fn capture_retains_utf8_bytes_across_would_block() {
        let shared = active_shared();
        let (reader, mut writer) = pipe_pair();
        let mut pipes = stdout_only(reader);
        writer.write_all(&[0xf0, 0x9f]).unwrap();
        pipes.capture(&shared);
        assert_eq!(
            shared.lock().active.as_ref().unwrap().snapshot().stdout,
            "�"
        );
        writer.write_all(&[0xa6, 0x80]).unwrap();
        pipes.capture(&shared);
        assert_eq!(
            shared.lock().active.as_ref().unwrap().snapshot().stdout,
            "🦀"
        );
    }

    #[test]
    fn actual_read_failure_is_recorded_and_the_descriptor_is_closed() {
        let shared = active_shared();
        // This FD is live but opened write-only. read gets EBADF without an
        // invalid/raw FD lifetime or any competing descriptor ownership.
        let write_only = fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")
            .unwrap();
        let fd: OwnedFd = write_only.into();
        let mut pipes = stdout_only(ChildStdout::from(fd));
        pipes.capture(&shared);
        assert!(pipes.done());
        assert!(pipes
            .read_error
            .as_ref()
            .unwrap()
            .starts_with("Stdout capture:"));
    }

    struct FailingReader(io::ErrorKind);
    impl Read for FailingReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(self.0, "injected read failure"))
        }
    }

    #[test]
    fn would_block_and_signal_storms_return_control_in_one_read_attempt() {
        let mut buffer = [0; 8];
        for kind in [io::ErrorKind::Interrupted, io::ErrorKind::WouldBlock] {
            assert!(matches!(
                read_chunk(&mut FailingReader(kind), &mut buffer),
                Ok(Chunk::Pending)
            ));
        }
        assert_eq!(
            read_chunk(
                &mut FailingReader(io::ErrorKind::PermissionDenied),
                &mut buffer
            )
            .err()
            .unwrap()
            .kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    fn command(script: &str) -> Child {
        Command::new("/bin/sh")
            .args(["-c", script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap()
    }

    #[test]
    fn wnowait_reserves_the_leader_until_group_cleanup_and_reap() {
        let mut owned = OwnedChild::new(command("exit 7"));
        let deadline = Instant::now() + Duration::from_secs(2);
        while !owned.exited().unwrap() {
            assert!(Instant::now() < deadline);
            thread::sleep(CHECK_INTERVAL);
        }
        assert!(owned.exited().unwrap());
        let status = owned.signal_and_reap().unwrap();
        assert_eq!(status.code(), Some(7));
        assert!(!owned.wait_owned);
        assert!(owned.signal_and_reap().is_err());
        assert_eq!(
            exited_without_reaping(&owned.child)
                .unwrap_err()
                .raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    #[test]
    fn lost_wait_ownership_disarms_drop_instead_of_signaling_a_stale_pid() {
        let mut owned = OwnedChild::new(command("exit 0"));
        // Simulate a competing reaper using this exact owned child. This is an
        // unsupported caller setup, but the error path must never signal it.
        owned.child.wait().unwrap();
        assert_eq!(
            owned.exited().unwrap_err().raw_os_error(),
            Some(libc::ECHILD)
        );
        assert!(!owned.wait_owned);
        assert!(owned.signal_and_reap().is_err());
    }

    #[test]
    fn unwind_guard_kills_and_reaps_its_owned_child() {
        let owned = OwnedChild::new(command("exec sleep 10"));
        let pid = owned.child.id();
        let started = Instant::now();
        let result = std::panic::catch_unwind(|| {
            let _owned = owned;
            panic!("injected supervisor failure");
        });
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
        let mut status = 0;
        // SAFETY: waitpid only observes the known test child and valid status
        // storage. It sends no signal, even if the numeric PID were recycled.
        assert_eq!(
            unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    #[test]
    fn error_messages_are_bounded_without_splitting_utf8() {
        let error = bounded_error("🦀".repeat(MAX_ERROR_BYTES));
        assert!(error.len() <= MAX_ERROR_BYTES);
        assert!(error.chars().all(|c| c == '🦀'));
    }
}
