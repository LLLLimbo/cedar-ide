//! Transport-neutral workspace client. SSH uses the user's existing OpenSSH configuration.
//! No passwords, host-key acceptance, key generation, port listeners, or telemetry.
use cedar_protocol::{
    read_frame, supports_capability, write_frame, Operation, Payload, Request, Response,
    JAVA_LANGUAGE_SESSION_CAPABILITIES, LANGUAGE_SESSION_CAPABILITIES, PROTOCOL_VERSION,
    RUN_TASK_CAPABILITIES,
};
#[cfg(not(windows))]
use cedar_workspace::Workspace;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

// The bundled agent has only redirected stdio and must not open a console
// window when launched by the GUI. This does not change other transports.
// https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// One connection generation's cancellation signal. Cancellation is permanent;
/// create a fresh token for a replacement connection.
///
/// Process Hello/List/Read/Search waits observe this at most every 50 ms,
/// subject to scheduling. Already enqueued mutation, Git, language, and task
/// operations retain their normal replies and deadlines. Synchronous embedded
/// workspace calls, OS process creation, and kernel cleanup are not interrupted.
#[derive(Clone, Debug, Default)]
pub struct ConnectionCancellation(Arc<AtomicBool>);

impl ConnectionCancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

fn cancellation_requested(cancellation: &Option<ConnectionCancellation>) -> bool {
    cancellation
        .as_ref()
        .is_some_and(ConnectionCancellation::is_cancelled)
}

