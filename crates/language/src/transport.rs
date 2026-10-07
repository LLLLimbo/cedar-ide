use crate::framing::{encode_json, FrameLimits};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;

#[cfg(any(windows, test))]
mod owned;
#[cfg(not(windows))]
mod portable;
#[cfg(windows)]
mod windows;
#[cfg(not(windows))]
use portable::Backend;
#[cfg(windows)]
use windows::Backend;

#[derive(Debug, Clone, Error)]
pub enum Error {
    #[error("language service I/O: {0}")]
    Io(String),
    #[error("language service protocol error: {0}")]
    Protocol(String),
    #[error("language service closed: {0}")]
    Closed(String),
    #[error("language service operation timed out: {0}")]
    Timeout(String),
    #[error("language service queue or pending-request limit reached")]
    QueueFull,
    #[error("language server error {code}: {message}")]
    Remote {
        code: i64,
        message: String,
        data: Option<Value>,
    },
    #[error("invalid language client state: {0}")]
    InvalidState(String),
    #[error("unsupported language service feature: {0}")]
    Unsupported(String),
}

/// Explicit process invocation. No shell expansion, installation or implicit
/// language-server discovery is performed. The process inherits the environment.
#[derive(Debug, Clone)]
pub struct ProcessConfig {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub working_directory: Option<PathBuf>,
    /// On non-Windows hosts, inherit stderr for diagnostics (`false` discards it).
    /// The Windows owned worker always drains stderr and retains at most a
    /// 16-KiB diagnostic tail when true; it never blocks on the host stderr.
    pub inherit_stderr: bool,
}

impl ProcessConfig {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            working_directory: None,
            inherit_stderr: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClientOptions {
    pub frame_limits: FrameLimits,
    /// Request/write deadline. On Windows this also bounds assembly of each
    /// incoming frame, starting at its first byte; idle frame boundaries do not
    /// time out. Increasing initialize's per-call timeout does not change this.
    pub request_timeout: Duration,
    pub shutdown_timeout: Duration,
    pub outbound_capacity: usize,
    pub event_capacity: usize,
    pub max_pending_requests: usize,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            frame_limits: FrameLimits::default(),
            request_timeout: Duration::from_secs(10),
            shutdown_timeout: Duration::from_secs(1),
            outbound_capacity: 64,
            event_capacity: 256,
            max_pending_requests: 128,
        }
    }
}

#[derive(Debug, Clone)]
pub enum RpcEvent {
    Notification {
        method: String,
        params: Value,
    },
    /// A method-not-found response has been queued for writing. Never execute these
    /// server requests implicitly (in particular workspace/applyEdit).
    UnsupportedServerRequest {
        method: String,
        id: Value,
    },
    /// Events were discarded to keep memory bounded and responses unblocked.
    /// Consumers must invalidate cached diagnostics and refresh their view.
    Lagged {
        dropped: usize,
    },
    Closed(Error),
}

type Reply = mpsc::SyncSender<Result<Value, Error>>;
struct Routing {
    pending: HashMap<u64, Reply>,
    terminal: Option<Error>,
}
struct Shared {
    routing: Mutex<Routing>,
    events: mpsc::SyncSender<RpcEvent>,
    dropped: AtomicUsize,
}

impl Shared {
    fn event(&self, event: RpcEvent) {
        if let Err(mpsc::TrySendError::Full(_)) = self.events.try_send(event) {
            let _ = self
                .dropped
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    Some(n.saturating_add(1))
                });
        }
    }

    fn fail(&self, error: Error) {
        let mut routing = self.routing.lock().unwrap();
        if routing.terminal.is_some() {
            return;
        }
        routing.terminal = Some(error.clone());
        for (_, sender) in routing.pending.drain() {
            let _ = sender.try_send(Err(error.clone()));
        }
        drop(routing);
        self.event(RpcEvent::Closed(error));
    }
}

