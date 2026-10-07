//! Bounded Debug Adapter Protocol transport for an explicitly selected stdio adapter.
//!
//! Request handles are independent of the event stream. In particular, callers
//! must keep `launch` pending while receiving `initialized`, setting breakpoints
//! and sending `configurationDone`. This is transport infrastructure, not an IDE
//! debugger integration or a sandbox for adapters/debuggees.

pub use cedar_language::dap::{initialize_request, Message};
pub use cedar_language::framing::FrameLimits;
use cedar_language::framing::{encode_json, read_frame, write_frame};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug, Clone, Error, PartialEq)]
pub enum Error {
    #[error("debug adapter I/O: {0}")]
    Io(String),
    #[error("debug adapter protocol: {0}")]
    Protocol(String),
    #[error("debug adapter closed: {0}")]
    Closed(String),
    #[error("DAP request {command} ({request_seq}) timed out")]
    Timeout { request_seq: u32, command: String },
    #[error("debug adapter queue or pending-request limit reached")]
    Capacity,
    #[error("debug adapter critical event queue overflowed; session state is invalid")]
    EventOverflow,
    #[error("invalid debug adapter configuration: {0}")]
    Invalid(String),
    #[error("DAP {command} failed: {message}")]
    Remote {
        command: String,
        message: String,
        body: Option<Value>,
    },
}

/// Direct executable invocation, without a shell. No adapter installation,
/// terminal launch, inherited reverse-request execution or discovery occurs.
#[derive(Debug, Clone)]
pub struct ProcessConfig {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub working_directory: Option<PathBuf>,
}
impl ProcessConfig {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: vec![],
            working_directory: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Options {
    pub frame_limits: FrameLimits,
    pub request_timeout: Duration,
    pub shutdown_timeout: Duration,
    pub max_pending_requests: usize,
    pub outbound_capacity: usize,
    /// Every non-output event is treated as critical. Overflow fails closed.
    pub event_capacity: usize,
    /// Combined output-event and adapter-stderr tail, in UTF-8 bytes.
    pub output_capacity_bytes: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            frame_limits: FrameLimits::default(),
            request_timeout: Duration::from_secs(15),
            shutdown_timeout: Duration::from_secs(1),
            max_pending_requests: 128,
            outbound_capacity: 64,
            event_capacity: 256,
            output_capacity_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub request_seq: u32,
    pub command: String,
    pub body: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Adapter {
        seq: u32,
        event: String,
        body: Option<Value>,
    },
    /// The failed response is queued; this never runs adapter-supplied commands.
    ReverseRequestRejected { request_seq: u32, command: String },
    /// Out-of-band terminal state is never hidden behind a full event queue.
    Closed(Error),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutputTail {
    pub text: String,
    pub discarded_bytes: u64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub pending_requests: usize,
    /// Includes expired, canceled, duplicate and unknown response identifiers.
    pub ignored_responses: u64,
    pub rejected_reverse_requests: u64,
}

struct Pending {
    command: String,
    deadline: Instant,
    reply: mpsc::SyncSender<Result<Response, Error>>,
}
struct State {
    next_seq: u32,
    pending: HashMap<u32, Pending>,
    events: VecDeque<Event>,
    output: OutputTail,
    terminal: Option<Error>,
    terminal_delivered: bool,
    ignored_responses: u64,
    rejected_reverse_requests: u64,
}
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    options: Options,
}
impl Shared {
    fn fail(&self, error: Error) {
        let mut state = self.state.lock().unwrap();
        Self::fail_locked(&mut state, error);
        self.changed.notify_all();
    }
    fn fail_locked(state: &mut State, error: Error) {
        if state.terminal.is_some() {
            return;
        }
        state.terminal = Some(error.clone());
        state.events.clear();
        for (_, pending) in state.pending.drain() {
            let _ = pending.reply.try_send(Err(error.clone()));
        }
    }
    fn sequence(state: &mut State) -> Result<u32, Error> {
        let seq = state.next_seq;
        state.next_seq = seq
            .checked_add(1)
            .ok_or_else(|| Error::Protocol("sequence exhausted".into()))?;
        Ok(seq)
    }
    fn event(&self, event: Event) {
        let mut state = self.state.lock().unwrap();
        if state.terminal.is_some() {
            return;
        }
        if state.events.len() == self.options.event_capacity {
            Self::fail_locked(&mut state, Error::EventOverflow);
        } else {
            state.events.push_back(event);
        }
        self.changed.notify_all();
    }
    fn output(&self, output: &str) {
        let mut state = self.state.lock().unwrap();
        let tail = &mut state.output;
        let cap = self.options.output_capacity_bytes;
        if output.len() >= cap {
            let mut start = output.len() - cap;
            while !output.is_char_boundary(start) {
                start += 1;
            }
            tail.discarded_bytes = tail
                .discarded_bytes
                .saturating_add((tail.text.len() + start) as u64);
            tail.text.clear();
            tail.text.push_str(&output[start..]);
        } else {
            let excess = (tail.text.len() + output.len()).saturating_sub(cap);
            if excess > 0 {
                let mut boundary = excess;
                while !tail.text.is_char_boundary(boundary) {
                    boundary += 1;
                }
                tail.discarded_bytes = tail.discarded_bytes.saturating_add(boundary as u64);
                tail.text.drain(..boundary);
            }
            tail.text.push_str(output);
        }
    }
}
struct Outbound {
    bytes: Vec<u8>,
    request_seq: Option<u32>,
}

/// A request is already queued when this handle is returned. Dropping a handle
/// cancels local interest only; it does not undo adapter-side effects or send DAP
/// cancel. Timeouts have the same caveat. Late responses are ignored and counted.
pub struct RequestHandle {
    request_seq: u32,
    receiver: mpsc::Receiver<Result<Response, Error>>,
    shared: Arc<Shared>,
}
impl RequestHandle {
    pub fn request_seq(&self) -> u32 {
        self.request_seq
    }
    pub fn try_result(&self) -> Option<Result<Response, Error>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(Error::Closed("request channel ended".into())))
            }
        }
    }
    /// Wait for this request's original deadline, not a fresh timeout.
    pub fn wait(self) -> Result<Response, Error> {
        self.receiver
            .recv()
            .unwrap_or_else(|_| Err(Error::Closed("request channel ended".into())))
    }
}
impl Drop for RequestHandle {
    fn drop(&mut self) {
        self.shared
            .state
            .lock()
            .unwrap()
            .pending
            .remove(&self.request_seq);
    }
}