fn cancellation_error() -> String {
    "transport_cancelled: connection cancelled; reconnect with a fresh token".into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionSpec {
    Local {
        root: PathBuf,
        allow_run: bool,
    },
    /// First release supports a POSIX shell / Linux remote, and native Windows/Linux clients.
    Ssh {
        host: String,
        port: u16,
        root: String,
        agent_path: String,
        allow_run: bool,
    },
}

pub struct Client {
    backend: Backend,
    cancellation: Option<ConnectionCancellation>,
    // A session uses exactly one validated implementation/capability/root snapshot.
    // Capability claims describe compatibility, never execution authorization.
    handshake: Payload,
}
enum Backend {
    #[cfg(not(windows))]
    Local(Box<Workspace>),
    Process(ProcessClient),
}
impl Client {
    pub fn connect(spec: ConnectionSpec) -> Result<Self, String> {
        Self::connect_inner(spec, None)
    }
    pub fn connect_with_cancellation(
        spec: ConnectionSpec,
        cancellation: ConnectionCancellation,
    ) -> Result<Self, String> {
        Self::connect_inner(spec, Some(cancellation))
    }
    fn connect_inner(
        spec: ConnectionSpec,
        cancellation: Option<ConnectionCancellation>,
    ) -> Result<Self, String> {
        if cancellation_requested(&cancellation) {
            return Err(cancellation_error());
        }
        match spec {
            ConnectionSpec::Local { root, allow_run } => {
                #[cfg(windows)]
                {
                    Self::connect_bundled_windows_agent(&root, allow_run, cancellation)
                }
                #[cfg(not(windows))]
                {
                    let mut workspace = Workspace::open(root).map_err(|e| e.to_string())?;
                    workspace.set_allow_run(allow_run);
                    let handshake = workspace
                        .handle(Operation::Hello)
                        .map_err(|e| e.to_string())?;
                    validate_handshake(&handshake)?;
                    if cancellation_requested(&cancellation) {
                        return Err(cancellation_error());
                    }
                    Ok(Self {
                        backend: Backend::Local(Box::new(workspace)),
                        cancellation,
                        handshake,
                    })
                }
            }
            ConnectionSpec::Ssh {
                host,
                port,
                root,
                agent_path,
                allow_run,
            } => {
                let args = ssh_arguments(&host, port, &root, &agent_path, allow_run)?;
                let mut cmd = Command::new("ssh");
                cmd.args(args);
                Self::from_command_with_cancellation(cmd, cancellation)
            }
        }
    }
    #[cfg(windows)]
    fn connect_bundled_windows_agent(
        root: &Path,
        allow_run: bool,
        cancellation: Option<ConnectionCancellation>,
    ) -> Result<Self, String> {
        let executable = std::env::current_exe().map_err(|e| {
            format!("bundled_agent_missing: cannot locate this executable's bundled cedar-agent.exe: {e}")
        })?;
        let agent = bundled_windows_agent_path(&executable)?;
        if !agent.is_file() {
            return Err(format!(
                "bundled_agent_missing: bundled cedar-agent.exe is missing at {}; restore it beside this executable",
                agent.display()
            ));
        }
        let mut cmd = Command::new(&agent);
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.arg("--root").arg(root);
        if allow_run {
            cmd.arg("--allow-run");
        }
        let process = ProcessClient::spawn_bundled_agent(cmd, cancellation)?;
        Self::from_process(process).map_err(|e| {
            if e.starts_with("transport_cancelled:") {
                return e;
            }
            format!(
                "bundled_agent_start_failed: bundled cedar-agent.exe at {} could not establish a workspace connection: {e}",
                agent.display()
            )
        })
    }
    /// Use a separately deployed local agent, also useful for process-isolated integration tests.
    pub fn spawn_agent(agent: &Path, root: &Path, allow_run: bool) -> Result<Self, String> {
        Self::spawn_agent_inner(agent, root, allow_run, None)
    }
    pub fn spawn_agent_with_cancellation(
        agent: &Path,
        root: &Path,
        allow_run: bool,
        cancellation: ConnectionCancellation,
    ) -> Result<Self, String> {
        Self::spawn_agent_inner(agent, root, allow_run, Some(cancellation))
    }
    fn spawn_agent_inner(
        agent: &Path,
        root: &Path,
        allow_run: bool,
        cancellation: Option<ConnectionCancellation>,
    ) -> Result<Self, String> {
        let mut cmd = Command::new(agent);
        cmd.arg("--root").arg(root);
        if allow_run {
            cmd.arg("--allow-run");
        }
        Self::from_command_with_cancellation(cmd, cancellation)
    }
    fn from_command_with_cancellation(
        cmd: Command,
        cancellation: Option<ConnectionCancellation>,
    ) -> Result<Self, String> {
        Self::from_process(ProcessClient::spawn(cmd, cancellation)?)
    }
    fn from_process(mut process: ProcessClient) -> Result<Self, String> {
        let handshake = process.request(Operation::Hello)?;
        validate_handshake(&handshake)?;
        Ok(Self {
            cancellation: process.cancellation.clone(),
            backend: Backend::Process(process),
            handshake,
        })
    }
    /// The first validated Hello, retained unchanged for this connection.
    /// Agent metadata is a compatibility claim, not verified identity or trust.
    pub fn handshake(&self) -> &Payload {
        &self.handshake
    }
    pub fn request(&mut self, op: Operation) -> Result<Payload, String> {
        if cancellation_requested(&self.cancellation) {
            return Err(match &mut self.backend {
                #[cfg(not(windows))]
                Backend::Local(_) => cancellation_error(),
                Backend::Process(process) => process.fail(cancellation_error()),
            });
        }
        if !self.is_connected() {
            return Err(
                "disconnected: reconnect before retrying; unsaved buffers remain local".into(),
            );
        }
        if matches!(op, Operation::Hello) {
            return Ok(self.handshake.clone());
        }
        self.require_capabilities(&op)?;
        match &mut self.backend {
            #[cfg(not(windows))]
            Backend::Local(ws) => ws.handle(op).map_err(|e| e.to_string()),
            Backend::Process(p) => p.request(op),
        }
    }
    fn require_capabilities(&self, op: &Operation) -> Result<(), String> {
        let Payload::Hello { agent, .. } = &self.handshake else {
            unreachable!("client handshake is validated before construction");
        };
        let require = |capability: &str| {
            if supports_capability(agent.as_ref(), capability) {
                Ok(())
            } else {
                Err(format!(
                    "unsupported_operation: agent does not advertise {capability}; reconnect with a compatible agent"
                ))
            }
        };
        if let Some(capability) = op.capability_name() {
            require(capability)?;
        }
        // Do not launch a process whose required lifecycle cannot be managed.
        // In particular, never downgrade RunStart to the legacy blocking Run.
        let lifecycle = match op {
            Operation::RunStart { .. } => RUN_TASK_CAPABILITIES,
            Operation::LanguageStart { .. } => LANGUAGE_SESSION_CAPABILITIES,
            Operation::LanguageStartJava { .. }
            | Operation::LanguageRefreshJavaDiagnostics { .. } => {
                JAVA_LANGUAGE_SESSION_CAPABILITIES
            }
            _ if is_language_session_operation(op) => match &self.backend {
                Backend::Process(process) if process.java_language_session => {
                    JAVA_LANGUAGE_SESSION_CAPABILITIES
                }
                _ if !supports_capability(agent.as_ref(), "language_start")
                    && supports_capability(agent.as_ref(), "language_start_java") =>
                {
                    // Keep idempotent Stop and ordinary no-session responses
                    // available on Java-only peers after active mode is cleared.
                    JAVA_LANGUAGE_SESSION_CAPABILITIES
                }
                _ => LANGUAGE_SESSION_CAPABILITIES,
            },
            _ => &[],
        };
        for capability in lifecycle {
            require(capability)?;
        }
        Ok(())
    }
    pub fn is_connected(&self) -> bool {
        match &self.backend {
            #[cfg(not(windows))]
            Backend::Local(_) => true,
            Backend::Process(p) => p.connected,
        }
    }
    /// Close this connection and, for a process transport, wait at most `timeout`
    /// for its owned-child reaper to confirm an observed or waited child exit.
    /// Wait/kill failures remain errors. Ordinary Drop stays asynchronous.
    /// This does not join detached pipe-reader threads or attest
    /// to graceful language-server shutdown; inspect LanguageStop's payload.
    pub fn close_and_wait(mut self, timeout: Duration) -> Result<(), String> {
        match &mut self.backend {
            #[cfg(not(windows))]
            Backend::Local(_) => Ok(()),
            Backend::Process(process) => process.close_and_wait(timeout),
        }
    }
}

// Derive exactly one candidate. No PATH, cwd, environment override, build-tree
// search or fallback may move Windows Local into a different spawning host.
#[cfg(any(windows, test))]
fn bundled_windows_agent_path(executable: &Path) -> Result<PathBuf, String> {
    if !executable.is_absolute() {
        return Err("bundled_agent_missing: the current executable path must be absolute".into());
    }
    let directory = executable.parent().ok_or_else(|| {
        "bundled_agent_missing: the current executable has no bundle directory".to_owned()
    })?;
    Ok(directory.join("cedar-agent.exe"))
}

fn validate_handshake(handshake: &Payload) -> Result<(), String> {
    let Payload::Hello {
        protocol, agent, ..
    } = handshake
    else {
        return Err("protocol_error: expected hello".into());
    };
    if *protocol != PROTOCOL_VERSION {
        return Err(format!(
            "protocol_mismatch: agent uses {protocol}, client needs {PROTOCOL_VERSION}"
        ));
    }
    if let Some(agent) = agent {
        agent.validate().map_err(|e| e.to_string())?;
    }
    for capability in ["list", "read"] {
        if !supports_capability(agent.as_ref(), capability) {
            return Err(format!(
                "unsupported_workspace: agent must support {capability} to open a workspace"
            ));
        }
    }
    Ok(())
}

/// Return explicit noninteractive SSH arguments. Shell quote only the remote POSIX command.
pub fn ssh_arguments(
    host: &str,
    port: u16,
    root: &str,
    agent: &str,
    allow_run: bool,
) -> Result<Vec<String>, String> {
    if host.is_empty()
        || host.starts_with('-')
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.@[]:%".contains(c))
    {
        return Err("invalid_host: use an SSH config alias, hostname, or user@hostname".into());
    }
    if port == 0
        || !root.starts_with('/')
        || agent.is_empty()
        || agent.starts_with('-')
        || root.chars().any(char::is_control)
        || agent.chars().any(char::is_control)
    {
        return Err("invalid_connection: absolute POSIX root, nonempty executable and port 1..65535 required; control characters forbidden".into());
    }
    let mut remote = format!("exec {} --root {}", posix_quote(agent), posix_quote(root));
    if allow_run {
        remote.push_str(" --allow-run");
    }
    let mut args = vec![
        "-T",
        "-o",
        // These config aliases were introduced in OpenSSH 8.7. Older clients
        // cannot inherit them either; ignore only these absent aliases, never
        // strict-host-key or other security controls.
        "IgnoreUnknown=StdinNull,SessionType,ForkAfterAuthentication",
        "-o",
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "NoHostAuthenticationForLocalhost=no",
        "-o",
        "UpdateHostKeys=no",
        "-o",
        "CheckHostIP=no",
        "-o",
        "AddKeysToAgent=no",
        "-o",
        "GSSAPIDelegateCredentials=no",
        "-o",
        "Tunnel=no",
        "-o",
        "ForwardAgent=no",
        "-o",
        "ForwardX11=no",
        "-o",
        "ClearAllForwardings=yes",
        "-o",
        "PermitLocalCommand=no",
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
        "-o",
        "RemoteCommand=none",
        "-o",
        "StdinNull=no",
        "-o",
        "SessionType=default",
        "-o",
        "ForkAfterAuthentication=no",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=2",
        "-p",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    args.extend([port.to_string(), host.into(), remote]);
    Ok(args)
}
fn posix_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

const CLOSE_GRACE: Duration = Duration::from_secs(2);
const REAP_INTERVAL: Duration = Duration::from_millis(10);
const CANCELLATION_INTERVAL: Duration = Duration::from_millis(50);
const STDERR_TAIL_BYTES: usize = 4096;
// Java can spend 60 seconds in an agent request and a further 10 seconds in
// bounded shutdown. Leave transport headroom without extending task/file limits.
const JAVA_LANGUAGE_REQUEST_TIMEOUT: Duration = Duration::from_secs(75);

fn is_language_session_operation(op: &Operation) -> bool {
    matches!(
        op,
        Operation::LanguageOpen { .. }
            | Operation::LanguageChange { .. }
            | Operation::LanguageClose { .. }
            | Operation::LanguageQuery { .. }
            | Operation::LanguageFormat { .. }
            | Operation::LanguageRefreshJavaDiagnostics { .. }
            | Operation::LanguageReferences { .. }
            | Operation::LanguageDocumentSymbols { .. }
            | Operation::LanguageResolveUri { .. }
            | Operation::LanguageResolveCompletion { .. }
            | Operation::LanguageEvents
            | Operation::LanguageStop
    )
}

struct ProcessClient {
    cancellation: Option<ConnectionCancellation>,
    requests: Option<mpsc::SyncSender<Request>>,
    responses: Option<mpsc::Receiver<Result<Response, String>>>,
    shutdown: Option<mpsc::Sender<()>>,
    reaped: mpsc::Receiver<ReapResult>,
    stderr: Arc<Mutex<Vec<u8>>>,
    next_id: u64,
    connected: bool,
    java_language_session: bool,
}
impl ProcessClient {
    fn spawn(cmd: Command, cancellation: Option<ConnectionCancellation>) -> Result<Self, String> {
        Self::spawn_with_grace_and_cancellation(cmd, CLOSE_GRACE, cancellation)
    }
    #[cfg(test)]
    fn spawn_with_grace(cmd: Command, grace: Duration) -> Result<Self, String> {
        Self::spawn_with_grace_and_cancellation(cmd, grace, None)
    }
    fn spawn_with_grace_and_cancellation(
        cmd: Command,
        grace: Duration,
        cancellation: Option<ConnectionCancellation>,
    ) -> Result<Self, String> {
        Self::spawn_with_grace_and_error(cmd, grace, cancellation, |e| {
            format!("spawn_failed: {e}. Install OpenSSH and deploy cedar-agent first.")
        })
    }
    #[cfg(windows)]
    fn spawn_bundled_agent(
        cmd: Command,
        cancellation: Option<ConnectionCancellation>,
    ) -> Result<Self, String> {
        Self::spawn_with_grace_and_error(cmd, CLOSE_GRACE, cancellation, |e| e.to_string()).map_err(
            |e| {
                if e.starts_with("transport_cancelled:") {
                    return e;
                }
                format!("bundled_agent_start_failed: could not start bundled cedar-agent.exe: {e}")
            },
        )
    }
    fn spawn_with_grace_and_error(
        mut cmd: Command,
        grace: Duration,
        cancellation: Option<ConnectionCancellation>,
        spawn_error: impl FnOnce(io::Error) -> String,
    ) -> Result<Self, String> {
        if cancellation_requested(&cancellation) {
            return Err(cancellation_error());
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(spawn_error)?;
        let mut stdin = child.stdin.take().ok_or("missing stdin")?;
        let stdout = child.stdout.take().ok_or("missing stdout")?;
        let mut err = child.stderr.take().ok_or("missing stderr")?;
        // Keep a single outstanding request. The public API is sequential, and
        // a stopped writer must never accumulate work or block the caller.
        let (request_tx, request_rx) = mpsc::sync_channel::<Request>(1);
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let (reaped_tx, reaped_rx) = mpsc::channel();
        // Create the owner now, not from Drop. Neither normal close nor a
        // transport failure waits for process exit on the caller/UI thread.
        let owned = OwnedProcess {
            child,
            completion: None,
        };
        thread::Builder::new()
            .name("cedar-transport-reaper".into())
            .spawn(move || {
                let result = reap_after_close(owned, shutdown_rx, grace);
                let _ = reaped_tx.send(result);
            })
            .map_err(|e| format!("spawn_failed: transport reaper: {e}"))?;
        let (response_tx, response_rx) = mpsc::sync_channel(1);
        let writer_errors = response_tx.clone();
        thread::spawn(move || {
            while let Ok(request) = request_rx.recv() {
                if let Err(e) = write_frame(&mut stdin, &request) {
                    let _ = writer_errors.send(Err(format!("transport_write: {e}")));
                    break;
                }
            }
        });
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_frame::<_, Response>(&mut reader) {
                    Ok(Some(response)) => {
                        if response_tx.send(Ok(response)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {
                        let _ = response_tx.send(Err("transport_eof: agent disconnected".into()));
                        break;
                    }
                    Err(e) => {
                        let _ = response_tx.send(Err(format!("transport_read: {e}")));
                        break;
                    }
                }
            }
        });
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let capture = stderr.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 1024];
            loop {
                match err.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut tail = capture.lock().unwrap_or_else(|p| p.into_inner());
                        tail.extend_from_slice(&buf[..n]);
                        let excess = tail.len().saturating_sub(STDERR_TAIL_BYTES);
                        tail.drain(..excess);
                    }
                }
            }
        });
        Ok(Self {
            cancellation,
            requests: Some(request_tx),
            responses: Some(response_rx),
            shutdown: Some(shutdown_tx),
            reaped: reaped_rx,
            stderr,
            next_id: 0,
            connected: true,
            java_language_session: false,
        })
    }
    fn fail(&mut self, message: String) -> String {
        self.close();
        let message = format!("{message}; outcome may be unknown, reload before retrying a write; commands are never automatically replayed");
        let detail = self.stderr.lock().unwrap_or_else(|p| p.into_inner());
        if detail.is_empty() {
            message
        } else {
            format!("{message}\n{}", String::from_utf8_lossy(&detail).trim())
        }
    }
    fn request(&mut self, op: Operation) -> Result<Payload, String> {
        let timeout = self.request_timeout(&op);
        self.request_with_timeout(op, timeout)
    }
    fn request_timeout(&self, op: &Operation) -> Duration {
        match op {
            Operation::LanguageStartJava { .. } => JAVA_LANGUAGE_REQUEST_TIMEOUT,
            _ if self.java_language_session && is_language_session_operation(op) => {
                JAVA_LANGUAGE_REQUEST_TIMEOUT
            }
            Operation::LanguageStart { .. } => Duration::from_secs(75),
            Operation::Run { timeout_secs, .. } => {
                Duration::from_secs((*timeout_secs).clamp(1, 300) + 10)
            }
            _ => Duration::from_secs(30),
        }
    }
    // Kept private: fault tests use short deadlines without changing production
    // operation limits or exposing a weaker connection mode to the UI.
    fn request_with_timeout(
        &mut self,
        op: Operation,
        timeout: Duration,
    ) -> Result<Payload, String> {
        if cancellation_requested(&self.cancellation) {
            return Err(self.fail(cancellation_error()));
        }
        if !self.connected {
            return Err(
                "disconnected: reconnect before retrying; unsaved buffers remain local".into(),
            );
        }
        let starts_java = matches!(op, Operation::LanguageStartJava { .. });
        let starts_generic = matches!(op, Operation::LanguageStart { .. });
        let stops_language = matches!(op, Operation::LanguageStop);
        let interruptible = self.cancellation.is_some()
            && matches!(
                op,
                Operation::Hello
                    | Operation::List { .. }
                    | Operation::Read { .. }
                    | Operation::Search { .. }
            );
        self.next_id = self.next_id.checked_add(1).ok_or("request id exhausted")?;
        let id = self.next_id;
        if self
            .requests
            .as_ref()
            .ok_or("disconnected")?
            .try_send(Request { id, op })
            .is_err()
        {
            return Err(self.fail("transport_write: writer stopped or request queue full".into()));
        }
        // The deadline belongs to the request, never to a polling slice. Only
        // the four read-only operations above observe cancellation in flight;
        // once queued, every other operation keeps its authoritative reply.
        let deadline = Instant::now() + timeout;
        let response = loop {
            if interruptible && cancellation_requested(&self.cancellation) {
                return Err(self.fail(cancellation_error()));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let wait = if interruptible {
                remaining.min(CANCELLATION_INTERVAL)
            } else {
                remaining
            };
            let received = self
                .responses
                .as_ref()
                .ok_or("disconnected")?
                .recv_timeout(wait);
            // Cancellation wins if it was observed before accepting a queued
            // read-only response, including a response arriving in this slice.
            if interruptible && cancellation_requested(&self.cancellation) {
                return Err(self.fail(cancellation_error()));
            }
            match received {
                Ok(Ok(response)) => break response,
                Ok(Err(error)) => return Err(self.fail(error)),
                Err(mpsc::RecvTimeoutError::Timeout)
                    if interruptible && Instant::now() < deadline => {}
                Err(error) => return Err(self.fail(format!("transport_timeout: {error}"))),
            }
        };
        if response.id != id {
            return Err(self.fail(format!(
                "protocol_error: response id {} expected {id}",
                response.id
            )));
        }
        // Failed startup never replaces a running session. Query errors may be
        // recoverable, so only an authoritative absent session clears the mode.
        // Stop consumes the agent session even if bounded cleanup reports error.
        match &response.result {
            _ if stops_language => self.java_language_session = false,
            Ok(Payload::Language { value })
                if (starts_java || starts_generic)
                    && value.get("started").and_then(|v| v.as_bool()) == Some(true) =>
            {
                self.java_language_session = starts_java;
            }
            Err(error) if error.code == "language_not_running" => {
                self.java_language_session = false;
            }
            _ => {}
        }
        response.result.map_err(|e| e.to_string())
    }
    fn close(&mut self) {
        self.connected = false;
        self.java_language_session = false;
        // Dropping the only request sender lets the writer close child stdin.
        // Drop the receiver too, releasing readers blocked on a full queue.
        self.requests.take();
        self.responses.take();
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
    fn close_and_wait(&mut self, timeout: Duration) -> Result<(), String> {
        self.close();
        self.reaped
            .recv_timeout(timeout)
            .map_err(|error| {
                format!("transport_close: owned-child cleanup did not complete: {error}")
            })?
            .map_err(|error| {
                format!(
                    "transport_cleanup_unverified: owned-child {} failed",
                    error.operation()
                )
            })
    }
}
impl Drop for ProcessClient {
    fn drop(&mut self) {
        self.close();
    }
}

// Drop also covers failure to create the reaper thread during connection setup.
// This owns only the direct child, never arbitrary remote or descendant PIDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReapError {
    TryWait,
    Kill,
    Wait,
}
impl ReapError {
    fn operation(self) -> &'static str {
        match self {
            Self::TryWait => "try_wait",
            Self::Kill => "kill",
            Self::Wait => "wait",
        }
    }
}
type ReapResult = Result<(), ReapError>;

