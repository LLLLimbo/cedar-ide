//! Bounded Windows task control. The real owner is constructed, used and
//! destroyed on the supervisor thread; no handles or pipe buffers enter Store.
use super::*;
use std::io;

#[derive(Debug, Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug, Default, Clone, Copy)]
struct CaptureProgress {
    stdout_eof: bool,
    stderr_eof: bool,
}

// The narrow controller boundary lets deterministic tests exercise cancellation
// during suspended creation, failure precedence and cleanup publication order.
// Production has exactly one implementation: the owned Windows primitive.
trait Executor {
    fn resume(&mut self) -> io::Result<()>;
    fn capture_round(&mut self, sink: &mut dyn FnMut(Stream, &[u8]))
        -> io::Result<CaptureProgress>;
    fn try_exit(&mut self) -> io::Result<Option<u32>>;
    fn terminate_tree(&mut self) -> io::Result<()>;
    fn cancel_capture_and_complete(&mut self) -> io::Result<()>;
    fn wait_exit(&mut self) -> io::Result<u32>;
}

#[cfg(windows)]
impl Executor for cedar_winprocess::WindowsCommand {
    fn resume(&mut self) -> io::Result<()> {
        self.resume()
    }

    fn capture_round(
        &mut self,
        sink: &mut dyn FnMut(Stream, &[u8]),
    ) -> io::Result<CaptureProgress> {
        self.capture_round(|stream, bytes| {
            let stream = match stream {
                cedar_winprocess::Stream::Stdout => Stream::Stdout,
                cedar_winprocess::Stream::Stderr => Stream::Stderr,
            };
            sink(stream, bytes);
        })
        .map(|progress| CaptureProgress {
            stdout_eof: progress.stdout_eof,
            stderr_eof: progress.stderr_eof,
        })
    }

    fn try_exit(&mut self) -> io::Result<Option<u32>> {
        self.try_exit().map(|exit| exit.map(|exit| exit.code))
    }

    fn terminate_tree(&mut self) -> io::Result<()> {
        self.terminate_tree()
    }

    fn cancel_capture_and_complete(&mut self) -> io::Result<()> {
        self.cancel_capture_and_complete()
    }

    fn wait_exit(&mut self) -> io::Result<u32> {
        self.wait_exit().map(|exit| exit.code)
    }
}

#[cfg(windows)]
pub(super) fn run(root: &Path, request: Request, shared: &Shared) -> Outcome {
    run_with(request, shared, |request| {
        cedar_winprocess::WindowsCommand::spawn_suspended(&cedar_winprocess::LaunchSpec {
            executable: PathBuf::from(&request.program),
            arguments: request.args.clone(),
            cwd: root.to_owned(),
        })
    })
}

fn before_launch(request: &Request, shared: &Shared) -> Option<Outcome> {
    if shared.cancelled() {
        Some(Outcome::new(TaskState::Cancelled))
    } else if request.accepted.elapsed() >= request.timeout {
        Some(Outcome::new(TaskState::TimedOut))
    } else {
        None
    }
}

