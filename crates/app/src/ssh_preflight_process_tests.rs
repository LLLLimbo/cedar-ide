//! Pure SSH preflight against one live, ordinary trust-off stdio connection.
//! Only the explicitly selected normal agent and generated workspace are used.
//! SSH rejection claims describe this implementation path, not an OS-wide audit.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

const BUDGET: Duration = Duration::from_secs(60);
const WAIT: Duration = Duration::from_secs(5);
const SOURCE: &str = "generated source 草稿 🐻\nsecond line\n";
const INVALID_FORMS: usize = 14;
type Check<T> = Result<T, &'static str>;

fn require(condition: bool, message: &'static str) -> Check<()> {
    if condition {
        Ok(())
    } else {
        Err(message)
    }
}

fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn file(index: usize) -> String {
    format!("probe-{index:02} 草稿.txt")
}

fn local_form(root: &str) -> ConnectForm {
    ConnectForm {
        ssh: false,
        local_root: root.into(),
        host: String::new(),
        port: "22".into(),
        remote_root: String::new(),
        agent: "cedar-agent".into(),
        allow_run: false,
    }
}

fn invalid_forms() -> Vec<(ConnectForm, &'static str)> {
    let base = ConnectForm {
        ssh: true,
        local_root: String::new(),
        host: "cedar-preflight.invalid".into(),
        port: "22".into(),
        remote_root: "/generated-preflight-workspace".into(),
        agent: "cedar-agent".into(),
        allow_run: false,
    };
    let mut forms = Vec::new();
    for host in ["-option", "user name@host", "host;command", "ho\nst"] {
        let mut form = base.clone();
        form.host = host.into();
        forms.push((form, "invalid_host:"));
    }
    for root in [
        "relative/workspace",
        "C:\\generated\\workspace",
        "/generated/line\nbreak",
        "/generated/nul\0byte",
    ] {
        let mut form = base.clone();
        form.remote_root = root.into();
        forms.push((form, "invalid_connection:"));
    }
    for agent in ["-agent", "cedar\tagent", "cedar\0agent"] {
        let mut form = base.clone();
        form.agent = agent.into();
        forms.push((form, "invalid_connection:"));
    }
    for (port, error) in [
        (
            "0",
            "Enter a host, remote workspace root, agent path, and valid port",
        ),
        ("65536", "Port must be between 1 and 65535"),
        ("not-a-port", "Port must be between 1 and 65535"),
    ] {
        let mut form = base.clone();
        form.port = port.into();
        forms.push((form, error));
    }
    forms
}

struct Harness {
    app: CedarApp,
    root: Option<tempfile::TempDir>,
    started: Instant,
    generation: u64,
    accepted_root: String,
    accepted_key: WorkspaceKey,
    accepted_info: AgentInfo,
    read_responses: usize,
    rejected: usize,
    closed: bool,
}