struct OwnedProcess {
    child: Child,
    // Some means wait ownership has ended, either with verified reaping or an
    // error. Preserve the distinction and never signal a disowned PID again.
    completion: Option<ReapResult>,
}
impl OwnedProcess {
    // Require exclusive wait ownership, as the task supervisor does. A failed
    // wait may mean another reaper consumed this PID; never signal it afterward.
    fn poll_exit(&mut self) -> Option<ReapResult> {
        if self.completion.is_some() {
            return self.completion;
        }
        loop {
            match self.child.try_wait() {
                Ok(None) => return None,
                Ok(Some(_)) => {
                    self.completion = Some(Ok(()));
                    return self.completion;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    self.completion = Some(Err(ReapError::TryWait));
                    return self.completion;
                }
            }
        }
    }
    fn terminate_and_reap(&mut self) -> ReapResult {
        if let Some(result) = self.poll_exit() {
            return result;
        }
        let killed = self.child.kill().map_err(|_| ReapError::Kill);
        let waited = loop {
            match self.child.wait() {
                Ok(_) => break Ok(()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break Err(ReapError::Wait),
            }
        };
        // Even if the later wait observes exit, a failed termination attempt is
        // not a successful cleanup result. A failed wait disowns the process.
        let result = waited.and(killed);
        self.completion = Some(result);
        result
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        let _ = self.terminate_and_reap();
    }
}
fn reap_after_close(
    mut owned: OwnedProcess,
    shutdown: mpsc::Receiver<()>,
    grace: Duration,
) -> ReapResult {
    loop {
        if let Some(result) = owned.poll_exit() {
            return result;
        }
        match shutdown.recv_timeout(REAP_INTERVAL) {
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            _ => break,
        }
    }
    // A normal agent observes EOF and drops its Workspace, including task and
    // language-server owners. SSH gets the same opportunity to forward EOF.
    // After the bound, kill/reap only our direct child. This cannot prove remote
    // cleanup after network loss, SIGKILL, or an escaped descendant.
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline {
        if let Some(result) = owned.poll_exit() {
            return result;
        }
        thread::sleep(REAP_INTERVAL);
    }
    owned.terminate_and_reap()
}

#[cfg(test)]
mod transport_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_bundle_resolver_uses_only_the_exact_executable_sibling() {
        // Synthetic paths make this independent of Cargo's deps directory and
        // of whether an agent happens to be installed anywhere on this host.
        let base = std::env::temp_dir().join("cedar-synthetic-install");
        for directory in [base.join("bin"), base.join("spaces and Unicode λ")] {
            for executable in ["cedar.exe", "renamed-client.exe", "fixture.exe"] {
                assert_eq!(
                    bundled_windows_agent_path(&directory.join(executable)).unwrap(),
                    directory.join("cedar-agent.exe")
                );
            }
        }
        // A binary under deps must never reach up to a parent build directory.
        assert_eq!(
            bundled_windows_agent_path(&base.join("deps/cedar-test.exe")).unwrap(),
            base.join("deps/cedar-agent.exe")
        );
    }

