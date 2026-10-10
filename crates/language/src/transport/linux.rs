//! One retained Linux owner of the child, its private process group and pipes.
//!
//! The host must preserve exclusive wait ownership (including its SIGCHLD
//! disposition): no other thread or handler may reap this child. WNOWAIT keeps
//! the root PID reserved until group cleanup has been attempted. Loss of that
//! ownership permanently disarms all numeric-PID operations. This is not a
//! guarantee about escaped descendants, group-zero accounting, or agent death.
use super::owned::Incoming;
use super::{
    route_message, AbortHandle, ClientOptions, Error, ProcessConfig, Shared, WriteCommand,
};
use crate::framing::FrameLimits;
use crate::{
    LinuxCleanupErrors, LinuxCleanupStatus, LinuxExitStatus, LinuxRootExit, LinuxShutdownOutcome,
    LinuxShutdownReason,
};
use serde_json::Value;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(5);
const FINAL_DRAIN: Duration = Duration::from_millis(250);
const CLEANUP_OBSERVATION: Duration = Duration::from_secs(3);
const READ_CHUNK: usize = 8192;
const READS_PER_ROUND: usize = 4;
const WRITE_CHUNK: usize = 8192;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Default)]
struct State {
    reason: Option<LinuxShutdownReason>,
    transport_failure_observed: bool,
    root_exit: LinuxRootExit,
    errors: LinuxCleanupErrors,
    root_reaped: bool,
    io_released: bool,
    process_started: bool,
    graceful_deadline: Option<Instant>,
    cleanup_started: Option<Instant>,
    completed_at: Option<Instant>,
}

struct Control {
    stop: Arc<AtomicBool>,
    state: Mutex<State>,
    changed: Condvar,
    observation_budget: Duration,
}

impl Control {
    fn new(observation_budget: Duration) -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
            observation_budget,
        }
    }

    fn begin_cleanup(&self, reason: Option<LinuxShutdownReason>) {
        let mut state = lock(&self.state);
        if let Some(reason) = reason {
            state.reason.get_or_insert(reason);
        }
        // Every path, including concurrent callers and Drop, shares this arm.
        state.cleanup_started.get_or_insert_with(Instant::now);
        self.changed.notify_all();
    }

    fn transport_failed(&self) {
        let mut state = lock(&self.state);
        state.transport_failure_observed = true;
        state
            .reason
            .get_or_insert(LinuxShutdownReason::TransportFailure);
        state.cleanup_started.get_or_insert_with(Instant::now);
        self.changed.notify_all();
    }

    fn failure(&self, error: Error) -> Error {
        self.transport_failed();
        error
    }

    fn record_root(&self, status: LinuxExitStatus, termination_started: bool) {
        let mut state = lock(&self.state);
        if state.root_exit == LinuxRootExit::Unobserved {
            state.root_exit = if termination_started {
                LinuxRootExit::AfterTermination(status)
            } else {
                LinuxRootExit::BeforeTermination(status)
            };
        }
    }

    fn snapshot(&self, joined: bool, expired: bool, now: Instant) -> LinuxShutdownOutcome {
        let state = lock(&self.state);
        let mut errors = state.errors;
        errors.cleanup_deadline_expired |= expired;
        // A first late observer may join a worker which completed within its
        // original window. Report owner completion time in that case, not the
        // age of the evidence when a caller happens to inspect it.
        let observed_at = if joined {
            state.completed_at.unwrap_or(now)
        } else {
            now
        };
        let cleanup = if !joined
            || expired
            || errors.worker_panicked
            || errors.ownership_lost
            || !state.root_reaped
            || !state.io_released
        {
            LinuxCleanupStatus::Unverified
        } else if errors == LinuxCleanupErrors::default() {
            LinuxCleanupStatus::Joined
        } else {
            LinuxCleanupStatus::JoinedWithErrors
        };
        LinuxShutdownOutcome {
            reason: state
                .reason
                .unwrap_or(LinuxShutdownReason::TransportFailure),
            transport_failure_observed: state.transport_failure_observed,
            root_exit: state.root_exit,
            cleanup,
            worker_joined: joined,
            root_reaped: state.root_reaped,
            io_released: state.io_released,
            cleanup_observation_elapsed_ms: millis(
                state.cleanup_started.map_or(Duration::ZERO, |start| {
                    observed_at.saturating_duration_since(start)
                }),
            ),
            cleanup_observation_budget_ms: millis(self.observation_budget),
            errors,
        }
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[derive(Debug, PartialEq, Eq)]
enum CleanupDecision {
    Join,
    Expired,
    Pending,
}

fn cleanup_decision(
    completed_at: Option<Instant>,
    deadline: Instant,
    now: Instant,
    thread_finished: bool,
) -> CleanupDecision {
    if completed_at.is_some_and(|at| at <= deadline) && thread_finished {
        CleanupDecision::Join
    } else if now >= deadline {
        CleanupDecision::Expired
    } else {
        CleanupDecision::Pending
    }
}

fn verified_cleanup(outcome: Option<LinuxShutdownOutcome>) -> bool {
    outcome.is_some_and(|outcome| {
        outcome.cleanup == LinuxCleanupStatus::Joined
            && outcome.worker_joined
            && outcome.root_reaped
            && outcome.io_released
            && outcome.root_exit != LinuxRootExit::Unobserved
            && outcome.errors == LinuxCleanupErrors::default()
    })
}

fn startup_failure(
    error: Error,
    process_started: bool,
    outcome: Option<LinuxShutdownOutcome>,
) -> Error {
    if process_started && !verified_cleanup(outcome) {
        Error::CleanupUnverified
    } else {
        error
    }
}

fn readiness_result(
    ready: &mpsc::Receiver<Result<u32, Error>>,
    control: &Control,
) -> Option<Result<u32, Error>> {
    loop {
        match ready.recv_timeout(POLL_INTERVAL) {
            Ok(result) => return Some(result),
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if lock(&control.state).cleanup_started.is_some() {
                    // A panic may retain the sender while its ownership guard
                    // finishes cleanup. Honor that already-armed cleanup clock,
                    // while preferring a ready result queued during this check.
                    return ready.try_recv().ok();
                }
            }
        }
    }
}

struct WorkerState {
    join: Option<JoinHandle<()>>,
    outcome: Option<(Result<(), Error>, LinuxShutdownOutcome)>,
}

