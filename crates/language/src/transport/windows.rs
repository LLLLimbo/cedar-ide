//! One joined owner of the Windows process, its Job and every pipe operation.
//! No blocking host stderr writes, detached I/O threads, PID-based termination,
//! or flush-to-child-consumption operations are used here.
use super::owned::{retain_tail, ActiveWrite, Incoming};
use super::{route_message, ClientOptions, Error, ProcessConfig, Shared, WriteCommand};
use cedar_winprocess::{
    LaunchSpec, StdinWriteProgress, Stream, WindowsCommand, MAX_STDIN_WRITE_BYTES,
};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(5);
const FINAL_DRAIN: Duration = Duration::from_millis(250);

struct Control {
    stop: AtomicBool,
    done: Mutex<bool>,
    completed: Condvar,
}

pub(super) struct Backend {
    process_id: u32,
    control: Arc<Control>,
    worker: Mutex<Option<JoinHandle<()>>>,
    wake: thread::Thread,
}

impl Backend {
    pub(super) fn spawn(
        config: ProcessConfig,
        options: &ClientOptions,
        shared: Arc<Shared>,
        outbound: mpsc::SyncSender<WriteCommand>,
        writes: mpsc::Receiver<WriteCommand>,
    ) -> Result<Self, Error> {
        // These restrictions are deliberate: the controlled Windows launcher
        // never performs PATH/PATHEXT lookup or invokes a command interpreter.
        let spec = LaunchSpec {
            executable: config.program,
            arguments: config
                .args
                .into_iter()
                .map(|arg| {
                    arg.into_string()
                        .map_err(|_| Error::InvalidState("Windows arguments must be UTF-8".into()))
                })
                .collect::<Result<_, _>>()?,
            cwd: config
                .working_directory
                .map_or_else(std::env::current_dir, Ok)
                .map_err(|e| Error::Io(format!("read working directory: {e}")))?,
        };
        let control = Arc::new(Control {
            stop: AtomicBool::new(false),
            done: Mutex::new(false),
            completed: Condvar::new(),
        });
        let worker_control = Arc::clone(&control);
        let options = options.clone();
        let (started, ready) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("cedar-lsp-owner".into())
            .spawn(move || {
                // Declared before the process: completion is published only after
                // the owned process and all pending kernel I/O have been destroyed,
                // including launch failures and panic unwinding.
                let _completion = Completion {
                    control: &worker_control,
                    shared: &shared,
                };
                let mut command = match WindowsCommand::spawn_suspended_with_piped_stdin(&spec) {
                    Ok(command) => command,
                    Err(e) => {
                        let error = Error::Io(format!("launch {}: {e}", spec.executable.display()));
                        let _ = started.try_send(Err(error.clone()));
                        shared.fail(error);
                        return;
                    }
                };
                let launch = if worker_control.stop.load(Ordering::Acquire) {
                    Err(Error::Closed("client stopped before resume".into()))
                } else {
                    command
                        .resume()
                        .map_err(|e| Error::Io(format!("resume language server: {e}")))
                };
                if let Err(error) = launch {
                    let _ = started.try_send(Err(error.clone()));
                    shared.fail(error);
                    return;
                }
                if started.try_send(Ok(command.process_id())).is_err() {
                    return;
                }
                let mut connection = Connection {
                    command,
                    incoming: Incoming::new(options.frame_limits, options.request_timeout),
                    active: None,
                    stderr: Vec::new(),
                    retain_stderr: config.inherit_stderr,
                    writes,
                };
                let error = connection.run(&worker_control, &shared, &outbound, &options);
                let error = connection.with_stderr(error);
                shared.fail(error.clone());
                if let Some(active) = connection.active.take() {
                    active.finish(Err(error.clone()));
                }
                // A disconnected writer acknowledgement wakes notify even if the
                // terminal event queue was full. Requests are already failed above.
                while let Ok(write) = connection.writes.try_recv() {
                    if let Some(ack) = write.ack {
                        let _ = ack.try_send(Err(error.clone()));
                    }
                }
                connection.cleanup();
            })
            .map_err(|e| Error::Io(format!("start owned language worker: {e}")))?;
        let mut backend = Self {
            process_id: 0,
            control,
            wake: worker.thread().clone(),
            worker: Mutex::new(Some(worker)),
        };
        match ready.recv() {
            Ok(Ok(id)) => {
                backend.process_id = id;
                Ok(backend)
            }
            Ok(Err(error)) => {
                backend.abort();
                Err(error)
            }
            Err(_) => {
                backend.abort();
                Err(Error::Closed(
                    "language worker stopped during launch".into(),
                ))
            }
        }
    }