    #[test]
    fn windows_bundle_resolver_rejects_relative_or_parentless_executable_paths() {
        for executable in [
            Path::new(""),
            Path::new("cedar.exe"),
            Path::new("bin/cedar.exe"),
        ] {
            assert!(bundled_windows_agent_path(executable)
                .unwrap_err()
                .starts_with("bundled_agent_missing:"));
        }
        let root = std::env::temp_dir().ancestors().last().unwrap().to_owned();
        assert!(bundled_windows_agent_path(&root)
            .unwrap_err()
            .starts_with("bundled_agent_missing:"));
    }

    #[test]
    fn ssh_is_strict_and_no_forwarding() {
        let args = ssh_arguments("user@host", 22, "/work", "cedar-agent", false).unwrap();
        for expected in [
            "BatchMode=yes",
            "StrictHostKeyChecking=yes",
            "NoHostAuthenticationForLocalhost=no",
            "ForwardAgent=no",
            "ForwardX11=no",
            "ClearAllForwardings=yes",
            "UpdateHostKeys=no",
            "CheckHostIP=no",
            "AddKeysToAgent=no",
            "GSSAPIDelegateCredentials=no",
            "Tunnel=no",
            "PermitLocalCommand=no",
            "ControlMaster=no",
            "ControlPath=none",
            "RemoteCommand=none",
            "StdinNull=no",
            "SessionType=default",
            "ForkAfterAuthentication=no",
            "ConnectTimeout=10",
            "ServerAliveInterval=15",
            "ServerAliveCountMax=2",
        ] {
            assert!(args.iter().any(|x| x == expected));
        }
    }
    #[test]
    fn remote_command_quotes_metacharacters() {
        let args =
            ssh_arguments("host", 22, "/tmp/a'b; $(echo pwned)", "/opt/my agent", true).unwrap();
        assert_eq!(
            args.last().unwrap(),
            "exec '/opt/my agent' --root '/tmp/a'\\''b; $(echo pwned)' --allow-run"
        );
    }
    #[test]
    fn rejects_option_injection() {
        for host in ["-oProxyCommand=x", "host\nother", "host;touch x", ""] {
            assert!(ssh_arguments(host, 22, "/work", "cedar-agent", false).is_err());
        }
    }
    #[test]
    fn rejects_control_path_and_zero_port() {
        assert!(ssh_arguments("host", 0, "/work", "cedar-agent", false).is_err());
        assert!(ssh_arguments("host", 22, "/work\n", "cedar-agent", false).is_err());
    }
    #[cfg(not(windows))]
    #[test]
    fn local_connection_retains_validated_metadata_without_granting_execution_trust() {
        let root = tempfile::tempdir().unwrap();
        let mut client = Client::connect(ConnectionSpec::Local {
            root: root.path().into(),
            allow_run: false,
        })
        .unwrap();
        let Payload::Hello {
            protocol,
            root: reported_root,
            agent: Some(agent),
        } = client.handshake()
        else {
            panic!("local workspace must advertise metadata");
        };
        assert_eq!(*protocol, PROTOCOL_VERSION);
        assert_eq!(
            *reported_root,
            root.path().canonicalize().unwrap().to_string_lossy()
        );
        agent.validate().unwrap();
        assert_eq!(agent.os, std::env::consts::OS);
        assert_eq!(agent.arch, std::env::consts::ARCH);
        assert!(agent.supports("list"));
        assert!(agent.supports("read"));
        let supports_run = agent.supports("run");
        let snapshot = serde_json::to_value(client.handshake()).unwrap();
        assert_eq!(
            serde_json::to_value(client.request(Operation::Hello).unwrap()).unwrap(),
            snapshot
        );
        let error = client
            .request(Operation::Run {
                program: "never-executed".into(),
                args: vec![],
                timeout_secs: 1,
            })
            .unwrap_err();
        assert!(
            error.starts_with(if supports_run {
                "run_disabled:"
            } else {
                "unsupported_operation:"
            }),
            "{error}"
        );
        assert!(client.is_connected());
        assert_eq!(serde_json::to_value(client.handshake()).unwrap(), snapshot);
    }
    #[cfg(not(windows))]
    #[test]
    fn local_errors_dont_disconnect() {
        let root = tempfile::tempdir().unwrap();
        let mut client = Client::connect(ConnectionSpec::Local {
            root: root.path().into(),
            allow_run: false,
        })
        .unwrap();
        assert!(client
            .request(Operation::Read {
                path: "missing".into()
            })
            .is_err());
        assert!(client.is_connected());
    }
}
