//! Transport-neutral workspace client. SSH uses the user's existing OpenSSH configuration.
//! No passwords, host-key acceptance, key generation, port listeners, or telemetry.
use cedar_protocol::{
    read_frame, write_frame, Operation, Payload, Request, Response, PROTOCOL_VERSION,
};
use cedar_workspace::Workspace;
use std::{
    io::{BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
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
        let mut process = ProcessClient::spawn(cmd)?;
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
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "UpdateHostKeys=no",
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

struct ProcessClient {
    child: Child,
    requests: Option<mpsc::Sender<Request>>,
    responses: mpsc::Receiver<Result<Response, String>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    next_id: u64,
    connected: bool,
}
impl ProcessClient {
    fn spawn(mut cmd: Command) -> Result<Self, String> {
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
        let (request_tx, request_rx) = mpsc::channel::<Request>();
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
                        let excess = tail.len().saturating_sub(4096);
                        tail.drain(..excess);
                    }
                }
            }
        });
        Ok(Self {
            child,
            requests: Some(request_tx),
            responses: response_rx,
            stderr,
            next_id: 0,
            connected: true,
        })
    }
    fn fail(&mut self, message: String) -> String {
        self.connected = false;
        self.requests.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        let detail = self.stderr.lock().unwrap_or_else(|p| p.into_inner());
        if detail.is_empty() {
            message
        } else {
            format!("{message}\n{}", String::from_utf8_lossy(&detail).trim())
        }
    }
    fn request(&mut self, op: Operation) -> Result<Payload, String> {
        if !self.connected {
            return Err(
                "disconnected: reconnect before retrying; unsaved buffers remain local".into(),
            );
        }
        let timeout = match &op {
            Operation::LanguageStart { .. } => Duration::from_secs(75),
            Operation::Run { timeout_secs, .. } => {
                Duration::from_secs((*timeout_secs).clamp(1, 300) + 10)
            }
            _ => Duration::from_secs(30),
        };
        self.next_id = self.next_id.checked_add(1).ok_or("request id exhausted")?;
        let id = self.next_id;
        if self
            .requests
            .as_ref()
            .ok_or("disconnected")?
            .send(Request { id, op })
            .is_err()
        {
            return Err(self.fail("transport_write: writer stopped".into()));
        }
        let response=match self.responses.recv_timeout(timeout) {
            Ok(Ok(response))=>response,
            Ok(Err(error))=>return Err(self.fail(error)),
            Err(error)=>return Err(self.fail(format!("transport_timeout: {error}; outcome may be unknown, reload before retrying a write"))),
        };
        if response.id != id {
            return Err(self.fail(format!(
                "protocol_error: response id {} expected {id}",
                response.id
            )));
        }
        response.result.map_err(|e| e.to_string())
    }
}
impl Drop for ProcessClient {
    fn drop(&mut self) {
        self.requests.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ssh_is_strict_and_no_forwarding() {
        let args = ssh_arguments("user@host", 22, "/work", "cedar-agent", false).unwrap();
        for expected in [
            "BatchMode=yes",
            "StrictHostKeyChecking=yes",
            "ForwardAgent=no",
            "ForwardX11=no",
            "ClearAllForwardings=yes",
            "UpdateHostKeys=no",
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