impl Drop for Harness {
    fn drop(&mut self) {
        // Failure still releases only this test's owned worker. The success
        // path separately requires its consuming close/reaper receipt.
        self.app.worker = None;
        if !self.closed {
            let deadline = Instant::now() + WAIT;
            while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
                match self.app.result_rx.recv_timeout(remaining) {
                    Ok(WorkerEvent::Closed { generation, .. }) if generation == self.generation => {
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        }
    }
}

impl Harness {
    fn new() -> Check<Self> {
        let started = Instant::now();
        let agent = PathBuf::from(
            std::env::var_os("CEDAR_SSH_PREFLIGHT_AGENT_BIN")
                .ok_or("explicit normal-agent selection missing")?,
        );
        require(
            agent.is_absolute()
                && agent.is_file()
                && agent.file_name().is_some_and(|name| {
                    name == if cfg!(windows) {
                        "cedar-agent.exe"
                    } else {
                        "cedar-agent"
                    }
                }),
            "normal-agent selection invalid",
        )?;
        let root = tempfile::Builder::new()
            .prefix("cedar-preflight 草稿-")
            .tempdir()
            .map_err(|_| "owned fixture creation failed")?;
        for index in 0..=INVALID_FORMS {
            fs::write(root.path().join(file(index)), SOURCE)
                .map_err(|_| "generated source creation failed")?;
        }
        let root_text = root
            .path()
            .to_str()
            .ok_or("fixture path is not Unicode")?
            .to_owned();
        let expected_root = fs::canonicalize(root.path())
            .map_err(|_| "generated root canonicalization failed")?
            .to_string_lossy()
            .into_owned();
        let connection = local_form(&root_text);
        let accepted_key = connection.key();
        let mut app = CedarApp::empty();
        app.open_form = false;
        app.generation += 1;
        app.state = ConnectionState::Connecting;
        app.connecting_form = Some(connection);
        let generation = app.generation;
        // Never pass an SSH ConnectionSpec to the test's process startup.
        app.worker = Some(Worker::spawn_agent(
            agent,
            root.path().to_owned(),
            generation,
            app.result_tx.clone(),
            app.editor_ctx.clone(),
        ));
        // Establish failure cleanup before awaiting even the first response.
        let mut h = Self {
            app,
            root: Some(root),
            started,
            generation,
            accepted_root: String::new(),
            accepted_key,
            accepted_info: AgentInfo {
                schema: 0,
                version: String::new(),
                os: String::new(),
                arch: String::new(),
                capabilities: Vec::new(),
                capability_groups: Vec::new(),
            },
            read_responses: 0,
            rejected: 0,
            closed: false,
        };
        let hello = h.response(0)?;
        require(
            matches!(&hello.result, Ok(Payload::Hello { protocol, root, agent: Some(_) })
                if *protocol == cedar_protocol::PROTOCOL_VERSION && root == &expected_root),
            "normal-agent Hello metadata absent",
        )?;
        h.app.apply_event(hello); // Ordinary Hello schedules the initial List.
        let list = h.response(1)?;
        require(
            matches!(&list.result, Ok(Payload::Entries { .. })),
            "normal-agent initial List missing",
        )?;
        h.app.apply_event(list);
        h.accepted_root = h.app.root.clone();
        h.accepted_info = h
            .app
            .agent_info
            .clone()
            .ok_or("accepted agent metadata absent")?;
        require(
            !h.accepted_root.is_empty()
                && ["list", "read"].iter().all(|capability| {
                    cedar_protocol::supports_capability(Some(&h.accepted_info), capability)
                }),
            "normal-agent workspace or read capabilities absent",
        )?;
        h.retained()?;
        h.read(0)?;
        Ok(h)
    }

    fn wait(&self) -> Check<Duration> {
        let remaining = BUDGET
            .checked_sub(self.started.elapsed())
            .ok_or("acceptance deadline exceeded")?;
        require(!remaining.is_zero(), "acceptance deadline exceeded")?;
        Ok(remaining.min(WAIT))
    }

    fn response(&self, id: u64) -> Check<Event> {
        let event = self
            .app
            .result_rx
            .recv_timeout(self.wait()?)
            .map_err(|_| "normal-agent response timed out")?;
        let WorkerEvent::Response(reply) = event else {
            return Err("original worker closed during preflight acceptance");
        };
        require(
            reply.generation == self.generation
                && reply.id == id
                && reply.connected
                && reply.result.is_ok(),
            "response did not belong to the original live worker",
        )?;
        Ok(reply)
    }

    fn retained(&self) -> Check<()> {
        self.wait()?;
        require(
            self.app.ready()
                && self.app.worker.is_some()
                && self.app.generation == self.generation
                && self.app.pending.is_empty()
                && self.app.connecting_form.is_none()
                && !self.app.dirty()
                && !self.app.execution_trusted()
                && !self.app.unverified_local_close
                && self.app.root == self.accepted_root
                && self.app.workspace_key.as_ref() == Some(&self.accepted_key)
                && self.app.agent_info.as_ref() == Some(&self.accepted_info)
                && self.app.active_form.as_ref().is_some_and(|form| {
                    !form.ssh && !form.allow_run && form.key() == self.accepted_key
                }),
            "preflight changed the original clean connection",
        )?;
        require(
            matches!(
                self.app.result_rx.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ),
            "unexpected handshake, response, or close event",
        )
    }

    fn read(&mut self, index: usize) -> Check<()> {
        self.retained()?;
        let path = file(index);
        let id = self.app.next_request;
        self.app.open(path.clone(), None);
        require(
            self.app.next_request == id + 1
                && self.app.pending.len() == 1
                && matches!(self.app.pending.get(&id), Some(Job::Open { path: actual, .. })
                    if actual == &path),
            "fresh Open did not dispatch exactly one ordinary Read",
        )?;
        let event = self.response(id)?;
        require(
            matches!(&event.result, Ok(Payload::File { path: actual, text, revision: actual_revision })
                if actual == &path && text == SOURCE
                    && actual_revision == &revision(SOURCE.as_bytes())),
            "original worker Read did not return the generated source",
        )?;
        self.app.apply_event(event);
        self.read_responses += 1;
        require(
            self.app.documents.len() == self.read_responses
                && self.app.active().is_some_and(|doc| {
                    doc.path == path
                        && doc.text == SOURCE
                        && doc.saved_text == SOURCE
                        && !doc.dirty()
                }),
            "successful Read did not reach the clean editor",
        )?;
        self.retained()
    }

    fn reject(&mut self, form: ConnectForm, expected_error: &str) -> Check<()> {
        self.retained()?;
        require(
            !self.app.mutation_pending() && self.app.disconnect_problem().is_none(),
            "clean connection had an unrelated transition guard",
        )?;
        let next_request = self.app.next_request;
        let next_document = self.app.next_document;
        let active_document = self.app.active_document;
        let notice = self.app.notice.clone();
        let ctx = self.app.editor_ctx.clone();
        self.app.connect(&ctx, form);
        require(
            self.app
                .error
                .as_deref()
                .is_some_and(|error| error.starts_with(expected_error)),
            "invalid form did not reach the pure SSH preflight error",
        )?;
        require(
            self.app.next_request == next_request
                && self.app.next_document == next_document
                && self.app.active_document == active_document
                && self.app.documents.len() == self.read_responses
                && self.app.documents.iter().all(|doc| {
                    doc.text == SOURCE
                        && doc.saved_text == SOURCE
                        && doc.revision.as_deref() == Some(revision(SOURCE.as_bytes()).as_str())
                        && !doc.dirty()
                        && !doc.saving
                })
                && self.app.notice == notice,
            "invalid form reset workspace or request state",
        )?;
        self.retained()?;
        self.rejected += 1;
        // A distinct generated file forces a real Read, rather than selecting
        // an already open buffer. Its original generation/ID proves liveness.
        self.read(self.rejected)
    }

    fn disconnect(&mut self) -> Check<()> {
        self.retained()?;
        require(
            self.app.disconnect_problem().is_none(),
            "explicit Disconnect was unexpectedly blocked",
        )?;
        self.app.disconnect_idle();
        require(
            self.app.state == ConnectionState::Disconnecting
                && self.app.generation == self.generation
                && self.app.worker.is_none()
                && self.app.pending.is_empty(),
            "explicit Disconnect did not release the owned worker",
        )?;
        let event = self
            .app
            .result_rx
            .recv_timeout(self.wait()?)
            .map_err(|_| "owned normal-agent cleanup timed out")?;
        require(
            matches!(&event, WorkerEvent::Closed { generation, result: Ok(()) }
                if *generation == self.generation),
            "owned normal-agent cleanup was not verified",
        )?;
        // Worker publishes this only after Client::close_and_wait consumes
        // the direct child and observes its authoritative reaper result.
        self.closed = true;
        self.app.apply_worker_event(event);
        require(
            self.app.state == ConnectionState::Disconnected
                && !self.app.unverified_local_close
                && self.app.worker.is_none()
                && self.app.error.is_none(),
            "verified cleanup was not adopted by the app",
        )
    }
}

#[test]
#[ignore = "requires CEDAR_SSH_PREFLIGHT_AGENT_BIN selecting the normal shipping agent"]
fn normal_agent_ssh_preflight_preserves_live_clean_connection() -> Check<()> {
    let cases = invalid_forms();
    require(
        cases.len() == INVALID_FORMS,
        "invalid-form case count differed",
    )?;
    // Fail closed before starting any process. If pure preflight regresses,
    // the acceptance must never hand an invalid form to a live connect path
    // that might resolve or start the system SSH executable.
    for (form, expected_error) in &cases {
        require(
            form.spec()
                .err()
                .is_some_and(|error| error.starts_with(*expected_error)),
            "pure SSH preflight prerequisite failed before process startup",
        )?;
    }
    let mut h = Harness::new()?;
    for (form, expected_error) in cases {
        h.reject(form, expected_error)?;
    }
    require(
        h.rejected == INVALID_FORMS
            && h.read_responses == INVALID_FORMS + 1
            && h.app.next_request == INVALID_FORMS as u64 + 3,
        "observed request or rejection counts differed",
    )?;
    h.disconnect()?;
    let root = h.root.take().ok_or("owned fixture absent during cleanup")?;
    for index in 0..=INVALID_FORMS {
        let bytes = fs::read(root.path().join(file(index)))
            .map_err(|_| "generated source verification failed")?;
        require(
            revision(&bytes) == revision(SOURCE.as_bytes()),
            "generated source changed",
        )?;
    }
    let root_path = root.path().to_owned();
    root.close().map_err(|_| "owned fixture cleanup failed")?;
    require(!root_path.exists(), "owned fixture remained after cleanup")?;
    h.wait()?;
    println!(
        "{}",
        serde_json::json!({
            "success": true,
            "invalid_forms_rejected": 14,
            "host_cases": 4,
            "root_cases": 4,
            "agent_cases": 3,
            "port_cases": 3,
            "normal_agent_connections": 1,
            "hello_responses": 1,
            "list_responses": 1,
            "read_responses": 15,
            "post_rejection_reads": 14,
            "owned_agents_reaped": 1,
            "source_hashes_verified": 15,
            "clean_path_preflight_verified": true,
            "original_worker_generation_retained": true,
            "original_root_and_capabilities_retained": true,
            "trust_off": true,
            "rejected_connect_path_reached_no_ssh_spawn": true,
            "ssh_claim_scope": "pure preflight return before Worker::spawn",
            "explicit_disconnect_verified": true,
            "fixture_removed": true
        })
    );
    Ok(())
}
