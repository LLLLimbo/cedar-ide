//! Synthetic, local process transport only. No workspace execution or SSH.
use cedar_client::{Client, ConnectionCancellation};
use cedar_protocol::{
    Operation, Request, JAVA_LANGUAGE_SESSION_CAPABILITIES, LANGUAGE_SESSION_CAPABILITIES,
    RUN_TASK_CAPABILITIES,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub const CALLER_BOUND: Duration = Duration::from_secs(2);
pub const CLEANUP_BOUND: Duration = Duration::from_secs(5);

// A missing cancellation path must fail before the production 30s deadline.
// The guard is joined when disarmed, including before resource measurements.
pub struct Watchdog {
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Watchdog {
    pub fn start(timeout: Duration) -> Self {
        let (stop, cancelled) = mpsc::channel();
        let worker = thread::spawn(move || {
            if matches!(
                cancelled.recv_timeout(timeout),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                eprintln!("connection cancellation test watchdog expired");
                std::process::exit(124);
            }
        });
        Self {
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

pub fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cedar-client-transport-peer"))
}

pub struct Fixture {
    directory: tempfile::TempDir,
}

impl Fixture {
    pub fn new(mode: &str) -> Self {
        let directory = tempfile::Builder::new()
            .prefix("cedar cancellation 雪 ")
            .tempdir()
            .unwrap();
        fs::write(
            directory.path().join(".cedar-transport-fixture"),
            b"cedar-transport-fixture-v1\n",
        )
        .unwrap();
        fs::write(directory.path().join("fixture-mode"), mode).unwrap();
        let mut capabilities = vec![
            "list",
            "read",
            "write",
            "search",
            "git_status",
            "language_query",
        ];
        capabilities.extend(LANGUAGE_SESSION_CAPABILITIES);
        capabilities.extend(JAVA_LANGUAGE_SESSION_CAPABILITIES);
        capabilities.extend(RUN_TASK_CAPABILITIES);
        capabilities.sort_unstable();
        capabilities.dedup();
        fs::write(directory.path().join("hello.json"), serde_json::json!({
            "type": "hello", "protocol": cedar_protocol::PROTOCOL_VERSION, "root": "/fixture",
            "agent": {"schema": cedar_protocol::AGENT_INFO_SCHEMA, "version": "cancellation-fixture",
                "os": "fixture_os", "arch": "fixture_arch", "capabilities": capabilities}
        }).to_string()).unwrap();
        Self { directory }
    }

    pub fn root(&self) -> &Path {
        self.directory.path()
    }

    pub fn connect(&self, cancellation: ConnectionCancellation) -> Client {
        Client::spawn_agent_with_cancellation(&binary(), self.root(), false, cancellation).unwrap()
    }

    pub fn ready(&self, id: u64) {
        wait_for(&self.root().join(format!("ready-{id}")));
        let witnessed: Request =
            serde_json::from_slice(&fs::read(self.root().join(format!("ready-{id}"))).unwrap())
                .unwrap();
        assert_eq!(witnessed.id, id);
        let requests = self.requests();
        assert_eq!(requests.len(), id as usize);
        assert_eq!(requests.last().unwrap().id, id);
    }

    pub fn release(&self, id: u64) {
        fs::write(
            self.root().join(format!("release-{id}")),
            b"controller released",
        )
        .unwrap();
    }

    pub fn requests(&self) -> Vec<Request> {
        fs::read_to_string(self.root().join("requests"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    pub fn assert_closed(&self, count: usize) {
        assert!(
            self.root().join("eof").is_file(),
            "peer did not observe EOF"
        );
        assert!(!self.root().join("safety-expired").exists());
        assert_eq!(
            self.requests().len(),
            count,
            "unexpected replay, Hello, or shutdown probe"
        );
    }
}

pub fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.is_file() {
        assert!(
            Instant::now() < deadline,
            "fixture marker not published: {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(5));
    }
}

pub fn read() -> Operation {
    Operation::Read {
        path: "fixture.txt".into(),
    }
}

pub fn list() -> Operation {
    Operation::List {
        path: String::new(),
    }
}

pub fn assert_cancelled(error: &str) {
    assert!(error.starts_with("transport_cancelled:"), "{error}");
}

pub fn cancelled_read_cycle() {
    let fixture = Fixture::new("stalled_read");
    let cancellation = ConnectionCancellation::new();
    let mut client = fixture.connect(cancellation.clone());
    let root = fixture.root().to_owned();
    let controller = thread::spawn(move || {
        wait_for(&root.join("ready-2"));
        cancellation.cancel();
        Instant::now()
    });
    let result = client.request(read());
    let cancelled_at = controller.join().unwrap();
    assert_cancelled(&result.unwrap_err());
    assert!(cancelled_at.elapsed() < CALLER_BOUND);
    assert_cancelled(&client.request(list()).unwrap_err());
    client.close_and_wait(CLEANUP_BOUND).unwrap();
    fixture.assert_closed(2);
}