pub(super) struct Backend {
    process_id: u32,
    control: Arc<Control>,
    worker: Mutex<WorkerState>,
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
        let control = Arc::new(Control::new(CLEANUP_OBSERVATION));
        let owner_control = Arc::clone(&control);
        let options = options.clone();
        let (started, ready) = mpsc::sync_channel(1);
        // Allocate the sole owner thread before creating any process. Thread
        // allocation failure therefore cannot leave a child without its owner.
        let worker = thread::Builder::new()
            .name("cedar-lsp-owner".into())
            .spawn(move || {
                // Declared before the child guard: completion follows descriptor
                // release and final wait responsibility, including unwinding.
                let _completion = Completion {
                    control: &owner_control,
                    shared: &shared,
                };
                let mut command = Command::new(&config.program);
                command
                    .args(&config.args)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(if config.inherit_stderr {
                        Stdio::inherit()
                    } else {
                        Stdio::null()
                    })
                    .process_group(0);
                if let Some(directory) = &config.working_directory {
                    command.current_dir(directory);
                }
                let child = match command.spawn() {
                    Ok(child) => child,
                    Err(error) => {
                        lock(&owner_control.state).io_released = true;
                        let error =
                            Error::Io(format!("launch {}: {error}", config.program.display()));
                        owner_control.transport_failed();
                        let _ = started.try_send(Err(error.clone()));
                        shared.fail(error);
                        return;
                    }
                };
                // This guard is installed immediately after spawn, before any
                // fallible pipe setup or readiness publication.
                let process = OwnedProcess::new(child, Arc::clone(&owner_control));
                let mut connection = Connection {
                    process,
                    incoming: Incoming::new(options.frame_limits, options.request_timeout),
                    active: None,
                    writes,
                };
                if let Err(error) = connection.process.prepare_pipes() {
                    let error =
                        owner_control.failure(Error::Io(format!("prepare server pipes: {error}")));
                    let _ = started.try_send(Err(error.clone()));
                    shared.fail(error);
                    return;
                }
                if started.try_send(Ok(connection.process.child.id())).is_err() {
                    owner_control.begin_cleanup(Some(LinuxShutdownReason::Aborted));
                    return;
                }
                let error = connection.run(&owner_control, &shared, &outbound, &options);
                // Natural-exit capture has already routed every complete final
                // reply (or recorded an incomplete/invalid tail) before this.
                shared.fail(error.clone());
                if let Some(active) = connection.active.take() {
                    active.finish(Err(error.clone()));
                }
                while let Ok(write) = connection.writes.try_recv() {
                    if let Some(ack) = write.ack {
                        let _ = ack.try_send(Err(error.clone()));
                    }
                }
                // Its destructor remains on this same owner if kernel wait is
                // delayed beyond the caller's observation budget.
                drop(connection);
            })
            .map_err(|error| Error::Io(format!("start owned language worker: {error}")))?;
        let mut backend = Self {
            process_id: 0,
            control,
            wake: worker.thread().clone(),
            worker: Mutex::new(WorkerState {
                join: Some(worker),
                outcome: None,
            }),
        };
        // The fixed cleanup budget starts at cleanup, not during the existing
        // synchronous Command::spawn/exec readiness handoff. Polling only lets
        // an already-triggered setup/unwind cleanup use its original deadline.
        match readiness_result(&ready, &backend.control) {
            Some(Ok(id)) => {
                backend.process_id = id;
                Ok(backend)
            }
            Some(Err(error)) => {
                backend.abort();
                let process_started = lock(&backend.control.state).process_started;
                Err(startup_failure(
                    error,
                    process_started,
                    backend.linux_shutdown_outcome(),
                ))
            }
            None => {
                backend.abort();
                let process_started = lock(&backend.control.state).process_started;
                Err(startup_failure(
                    Error::Closed("language worker stopped during launch".into()),
                    process_started,
                    backend.linux_shutdown_outcome(),
                ))
            }
        }
    }

    pub(super) fn process_id(&self) -> u32 {
        self.process_id
    }

    pub(super) fn abort_handle(&self) -> AbortHandle {
        AbortHandle {
            stop: Arc::clone(&self.control.stop),
            wake: Some(self.wake.clone()),
        }
    }

    pub(super) fn wake(&self) {
        self.wake.unpark();
    }

    pub(super) fn shutdown_outcome(&self) -> Option<crate::WindowsShutdownOutcome> {
        None
    }

    pub(super) fn linux_shutdown_outcome(&self) -> Option<LinuxShutdownOutcome> {
        lock(&self.worker)
            .outcome
            .as_ref()
            .map(|(_, outcome)| *outcome)
    }

    pub(super) fn transport_failed(&self) {
        self.control.transport_failed();
    }

    pub(super) fn begin_abort(&self) {
        self.control
            .begin_cleanup(Some(LinuxShutdownReason::Aborted));
    }

    pub(super) fn abort(&self) {
        self.begin_abort();
        self.abort_handle().signal();
        let _ = self.observe_cleanup();
    }

    pub(super) fn begin_shutdown(&self, timeout: Duration) {
        lock(&self.control.state)
            .graceful_deadline
            .get_or_insert_with(|| Instant::now() + timeout);
        self.wake();
    }

    pub(super) fn finish(&self, _shared: &Shared, timeout: Duration) -> Result<(), Error> {
        self.begin_shutdown(timeout);
        loop {
            let mut state = lock(&self.control.state);
            if state.cleanup_started.is_some() || state.completed_at.is_some() {
                break;
            }
            let remaining = state
                .graceful_deadline
                .unwrap()
                .saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                drop(state);
                self.control
                    .begin_cleanup(Some(LinuxShutdownReason::GraceExpired));
                self.abort_handle().signal();
                break;
            }
            state = self
                .control
                .changed
                .wait_timeout(state, remaining.min(POLL_INTERVAL))
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
            drop(state);
        }
        self.observe_cleanup()
    }

    fn observe_cleanup(&self) -> Result<(), Error> {
        self.control.begin_cleanup(None);
        loop {
            let mut worker = lock(&self.worker);
            if let Some((result, _)) = &worker.outcome {
                return result.clone();
            }
            let state = lock(&self.control.state);
            let deadline = state.cleanup_started.unwrap() + self.control.observation_budget;
            let now = Instant::now();
            let finished = worker.join.as_ref().is_some_and(JoinHandle::is_finished);
            let decision = cleanup_decision(state.completed_at, deadline, now, finished);
            drop(state);
            // A completion notification alone is not sufficient: joining an
            // unfinished thread could exceed the observation deadline.
            if decision == CleanupDecision::Join {
                let joined = worker.join.take().unwrap().join();
                if joined.is_err() {
                    let mut state = lock(&self.control.state);
                    state.errors.worker_panicked = true;
                    state
                        .reason
                        .get_or_insert(LinuxShutdownReason::WorkerPanicked);
                }
                let outcome = self.control.snapshot(true, false, Instant::now());
                let result = if outcome.cleanup == LinuxCleanupStatus::Unverified {
                    Err(Error::CleanupUnverified)
                } else {
                    Ok(())
                };
                worker.outcome = Some((result.clone(), outcome));
                return result;
            }
            if decision == CleanupDecision::Expired {
                let result = Err(Error::CleanupUnverified);
                let outcome = self.control.snapshot(false, true, now);
                // Keep the same JoinHandle and running owner. The immutable
                // observation never upgrades after a later wait or Drop.
                worker.outcome = Some((result.clone(), outcome));
                return result;
            }
            drop(worker);
            let state = lock(&self.control.state);
            let remaining = deadline.saturating_duration_since(Instant::now());
            let _ = self
                .control
                .changed
                .wait_timeout(state, remaining.min(POLL_INTERVAL))
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.abort();
        // A cached timeout detaches only the existing ownership thread; it does
        // not create a watcher or transfer numeric-PID operations to this caller.
    }
}

struct Completion<'a> {
    control: &'a Control,
    shared: &'a Shared,
}

impl Drop for Completion<'_> {
    fn drop(&mut self) {
        let panicked = thread::panicking();
        self.control.begin_cleanup(Some(if panicked {
            LinuxShutdownReason::WorkerPanicked
        } else {
            LinuxShutdownReason::TransportFailure
        }));
        if panicked {
            lock(&self.control.state).errors.worker_panicked = true;
        }
        // A poisoned routing lock must not double-panic while releasing the
        // ownership guard. Local lifecycle locks recover their poisoned value.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.shared
                .fail(Error::Closed("language worker stopped".into()));
        }));
        lock(&self.control.state).completed_at = Some(Instant::now());
        self.control.changed.notify_all();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Observation {
    Running,
    Interrupted,
    Exited(LinuxExitStatus),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SignalTarget {
    Group,
    Root,
}

// This seam exercises ownership transitions without signaling any real process.
trait RootOperations {
    fn observe(&mut self) -> io::Result<Observation>;
    fn signal(&mut self, target: SignalTarget) -> io::Result<()>;
    fn wait(&mut self) -> io::Result<LinuxExitStatus>;
}

struct LinuxOperations<'a> {
    child: &'a mut Child,
}

