//! Transport-neutral workspace client. SSH uses the user's existing OpenSSH configuration.
//! No passwords, host-key acceptance, key generation, port listeners, or telemetry.
use cedar_protocol::{
    read_frame, write_frame, Operation, Payload, Request, Response, PROTOCOL_VERSION,
};
use cedar_workspace::Workspace;
use std::{
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

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
}
enum Backend {
    Local(Box<Workspace>),
    Process(ProcessClient),
}
impl Client {
    pub fn connect(spec: ConnectionSpec) -> Result<Self, String> {
        match spec {
            ConnectionSpec::Local { root, allow_run } => {
                let mut workspace = Workspace::open(root).map_err(|e| e.to_string())?;
                workspace.set_allow_run(allow_run);
                Ok(Self {
                    backend: Backend::Local(Box::new(workspace)),
                })
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
                Self::from_command(cmd)
            }
        }
    }
    /// Use a separately deployed local agent, also useful for process-isolated integration tests.
    pub fn spawn_agent(agent: &Path, root: &Path, allow_run: bool) -> Result<Self, String> {
        let mut cmd = Command::new(agent);
        cmd.arg("--root").arg(root);
        if allow_run {
            cmd.arg("--allow-run");
        }
        Self::from_command(cmd)
    }
    fn from_command(cmd: Command) -> Result<Self, String> {
        Self::from_process(ProcessClient::spawn(cmd)?)
    }
    fn from_process(mut process: ProcessClient) -> Result<Self, String> {
        match process.request(Operation::Hello)? {
            Payload::Hello { protocol, .. } if protocol == PROTOCOL_VERSION => Ok(Self {
                backend: Backend::Process(process),
            }),
            Payload::Hello { protocol, .. } => Err(format!(
                "protocol_mismatch: agent uses {protocol}, client needs {PROTOCOL_VERSION}"
            )),
            _ => Err("protocol_error: expected hello".into()),
        }
    }
    pub fn request(&mut self, op: Operation) -> Result<Payload, String> {
        match &mut self.backend {
            Backend::Local(ws) => ws.handle(op).map_err(|e| e.to_string()),
            Backend::Process(p) => p.request(op),
        }
    }
    pub fn is_connected(&self) -> bool {
        match &self.backend {
            Backend::Local(_) => true,
            Backend::Process(p) => p.connected,
        }
    }
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
const STDERR_TAIL_BYTES: usize = 4096;

struct ProcessClient {
    requests: Option<mpsc::SyncSender<Request>>,
    responses: Option<mpsc::Receiver<Result<Response, String>>>,
    shutdown: Option<mpsc::Sender<()>>,
    #[cfg(test)]
    reaped: mpsc::Receiver<()>,
    stderr: Arc<Mutex<Vec<u8>>>,
    next_id: u64,
    connected: bool,
}
impl ProcessClient {
    fn spawn(cmd: Command) -> Result<Self, String> {
        Self::spawn_with_grace(cmd, CLOSE_GRACE)
    }
    fn spawn_with_grace(mut cmd: Command, grace: Duration) -> Result<Self, String> {
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                format!("spawn_failed: {e}. Install OpenSSH and deploy cedar-agent first.")
            })?;
        let mut stdin = child.stdin.take().ok_or("missing stdin")?;
        let stdout = child.stdout.take().ok_or("missing stdout")?;
        let mut err = child.stderr.take().ok_or("missing stderr")?;
        // Keep a single outstanding request. The public API is sequential, and
        // a stopped writer must never accumulate work or block the caller.
        let (request_tx, request_rx) = mpsc::sync_channel::<Request>(1);
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        #[cfg(test)]
        let (reaped_tx, reaped_rx) = mpsc::channel();
        // Create the owner now, not from Drop. Neither normal close nor a
        // transport failure waits for process exit on the caller/UI thread.
        let owned = OwnedProcess {
            child,
            wait_owned: true,
        };
        thread::Builder::new()
            .name("cedar-transport-reaper".into())
            .spawn(move || {
                reap_after_close(owned, shutdown_rx, grace);
                #[cfg(test)]
                let _ = reaped_tx.send(());
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
            requests: Some(request_tx),
            responses: Some(response_rx),
            shutdown: Some(shutdown_tx),
            #[cfg(test)]
            reaped: reaped_rx,
            stderr,
            next_id: 0,
            connected: true,
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
        let timeout = match &op {
            Operation::LanguageStart { .. } => Duration::from_secs(75),
            Operation::Run { timeout_secs, .. } => {
                Duration::from_secs((*timeout_secs).clamp(1, 300) + 10)
            }
            _ => Duration::from_secs(30),
        };
        self.request_with_timeout(op, timeout)
    }
    // Kept private: fault tests use short deadlines without changing production
    // operation limits or exposing a weaker connection mode to the UI.
    fn request_with_timeout(
        &mut self,
        op: Operation,
        timeout: Duration,
    ) -> Result<Payload, String> {
        if !self.connected {
            return Err(
                "disconnected: reconnect before retrying; unsaved buffers remain local".into(),
            );
        }
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
        let response = match self
            .responses
            .as_ref()
            .ok_or("disconnected")?
            .recv_timeout(timeout)
        {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => return Err(self.fail(error)),
            Err(error) => return Err(self.fail(format!("transport_timeout: {error}"))),
        };
        if response.id != id {
            return Err(self.fail(format!(
                "protocol_error: response id {} expected {id}",
                response.id
            )));
        }
        response.result.map_err(|e| e.to_string())
    }
    fn close(&mut self) {
        self.connected = false;
        // Dropping the only request sender lets the writer close child stdin.
        // Drop the receiver too, releasing readers blocked on a full queue.
        self.requests.take();
        self.responses.take();
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}
impl Drop for ProcessClient {
    fn drop(&mut self) {
        self.close();
    }
}

// Drop also covers failure to create the reaper thread during connection setup.
// This owns only the direct child, never arbitrary remote or descendant PIDs.
struct OwnedProcess {
    child: Child,
    wait_owned: bool,
}
impl OwnedProcess {
    // Require exclusive wait ownership, as the task supervisor does. A failed
    // wait may mean another reaper consumed this PID; never signal it afterward.
    fn exited_or_unowned(&mut self) -> bool {
        if !self.wait_owned {
            return true;
        }
        loop {
            match self.child.try_wait() {
                Ok(None) => return false,
                Ok(Some(_)) => {
                    self.wait_owned = false;
                    return true;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    self.wait_owned = false;
                    return true;
                }
            }
        }
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        if !self.exited_or_unowned() {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.wait_owned = false;
        }
    }
}
fn reap_after_close(mut owned: OwnedProcess, shutdown: mpsc::Receiver<()>, grace: Duration) {
    loop {
        if owned.exited_or_unowned() {
            return;
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
        if owned.exited_or_unowned() {
            return;
        }
        thread::sleep(REAP_INTERVAL);
    }
}

#[cfg(test)]
mod transport_tests;

#[cfg(test)]
mod tests {
    use super::*;
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
