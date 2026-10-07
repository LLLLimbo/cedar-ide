use super::*;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Instant;

const CHECK_INTERVAL: Duration = Duration::from_millis(10);
const DRAIN_TIMEOUT: Duration = Duration::from_millis(250);
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
    windows_exit_code: Option<u32>,
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
            windows_exit_code: self.windows_exit_code,
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
            record.exit_code = outcome.exit_code;
            record.windows_exit_code = outcome.windows_exit_code;
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
        #[cfg(windows)]
        if root.to_str().is_none() {
            return Err(TaskError::InvalidRoot(
                "Windows workspace root must be valid UTF-8".into(),
            ));
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
                    // The platform owner's drop guard has already cleaned up. Do not
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
            windows_exit_code: None,
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
    exit_code: Option<i32>,
    windows_exit_code: Option<u32>,
    error: Option<String>,
    truncated: bool,
}

impl Outcome {
    fn new(state: TaskState) -> Self {
        Self {
            state,
            exit_code: None,
            windows_exit_code: None,
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use unix::run;

#[cfg(any(windows, test))]
mod windows;
#[cfg(windows)]
use windows::run;