struct WriteCommand {
    bytes: Vec<u8>,
    deadline: Instant,
    ack: Option<mpsc::SyncSender<Result<(), Error>>>,
}

/// Thread-safe stdio JSON-RPC transport. Requests may be made concurrently; a
/// background worker routes IDs and notifications independently. Poll events
/// from one consumer. Every queue and payload is bounded.
///
/// Windows requires an isolated host with controlled process spawning. Its
/// joined worker owns an atomic Job and completes all pending pipe I/O on Drop.
/// Other platforms kill/reap the direct child only. Launch a trusted server.
/// This transport does not make a caller's sequential protocol handler concurrent.
pub struct StdioRpc {
    backend: Backend,
    outbound: Option<mpsc::SyncSender<WriteCommand>>,
    shared: Arc<Shared>,
    events: Mutex<mpsc::Receiver<RpcEvent>>,
    next_id: AtomicU64,
    options: ClientOptions,
}

impl StdioRpc {
    pub fn spawn(config: ProcessConfig, options: ClientOptions) -> Result<Self, Error> {
        if options.outbound_capacity == 0
            || options.event_capacity == 0
            || options.max_pending_requests == 0
            || options.frame_limits.max_header_bytes < 32
            || options.frame_limits.max_content_bytes == 0
            || options.request_timeout.is_zero()
            || Instant::now()
                .checked_add(options.request_timeout)
                .is_none()
            || Instant::now()
                .checked_add(options.shutdown_timeout)
                .is_none()
        {
            return Err(Error::InvalidState(
                "queue limits, frame limits, and request timeout must be positive".into(),
            ));
        }
        // Overflow checks make the retained raw-payload budget explicit: the
        // bounded outbound queue plus one active write, pending replies, events,
        // and one incoming frame each carry at most max_content_bytes. JSON
        // nodes have bounded per-byte overhead; callers own returned values.
        let slots = options
            .outbound_capacity
            .checked_add(options.max_pending_requests)
            .and_then(|n| n.checked_add(options.event_capacity))
            .and_then(|n| n.checked_add(2));
        if slots
            .and_then(|n| n.checked_mul(options.frame_limits.max_content_bytes))
            .and_then(|n| n.checked_add(options.frame_limits.max_header_bytes))
            .is_none()
        {
            return Err(Error::InvalidState(
                "retained-payload limit overflow".into(),
            ));
        }
        let (outbound, writes) = mpsc::sync_channel(options.outbound_capacity);
        let (events_tx, events_rx) = mpsc::sync_channel(options.event_capacity);
        let shared = Arc::new(Shared {
            routing: Mutex::new(Routing {
                pending: HashMap::new(),
                terminal: None,
            }),
            events: events_tx,
            dropped: AtomicUsize::new(0),
        });
        let backend = Backend::spawn(
            config,
            &options,
            Arc::clone(&shared),
            outbound.clone(),
            writes,
        )?;
        Ok(Self {
            backend,
            outbound: Some(outbound),
            shared,
            events: Mutex::new(events_rx),
            next_id: AtomicU64::new(1),
            options,
        })
    }

    pub fn process_id(&self) -> u32 {
        self.backend.process_id()
    }

    pub fn request(&self, method: &str, params: Value) -> Result<Value, Error> {
        self.request_with_timeout(method, params, self.options.request_timeout)
    }