impl RootOperations for LinuxOperations<'_> {
    fn observe(&mut self) -> io::Result<Observation> {
        observe_pid(self.child.id())
    }

    fn signal(&mut self, target: SignalTarget) -> io::Result<()> {
        let pid = i32::try_from(self.child.id())
            .ok()
            .filter(|pid| *pid > 0)
            .ok_or_else(|| io::Error::other("invalid owned child PID"))?;
        let target = match target {
            SignalTarget::Group => -pid,
            SignalTarget::Root => pid,
        };
        // SAFETY: RootState verified exclusive, unreaped ownership immediately
        // before this call. The private group was established by process_group.
        if unsafe { libc::kill(target, libc::SIGKILL) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn wait(&mut self) -> io::Result<LinuxExitStatus> {
        self.child.wait().and_then(exit_status)
    }
}

fn exit_status(status: ExitStatus) -> io::Result<LinuxExitStatus> {
    if let Some(code) = status.code() {
        Ok(LinuxExitStatus::Code(code))
    } else if let Some(signal) = status.signal() {
        Ok(LinuxExitStatus::Signal(signal))
    } else {
        Err(io::Error::other("child wait did not return an exit status"))
    }
}

fn observe_pid(pid: u32) -> io::Result<Observation> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Err(io::Error::other("invalid owned child PID"));
    }
    // SAFETY: zeroed storage is valid for siginfo_t; waitid writes only within
    // it, and WNOWAIT reserves the child identity without reaping it.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
        )
    };
    if result != 0 {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::Interrupted {
            Ok(Observation::Interrupted)
        } else {
            Err(error)
        };
    }
    // SAFETY: these accessors read the SIGCHLD result of successful waitid.
    let observed = unsafe { info.si_pid() };
    if observed == 0 {
        return Ok(Observation::Running);
    }
    if observed != pid as i32 {
        return Err(io::Error::other("waitid returned another child identity"));
    }
    let status = unsafe { info.si_status() };
    match info.si_code {
        libc::CLD_EXITED if (0..=255).contains(&status) => {
            Ok(Observation::Exited(LinuxExitStatus::Code(status)))
        }
        libc::CLD_KILLED | libc::CLD_DUMPED if status > 0 => {
            Ok(Observation::Exited(LinuxExitStatus::Signal(status)))
        }
        _ => Err(io::Error::other(
            "waitid returned an unexpected child status",
        )),
    }
}

struct RootState {
    wait_owned: bool,
    observed: Option<LinuxExitStatus>,
    termination_started: bool,
    group_attempted: bool,
    root_attempted: bool,
    reaped: bool,
}

impl RootState {
    fn new() -> Self {
        Self {
            wait_owned: true,
            observed: None,
            termination_started: false,
            group_attempted: false,
            root_attempted: false,
            reaped: false,
        }
    }

    fn lose_ownership(&mut self, control: &Control) {
        self.wait_owned = false;
        let mut state = lock(&control.state);
        state.errors.observe_root = true;
        state.errors.ownership_lost = true;
    }

    fn observe(
        &mut self,
        ops: &mut impl RootOperations,
        control: &Control,
    ) -> io::Result<Observation> {
        if !self.wait_owned {
            return Err(io::Error::other("child wait ownership is unavailable"));
        }
        let result = match ops.observe() {
            Ok(Observation::Exited(status)) => {
                if self.observed.is_some_and(|previous| previous != status) {
                    Err(io::Error::other("owned child exit status changed"))
                } else {
                    self.observed = Some(status);
                    control.record_root(status, self.termination_started);
                    Ok(Observation::Exited(status))
                }
            }
            Ok(Observation::Running) if self.observed.is_some() => {
                Err(io::Error::other("owned child exit status disappeared"))
            }
            result => result,
        };
        if result.is_err() {
            // No subsequent waitid, Child.wait or signal may use this PID: an
            // external reaper could already have allowed it to be recycled.
            self.lose_ownership(control);
        }
        result
    }

    fn signal_once(
        &mut self,
        ops: &mut impl RootOperations,
        control: &Control,
        target: SignalTarget,
    ) -> bool {
        match self.observe(ops, control) {
            Ok(Observation::Interrupted) | Err(_) => return false,
            Ok(Observation::Exited(_)) if target == SignalTarget::Root => return true,
            Ok(_) => {}
        }
        self.termination_started = true;
        match ops.signal(target) {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => false,
            // The reserved identity was valid, but there is no remaining target
            // in that group/root. Absence is a completed cleanup attempt.
            Err(error) if error.raw_os_error() == Some(libc::ESRCH) => true,
            Err(_) => {
                let mut state = lock(&control.state);
                match target {
                    SignalTarget::Group => state.errors.terminate_group = true,
                    SignalTarget::Root => state.errors.terminate_root = true,
                }
                true
            }
        }
    }

    fn terminate_round(&mut self, ops: &mut impl RootOperations, control: &Control) -> bool {
        if !self.wait_owned || self.reaped {
            return true;
        }
        if !self.group_attempted {
            self.group_attempted = self.signal_once(ops, control, SignalTarget::Group);
            if !self.group_attempted || !self.wait_owned {
                return false;
            }
        }
        if !self.root_attempted {
            self.root_attempted = self.signal_once(ops, control, SignalTarget::Root);
        }
        self.group_attempted && self.root_attempted
    }

    // One bounded round. Only a waitable root is passed to Child.wait; delayed
    // exits leave this exact owner alive after caller observation times out.
    fn cleanup_round(&mut self, ops: &mut impl RootOperations, control: &Control) -> bool {
        if !self.wait_owned || self.reaped {
            return true;
        }
        if !self.terminate_round(ops, control) {
            return !self.wait_owned;
        }
        match self.observe(ops, control) {
            Ok(Observation::Exited(expected)) => match ops.wait() {
                Ok(status) => {
                    self.wait_owned = false;
                    self.reaped = status == expected;
                    let mut state = lock(&control.state);
                    state.root_reaped = self.reaped;
                    if status != expected {
                        state.errors.wait_root = true;
                        state.errors.ownership_lost = true;
                    }
                    true
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => false,
                Err(_) => {
                    self.wait_owned = false;
                    let mut state = lock(&control.state);
                    state.errors.wait_root = true;
                    state.errors.ownership_lost = true;
                    true
                }
            },
            Ok(_) => false,
            Err(_) => true,
        }
    }
}

struct OwnedProcess {
    child: Child,
    root: RootState,
    control: Arc<Control>,
}

impl OwnedProcess {
    fn new(child: Child, control: Arc<Control>) -> Self {
        lock(&control.state).process_started = true;
        Self {
            child,
            root: RootState::new(),
            control,
        }
    }

    fn prepare_pipes(&self) -> io::Result<()> {
        let stdin = self
            .child
            .stdin
            .as_ref()
            .ok_or_else(|| io::Error::other("missing stdin pipe"))?;
        let stdout = self
            .child
            .stdout
            .as_ref()
            .ok_or_else(|| io::Error::other("missing stdout pipe"))?;
        nonblocking(stdin)?;
        nonblocking(stdout)
    }

    fn observe(&mut self) -> io::Result<Observation> {
        self.root.observe(
            &mut LinuxOperations {
                child: &mut self.child,
            },
            &self.control,
        )
    }

    fn terminate_round(&mut self) {
        self.root.terminate_round(
            &mut LinuxOperations {
                child: &mut self.child,
            },
            &self.control,
        );
    }

