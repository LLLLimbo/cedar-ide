//! Bounded real-pipe driver and observation-only fixture handles. Cleanup may
//! terminate only the Child created here, never a process found by numeric PID.
use cedar_protocol::{
    read_frame, write_frame, AgentInfo, Operation, Payload, RemoteError, Request, Response,
    PROTOCOL_VERSION,
};
use cedar_tasks::TaskSnapshot;
use std::{
    fs,
    io::{self, BufReader, Read, Write},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        GetExitCodeProcess, GetProcessHandleCount, OpenProcess, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    },
};

const WAIT: Duration = Duration::from_secs(3);
const CAP_EXIT: u32 = 124;

pub struct Watchdog {
    stop: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}
impl Watchdog {
    pub fn start() -> Self {
        Self::start_with_timeout(Duration::from_secs(45))
    }
    pub fn start_with_timeout(timeout: Duration) -> Self {
        let (stop, receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            if receiver.recv_timeout(timeout).is_err() {
                eprintln!("Windows agent test exceeded its driver watchdog deadline");
                std::process::exit(126);
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Watchdog {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn binary(variable: &str) -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os(variable)
            .unwrap_or_else(|| panic!("set {variable} to the exact prebuilt executable")),
    );
    assert!(
        path.is_absolute(),
        "{variable} must be absolute: {}",
        path.display()
    );
    assert!(path.is_file(), "{variable} is missing: {}", path.display());
    assert!(path.to_str().is_some(), "{variable} must be UTF-8");
    assert_eq!(path.extension().and_then(|s| s.to_str()), Some("exe"));
    path
}
pub fn agent_binary() -> PathBuf {
    binary("CEDAR_AGENT_BIN")
}
pub fn fixture() -> PathBuf {
    binary("CEDAR_WINPROCESS_FIXTURE_BIN")
}
pub fn text_path(path: &Path) -> String {
    path.to_str().expect("UTF-8 test path").to_owned()
}
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn start(args: &[&str], seconds: u64) -> Operation {
    Operation::RunStart {
        program: text_path(&fixture()),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        timeout_secs: seconds,
    }
}
pub fn task(payload: Payload) -> TaskSnapshot {
    let Payload::RunTask { snapshot } = payload else {
        panic!("expected RunTask, got {payload:?}")
    };
    serde_json::from_value(snapshot).expect("typed TaskSnapshot")
}
pub fn metadata(payload: Payload) -> AgentInfo {
    let Payload::Hello {
        protocol,
        root,
        agent: Some(info),
    } = payload
    else {
        panic!("expected current agent metadata, got {payload:?}")
    };
    assert_eq!(protocol, PROTOCOL_VERSION);
    assert!(Path::new(&root).is_absolute());
    info.validate().unwrap();
    info
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        // Guard cleanup is deliberately separate from all cleanup assertions.
        // An error means wait ownership is uncertain; do not signal after it.
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let deadline = Instant::now() + WAIT;
            while let Ok(None) = self.0.try_wait() {
                if Instant::now() >= deadline {
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

struct Drain {
    bytes: Arc<Mutex<Vec<u8>>>,
    thread: Option<JoinHandle<()>>,
}
impl Drain {
    fn new(mut pipe: impl Read + Send + 'static) -> Self {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&bytes);
        let thread = thread::spawn(move || {
            let mut buffer = [0; 4096];
            loop {
                match pipe.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        let mut bytes = captured.lock().unwrap();
                        let keep = count.min((32usize * 1024).saturating_sub(bytes.len()));
                        bytes.extend_from_slice(&buffer[..keep]);
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => panic!("pipe drain: {e}"),
                }
            }
        });
        Self {
            bytes,
            thread: Some(thread),
        }
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes.lock().unwrap()).into_owned()
    }
    fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }
    fn join(&mut self) {
        assert!(self.finished(), "join requires already-observed pipe EOF");
        if let Some(thread) = self.thread.take() {
            thread.join().expect("pipe drain thread");
        }
    }
}

pub struct RawAgent {
    child: OwnedChild,
    input: Option<ChildStdin>,
    output: Option<BufReader<ChildStdout>>,
    stderr: Drain,
    next_id: u64,
    request_worker: Option<JoinHandle<()>>,
}
impl RawAgent {
    pub fn new(root: &Path, allow_run: bool) -> Self {
        Self::with_cwd(root, allow_run, None)
    }
    pub fn with_cwd(root: &Path, allow_run: bool, cwd: Option<&Path>) -> Self {
        let mut command = Command::new(agent_binary());
        command.arg("--root").arg(root);
        if allow_run {
            command.arg("--allow-run");
        }
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        Self::from_command(command)
    }
    pub fn from_command(mut command: Command) -> Self {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("spawn exact agent binary");
        let input = child.stdin.take();
        let output = child.stdout.take().map(BufReader::new);
        let stderr = Drain::new(child.stderr.take().unwrap());
        Self {
            child: OwnedChild(child),
            input,
            output,
            stderr,
            next_id: 0,
            request_worker: None,
        }
    }
    pub fn diagnostics(&self) -> String {
        self.stderr.text()
    }
    pub fn handle_count(&self) -> u32 {
        let mut count = 0;
        // SAFETY: This is the exact live Child handle returned by spawn, with
        // process query access, and count points to a writable DWORD. No PID
        // lookup, global enumeration or additional termination authority.
        // https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesshandlecount
        assert_ne!(
            unsafe { GetProcessHandleCount(self.child.0.as_raw_handle(), &mut count) },
            0
        );
        count
    }
    pub fn request(&mut self, op: Operation) -> Result<Payload, RemoteError> {
        self.request_with_timeout(op, WAIT)
    }
    pub fn request_with_timeout(
        &mut self,
        op: Operation,
        timeout: Duration,
    ) -> Result<Payload, RemoteError> {
        self.next_id += 1;
        let request = Request {
            id: self.next_id,
            op,
        };
        let mut input = self.input.take().expect("agent stdin is open");
        let mut output = self.output.take().expect("agent stdout is open");
        let (send, receive) = mpsc::channel();
        assert!(
            self.request_worker.is_none(),
            "previous request worker is still owned"
        );
        self.request_worker = Some(thread::spawn(move || {
            // Both write and read run under the caller's deadline. An oversized
            // malformed frame or stopped agent cannot block the test thread.
            let result = write_frame(&mut input, &request)
                .and_then(|_| read_frame::<_, Response>(&mut output));
            let _ = send.send((input, output, result));
        }));
        let (input, output, response) = receive.recv_timeout(timeout).unwrap_or_else(|e| {
            panic!(
                "agent request {} exceeded deadline: {e}; {}",
                self.next_id,
                self.diagnostics()
            )
        });
        self.input = Some(input);
        self.output = Some(output);
        self.request_worker
            .take()
            .unwrap()
            .join()
            .expect("agent request worker");
        let response = response
            .unwrap_or_else(|e| panic!("agent protocol: {e}; {}", self.diagnostics()))
            .expect("agent response EOF");
        assert_eq!(response.id, self.next_id, "response/request ID mismatch");
        response.result
    }
    pub fn ok(&mut self, op: Operation) -> Payload {
        self.request(op).unwrap()
    }
    pub fn start(&mut self, op: Operation) -> u64 {
        task(self.ok(op)).id
    }
    pub fn terminal(&mut self, id: u64, context: &str) -> TaskSnapshot {
        let deadline = Instant::now() + WAIT;
        loop {
            let snapshot = task(self.ok(Operation::RunPoll { task_id: id }));
            assert_eq!(snapshot.id, id);
            assert_ne!(
                snapshot.windows_exit_code,
                Some(CAP_EXIT),
                "{context}: fixture reached safety cap"
            );
            if snapshot.state.is_terminal() {
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "{context}: no terminal snapshot: {snapshot:?}; {}",
                self.diagnostics()
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
    pub fn stable(&mut self, terminal: &TaskSnapshot) {
        assert!(terminal.state.is_terminal());
        for op in [
            Operation::RunPoll {
                task_id: terminal.id,
            },
            Operation::RunCancel {
                task_id: terminal.id,
            },
            Operation::RunCancel {
                task_id: terminal.id,
            },
            Operation::RunPoll {
                task_id: terminal.id,
            },
        ] {
            assert_eq!(task(self.ok(op)), *terminal, "terminal snapshot mutated");
        }
    }
    pub fn wait_exit(&mut self, context: &str) -> ExitStatus {
        self.wait_exit_with_timeout(context, WAIT)
    }
    pub fn wait_exit_with_timeout(&mut self, context: &str, timeout: Duration) -> ExitStatus {
        let deadline = Instant::now() + timeout;
        loop {
            let exit = self.child.0.try_wait().expect("wait owned agent");
            if let Some(status) = exit {
                if self.stderr.finished()
                    && self
                        .request_worker
                        .as_ref()
                        .is_none_or(|worker| worker.is_finished())
                {
                    self.stderr.join();
                    if let Some(worker) = self.request_worker.take() {
                        worker
                            .join()
                            .expect("agent request worker after process exit");
                    }
                    return status;
                }
            }
            assert!(
                Instant::now() < deadline,
                "{context}: agent did not exit/drain, status={exit:?}, stderr={}",
                self.diagnostics()
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
    pub fn close_cleanly(&mut self) {
        self.close_cleanly_with_timeout(WAIT);
    }
    pub fn close_cleanly_with_timeout(&mut self, timeout: Duration) {
        self.input.take();
        assert!(
            self.wait_exit_with_timeout("clean agent stdin EOF", timeout)
                .success(),
            "{}",
            self.diagnostics()
        );
    }
    #[allow(dead_code)] // Used by the app's longer-running real Java acceptance.
    pub fn has_protocol_pipes(&self) -> bool {
        self.input.is_some() && self.output.is_some()
    }
    /// Explicit failure cleanup, separate from the Drop backstop. Only the exact
    /// Child created by this driver can be terminated, never a PID lookup.
    #[allow(dead_code)] // Used by the app's longer-running real Java acceptance.
    pub fn abort_and_wait_with_timeout(&mut self, timeout: Duration) -> ExitStatus {
        self.input.take();
        self.output.take();
        if self
            .child
            .0
            .try_wait()
            .expect("query owned agent")
            .is_none()
        {
            self.child
                .0
                .kill()
                .expect("terminate owned agent after failure");
        }
        self.wait_exit_with_timeout("explicit owned-agent failure cleanup", timeout)
    }
    pub fn inject_failure(&mut self, failure: &str) {
        match failure {
            "eof" => {
                self.input.take();
            }
            "forced-agent-death" => {
                // Only the exact Child we spawned is terminated. There is no
                // test-owned job around it, so this cannot fake task cleanup.
                assert!(self.child.0.try_wait().unwrap().is_none());
                self.child.0.kill().expect("terminate owned agent Child");
            }
            "broken-stdout" => {
                self.output.take();
                self.send_raw(
                    b"{\"id\":999,\"op\":{\"type\":\"hello\"}}\n".to_vec(),
                    false,
                );
                assert!(
                    self.input.is_some(),
                    "stdin stays open to isolate broken stdout"
                );
            }
            "malformed" => self.send_raw(b"not-json\n".to_vec(), false),
            "truncated" => self.send_raw(b"{\"id\":999".to_vec(), true),
            "oversized" => self.send_raw(vec![b'x'; cedar_protocol::MAX_FRAME_BYTES + 1], true),
            _ => panic!("unknown failure {failure}"),
        }
    }
    fn send_raw(&mut self, bytes: Vec<u8>, close: bool) {
        let mut input = self.input.take().unwrap();
        let (send, receive) = mpsc::channel();
        thread::spawn(move || {
            // BrokenPipe is permitted: the agent may reject a large frame
            // before the writer finishes. Exit status and stderr prove failure.
            let _ = input.write_all(&bytes).and_then(|_| input.flush());
            let _ = send.send(if close { None } else { Some(input) });
        });
        self.input = receive
            .recv_timeout(WAIT)
            .expect("raw injection writer deadline");
    }
}
impl Drop for RawAgent {
    fn drop(&mut self) {
        self.input.take();
        self.output.take();
        // OwnedChild runs after this body; no cleanup assertion lives in Drop.
    }
}

struct ObservedProcess {
    handle: OwnedHandle,
    pid: u32,
}
impl ObservedProcess {
    fn from_file(path: &Path) -> Self {
        let pid = fs::read_to_string(path)
            .expect("published fixture PID")
            .parse::<u32>()
            .expect("fixture PID integer");
        assert_ne!(pid, 0);
        // SAFETY: PID is from our ready synthetic child in a private directory.
        // Only QUERY_LIMITED_INFORMATION and SYNCHRONIZE are requested. The
        // handle is not inheritable and grants no termination authority.
        // https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocess
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        assert!(
            !raw.is_null(),
            "observe fixture {pid}: {}",
            io::Error::last_os_error()
        );
        // SAFETY: The successful OpenProcess result is newly owned; close once.
        let observed = Self {
            handle: unsafe { OwnedHandle::from_raw_handle(raw) },
            pid,
        };
        assert!(
            observed.alive(),
            "fixture {pid} was not live when its handle was opened"
        );
        observed
    }
    fn alive(&self) -> bool {
        // SAFETY: Owned observation handle remains open for this zero-time wait.
        // https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => true,
            WAIT_OBJECT_0 => false,
            result => panic!("wait fixture {} failed: {result:#x}", self.pid),
        }
    }
    fn exit_code(&self) -> u32 {
        assert!(!self.alive(), "exit code queried before wait signalled");
        let mut code = 0;
        // SAFETY: Held QUERY_LIMITED_INFORMATION handle and writable DWORD.
        // Wait signalled first, so native exit 259 is not confused with liveness.
        // https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getexitcodeprocess
        assert_ne!(
            unsafe { GetExitCodeProcess(self.handle.as_raw_handle(), &mut code) },
            0
        );
        code
    }
}

pub struct ObservedTree(Vec<ObservedProcess>);
impl ObservedTree {
    pub fn ready(dir: &Path) -> Self {
        let deadline = Instant::now() + WAIT;
        while !dir.join("tree.ready").is_file() {
            assert!(
                Instant::now() < deadline,
                "tree never published readiness: {}",
                dir.display()
            );
            thread::sleep(Duration::from_millis(5));
        }
        Self::observe(dir, &["root.pid", "branch.pid", "leaf.pid"])
    }
    pub fn observe(dir: &Path, names: &[&str]) -> Self {
        let tree = Self(
            names
                .iter()
                .map(|name| ObservedProcess::from_file(&dir.join(name)))
                .collect(),
        );
        assert!(
            tree.all_alive(),
            "all fixture processes must be known live at readiness"
        );
        tree
    }
    pub fn all_alive(&self) -> bool {
        self.0.iter().all(ObservedProcess::alive)
    }
    pub fn assert_already_stopped(&self, context: &str) {
        assert!(
            self.0.iter().all(|process| !process.alive()),
            "{context}: terminal snapshot was published before the whole tree stopped"
        );
        self.assert_stopped(context);
    }
    pub fn assert_stopped(&self, context: &str) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.0.iter().any(ObservedProcess::alive) {
            assert!(
                Instant::now() < deadline,
                "{context}: observed task processes survived: {:?}",
                self.0
                    .iter()
                    .filter(|p| p.alive())
                    .map(|p| p.pid)
                    .collect::<Vec<_>>()
            );
            thread::sleep(Duration::from_millis(5));
        }
        for process in &self.0 {
            let code = process.exit_code();
            assert_ne!(
                code, CAP_EXIT,
                "{context}: fixture {} exited at its safety cap, not agent cleanup",
                process.pid
            );
            assert_ne!(
                code, 125,
                "{context}: fixture {} failed independently",
                process.pid
            );
        }
    }
}

pub struct ProbeOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}
pub fn run_probe(mut command: Command, context: &str) -> ProbeOutput {
    let mut child = OwnedChild(command.spawn().expect("spawn isolated bundle probe"));
    let mut stdout = Drain::new(child.0.stdout.take().unwrap());
    let mut stderr = Drain::new(child.0.stderr.take().unwrap());
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let status = child.0.try_wait().expect("wait owned bundle probe");
        if let Some(status) = status {
            if stdout.finished() && stderr.finished() {
                stdout.join();
                stderr.join();
                return ProbeOutput {
                    status,
                    stdout: stdout.text(),
                    stderr: stderr.text(),
                };
            }
        }
        assert!(
            Instant::now() < deadline,
            "{context}: bundle probe exceeded deadline: stdout={} stderr={}",
            stdout.text(),
            stderr.text()
        );
        thread::sleep(Duration::from_millis(5));
    }
}