fn run_with<E: Executor>(
    request: Request,
    shared: &Shared,
    spawn: impl FnOnce(&Request) -> io::Result<E>,
) -> Outcome {
    if let Some(outcome) = before_launch(&request, shared) {
        return outcome;
    }
    let mut owner = match spawn(&request) {
        Ok(owner) => owner,
        Err(error) => {
            return Outcome {
                error: Some(bounded_error(error.to_string())),
                ..Outcome::new(TaskState::SpawnFailed)
            };
        }
    };
    let mut capture = Capture::default();
    // Creation can take time and cancellation can arrive while it runs. The
    // atomically assigned child is still suspended at this second check.
    let mut outcome = if let Some(outcome) = before_launch(&request, shared) {
        outcome
    } else if let Err(error) = owner.resume() {
        Outcome::failed(error)
    } else {
        {
            let mut store = shared.lock();
            if let Some(record) = store.active.as_mut() {
                if !record.cancel_requested {
                    record.state = TaskState::Running;
                }
            }
        }
        loop {
            capture.round(&mut owner, shared);
            // Output/capture failure wins this observation round. A natural
            // root exit observed before cancel/deadline wins both control flags.
            if capture.output_limit {
                break Outcome::new(TaskState::OutputLimit);
            }
            if let Some(error) = &capture.read_error {
                break Outcome::failed(error);
            }
            match owner.try_exit() {
                Ok(Some(code)) => break natural_exit(code),
                Ok(None) => {}
                Err(error) => break Outcome::failed(error),
            }
            if shared.cancelled() {
                break Outcome::new(TaskState::Cancelled);
            }
            if request.accepted.elapsed() >= request.timeout {
                break Outcome::new(TaskState::TimedOut);
            }
            shared.wait_tick();
        }
    };

    // Natural root exit does not imply tree exit. Stop every owned descendant
    // before draining so inherited writers cannot extend their lifetime.
    if let Err(error) = owner.terminate_tree() {
        outcome.state = TaskState::Failed;
        outcome.error = Some(bounded_error(error.to_string()));
    }
    capture.drain(&mut owner, shared);
    // Cancellation is never EOF. Record real EOF before stopping capture, and
    // join outstanding I/O even if either stream's capture failed.
    outcome.truncated = !capture.done() || capture.output_limit || capture.read_error.is_some();
    if let Err(error) = owner.cancel_capture_and_complete() {
        outcome.truncated = true;
        capture.record_error(error);
    }
    match owner.wait_exit() {
        Ok(code) => {
            outcome.exit_code = i32::try_from(code).ok();
            outcome.windows_exit_code = Some(code);
        }
        Err(error) => {
            outcome.state = TaskState::Failed;
            outcome.error = Some(bounded_error(error.to_string()));
        }
    }
    if let Some(error) = capture.read_error {
        if outcome.error.is_none() {
            outcome.error = Some(error);
        }
        if matches!(outcome.state, TaskState::Succeeded | TaskState::Failed) {
            outcome.state = TaskState::Failed;
        }
    }
    // As on Unix, late drain results refine natural completion, but cannot
    // relabel a previously selected cancellation or timeout.
    if capture.output_limit && matches!(outcome.state, TaskState::Succeeded | TaskState::Failed) {
        outcome.state = TaskState::OutputLimit;
    }
    // Drop waits for owned job cleanup and releases handles. Only after this
    // returns may supervise() move the record into terminal history.
    drop(owner);
    outcome
}

fn natural_exit(code: u32) -> Outcome {
    Outcome {
        exit_code: i32::try_from(code).ok(),
        windows_exit_code: Some(code),
        ..Outcome::new(if code == 0 {
            TaskState::Succeeded
        } else {
            TaskState::Failed
        })
    }
}

#[derive(Default)]
struct Capture {
    progress: CaptureProgress,
    output_limit: bool,
    read_error: Option<String>,
}

impl Capture {
    fn done(&self) -> bool {
        self.progress.stdout_eof && self.progress.stderr_eof
    }

    fn record_error(&mut self, error: impl ToString) {
        if self.read_error.is_none() {
            self.read_error = Some(bounded_error(error.to_string()));
        }
    }

    fn round(&mut self, owner: &mut impl Executor, shared: &Shared) {
        let result = owner.capture_round(&mut |stream, bytes| {
            let mut store = shared.lock();
            let Some(record) = store.active.as_mut() else {
                return;
            };
            let target = match stream {
                Stream::Stdout => &mut record.stdout,
                Stream::Stderr => &mut record.stderr,
            };
            let available = MAX_OUTPUT_BYTES_PER_STREAM - target.len();
            target.extend_from_slice(&bytes[..bytes.len().min(available)]);
            // Filling the cap exactly is not evidence that bytes were lost.
            if bytes.len() > available {
                self.output_limit = true;
                record.truncated = true;
            }
        });
        match result {
            Ok(progress) => self.progress = progress,
            Err(error) => self.record_error(error),
        }
    }