    /// The timeout includes queueing and writing. Expiration sends a best-effort
    /// `$/cancelRequest` and removes pending state; a late response is discarded.
    pub fn request_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, Error> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| Error::InvalidState("timeout overflow".into()))?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if id > i32::MAX as u64 {
            return Err(Error::InvalidState(
                "request ID space exhausted; restart the server".into(),
            ));
        }
        let bytes = self.encode(&outgoing_message(method, params, Some(id))?)?;
        let (sender, receiver) = mpsc::sync_channel(1);
        {
            let mut routing = self.shared.routing.lock().unwrap();
            if let Some(error) = &routing.terminal {
                return Err(error.clone());
            }
            if routing.pending.len() >= self.options.max_pending_requests {
                return Err(Error::QueueFull);
            }
            routing.pending.insert(id, sender);
        }
        if let Err(error) = self.enqueue(WriteCommand {
            bytes,
            deadline,
            ack: None,
        }) {
            self.shared.routing.lock().unwrap().pending.remove(&id);
            return Err(error);
        }
        match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.shared.routing.lock().unwrap().pending.remove(&id);
                if let Ok(bytes) = self.encode(
                    &json!({"jsonrpc":"2.0", "method":"$/cancelRequest", "params":{"id":id}}),
                ) {
                    let _ = self.enqueue(WriteCommand {
                        bytes,
                        deadline: Instant::now() + self.options.request_timeout,
                        ack: None,
                    });
                }
                Err(Error::Timeout(method.into()))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(Error::Closed("response router stopped".into()))
            }
        }
    }

    /// Wait until the notification has been written, not until the server has
    /// processed it. A write timeout aborts the connection: its delivery is unknown.
    pub fn notify(&self, method: &str, params: Value) -> Result<(), Error> {
        let bytes = self.encode(&outgoing_message(method, params, None)?)?;
        let (sender, receiver) = mpsc::sync_channel(1);
        self.enqueue(WriteCommand {
            bytes,
            deadline: Instant::now() + self.options.request_timeout,
            ack: Some(sender),
        })?;
        match receiver.recv_timeout(self.options.request_timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let error = Error::Timeout(format!("write notification {method}"));
                self.abort(error.clone());
                Err(error)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let routing = self.shared.routing.lock().unwrap();
                Err(routing
                    .terminal
                    .clone()
                    .unwrap_or_else(|| Error::Closed("writer stopped".into())))
            }
        }
    }

    /// One consumer should drain events. `Ok(None)` means no event arrived before
    /// this polling deadline, not EOF. `Lagged` is always surfaced before more events.
    pub fn next_event(&self, timeout: Duration) -> Result<Option<RpcEvent>, Error> {
        let dropped = self.shared.dropped.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            return Ok(Some(RpcEvent::Lagged { dropped }));
        }
        let receiver = self.events.lock().unwrap();
        match receiver.try_recv() {
            Ok(event) => return Ok(Some(event)),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err(Error::Closed("event stream stopped".into()))
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if let Some(error) = self.shared.routing.lock().unwrap().terminal.clone() {
            return Err(error);
        }
        match receiver.recv_timeout(timeout) {
            Ok(event) => Ok(Some(event)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(Error::Closed("event stream stopped".into()))
            }
        }
    }

    /// Wait briefly after `exit`, then stop the owned process if necessary.
    /// On Windows this joins the worker, the Job tree and all outstanding I/O.
    pub fn finish_process(&self) -> Result<(), Error> {
        self.backend
            .finish(&self.shared, self.options.shutdown_timeout)
    }

    pub fn abort(&self, reason: Error) {
        self.shared.fail(reason);
        self.backend.abort();
    }

    fn encode(&self, value: &Value) -> Result<Vec<u8>, Error> {
        encode_json(value, self.options.frame_limits).map_err(|e| Error::Protocol(e.to_string()))
    }

    fn enqueue(&self, command: WriteCommand) -> Result<(), Error> {
        if let Some(error) = &self.shared.routing.lock().unwrap().terminal {
            return Err(error.clone());
        }
        self.outbound
            .as_ref()
            .ok_or_else(|| Error::Closed("writer closed".into()))?
            .try_send(command)
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => Error::QueueFull,
                mpsc::TrySendError::Disconnected(_) => Error::Closed("writer stopped".into()),
            })?;
        self.backend.wake();
        Ok(())
    }
}

impl Drop for StdioRpc {
    fn drop(&mut self) {
        self.abort(Error::Closed("client dropped".into()));
        self.outbound.take();
    }
}