    pub(super) fn process_id(&self) -> u32 {
        self.process_id
    }

    pub(super) fn wake(&self) {
        // Never wait for a concurrent joiner or compete for queue space.
        self.wake.unpark();
    }

    fn join(&self) -> Result<(), Error> {
        // Serialize joiners until cleanup really finishes. Taking a handle then
        // unlocking would let another abort falsely report completed teardown.
        let mut owner = self.worker.lock().unwrap();
        if let Some(worker) = owner.take() {
            worker
                .join()
                .map_err(|_| Error::Closed("language worker panicked".into()))?;
        }
        Ok(())
    }

    pub(super) fn abort(&self) {
        self.control.stop.store(true, Ordering::Release);
        self.wake(); // independent of outbound queue fullness and pipe progress
        let _ = self.join();
    }

    pub(super) fn finish(&self, shared: &Shared, timeout: Duration) -> Result<(), Error> {
        let done = self.control.done.lock().unwrap();
        let (done, _) = self
            .control
            .completed
            .wait_timeout_while(done, timeout, |done| !*done)
            .unwrap();
        let completed = *done;
        drop(done);
        if !completed {
            shared.fail(Error::Closed("shutdown grace period elapsed".into()));
            self.control.stop.store(true, Ordering::Release);
            self.wake();
        }
        self.join()
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.abort();
    }
}

struct Completion<'a> {
    control: &'a Control,
    shared: &'a Shared,
}
impl Drop for Completion<'_> {
    fn drop(&mut self) {
        self.shared
            .fail(Error::Closed("language worker stopped".into()));
        *self.control.done.lock().unwrap() = true;
        self.control.completed.notify_all();
    }
}

struct Connection {
    command: WindowsCommand,
    incoming: Incoming,
    active: Option<ActiveWrite>,
    stderr: Vec<u8>,
    retain_stderr: bool,
    writes: mpsc::Receiver<WriteCommand>,
}

impl Connection {
    fn run(
        &mut self,
        control: &Control,
        shared: &Shared,
        outbound: &mpsc::SyncSender<WriteCommand>,
        options: &ClientOptions,
    ) -> Error {
        let mut exited = None;
        loop {
            if control.stop.load(Ordering::Acquire) {
                return Error::Closed("language worker stopped".into());
            }
            if let Some(error) = shared.routing.lock().unwrap().terminal.clone() {
                return error;
            }
            if let Err(error) = self.incoming.check_deadline(Instant::now()) {
                return error;
            }
            // Each round is bounded by the primitive (four 8-KiB chunks per
            // stream). Parse/route one frame at a time, never collect a batch.
            let mut failure = None;
            let capture = self.command.capture_round(|stream, bytes| match stream {
                Stream::Stderr => {
                    if self.retain_stderr {
                        retain_tail(&mut self.stderr, bytes);
                    }
                }
                Stream::Stdout if failure.is_none() => {
                    let mut input = bytes;
                    while !input.is_empty() {
                        match self.incoming.push(&mut input, Instant::now()) {
                            Ok(Some(bytes)) => {
                                let result = serde_json::from_slice::<Value>(&bytes)
                                    .map_err(|e| Error::Protocol(format!("invalid JSON: {e}")))
                                    .and_then(|message| {
                                        route_message(
                                            message,
                                            shared,
                                            outbound,
                                            options.request_timeout,
                                            options.frame_limits,
                                        )
                                    });
                                if let Err(error) = result {
                                    failure = Some(error);
                                    break;
                                }
                            }
                            Ok(None) => break,
                            Err(error) => {
                                failure = Some(error);
                                break;
                            }
                        }
                    }
                }
                Stream::Stdout => {}
            });
            if let Some(error) = failure {
                return error;
            }
            let capture = match capture {
                Ok(capture) => capture,
                Err(e) => return Error::Io(format!("capture server output: {e}")),
            };
            // Stderr EOF is harmless. Stdout EOF is terminal even if a process
            // or descendant is still alive and retains another pipe endpoint.
            if capture.stdout_eof {
                return self.incoming.eof();
            }
            match self.command.try_exit() {
                Ok(Some(exit)) if exited.is_none() => {
                    exited = Some((exit.code, Instant::now() + FINAL_DRAIN));
                    // Stop pipe-holding descendants now, then allow pending
                    // overlapped reads to complete before declaring EOF.
                    if let Err(error) = self.command.terminate_tree() {
                        return Error::Io(format!("terminate exited server tree: {error}"));
                    }
                    // The child may have written between this round's empty
                    // capture and exit observation. Always capture again after
                    // observing exit before declaring the buffered stream empty.
                    continue;
                }
                Ok(Some(_)) => {}
                Ok(None) => {}
                Err(e) => return Error::Io(format!("observe server: {e}")),
            }
            if let Some((code, deadline)) = exited {
                // Root exit must not discard a response already buffered behind
                // the first bounded capture round. Route available final bytes,
                // but never wait for pipe-holding descendants or submit writes.
                if Instant::now() >= deadline {
                    return Error::Closed(format!("server exited with Windows code {code}"));
                }
                if capture.bytes == 0 {
                    thread::park_timeout(POLL_INTERVAL);
                }
                continue;
            }
            if control.stop.load(Ordering::Acquire) {
                continue;
            }
            match self.write_round(options) {
                Ok(progress) if progress || capture.bytes > 0 => continue,
                Ok(_) => thread::park_timeout(POLL_INTERVAL),
                Err(error) => return error,
            }
        }
    }