pub struct DapClient {
    shared: Arc<Shared>,
    outbound: mpsc::SyncSender<Outbound>,
    child: Arc<Mutex<Child>>,
    monitor: Option<thread::JoinHandle<()>>,
    process_id: u32,
}
impl DapClient {
    pub fn spawn(config: ProcessConfig, options: Options) -> Result<Self, Error> {
        if options.max_pending_requests == 0
            || options.outbound_capacity == 0
            || options.event_capacity == 0
            || options.frame_limits.max_content_bytes == 0
            || options.frame_limits.max_header_bytes < 32
            || options.request_timeout.is_zero()
            || options.shutdown_timeout.is_zero()
        {
            return Err(Error::Invalid(
                "capacities and timeouts must be positive; frame header limit must be at least 32"
                    .into(),
            ));
        }
        let mut command = Command::new(&config.program);
        command
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &config.working_directory {
            command.current_dir(cwd);
        }
        let mut child = command.spawn().map_err(|e| Error::Io(e.to_string()))?;
        let process_id = child.id();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let child = Arc::new(Mutex::new(child));
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                next_seq: 1,
                pending: HashMap::new(),
                events: VecDeque::new(),
                output: OutputTail::default(),
                terminal: None,
                terminal_delivered: false,
                ignored_responses: 0,
                rejected_reverse_requests: 0,
            }),
            changed: Condvar::new(),
            options,
        });
        let (outbound, receiver) = mpsc::sync_channel::<Outbound>(shared.options.outbound_capacity);
        let writer_shared = shared.clone();
        thread::spawn(move || {
            let mut stdin = stdin;
            while let Ok(frame) = receiver
                .recv_timeout(Duration::from_millis(20))
                .or_else(|e| match e {
                    mpsc::RecvTimeoutError::Timeout
                        if writer_shared.state.lock().unwrap().terminal.is_none() =>
                    {
                        Ok(Outbound {
                            bytes: vec![],
                            request_seq: None,
                        })
                    }
                    _ => Err(e),
                })
            {
                if writer_shared.state.lock().unwrap().terminal.is_some() {
                    break;
                }
                if frame.bytes.is_empty() {
                    continue;
                }
                if let Some(seq) = frame.request_seq {
                    if !writer_shared
                        .state
                        .lock()
                        .unwrap()
                        .pending
                        .contains_key(&seq)
                    {
                        continue;
                    }
                }
                if let Err(error) =
                    write_frame(&mut stdin, &frame.bytes, writer_shared.options.frame_limits)
                {
                    writer_shared.fail(Error::Io(error.to_string()));
                    break;
                }
            }
        });
        let reader_shared = shared.clone();
        let reverse_writer = outbound.clone();
        thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            loop {
                let bytes = match read_frame(&mut stdout, reader_shared.options.frame_limits) {
                    Ok(Some(bytes)) => bytes,
                    Ok(None) => {
                        reader_shared.fail(Error::Closed("adapter stdout reached EOF".into()));
                        break;
                    }
                    Err(error) => {
                        reader_shared.fail(Error::Protocol(error.to_string()));
                        break;
                    }
                };
                let message = match serde_json::from_slice::<Message>(&bytes) {
                    Ok(message) => message,
                    Err(error) => {
                        reader_shared.fail(Error::Protocol(error.to_string()));
                        break;
                    }
                };
                match message {
                    Message::Response {
                        request_seq,
                        command,
                        success,
                        message,
                        body,
                        ..
                    } => {
                        let mut state = reader_shared.state.lock().unwrap();
                        if state.terminal.is_some() {
                            break;
                        }
                        if let Some(pending) = state.pending.remove(&request_seq) {
                            if pending.command != command {
                                let error = Error::Protocol(format!("response {request_seq} command {command:?} does not match {:?}", pending.command));
                                let _ = pending.reply.try_send(Err(error.clone()));
                                Shared::fail_locked(&mut state, error);
                                reader_shared.changed.notify_all();
                                break;
                            }
                            let result = if Instant::now() >= pending.deadline {
                                state.ignored_responses = state.ignored_responses.saturating_add(1);
                                Err(Error::Timeout {
                                    request_seq,
                                    command,
                                })
                            } else if success {
                                Ok(Response {
                                    request_seq,
                                    command,
                                    body,
                                })
                            } else {
                                Err(Error::Remote {
                                    command,
                                    message: message
                                        .unwrap_or_else(|| "unspecified adapter error".into()),
                                    body,
                                })
                            };
                            let _ = pending.reply.try_send(result);
                        } else {
                            state.ignored_responses = state.ignored_responses.saturating_add(1);
                        }
                    }
                    Message::Event { seq, event, body } if event == "output" => {
                        let _ = seq;
                        match body
                            .as_ref()
                            .and_then(|body| body.get("output"))
                            .and_then(Value::as_str)
                        {
                            Some(output) => reader_shared.output(output),
                            None => {
                                reader_shared.fail(Error::Protocol(
                                    "output event is missing string body.output".into(),
                                ));
                                break;
                            }
                        }
                    }
                    Message::Event { seq, event, body } => {
                        reader_shared.event(Event::Adapter { seq, event, body })
                    }
                    Message::Request {
                        seq: request_seq,
                        command,
                        ..
                    } => {
                        let seq = {
                            let mut state = reader_shared.state.lock().unwrap();
                            if state.terminal.is_some() {
                                break;
                            }
                            state.rejected_reverse_requests =
                                state.rejected_reverse_requests.saturating_add(1);
                            match Shared::sequence(&mut state) {
                                Ok(seq) => seq,
                                Err(error) => {
                                    Shared::fail_locked(&mut state, error);
                                    reader_shared.changed.notify_all();
                                    break;
                                }
                            }
                        };
                        let refusal = Message::Response {
                            seq,
                            request_seq,
                            success: false,
                            command: command.clone(),
                            message: Some("Cedar does not execute adapter reverse requests".into()),
                            body: None,
                        };
                        let bytes = match encode_json(&refusal, reader_shared.options.frame_limits)
                        {
                            Ok(bytes) => bytes,
                            Err(error) => {
                                reader_shared.fail(Error::Protocol(error.to_string()));
                                break;
                            }
                        };
                        if reverse_writer
                            .try_send(Outbound {
                                bytes,
                                request_seq: None,
                            })
                            .is_err()
                        {
                            reader_shared.fail(Error::Capacity);
                            break;
                        }
                        reader_shared.event(Event::ReverseRequestRejected {
                            request_seq,
                            command,
                        });
                    }
                }
                if reader_shared.state.lock().unwrap().terminal.is_some() {
                    break;
                }
            }
        });
        let stderr_shared = shared.clone();
        thread::spawn(move || {
            let mut bytes = [0; 4096];
            while let Ok(n) = stderr.read(&mut bytes) {
                if n == 0 {
                    break;
                }
                stderr_shared.output(&String::from_utf8_lossy(&bytes[..n]));
                if stderr_shared.state.lock().unwrap().terminal.is_some() {
                    break;
                }
            }
        });
        let monitor_shared = shared.clone();
        let monitor_child = child.clone();
        let monitor = thread::spawn(move || {
            let mut exited: Option<(String, Instant)> = None;
            loop {
                {
                    let mut state = monitor_shared.state.lock().unwrap();
                    let now = Instant::now();
                    let expired: Vec<_> = state
                        .pending
                        .iter()
                        .filter(|(_, p)| now >= p.deadline)
                        .map(|(&seq, _)| seq)
                        .collect();
                    for seq in expired {
                        let p = state.pending.remove(&seq).unwrap();
                        let _ = p.reply.try_send(Err(Error::Timeout {
                            request_seq: seq,
                            command: p.command,
                        }));
                    }
                    if state.terminal.is_some() {
                        drop(state);
                        let mut child = monitor_child.lock().unwrap();
                        let _ = child.kill();
                        // Reap the direct child only. Detached debuggee/process
                        // groups are outside this portable transport guarantee.
                        let _ = child.wait();
                        break;
                    }
                }
                if let Some((status, deadline)) = &exited {
                    if Instant::now() >= *deadline {
                        monitor_shared.fail(Error::Closed(format!("adapter exited: {status}")));
                    }
                } else {
                    let status = monitor_child.lock().unwrap().try_wait();
                    match status {
                        Ok(Some(status)) => {
                            // Reaping can race a reader draining a final response.
                            // Allow bounded drain, while continuing to expire
                            // requests if a descendant inherited the stdout pipe.
                            let deadline = Instant::now()
                                .checked_add(monitor_shared.options.shutdown_timeout)
                                .unwrap_or_else(Instant::now);
                            exited = Some((status.to_string(), deadline));
                        }
                        Err(error) => monitor_shared.fail(Error::Io(error.to_string())),
                        Ok(None) => {}
                    }
                }
                thread::sleep(Duration::from_millis(5));
            }
        });
        Ok(Self {
            shared,
            outbound,
            child,
            monitor: Some(monitor),
            process_id,
        })
    }
    pub fn process_id(&self) -> u32 {
        self.process_id
    }
    pub fn request(&self, command: &str, arguments: Option<Value>) -> Result<RequestHandle, Error> {
        self.request_with_timeout(command, arguments, self.shared.options.request_timeout)
    }
    pub fn request_with_timeout(
        &self,
        command: &str,
        arguments: Option<Value>,
        timeout: Duration,
    ) -> Result<RequestHandle, Error> {
        if command.is_empty()
            || command.len() > 128
            || !command.as_bytes()[0].is_ascii_alphabetic()
            || !command
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || timeout.is_zero()
        {
            return Err(Error::Invalid(
                "command must be an ASCII DAP identifier and timeout must be positive".into(),
            ));
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| Error::Invalid("timeout is too large".into()))?;
        let (reply, receiver) = mpsc::sync_channel(1);
        let mut state = self.shared.state.lock().unwrap();
        if let Some(error) = &state.terminal {
            return Err(error.clone());
        }
        if state.pending.len() >= self.shared.options.max_pending_requests {
            return Err(Error::Capacity);
        }
        let seq = Shared::sequence(&mut state)?;
        let bytes = encode_json(
            &Message::Request {
                seq,
                command: command.into(),
                arguments,
            },
            self.shared.options.frame_limits,
        )
        .map_err(|error| Error::Protocol(error.to_string()))?;
        state.pending.insert(
            seq,
            Pending {
                command: command.into(),
                deadline,
                reply,
            },
        );
        if let Err(error) = self.outbound.try_send(Outbound {
            bytes,
            request_seq: Some(seq),
        }) {
            state.pending.remove(&seq);
            return Err(match error {
                mpsc::TrySendError::Full(_) => Error::Capacity,
                mpsc::TrySendError::Disconnected(_) => Error::Closed("writer stopped".into()),
            });
        }
        Ok(RequestHandle {
            request_seq: seq,
            receiver,
            shared: self.shared.clone(),
        })
    }
    pub fn initialize(&self, adapter_id: &str) -> Result<RequestHandle, Error> {
        let Message::Request { arguments, .. } = initialize_request(0, adapter_id) else {
            unreachable!()
        };
        self.request("initialize", arguments)
    }
    /// The first read after failure returns Closed even if the event queue was
    /// full. Later reads return None; terminal_error remains inspectable forever.
    pub fn next_event(&self, timeout: Duration) -> Option<Event> {
        let mut state = self.shared.state.lock().unwrap();
        let deadline = Instant::now().checked_add(timeout)?;
        loop {
            if let Some(error) = state.terminal.clone() {
                if state.terminal_delivered {
                    return None;
                }
                state.terminal_delivered = true;
                return Some(Event::Closed(error));
            }
            if let Some(event) = state.events.pop_front() {
                return Some(event);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, remaining)
                .unwrap()
                .0;
        }
    }
    pub fn terminal_error(&self) -> Option<Error> {
        self.shared.state.lock().unwrap().terminal.clone()
    }
    pub fn output_tail(&self) -> OutputTail {
        self.shared.state.lock().unwrap().output.clone()
    }
    pub fn stats(&self) -> Stats {
        let state = self.shared.state.lock().unwrap();
        Stats {
            pending_requests: state.pending.len(),
            ignored_responses: state.ignored_responses,
            rejected_reverse_requests: state.rejected_reverse_requests,
        }
    }
    /// Request adapter-managed debuggee termination, then always reap the direct
    /// adapter. A successful response is not proof every descendant has exited.
    pub fn disconnect(&mut self, terminate_debuggee: bool) -> Result<Response, Error> {
        let result = self
            .request_with_timeout(
                "disconnect",
                Some(serde_json::json!({"terminateDebuggee":terminate_debuggee})),
                self.shared.options.shutdown_timeout,
            )
            .and_then(RequestHandle::wait);
        self.stop();
        result
    }
    /// Idempotently kill and reap the directly spawned adapter, without joining
    /// pipe workers that an unrelated descendant could keep blocked. This does
    /// NOT guarantee cleanup of detached adapters, launchers or debuggees.
    pub fn stop(&mut self) {
        self.shared.fail(Error::Closed("client stopped".into()));
        if let Some(monitor) = self.monitor.take() {
            let _ = monitor.join();
        }
        // try_wait is cached after reaping. Keep the ownership invariant explicit.
        let _ = self.child.lock().unwrap().try_wait();
    }
}
impl Drop for DapClient {
    fn drop(&mut self) {
        self.stop();
    }
}