fn outgoing_message(method: &str, params: Value, id: Option<u64>) -> Result<Value, Error> {
    if !params.is_null() && !params.is_object() && !params.is_array() {
        return Err(Error::InvalidState(
            "JSON-RPC params must be an object, array or omitted (null)".into(),
        ));
    }
    let mut message = json!({"jsonrpc":"2.0", "method":method});
    if let Some(id) = id {
        message["id"] = json!(id);
    }
    if !params.is_null() {
        message["params"] = params;
    }
    Ok(message)
}

fn route_message(
    message: Value,
    shared: &Shared,
    outbound: &mpsc::SyncSender<WriteCommand>,
    timeout: Duration,
    limits: FrameLimits,
) -> Result<(), Error> {
    if !message.is_object() || message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(Error::Protocol("expected a JSON-RPC 2.0 object".into()));
    }
    if let Some(method) = message.get("method") {
        let method = method
            .as_str()
            .ok_or_else(|| Error::Protocol("method must be a string".into()))?;
        if let Some(id) = message.get("id") {
            if !(id.is_string() || id.as_i64().is_some_and(|id| i32::try_from(id).is_ok())) {
                return Err(Error::Protocol("invalid server request ID".into()));
            }
            let response = json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32601,"message":"This client does not implement server-initiated requests"}});
            let bytes =
                encode_json(&response, limits).map_err(|e| Error::Protocol(e.to_string()))?;
            outbound
                .try_send(WriteCommand {
                    bytes,
                    deadline: Instant::now() + timeout,
                    ack: None,
                })
                .map_err(|_| Error::QueueFull)?;
            shared.event(RpcEvent::UnsupportedServerRequest {
                method: method.into(),
                id: id.clone(),
            });
        } else {
            shared.event(RpcEvent::Notification {
                method: method.into(),
                params: message.get("params").cloned().unwrap_or(Value::Null),
            });
        }
        return Ok(());
    }
    let id = message
        .get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Protocol("response has no numeric request ID".into()))?;
    let result = match (message.get("result"), message.get("error")) {
        (Some(result), None) => Ok(result.clone()),
        (None, Some(error)) => {
            let code = error
                .get("code")
                .and_then(Value::as_i64)
                .ok_or_else(|| Error::Protocol("invalid response error code".into()))?;
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Protocol("invalid response error message".into()))?;
            Err(Error::Remote {
                code,
                message: message.into(),
                data: error.get("data").cloned(),
            })
        }
        _ => {
            return Err(Error::Protocol(
                "response must contain exactly one of result or error".into(),
            ))
        }
    };
    if let Some(sender) = shared.routing.lock().unwrap().pending.remove(&id) {
        let _ = sender.try_send(result);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared(capacity: usize) -> (Shared, mpsc::Receiver<RpcEvent>) {
        let (events, receiver) = mpsc::sync_channel(capacity);
        (
            Shared {
                routing: Mutex::new(Routing {
                    pending: HashMap::new(),
                    terminal: None,
                }),
                events,
                dropped: AtomicUsize::new(0),
            },
            receiver,
        )
    }

    fn route(
        value: Value,
        shared: &Shared,
        writes: &mpsc::SyncSender<WriteCommand>,
    ) -> Result<(), Error> {
        route_message(
            value,
            shared,
            writes,
            Duration::from_secs(1),
            FrameLimits::default(),
        )
    }

    #[test]
    fn routing_correlates_out_of_order_responses_and_discards_late_ids() {
        let (shared, _) = shared(1);
        let (writes, _) = mpsc::sync_channel(1);
        let (first, first_rx) = mpsc::sync_channel(1);
        let (second, second_rx) = mpsc::sync_channel(1);
        shared
            .routing
            .lock()
            .unwrap()
            .pending
            .extend([(1, first), (2, second)]);
        route(
            json!({"jsonrpc":"2.0","id":2,"result":"second"}),
            &shared,
            &writes,
        )
        .unwrap();
        assert_eq!(second_rx.try_recv().unwrap().unwrap(), "second");
        assert!(first_rx.try_recv().is_err());
        route(
            json!({"jsonrpc":"2.0","id":1,"result":"first"}),
            &shared,
            &writes,
        )
        .unwrap();
        assert_eq!(first_rx.try_recv().unwrap().unwrap(), "first");
        route(
            json!({"jsonrpc":"2.0","id":1,"result":"late"}),
            &shared,
            &writes,
        )
        .unwrap();
        assert!(shared.routing.lock().unwrap().pending.is_empty());
    }

    #[test]
    fn full_event_queue_does_not_block_responses_or_terminal_failure() {
        let (shared, events) = shared(1);
        let (writes, _) = mpsc::sync_channel(1);
        route(
            json!({"jsonrpc":"2.0","method":"event","params":{"n":1}}),
            &shared,
            &writes,
        )
        .unwrap();
        route(
            json!({"jsonrpc":"2.0","method":"event","params":{"n":2}}),
            &shared,
            &writes,
        )
        .unwrap();
        let (pending, response) = mpsc::sync_channel(1);
        shared.routing.lock().unwrap().pending.insert(1, pending);
        shared.fail(Error::Closed("first failure".into()));
        shared.fail(Error::Closed("second failure".into()));
        assert!(
            matches!(response.try_recv().unwrap(), Err(Error::Closed(reason)) if reason == "first failure")
        );
        assert_eq!(shared.dropped.load(Ordering::Relaxed), 2);
        assert!(
            matches!(events.try_recv().unwrap(), RpcEvent::Notification { params, .. } if params["n"] == 1)
        );
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn rejected_server_request_is_bounded_and_never_reported_before_enqueue() {
        let (shared, events) = shared(2);
        let (writes, receiver) = mpsc::sync_channel(1);
        let request =
            json!({"jsonrpc":"2.0","id":"server-id","method":"workspace/applyEdit","params":{}});
        route(request.clone(), &shared, &writes).unwrap();
        assert!(
            matches!(events.try_recv().unwrap(), RpcEvent::UnsupportedServerRequest { id, .. } if id == "server-id")
        );
        assert!(matches!(
            route(request, &shared, &writes),
            Err(Error::QueueFull)
        ));
        assert!(events.try_recv().is_err());
        let reply: Value = serde_json::from_slice(&receiver.try_recv().unwrap().bytes).unwrap();
        assert_eq!(reply["error"]["code"], -32601);
        assert_eq!(reply["id"], "server-id");
    }

    #[test]
    fn malformed_response_never_consumes_pending_reply() {
        let (shared, _) = shared(1);
        let (writes, _) = mpsc::sync_channel(1);
        let (pending, _) = mpsc::sync_channel(1);
        shared.routing.lock().unwrap().pending.insert(1, pending);
        for value in [
            json!({"jsonrpc":"2.0","id":1,"result":true,"error":{"code":-1,"message":"bad"}}),
            json!({"jsonrpc":"2.0","id":1,"error":{"code":"bad","message":"bad"}}),
            json!({"jsonrpc":"1.0","id":1,"result":true}),
            json!({"jsonrpc":"2.0","method":"run","id":true}),
        ] {
            assert!(matches!(
                route(value, &shared, &writes),
                Err(Error::Protocol(_))
            ));
            assert_eq!(shared.routing.lock().unwrap().pending.len(), 1);
        }
    }

    #[test]
    fn impossible_retention_budget_is_rejected_before_launch() {
        let options = ClientOptions {
            outbound_capacity: usize::MAX,
            ..ClientOptions::default()
        };
        assert!(matches!(
            StdioRpc::spawn(ProcessConfig::new("never-launched"), options),
            Err(Error::InvalidState(_))
        ));
    }
}