    // At most one write submission/completion per turn leaves bounded service
    // opportunities for stop, incoming frames, root exit and output drainage.
    fn write_round(&mut self, options: &ClientOptions) -> Result<bool, Error> {
        if self.active.is_none() {
            match self.writes.try_recv() {
                Ok(write) => {
                    self.active = Some(ActiveWrite::new(write, options.frame_limits)?);
                }
                Err(mpsc::TryRecvError::Empty) => return Ok(false),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(Error::Closed("outbound queue closed".into()))
                }
            }
        }
        let active = self.active.as_mut().unwrap();
        if active.expired(Instant::now()) {
            let error = Error::Timeout("stdio write".into());
            if active.started() {
                // A pending completion may already have sent bytes. Cancel and
                // poison the entire connection; never replay an uncertain frame.
                return Err(error);
            }
            self.active.take().unwrap().finish(Err(error));
            return Ok(true); // expired while queued, zero transport submission
        }
        let progress = if active.pending() {
            self.command.poll_stdin_write()
        } else {
            self.command
                .begin_stdin_write(active.begin_chunk(MAX_STDIN_WRITE_BYTES))
        }
        .map_err(|e| Error::Io(format!("write server stdin: {e}")))?;
        match progress {
            StdinWriteProgress::Pending => Ok(false),
            StdinWriteProgress::Written(bytes) => {
                active.advance(bytes)?;
                // The same deadline covers queueing and all chunks, including
                // a completion that is observed after the deadline elapsed.
                if active.expired(Instant::now()) {
                    return Err(Error::Timeout("stdio write".into()));
                }
                if active.complete() {
                    self.active.take().unwrap().finish(Ok(()));
                }
                Ok(true)
            }
            StdinWriteProgress::Closed | StdinWriteProgress::Idle => Err(Error::Closed(
                "server stdin closed or lost its write state".into(),
            )),
        }
    }

    fn with_stderr(&self, error: Error) -> Error {
        if self.stderr.is_empty() {
            return error;
        }
        let tail = String::from_utf8_lossy(&self.stderr);
        match error {
            Error::Io(reason) => Error::Io(format!("{reason}; stderr tail: {tail}")),
            Error::Protocol(reason) => Error::Protocol(format!("{reason}; stderr tail: {tail}")),
            Error::Closed(reason) => Error::Closed(format!("{reason}; stderr tail: {tail}")),
            Error::Timeout(reason) => Error::Timeout(format!("{reason}; stderr tail: {tail}")),
            other => other,
        }
    }

    fn cleanup(&mut self) {
        let _ = self.command.terminate_tree();
        let _ = self.command.cancel_stdin_and_complete();
        // Drain final output only for a finite budget. No final bytes can revive
        // requests or enqueue more writes after the terminal state is published.
        let deadline = Instant::now() + FINAL_DRAIN;
        while Instant::now() < deadline {
            let progress = self.command.capture_round(|_, _| {});
            match progress {
                Ok(progress) if progress.stdout_eof && progress.stderr_eof => break,
                Ok(progress) if progress.bytes == 0 => thread::park_timeout(POLL_INTERVAL),
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let _ = self.command.cancel_capture_and_complete();
        let _ = self.command.wait_exit();
        // WindowsCommand's Drop is the backstop on failures and unwinding. OS
        // cancellation/completion may delay cleanup; live I/O is never freed to
        // satisfy a timer. Closing the Job also waits for owned descendants.
    }
}