    fn close_io(&mut self) {
        self.child.stdin.take();
        self.child.stdout.take();
        self.child.stderr.take();
        lock(&self.control.state).io_released = true;
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        self.control.begin_cleanup(Some(if thread::panicking() {
            LinuxShutdownReason::WorkerPanicked
        } else {
            LinuxShutdownReason::TransportFailure
        }));
        if thread::panicking() {
            lock(&self.control.state).errors.worker_panicked = true;
        }
        self.close_io();
        while !self.root.cleanup_round(
            &mut LinuxOperations {
                child: &mut self.child,
            },
            &self.control,
        ) {
            thread::park_timeout(POLL_INTERVAL);
        }
    }
}

fn nonblocking(pipe: &impl AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    // SAFETY: this live parent pipe owns fd for both fcntl operations.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

struct Connection {
    process: OwnedProcess,
    incoming: Incoming,
    active: Option<ActiveWrite>,
    writes: mpsc::Receiver<WriteCommand>,
}

struct Capture {
    bytes: usize,
    eof: bool,
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
            let graceful_deadline = lock(&control.state).graceful_deadline;
            if exited.is_none() {
                if control.stop.load(Ordering::Acquire) {
                    control.begin_cleanup(Some(LinuxShutdownReason::Aborted));
                    return Error::Closed("language worker stopped".into());
                }
                if let Some(error) = shared.routing.lock().unwrap().terminal.clone() {
                    control.transport_failed();
                    return error;
                }
                if graceful_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    control.begin_cleanup(Some(LinuxShutdownReason::GraceExpired));
                    return Error::Closed("shutdown grace period elapsed".into());
                }
            }
            if let Err(error) = self.incoming.check_deadline(Instant::now()) {
                return control.failure(error);
            }
            let capture = match self.capture_round(shared, outbound, options) {
                Ok(capture) => capture,
                Err(error) => return control.failure(error),
            };
            if exited.is_none() {
                match self.process.observe() {
                    Ok(Observation::Exited(status)) => {
                        control.begin_cleanup(Some(LinuxShutdownReason::RootExited));
                        exited = Some((status, Instant::now() + FINAL_DRAIN));
                        self.process.terminate_round();
                        // A final frame can have arrived after this round's
                        // empty read. Always capture again after observing exit.
                        continue;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        return control.failure(Error::Io(format!("observe server: {error}")))
                    }
                }
            }
            if capture.eof {
                let error = self.incoming.eof();
                if !matches!(error, Error::Closed(_)) {
                    return control.failure(error);
                }
            }
            if let Some((status, deadline)) = exited {
                self.process.terminate_round();
                if capture.eof {
                    return Error::Closed(format!("server exited with {status:?}"));
                }
                if Instant::now() >= deadline {
                    return incomplete_final_capture(&self.incoming, control);
                }
                // Root exit permanently prevents new write syscalls. Linux
                // has no pending kernel write completion to collect here.
                if capture.bytes == 0 {
                    thread::park_timeout(POLL_INTERVAL);
                }
                continue;
            }
            if capture.eof && graceful_deadline.is_none() {
                return control.failure(self.incoming.eof());
            }
            if control.stop.load(Ordering::Acquire) {
                continue;
            }
            match self.write_round(options.frame_limits) {
                Ok(progress) if progress || capture.bytes > 0 => {}
                Ok(_) => thread::park_timeout(POLL_INTERVAL),
                Err(error) => return control.failure(error),
            }
        }
    }

    fn capture_round(
        &mut self,
        shared: &Shared,
        outbound: &mpsc::SyncSender<WriteCommand>,
        options: &ClientOptions,
    ) -> Result<Capture, Error> {
        let mut capture = Capture {
            bytes: 0,
            eof: self.process.child.stdout.is_none(),
        };
        let mut buffer = [0u8; READ_CHUNK];
        for _ in 0..READS_PER_ROUND {
            let Some(stdout) = self.process.child.stdout.as_mut() else {
                break;
            };
            match read_once(stdout, &mut buffer)
                .map_err(|error| Error::Io(format!("read server stdout: {error}")))?
            {
                ReadProgress::Pending => break,
                ReadProgress::Eof => {
                    self.process.child.stdout.take();
                    capture.eof = true;
                    break;
                }
                ReadProgress::Bytes(count) => {
                    capture.bytes += count;
                    let mut input = &buffer[..count];
                    while !input.is_empty() {
                        match self.incoming.push(&mut input, Instant::now())? {
                            Some(bytes) => {
                                let message =
                                    serde_json::from_slice::<Value>(&bytes).map_err(|error| {
                                        Error::Protocol(format!("invalid JSON: {error}"))
                                    })?;
                                route_message(
                                    message,
                                    shared,
                                    outbound,
                                    options.request_timeout,
                                    options.frame_limits,
                                )?;
                            }
                            None => break,
                        }
                    }
                }
            }
        }
        Ok(capture)
    }

    fn write_round(&mut self, limits: FrameLimits) -> Result<bool, Error> {
        if self.active.is_none() {
            match self.writes.try_recv() {
                Ok(write) => self.active = Some(ActiveWrite::new(write, limits)?),
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
                return Err(error);
            }
            self.active.take().unwrap().finish(Err(error));
            return Ok(true);
        }
        let stdin = self
            .process
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| Error::Closed("server stdin is closed".into()))?;
        if !active.write_once(stdin)? {
            return Ok(false);
        }
        if active.expired(Instant::now()) {
            return Err(Error::Timeout("stdio write".into()));
        }
        if active.complete() {
            if active.command.close_stdin {
                // Acknowledge only after every byte was accepted and the parent
                // stdin descriptor was dropped. No blocking flush is needed.
                self.process.child.stdin.take();
            }
            self.active.take().unwrap().finish(Ok(()));
        }
        Ok(true)
    }
}

fn incomplete_final_capture(incoming: &Incoming, control: &Control) -> Error {
    lock(&control.state).errors.drain_output = true;
    let error = incoming.eof();
    control.failure(if matches!(error, Error::Protocol(_)) {
        error
    } else {
        Error::Io("final server stdout drain did not reach EOF".into())
    })
}

enum ReadProgress {
    Bytes(usize),
    Eof,
    Pending,
}

fn read_once(reader: &mut impl Read, bytes: &mut [u8]) -> io::Result<ReadProgress> {
    match reader.read(bytes) {
        Ok(0) => Ok(ReadProgress::Eof),
        Ok(count) => Ok(ReadProgress::Bytes(count)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ) =>
        {
            Ok(ReadProgress::Pending)
        }
        Err(error) => Err(error),
    }
}

/// Synchronous nonblocking writes have no pending submission. In particular,
/// EAGAIN and EINTR advance zero bytes, and only positive accepted bytes poison
/// the connection when the frame deadline later expires.
struct ActiveWrite {
    command: WriteCommand,
    header: Vec<u8>,
    written: usize,
}

impl ActiveWrite {
    fn new(command: WriteCommand, limits: FrameLimits) -> Result<Self, Error> {
        let header = format!("Content-Length: {}\r\n\r\n", command.bytes.len()).into_bytes();
        if header.len() > limits.max_header_bytes || command.bytes.len() > limits.max_content_bytes
        {
            return Err(Error::Protocol(
                "outbound frame exceeds configured limit".into(),
            ));
        }
        Ok(Self {
            command,
            header,
            written: 0,
        })
    }

    fn expired(&self, now: Instant) -> bool {
        now >= self.command.deadline
    }

    fn started(&self) -> bool {
        self.written != 0
    }

    fn complete(&self) -> bool {
        self.written == self.header.len() + self.command.bytes.len()
    }

    fn write_once(&mut self, writer: &mut impl Write) -> Result<bool, Error> {
        let bytes = if self.written < self.header.len() {
            &self.header[self.written..]
        } else {
            &self.command.bytes[self.written - self.header.len()..]
        };
        let bytes = &bytes[..bytes.len().min(WRITE_CHUNK)];
        match writer.write(bytes) {
            Ok(count) if count > 0 && count <= bytes.len() => {
                self.written += count;
                Ok(true)
            }
            Ok(_) => Err(Error::Io(
                "stdin write returned an invalid byte count".into(),
            )),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(Error::Io(format!("write server stdin: {error}"))),
        }
    }