    fn drain(&mut self, owner: &mut impl Executor, shared: &Shared) {
        let deadline = Instant::now() + DRAIN_TIMEOUT;
        while !self.done() && Instant::now() < deadline {
            self.round(owner, shared);
            if !self.done() {
                // The control cause is already selected. A cancel flag must
                // not turn final drainage into a tight spin.
                thread::sleep(CHECK_INTERVAL);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active_shared() -> Shared {
        Shared {
            store: Mutex::new(Store {
                active: Some(Record {
                    id: 1,
                    state: TaskState::Starting,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    exit_code: None,
                    windows_exit_code: None,
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

    fn request() -> Request {
        Request {
            program: "owned-fixture.exe".into(),
            args: Vec::new(),
            timeout: Duration::from_secs(3),
            accepted: Instant::now(),
        }
    }

    fn cancel(shared: &Shared) {
        shared.lock().active.as_mut().unwrap().cancel_requested = true;
    }

    struct Fake<'a> {
        shared: &'a Shared,
        events: &'a Mutex<Vec<&'static str>>,
        output: Vec<(Stream, Vec<u8>)>,
        exit: Option<u32>,
        eof: bool,
        cancel_on_capture: bool,
        read_error: bool,
        resume_error: bool,
        stop_error: bool,
        final_output: Vec<(Stream, Vec<u8>)>,
        final_read_error: bool,
        final_eof: Option<bool>,
        terminated: bool,
        capture_panic: bool,
        first_capture_delay: Duration,
    }

    impl<'a> Fake<'a> {
        fn new(shared: &'a Shared, events: &'a Mutex<Vec<&'static str>>) -> Self {
            Self {
                shared,
                events,
                output: Vec::new(),
                exit: Some(0),
                eof: true,
                cancel_on_capture: false,
                read_error: false,
                resume_error: false,
                stop_error: false,
                final_output: Vec::new(),
                final_read_error: false,
                final_eof: None,
                terminated: false,
                capture_panic: false,
                first_capture_delay: Duration::ZERO,
            }
        }

        fn record(&self, event: &'static str) {
            self.events.lock().unwrap().push(event);
        }
    }

    impl Executor for Fake<'_> {
        fn resume(&mut self) -> io::Result<()> {
            self.record("resume");
            if self.resume_error {
                Err(io::Error::other("resume failed"))
            } else {
                Ok(())
            }
        }

        fn capture_round(
            &mut self,
            sink: &mut dyn FnMut(Stream, &[u8]),
        ) -> io::Result<CaptureProgress> {
            self.record("capture");
            if !self.first_capture_delay.is_zero() {
                thread::sleep(std::mem::take(&mut self.first_capture_delay));
            }
            let output = if self.terminated {
                &mut self.final_output
            } else {
                &mut self.output
            };
            for (stream, bytes) in output.drain(..) {
                sink(stream, &bytes);
            }
            assert!(!self.capture_panic, "injected capture panic");
            if self.cancel_on_capture {
                cancel(self.shared);
            }
            if std::mem::take(&mut self.read_error)
                || (self.terminated && std::mem::take(&mut self.final_read_error))
            {
                return Err(io::Error::other("capture failed"));
            }
            let eof = if self.terminated {
                self.final_eof.unwrap_or(self.eof)
            } else {
                self.eof
            };
            Ok(CaptureProgress {
                stdout_eof: eof,
                stderr_eof: eof,
            })
        }

        fn try_exit(&mut self) -> io::Result<Option<u32>> {
            self.record("observe");
            Ok(self.exit)
        }

        fn terminate_tree(&mut self) -> io::Result<()> {
            self.record("terminate");
            self.terminated = true;
            Ok(())
        }

        fn cancel_capture_and_complete(&mut self) -> io::Result<()> {
            self.record("stop_capture");
            if self.stop_error {
                Err(io::Error::other("capture completion failed"))
            } else {
                Ok(())
            }
        }

        fn wait_exit(&mut self) -> io::Result<u32> {
            self.record("wait");
            Ok(self.exit.unwrap_or(1067))
        }
    }

    impl Drop for Fake<'_> {
        fn drop(&mut self) {
            let store = self.shared.lock();
            assert!(store.completed.is_empty());
            assert!(!store.active.as_ref().unwrap().state.is_terminal());
            self.record("drop");
        }
    }

    #[test]
    fn cancelled_or_expired_request_never_calls_the_spawner() {
        let shared = active_shared();
        cancel(&shared);
        let outcome = run_with::<Fake<'_>>(request(), &shared, |_| panic!("must not spawn"));
        assert_eq!(outcome.state, TaskState::Cancelled);

        let shared = active_shared();
        let mut expired = request();
        expired.accepted -= expired.timeout;
        let outcome = run_with::<Fake<'_>>(expired, &shared, |_| panic!("must not spawn"));
        assert_eq!(outcome.state, TaskState::TimedOut);
    }

    #[test]
    fn cancellation_during_suspended_creation_prevents_resume_and_cleans_up() {
        let shared = active_shared();
        let events = Mutex::new(Vec::new());
        let outcome = run_with(request(), &shared, |_| {
            cancel(&shared);
            Ok(Fake::new(&shared, &events))
        });
        assert_eq!(outcome.state, TaskState::Cancelled);
        assert_eq!(
            *events.lock().unwrap(),
            ["terminate", "capture", "stop_capture", "wait", "drop"]
        );
    }

    #[test]
    fn expiry_during_suspended_creation_prevents_resume() {
        let shared = active_shared();
        let events = Mutex::new(Vec::new());
        let mut short = request();
        short.timeout = Duration::from_millis(100);
        let outcome = run_with(short, &shared, |request| {
            thread::sleep(request.timeout);
            Ok(Fake::new(&shared, &events))
        });
        assert_eq!(outcome.state, TaskState::TimedOut);
        assert!(!events.lock().unwrap().contains(&"resume"));
        assert_eq!(events.lock().unwrap().last(), Some(&"drop"));
    }

    #[test]
    fn native_exit_bits_and_natural_exit_precedence_are_preserved() {
        for code in [0, 7, 259, i32::MAX as u32, 0xc000_0005, u32::MAX] {
            let shared = active_shared();
            let events = Mutex::new(Vec::new());
            let outcome = run_with(request(), &shared, |_| {
                let mut owner = Fake::new(&shared, &events);
                owner.exit = Some(code);
                owner.cancel_on_capture = true;
                Ok(owner)
            });
            assert_eq!(
                outcome.state,
                if code == 0 {
                    TaskState::Succeeded
                } else {
                    TaskState::Failed
                }
            );
            assert_eq!(outcome.exit_code, i32::try_from(code).ok());
            assert_eq!(outcome.windows_exit_code, Some(code));
            assert_eq!(
                *events.lock().unwrap(),
                [
                    "resume",
                    "capture",
                    "observe",
                    "terminate",
                    "stop_capture",
                    "wait",
                    "drop"
                ]
            );
            shared.finish(outcome);
            assert!(shared.lock().active.is_none());
            assert_eq!(shared.lock().completed.len(), 1);
        }
    }

    #[test]
    fn exact_output_caps_are_successful_until_more_bytes_are_observed() {
        for extra in [false, true] {
            let shared = active_shared();
            let events = Mutex::new(Vec::new());
            let outcome = run_with(request(), &shared, |_| {
                let mut owner = Fake::new(&shared, &events);
                owner.output = vec![
                    (Stream::Stdout, vec![b'o'; MAX_OUTPUT_BYTES_PER_STREAM]),
                    (Stream::Stderr, vec![b'e'; MAX_OUTPUT_BYTES_PER_STREAM]),
                ];
                if extra {
                    owner.output.push((Stream::Stderr, vec![b'!']));
                }
                owner.cancel_on_capture = true;
                Ok(owner)
            });
            assert_eq!(
                outcome.state,
                if extra {
                    TaskState::OutputLimit
                } else {
                    TaskState::Succeeded
                }
            );
            assert_eq!(outcome.truncated, extra);
            let store = shared.lock();
            let record = store.active.as_ref().unwrap();
            assert_eq!(record.stdout.len(), MAX_OUTPUT_BYTES_PER_STREAM);
            assert_eq!(record.stderr.len(), MAX_OUTPUT_BYTES_PER_STREAM);
        }
    }

    #[test]
    fn capture_failure_precedes_natural_exit_and_cancellation() {
        let shared = active_shared();
        let events = Mutex::new(Vec::new());
        let outcome = run_with(request(), &shared, |_| {
            let mut owner = Fake::new(&shared, &events);
            owner.cancel_on_capture = true;
            owner.read_error = true;
            Ok(owner)
        });
        assert_eq!(outcome.state, TaskState::Failed);
        assert_eq!(outcome.error.as_deref(), Some("capture failed"));
        assert!(outcome.truncated);
        assert!(!events.lock().unwrap().contains(&"observe"));
        assert_eq!(events.lock().unwrap().last(), Some(&"drop"));
    }

    #[test]
    fn resume_failure_and_spawn_failure_are_not_retried() {
        let shared = active_shared();
        let events = Mutex::new(Vec::new());
        let outcome = run_with(request(), &shared, |_| {
            let mut owner = Fake::new(&shared, &events);
            owner.resume_error = true;
            Ok(owner)
        });
        assert_eq!(outcome.state, TaskState::Failed);
        assert_eq!(outcome.error.as_deref(), Some("resume failed"));
        assert_eq!(
            *events.lock().unwrap(),
            [
                "resume",
                "terminate",
                "capture",
                "stop_capture",
                "wait",
                "drop"
            ]
        );
        let outcome = run_with::<Fake<'_>>(request(), &shared, |_| {
            Err(io::Error::other("launch failed"))
        });
        assert_eq!(outcome.state, TaskState::SpawnFailed);
        assert_eq!(outcome.windows_exit_code, None);
    }

    #[test]
    fn stopped_capture_is_not_eof_and_final_drain_is_bounded() {
        let shared = active_shared();
        let events = Mutex::new(Vec::new());
        let started = Instant::now();
        let outcome = run_with(request(), &shared, |_| {
            let mut owner = Fake::new(&shared, &events);
            owner.eof = false;
            Ok(owner)
        });
        assert_eq!(outcome.state, TaskState::Succeeded);
        assert!(outcome.truncated);
        assert!(started.elapsed() >= DRAIN_TIMEOUT);
        assert!(started.elapsed() < Duration::from_secs(2));
        let events = events.lock().unwrap();
        assert_eq!(
            &events[events.len() - 3..],
            ["stop_capture", "wait", "drop"]
        );
    }

    #[test]
    fn capture_completion_failure_is_reported_after_both_streams_finish() {
        let shared = active_shared();
        let events = Mutex::new(Vec::new());
        let outcome = run_with(request(), &shared, |_| {
            let mut owner = Fake::new(&shared, &events);
            owner.stop_error = true;
            Ok(owner)
        });
        assert_eq!(outcome.state, TaskState::Failed);
        assert!(outcome.truncated);
        assert_eq!(outcome.error.as_deref(), Some("capture completion failed"));
        assert_eq!(events.lock().unwrap().last(), Some(&"drop"));
    }

    #[test]
    fn final_drain_failures_refine_natural_exit_but_keep_selected_control_cause() {
        for cause in [
            TaskState::Succeeded,
            TaskState::Cancelled,
            TaskState::TimedOut,
        ] {
            for overflow in [false, true] {
                let shared = active_shared();
                let events = Mutex::new(Vec::new());
                let mut request = request();
                request.timeout = Duration::from_millis(100);
                let outcome = run_with(request, &shared, |request| {
                    let mut owner = Fake::new(&shared, &events);
                    owner.eof = false;
                    owner.final_eof = Some(true);
                    owner.final_read_error = !overflow;
                    if overflow {
                        owner.final_output =
                            vec![(Stream::Stdout, vec![b'x'; MAX_OUTPUT_BYTES_PER_STREAM + 1])];
                    }
                    if cause != TaskState::Succeeded {
                        owner.exit = None;
                    }
                    owner.cancel_on_capture = cause == TaskState::Cancelled;
                    if cause == TaskState::TimedOut {
                        owner.first_capture_delay = request.timeout;
                    }
                    Ok(owner)
                });
                let expected = if cause == TaskState::Succeeded {
                    if overflow {
                        TaskState::OutputLimit
                    } else {
                        TaskState::Failed
                    }
                } else {
                    cause
                };
                assert_eq!(outcome.state, expected);
                assert!(outcome.truncated);
                assert_eq!(events.lock().unwrap().last(), Some(&"drop"));
            }
        }
    }

    #[test]
    fn capture_unwind_drops_the_owner_before_a_failure_can_be_published() {
        let shared = active_shared();
        let events = Mutex::new(Vec::new());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_with(request(), &shared, |_| {
                let mut owner = Fake::new(&shared, &events);
                owner.output = vec![(Stream::Stdout, b"before panic".to_vec())];
                owner.capture_panic = true;
                Ok(owner)
            })
        }));
        assert!(result.is_err());
        assert_eq!(events.lock().unwrap().last(), Some(&"drop"));
        assert_eq!(
            shared.lock().active.as_ref().unwrap().stdout,
            b"before panic"
        );
        shared.finish(Outcome::failed("Command supervisor panicked"));
        assert_eq!(
            shared.lock().completed.front().unwrap().state,
            TaskState::Failed
        );
    }
}