    fn finish(self, result: Result<(), Error>) {
        if let Some(ack) = self.command.ack {
            let _ = ack.try_send(result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::AtomicUsize;

    struct FakeRoot {
        observations: VecDeque<Result<Observation, i32>>,
        default_observation: Observation,
        signal_results: VecDeque<Result<(), i32>>,
        wait_results: VecDeque<Result<LinuxExitStatus, i32>>,
        calls: Vec<&'static str>,
    }

    impl FakeRoot {
        fn new(observation: Observation) -> Self {
            Self {
                observations: VecDeque::new(),
                default_observation: observation,
                signal_results: VecDeque::new(),
                wait_results: VecDeque::new(),
                calls: Vec::new(),
            }
        }
    }

    impl RootOperations for FakeRoot {
        fn observe(&mut self) -> io::Result<Observation> {
            self.calls.push("observe");
            self.observations
                .pop_front()
                .unwrap_or(Ok(self.default_observation))
                .map_err(io::Error::from_raw_os_error)
        }

        fn signal(&mut self, target: SignalTarget) -> io::Result<()> {
            self.calls.push(match target {
                SignalTarget::Group => "group",
                SignalTarget::Root => "root",
            });
            self.signal_results
                .pop_front()
                .unwrap_or(Ok(()))
                .map_err(io::Error::from_raw_os_error)
        }

        fn wait(&mut self) -> io::Result<LinuxExitStatus> {
            self.calls.push("wait");
            self.wait_results
                .pop_front()
                .unwrap_or(Ok(LinuxExitStatus::Code(0)))
                .map_err(io::Error::from_raw_os_error)
        }
    }

    #[test]
    fn group_cleanup_precedes_exactly_one_reap_and_preserves_natural_status() {
        for status in [
            LinuxExitStatus::Code(23),
            LinuxExitStatus::Signal(libc::SIGTERM),
        ] {
            let control = Control::new(CLEANUP_OBSERVATION);
            let mut root = RootState::new();
            let mut ops = FakeRoot::new(Observation::Exited(status));
            ops.wait_results.push_back(Ok(status));
            assert!(root.cleanup_round(&mut ops, &control));
            assert_eq!(
                ops.calls,
                ["observe", "group", "observe", "observe", "wait"]
            );
            assert_eq!(
                lock(&control.state).root_exit,
                LinuxRootExit::BeforeTermination(status)
            );
            assert!(lock(&control.state).root_reaped);
            assert!(!root.wait_owned);
            let calls = ops.calls.clone();
            assert!(root.cleanup_round(&mut ops, &control));
            assert_eq!(ops.calls, calls);
        }
    }

    #[test]
    fn identity_loss_permanently_disarms_observation_signals_and_reap() {
        for errno in [libc::ECHILD, libc::EINVAL, libc::EIO] {
            let control = Control::new(CLEANUP_OBSERVATION);
            let mut root = RootState::new();
            let mut ops = FakeRoot::new(Observation::Exited(LinuxExitStatus::Code(0)));
            ops.observations.push_back(Err(errno));
            assert!(root.cleanup_round(&mut ops, &control));
            assert!(root.cleanup_round(&mut ops, &control));
            assert!(root.observe(&mut ops, &control).is_err());
            assert_eq!(ops.calls, ["observe"]);
            let state = lock(&control.state);
            assert!(state.errors.observe_root && state.errors.ownership_lost);
            assert!(!state.root_reaped);
        }
    }

    #[test]
    fn ownership_loss_between_group_and_root_prevents_fallback_signal() {
        let control = Control::new(CLEANUP_OBSERVATION);
        let mut root = RootState::new();
        let mut ops = FakeRoot::new(Observation::Running);
        ops.observations
            .extend([Ok(Observation::Running), Err(libc::ECHILD)]);
        assert!(!root.terminate_round(&mut ops, &control));
        assert!(root.cleanup_round(&mut ops, &control));
        assert_eq!(ops.calls, ["observe", "group", "observe"]);
        assert!(!root.wait_owned);
    }

    #[test]
    fn signal_failures_are_sticky_but_keep_the_same_wait_owner() {
        let control = Control::new(CLEANUP_OBSERVATION);
        let mut root = RootState::new();
        let mut ops = FakeRoot::new(Observation::Running);
        ops.signal_results
            .extend([Err(libc::EPERM), Err(libc::EACCES)]);
        assert!(!root.cleanup_round(&mut ops, &control));
        assert!(root.wait_owned);
        assert_eq!(
            ops.calls,
            ["observe", "group", "observe", "root", "observe"]
        );
        assert!(lock(&control.state).errors.terminate_group);
        assert!(lock(&control.state).errors.terminate_root);
        assert!(!lock(&control.state).errors.ownership_lost);
        ops.default_observation = Observation::Exited(LinuxExitStatus::Code(0));
        assert!(root.cleanup_round(&mut ops, &control));
        assert_eq!(&ops.calls[5..], ["observe", "wait"]);
        assert_eq!(
            lock(&control.state).root_exit,
            LinuxRootExit::AfterTermination(LinuxExitStatus::Code(0))
        );
    }

    #[test]
    fn absent_signal_target_is_explicit_and_does_not_skip_root_ownership_check() {
        let control = Control::new(CLEANUP_OBSERVATION);
        let mut root = RootState::new();
        let mut ops = FakeRoot::new(Observation::Running);
        ops.signal_results.push_back(Err(libc::ESRCH));
        assert!(!root.cleanup_round(&mut ops, &control));
        assert_eq!(
            ops.calls,
            ["observe", "group", "observe", "root", "observe"]
        );
        assert_eq!(lock(&control.state).errors, LinuxCleanupErrors::default());
    }

    #[test]
    fn interrupted_observation_and_signals_return_to_outer_deadline_checks() {
        let control = Control::new(CLEANUP_OBSERVATION);
        let mut root = RootState::new();
        let mut ops = FakeRoot::new(Observation::Interrupted);
        for round in 1..=10 {
            assert!(!root.cleanup_round(&mut ops, &control));
            assert_eq!(ops.calls.len(), round);
            assert!(root.wait_owned);
        }
        ops.default_observation = Observation::Running;
        ops.signal_results
            .extend([Err(libc::EINTR), Err(libc::EINTR)]);
        for _ in 0..2 {
            let before = ops.calls.len();
            assert!(!root.cleanup_round(&mut ops, &control));
            assert_eq!(ops.calls.len() - before, 2);
            assert!(!root.group_attempted);
        }
        assert_eq!(lock(&control.state).errors, LinuxCleanupErrors::default());
    }

    #[test]
    fn failed_reap_is_not_reported_as_reaped_and_is_never_retried() {
        let control = Control::new(CLEANUP_OBSERVATION);
        let mut root = RootState::new();
        let mut ops = FakeRoot::new(Observation::Exited(LinuxExitStatus::Code(0)));
        ops.wait_results.push_back(Err(libc::ECHILD));
        assert!(root.cleanup_round(&mut ops, &control));
        let calls = ops.calls.clone();
        assert!(root.cleanup_round(&mut ops, &control));
        assert_eq!(ops.calls, calls);
        let state = lock(&control.state);
        assert!(state.errors.wait_root && state.errors.ownership_lost);
        assert!(!state.root_reaped);
    }

    #[test]
    fn mismatched_reap_status_cannot_claim_the_original_root_was_reaped() {
        let control = Control::new(CLEANUP_OBSERVATION);
        let mut root = RootState::new();
        let mut ops = FakeRoot::new(Observation::Exited(LinuxExitStatus::Code(0)));
        ops.wait_results.push_back(Ok(LinuxExitStatus::Code(1)));
        assert!(root.cleanup_round(&mut ops, &control));
        let calls = ops.calls.clone();
        assert!(root.cleanup_round(&mut ops, &control));
        assert_eq!(ops.calls, calls);
        assert!(!root.wait_owned && !root.reaped);
        let state = lock(&control.state);
        assert!(!state.root_reaped);
        assert!(state.errors.wait_root && state.errors.ownership_lost);
    }

    #[test]
    fn changed_or_disappearing_reserved_exit_status_loses_identity() {
        for changed in [
            Observation::Running,
            Observation::Exited(LinuxExitStatus::Code(1)),
        ] {
            let control = Control::new(CLEANUP_OBSERVATION);
            let mut root = RootState::new();
            let mut ops = FakeRoot::new(Observation::Exited(LinuxExitStatus::Code(0)));
            assert!(matches!(
                root.observe(&mut ops, &control),
                Ok(Observation::Exited(_))
            ));
            ops.default_observation = changed;
            assert!(root.observe(&mut ops, &control).is_err());
            assert!(!root.wait_owned);
            assert!(lock(&control.state).errors.ownership_lost);
        }
    }

    struct ShortWriter {
        results: VecDeque<Result<usize, io::ErrorKind>>,
        bytes: Vec<u8>,
        calls: usize,
    }

    impl Write for ShortWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.calls += 1;
            let count = self
                .results
                .pop_front()
                .unwrap_or(Ok(1))
                .map_err(|kind| io::Error::new(kind, "injected write result"))?;
            self.bytes
                .extend_from_slice(&bytes[..count.min(bytes.len())]);
            Ok(count)
        }

        fn flush(&mut self) -> io::Result<()> {
            panic!("owned nonblocking writes never flush")
        }
    }

    fn active_write() -> (ActiveWrite, mpsc::Receiver<Result<(), Error>>) {
        let (send, recv) = mpsc::sync_channel(1);
        let command = WriteCommand {
            bytes: b"abc".to_vec(),
            close_stdin: true,
            deadline: Instant::now() + Duration::from_secs(1),
            ack: Some(send),
        };
        (
            ActiveWrite::new(command, FrameLimits::default()).unwrap(),
            recv,
        )
    }

    #[test]
    fn zero_acceptance_never_strands_a_write_and_short_writes_never_replay_bytes() {
        let (mut active, ack) = active_write();
        let mut writer = ShortWriter {
            results: VecDeque::from([
                Err(io::ErrorKind::WouldBlock),
                Err(io::ErrorKind::Interrupted),
            ]),
            bytes: Vec::new(),
            calls: 0,
        };
        for attempt in 1..=2 {
            assert!(!active.write_once(&mut writer).unwrap());
            assert_eq!(writer.calls, attempt);
            assert!(!active.started());
            assert_eq!(active.written, 0);
            assert!(ack.try_recv().is_err());
        }
        while !active.complete() {
            assert!(active.write_once(&mut writer).unwrap());
            assert!(active.started());
            assert!(ack.try_recv().is_err());
        }
        assert_eq!(writer.bytes, b"Content-Length: 3\r\n\r\nabc");
        assert!(active.command.close_stdin);
        active.finish(Ok(()));
        ack.try_recv().unwrap().unwrap();
    }

    #[test]
    fn deadline_poisoning_tracks_actual_accepted_bytes_after_would_block() {
        let (mut active, _) = active_write();
        let mut writer = ShortWriter {
            results: VecDeque::from([
                Err(io::ErrorKind::WouldBlock),
                Ok(1),
                Err(io::ErrorKind::Interrupted),
            ]),
            bytes: Vec::new(),
            calls: 0,
        };
        assert!(!active.write_once(&mut writer).unwrap());
        assert!(!active.started());
        assert!(active.write_once(&mut writer).unwrap());
        assert!(!active.write_once(&mut writer).unwrap());
        active.command.deadline = Instant::now();
        assert!(active.expired(Instant::now()));
        assert!(active.started());
        assert_eq!(active.written, 1);
    }

    struct InterruptedReader(usize);
    impl Read for InterruptedReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            self.0 += 1;
            Err(io::ErrorKind::Interrupted.into())
        }
    }

    #[test]
    fn interrupted_reads_are_one_bounded_attempt() {
        let mut reader = InterruptedReader(0);
        assert!(matches!(
            read_once(&mut reader, &mut [0; 8]),
            Ok(ReadProgress::Pending)
        ));
        assert_eq!(reader.0, 1);
    }

    #[test]
    fn final_capture_without_eof_invalidates_graceful_even_at_a_frame_boundary() {
        for partial in [false, true] {
            let control = Control::new(CLEANUP_OBSERVATION);
            control.begin_cleanup(Some(LinuxShutdownReason::RootExited));
            control.record_root(LinuxExitStatus::Code(0), false);
            let mut incoming = Incoming::new(FrameLimits::default(), Duration::from_secs(1));
            if partial {
                incoming
                    .push(&mut &b"Content-Length: 4\r\n\r\n{"[..], Instant::now())
                    .unwrap();
            }
            let error = incomplete_final_capture(&incoming, &control);
            assert_eq!(matches!(error, Error::Protocol(_)), partial);
            let before = control.snapshot(false, false, Instant::now());
            assert_eq!(before.reason, LinuxShutdownReason::RootExited);
            assert!(before.transport_failure_observed && before.errors.drain_output);
            assert!(!before.worker_joined && !before.root_reaped && !before.io_released);
            {
                let mut state = lock(&control.state);
                state.root_reaped = true;
                state.io_released = true;
            }
            let after = control.snapshot(true, false, Instant::now());
            assert_eq!(after.cleanup, LinuxCleanupStatus::JoinedWithErrors);
            assert!(after.transport_failure_observed);
            let outcome = crate::ShutdownOutcome {
                shutdown_response_received: true,
                exit_frame_completed: true,
                windows: None,
                linux: Some(after),
            };
            assert!(!outcome.is_graceful());
        }
    }

    fn backend_with(control: Arc<Control>, join: JoinHandle<()>) -> Backend {
        Backend {
            process_id: 0,
            wake: join.thread().clone(),
            control,
            worker: Mutex::new(WorkerState {
                join: Some(join),
                outcome: None,
            }),
        }
    }

    fn shared() -> Shared {
        let (events, _) = mpsc::sync_channel(1);
        Shared {
            routing: Mutex::new(super::super::Routing {
                pending: HashMap::new(),
                terminal: None,
            }),
            events,
            dropped: AtomicUsize::new(0),
        }
    }

    fn mark_completed(control: &Control) {
        let mut state = lock(&control.state);
        state.root_reaped = true;
        state.io_released = true;
        state.completed_at = Some(Instant::now());
        control.changed.notify_all();
    }

    #[test]
    fn completion_observation_order_and_exact_deadline_are_deterministic() {
        let start = Instant::now();
        let deadline = start + CLEANUP_OBSERVATION;
        let before = deadline - Duration::from_nanos(1);
        let after = deadline + Duration::from_nanos(1);
        assert_eq!(
            cleanup_decision(None, deadline, before, false),
            CleanupDecision::Pending
        );
        assert_eq!(
            cleanup_decision(None, deadline, deadline, false),
            CleanupDecision::Expired
        );
        assert_eq!(
            cleanup_decision(Some(before), deadline, before, false),
            CleanupDecision::Pending
        );
        assert_eq!(
            cleanup_decision(Some(before), deadline, deadline, false),
            CleanupDecision::Expired
        );
        assert_eq!(
            cleanup_decision(Some(before), deadline, before, true),
            CleanupDecision::Join
        );
        assert_eq!(
            cleanup_decision(Some(deadline), deadline, deadline, true),
            CleanupDecision::Join
        );
        assert_eq!(
            cleanup_decision(Some(before), deadline, after, true),
            CleanupDecision::Join
        );
        assert_eq!(
            cleanup_decision(Some(after), deadline, after, true),
            CleanupDecision::Expired
        );
        // The caller checks its immutable cached result before this decision;
        // the real-thread test below verifies timeout cannot later become Join.
    }

    #[test]
    fn first_late_join_reports_owner_completion_duration_not_evidence_age() {
        let control = Control::new(CLEANUP_OBSERVATION);
        let start = Instant::now();
        {
            let mut state = lock(&control.state);
            state.cleanup_started = Some(start);
            state.completed_at = Some(start + Duration::from_millis(17));
            state.root_reaped = true;
            state.io_released = true;
            state.reason = Some(LinuxShutdownReason::RootExited);
        }
        let outcome = control.snapshot(true, false, start + Duration::from_secs(60));
        assert_eq!(outcome.cleanup_observation_elapsed_ms, 17);
        assert_eq!(outcome.cleanup_observation_budget_ms, 3000);
        assert_eq!(outcome.cleanup, LinuxCleanupStatus::Joined);
    }

    #[test]
    fn concurrent_callers_share_one_timeout_and_never_upgrade_the_retained_owner() {
        let budget = Duration::from_millis(30);
        let control = Arc::new(Control::new(budget));
        let owner_control = Arc::clone(&control);
        let (release, released) = mpsc::sync_channel(1);
        let owner = thread::spawn(move || {
            released.recv().unwrap();
            mark_completed(&owner_control);
        });
        let owner_id = owner.thread().id();
        let backend = Arc::new(backend_with(control, owner));
        let first = Arc::clone(&backend);
        let second = Arc::clone(&backend);
        let first = thread::spawn(move || first.abort());
        let second = thread::spawn(move || second.abort());
        first.join().unwrap();
        second.join().unwrap();
        let deadline = lock(&backend.control.state).cleanup_started.unwrap() + budget;
        let observed = backend.linux_shutdown_outcome().unwrap();
        assert_eq!(observed.cleanup, LinuxCleanupStatus::Unverified);
        assert!(!observed.worker_joined && !observed.root_reaped && !observed.io_released);
        assert!(observed.errors.cleanup_deadline_expired);
        assert_eq!(observed.cleanup_observation_budget_ms, 30);
        assert!(observed.cleanup_observation_elapsed_ms >= 30);
        assert_eq!(
            lock(&backend.worker).join.as_ref().unwrap().thread().id(),
            owner_id
        );
        assert!(!lock(&backend.worker).join.as_ref().unwrap().is_finished());
        backend.abort();
        assert!(matches!(
            backend.finish(&shared(), Duration::from_secs(60)),
            Err(Error::CleanupUnverified)
        ));
        assert_eq!(
            lock(&backend.control.state).cleanup_started.unwrap() + budget,
            deadline
        );
        assert_eq!(backend.linux_shutdown_outcome().unwrap(), observed);
        release.send(()).unwrap();
        // Test-only collection of the retained owner proves it really finishes;
        // production callers intentionally leave the cached observation alone.
        lock(&backend.worker).join.take().unwrap().join().unwrap();
        backend.abort();
        assert_eq!(backend.linux_shutdown_outcome().unwrap(), observed);
    }

    #[test]
    fn completion_notification_without_finished_thread_does_not_authorize_join() {
        let control = Arc::new(Control::new(Duration::from_millis(20)));
        control.begin_cleanup(Some(LinuxShutdownReason::Aborted));
        mark_completed(&control);
        let (release, released) = mpsc::sync_channel(1);
        let owner = thread::spawn(move || released.recv().unwrap());
        let backend = backend_with(control, owner);
        assert!(matches!(
            backend.observe_cleanup(),
            Err(Error::CleanupUnverified)
        ));
        let outcome = backend.linux_shutdown_outcome().unwrap();
        assert!(!outcome.worker_joined);
        assert_eq!(outcome.cleanup, LinuxCleanupStatus::Unverified);
        release.send(()).unwrap();
        lock(&backend.worker).join.take().unwrap().join().unwrap();
        assert_eq!(backend.linux_shutdown_outcome().unwrap(), outcome);
    }

    #[test]
    fn actual_joined_panic_remains_unverified_on_repeated_observation() {
        let control = Arc::new(Control::new(Duration::from_secs(2)));
        control.begin_cleanup(Some(LinuxShutdownReason::Aborted));
        let owner_control = Arc::clone(&control);
        let owner = thread::spawn(move || {
            mark_completed(&owner_control);
            panic!("injected owner panic");
        });
        let backend = backend_with(control, owner);
        assert!(matches!(
            backend.observe_cleanup(),
            Err(Error::CleanupUnverified)
        ));
        let outcome = backend.linux_shutdown_outcome().unwrap();
        assert!(outcome.worker_joined && outcome.errors.worker_panicked);
        assert_eq!(outcome.cleanup, LinuxCleanupStatus::Unverified);
        assert!(matches!(
            backend.observe_cleanup(),
            Err(Error::CleanupUnverified)
        ));
        assert_eq!(backend.linux_shutdown_outcome().unwrap(), outcome);
    }

    #[test]
    fn graceful_deadline_and_cleanup_budget_are_independent_and_first_arm_wins() {
        let control = Arc::new(Control::new(Duration::from_secs(2)));
        let owner_control = Arc::clone(&control);
        let owner = thread::spawn(move || mark_completed(&owner_control));
        let backend = backend_with(control, owner);
        backend.begin_shutdown(Duration::from_secs(1));
        let first = lock(&backend.control.state).graceful_deadline.unwrap();
        assert!(lock(&backend.control.state).cleanup_started.is_none());
        backend.begin_shutdown(Duration::from_secs(60));
        assert_eq!(
            lock(&backend.control.state).graceful_deadline.unwrap(),
            first
        );
        backend.abort();
        let cleanup = lock(&backend.control.state).cleanup_started.unwrap();
        backend.abort();
        assert_eq!(
            lock(&backend.control.state).cleanup_started.unwrap(),
            cleanup
        );
    }

    #[test]
    fn queued_readiness_wins_over_an_already_armed_cleanup_trigger() {
        let control = Control::new(CLEANUP_OBSERVATION);
        control.begin_cleanup(Some(LinuxShutdownReason::RootExited));
        let (started, ready) = mpsc::sync_channel(1);
        started.send(Ok(37)).unwrap();
        assert!(matches!(readiness_result(&ready, &control), Some(Ok(37))));
    }

    #[test]
    fn readiness_wait_does_not_invent_a_cleanup_or_exec_timeout() {
        let control = Control::new(Duration::from_millis(1));
        let (started, ready) = mpsc::sync_channel(1);
        let owner = thread::spawn(move || {
            thread::sleep(POLL_INTERVAL * 3);
            started.send(Ok(41)).unwrap();
        });
        assert!(matches!(readiness_result(&ready, &control), Some(Ok(41))));
        owner.join().unwrap();
        assert!(lock(&control.state).cleanup_started.is_none());
    }

    #[test]
    fn panic_before_ready_observes_fixed_cleanup_deadline_with_the_same_retained_owner() {
        let control = Arc::new(Control::new(Duration::from_millis(30)));
        let owner_control = Arc::clone(&control);
        let (started, ready) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let owner = thread::spawn(move || {
            // Model panic unwinding before ready: the original owner retains
            // both its sender and cleanup responsibility until a delayed wait
            // returns. No caller-side watcher or replacement owner is created.
            let _held_started = started;
            owner_control.begin_cleanup(Some(LinuxShutdownReason::WorkerPanicked));
            {
                let mut state = lock(&owner_control.state);
                state.process_started = true;
                state.errors.worker_panicked = true;
            }
            released.recv().unwrap();
            mark_completed(&owner_control);
            panic!("injected pre-readiness unwind after retained cleanup");
        });
        let owner_id = owner.thread().id();
        let backend = backend_with(control, owner);
        assert!(readiness_result(&ready, &backend.control).is_none());
        assert!(matches!(ready.try_recv(), Err(mpsc::TryRecvError::Empty)));
        backend.abort();
        let outcome = backend.linux_shutdown_outcome().unwrap();
        assert_eq!(outcome.reason, LinuxShutdownReason::WorkerPanicked);
        assert_eq!(outcome.cleanup, LinuxCleanupStatus::Unverified);
        assert!(outcome.errors.cleanup_deadline_expired && outcome.errors.worker_panicked);
        assert!(!outcome.worker_joined && !outcome.root_reaped && !outcome.io_released);
        assert!(outcome.cleanup_observation_elapsed_ms >= 30);
        assert_eq!(
            lock(&backend.worker).join.as_ref().unwrap().thread().id(),
            owner_id
        );
        assert!(!lock(&backend.worker).join.as_ref().unwrap().is_finished());
        assert!(matches!(
            startup_failure(Error::Closed("startup panic".into()), true, Some(outcome)),
            Error::CleanupUnverified
        ));
        release.send(()).unwrap();
        assert!(lock(&backend.worker).join.take().unwrap().join().is_err());
        backend.abort();
        assert_eq!(backend.linux_shutdown_outcome().unwrap(), outcome);
    }

    #[test]
    fn failed_start_is_retryable_only_without_a_child_or_with_verified_cleanup() {
        let control = Control::new(CLEANUP_OBSERVATION);
        control.begin_cleanup(Some(LinuxShutdownReason::TransportFailure));
        control.record_root(LinuxExitStatus::Signal(libc::SIGKILL), true);
        mark_completed(&control);
        let verified = control.snapshot(true, false, Instant::now());
        let original = || Error::Io("injected pipe setup failure".into());
        assert!(matches!(
            startup_failure(original(), false, None),
            Error::Io(_)
        ));
        assert!(matches!(
            startup_failure(original(), true, Some(verified)),
            Error::Io(_)
        ));
        assert!(matches!(
            startup_failure(original(), true, None),
            Error::CleanupUnverified
        ));
        for mutate in [
            |outcome: &mut LinuxShutdownOutcome| {
                outcome.cleanup = LinuxCleanupStatus::JoinedWithErrors
            },
            |outcome: &mut LinuxShutdownOutcome| outcome.cleanup = LinuxCleanupStatus::Unverified,
            |outcome: &mut LinuxShutdownOutcome| outcome.worker_joined = false,
            |outcome: &mut LinuxShutdownOutcome| outcome.root_reaped = false,
            |outcome: &mut LinuxShutdownOutcome| outcome.io_released = false,
            |outcome: &mut LinuxShutdownOutcome| outcome.root_exit = LinuxRootExit::Unobserved,
            |outcome: &mut LinuxShutdownOutcome| outcome.errors.terminate_group = true,
        ] {
            let mut outcome = verified;
            mutate(&mut outcome);
            assert!(matches!(
                startup_failure(original(), true, Some(outcome)),
                Error::CleanupUnverified
            ));
            assert!(matches!(
                startup_failure(original(), false, Some(outcome)),
                Error::Io(_)
            ));
        }
    }

    const HELPER_FLAG: &str = "CEDAR_LINUX_OWNER_TEST_HELPER";

    fn helper_arguments(name: &str) -> Vec<std::ffi::OsString> {
        vec![
            "--exact".into(),
            format!("transport::linux::tests::{name}").into(),
            "--ignored".into(),
            "--nocapture".into(),
        ]
    }

    fn helper_command(name: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(helper_arguments(name))
            .env(HELPER_FLAG, "1")
            .env("RUST_BACKTRACE", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0);
        command
    }

    fn helper_enabled() -> bool {
        std::env::var_os(HELPER_FLAG).is_some_and(|value| value == "1")
    }

    fn assert_helper_running(owned: &mut OwnedProcess) {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match owned.observe().unwrap() {
                Observation::Running => return,
                Observation::Interrupted => assert!(Instant::now() < deadline),
                Observation::Exited(status) => {
                    panic!("helper exited before guard test: {status:?}")
                }
            }
        }
    }

    fn helper_watchdog(seconds: u64) {
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(seconds));
            std::process::exit(89);
        });
    }

    #[test]
    #[ignore = "private subprocess fixture; invoked only by owned-guard tests"]
    fn subprocess_idle() {
        if helper_enabled() {
            thread::sleep(Duration::from_secs(5));
            std::process::exit(89);
        }
    }

    #[test]
    fn actual_partial_pipe_setup_guard_closes_descriptors_and_reaps_root() {
        let control = Arc::new(Control::new(CLEANUP_OBSERVATION));
        let child = helper_command("subprocess_idle").spawn().unwrap();
        let mut owned = OwnedProcess::new(child, Arc::clone(&control));
        assert_helper_running(&mut owned);
        nonblocking(owned.child.stdin.as_ref().unwrap()).unwrap();
        // Simulate setup failing after one parent endpoint was configured.
        owned.child.stdout.take();
        assert!(owned.prepare_pipes().is_err());
        assert!(!lock(&control.state).root_reaped);
        let cleanup_started = Instant::now();
        drop(owned);
        assert!(cleanup_started.elapsed() < CLEANUP_OBSERVATION);
        let state = lock(&control.state);
        assert!(state.root_reaped && state.io_released);
        assert_eq!(
            state.root_exit,
            LinuxRootExit::AfterTermination(LinuxExitStatus::Signal(libc::SIGKILL))
        );
        assert_eq!(state.errors, LinuxCleanupErrors::default());
    }

    #[test]
    fn actual_unwind_guard_retains_cleanup_and_root_wait_responsibility() {
        let control = Arc::new(Control::new(CLEANUP_OBSERVATION));
        let owner_control = Arc::clone(&control);
        let child = helper_command("subprocess_idle").spawn().unwrap();
        let cleanup_started = Instant::now();
        let unwound = std::panic::catch_unwind(move || {
            let mut owned = OwnedProcess::new(child, owner_control);
            assert_helper_running(&mut owned);
            owned.prepare_pipes().unwrap();
            panic!("injected panic after child and pipe ownership");
        });
        assert!(unwound.is_err());
        assert!(cleanup_started.elapsed() < CLEANUP_OBSERVATION);
        let state = lock(&control.state);
        assert!(state.root_reaped && state.io_released);
        assert_eq!(
            state.root_exit,
            LinuxRootExit::AfterTermination(LinuxExitStatus::Signal(libc::SIGKILL))
        );
        assert!(state.errors.worker_panicked);
        assert!(!state.errors.ownership_lost && !state.errors.wait_root);
    }

    #[test]
    #[ignore = "private subprocess fixture; invoked only by inherited stderr test"]
    fn subprocess_stderr_writer() {
        if !helper_enabled() {
            return;
        }
        helper_watchdog(5);
        let stderr = std::io::stderr();
        // SAFETY: this helper owns a live inherited pipe as its stderr. Reading
        // the pipe capacity changes neither descriptor nor process settings.
        let capacity = unsafe { libc::fcntl(stderr.as_raw_fd(), libc::F_GETPIPE_SZ) };
        assert!(capacity > 0 && capacity <= 1024 * 1024);
        let mut stderr = stderr.lock();
        stderr.write_all(&vec![b'e'; capacity as usize]).unwrap();
        // The coordinator's parent never reads stderr while it is running.
        // This marker proves its inherited pipe is full before cancellation.
        std::fs::write("stderr-filled", b"full").unwrap();
        stderr.write_all(b"blocked").unwrap();
        panic!("inherited stderr was unexpectedly drained");
    }

    #[test]
    #[ignore = "private subprocess fixture; invoked only by inherited stderr test"]
    fn subprocess_stderr_owner() {
        if !helper_enabled() {
            return;
        }
        helper_watchdog(6);
        let mut config = ProcessConfig::new(std::env::current_exe().unwrap());
        config.args = helper_arguments("subprocess_stderr_writer");
        config.inherit_stderr = true;
        let rpc = super::super::StdioRpc::spawn(config, ClientOptions::default()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !std::path::Path::new("stderr-filled").exists() {
            assert!(
                Instant::now() < deadline,
                "server did not fill inherited stderr"
            );
            thread::sleep(POLL_INTERVAL);
        }
        rpc.abort(Error::Closed(
            "inherited stderr cancellation regression".into(),
        ));
        let outcome = rpc.linux_shutdown_outcome().unwrap();
        assert!(verified_cleanup(Some(outcome)));
        assert_eq!(
            outcome.root_exit,
            LinuxRootExit::AfterTermination(LinuxExitStatus::Signal(libc::SIGKILL))
        );
    }

    #[test]
    fn inherited_stderr_backpressure_cannot_block_the_owner_or_its_cleanup() {
        struct TestDirectory(std::path::PathBuf);
        impl Drop for TestDirectory {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = TestDirectory(std::env::temp_dir().join(format!(
            "cedar-owned-stderr-{}-{unique}",
            std::process::id(),
        )));
        std::fs::create_dir(&directory.0).unwrap();
        let control = Arc::new(Control::new(CLEANUP_OBSERVATION));
        let child = helper_command("subprocess_stderr_owner")
            .current_dir(&directory.0)
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut owned = OwnedProcess::new(child, Arc::clone(&control));
        let deadline = Instant::now() + Duration::from_secs(7);
        let status = loop {
            match owned.observe().unwrap() {
                Observation::Exited(status) => break status,
                Observation::Running | Observation::Interrupted => {
                    assert!(
                        Instant::now() < deadline,
                        "stderr coordinator exceeded watchdog"
                    );
                    thread::sleep(POLL_INTERVAL);
                }
            }
        };
        // Do not read the piped stderr before the coordinator exits: that would
        // relieve precisely the inherited backpressure this test establishes.
        drop(owned);
        assert_eq!(status, LinuxExitStatus::Code(0));
        assert!(lock(&control.state).root_reaped);
    }
}
