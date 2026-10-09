//! Explicit normal-agent acceptance over generated, trust-off report files.
//! No compiler, test engine, language server, SSH or network listener is started.
use super::*;
use cedar_client::Client;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, time::Duration};

const UPSTREAM: &str = include_str!("../tests/fixtures/surefire-3.5.4/enclosed-error.xml");
const FIRST: &str = "reports 雪/TEST-first.xml";
const SECOND: &str = "reports 雪/TEST-second.xml";
const SECOND_XML: &str = r#"<testsuite name="generated 雪" tests="1" failures="0" errors="0" skipped="0"><testcase name="passed café" classname="generated.Case" time="0.25"/></testsuite>"#;
const INVALID: &str = "reports 雪/invalid.xml";
type Check<T> = Result<T, &'static str>;

fn require(value: bool, message: &'static str) -> Check<()> {
    if value {
        Ok(())
    } else {
        Err(message)
    }
}

struct Harness {
    app: CedarApp,
    commands: Receiver<Command>,
    client: Option<Client>,
    agent: PathBuf,
    root: Option<tempfile::TempDir>,
    reads: u32,
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(client) = self.client.take() {
            let _ = client.close_and_wait(Duration::from_secs(5));
        }
    }
}

impl Harness {
    fn connect(&mut self) -> Check<()> {
        if let Some(client) = self.client.take() {
            client
                .close_and_wait(Duration::from_secs(5))
                .map_err(|_| "old agent cleanup failed")?;
        }
        let root = self.root.as_ref().ok_or("root absent")?.path();
        let client =
            Client::spawn_agent(&self.agent, root, false).map_err(|_| "agent connect failed")?;
        let (worker, commands) = Worker::recording();
        self.app.worker = Some(worker);
        self.commands = commands;
        self.app.generation += 1;
        self.app.state = ConnectionState::Connecting;
        self.app.connecting_form = Some(ConnectForm {
            local_root: root.to_string_lossy().into_owned(),
            allow_run: false,
            ..Default::default()
        });
        self.app.apply_event(Event {
            generation: self.app.generation,
            id: 0,
            connected: true,
            result: Ok(client.handshake().clone()),
        });
        self.client = Some(client);
        let command = self
            .commands
            .try_recv()
            .map_err(|_| "initial list missing")?;
        require(
            matches!(&command.op, Operation::List { path } if path.is_empty()),
            "unexpected initial operation",
        )?;
        let result = self.client.as_mut().unwrap().request(command.op);
        self.app.apply_event(Event {
            generation: self.app.generation,
            id: command.id,
            connected: true,
            result,
        });
        require(
            self.app.ready() && !self.app.execution_trusted(),
            "trust-off connection not ready",
        )?;
        self.idle()
    }

    fn idle(&self) -> Check<()> {
        require(
            self.commands.try_recv().is_err(),
            "unexpected automatic request",
        )
    }

    fn load(&mut self, path: &str) -> Check<Event> {
        self.app.test_report.path = path.into();
        self.app.test_report.path_edited();
        self.app.load_test_report();
        let command = self
            .commands
            .try_recv()
            .map_err(|_| "report read missing")?;
        require(
            matches!(&command.op, Operation::Read { path: selected } if selected == path),
            "report load was not exact ordinary Read",
        )?;
        self.reads += 1;
        let client = self.client.as_mut().ok_or("client absent")?;
        let result = client.request(command.op);
        require(client.is_connected(), "report request disconnected")?;
        Ok(Event {
            generation: self.app.generation,
            id: command.id,
            connected: true,
            result,
        })
    }

    fn draft_unchanged(&self) -> Check<()> {
        require(self.app.documents.len() == 1, "report opened an editor tab")?;
        let document = &self.app.documents[0];
        require(
            document.path == "draft.txt"
                && document.text == "unsaved draft 雪\n"
                && document.saved_text == "original source\n"
                && document.dirty(),
            "draft was altered",
        )
    }
}

#[test]
#[ignore = "requires explicit CEDAR_TEST_REPORT_AGENT_BIN selecting normal shipping agent"]
fn normal_agent_test_report_reads_are_explicit_trust_off_and_revision_bound() -> Check<()> {
    require(
        UPSTREAM.len() == 869
            && format!("{:x}", Sha256::digest(UPSTREAM.as_bytes()))
                == "3fd206e8ee8dc198a7e0adc960ec99a6a44f80ffeab025f4851d414feb21af2e",
        "upstream fixture identity differs",
    )?;
    let agent = PathBuf::from(
        std::env::var_os("CEDAR_TEST_REPORT_AGENT_BIN").ok_or("agent selection missing")?,
    );
    require(
        agent.is_absolute() && agent.is_file(),
        "agent selection invalid",
    )?;
    let root = tempfile::Builder::new()
        .prefix("cedar-report 雪-")
        .tempdir()
        .map_err(|_| "root create failed")?;
    fs::create_dir(root.path().join("reports 雪")).map_err(|_| "report dir create failed")?;
    for (path, text) in [
        (FIRST, UPSTREAM),
        (SECOND, SECOND_XML),
        (
            INVALID,
            "<!DOCTYPE testsuite SYSTEM 'file:///absent'><testsuite/>",
        ),
        ("draft.txt", "original source\n"),
    ] {
        fs::write(root.path().join(path), text).map_err(|_| "fixture write failed")?;
    }
    let (_, commands) = Worker::recording();
    let mut h = Harness {
        app: CedarApp::empty(),
        commands,
        client: None,
        agent,
        root: Some(root),
        reads: 0,
    };
    h.connect()?;
    let mut document = Document::new(
        1,
        "draft.txt".into(),
        "original source\n".into(),
        "a".repeat(64),
    );
    document.text = "unsaved draft 雪\n".into();
    h.app.documents.push(document);
    h.app.active_document = Some(1);
    h.app.next_document = 2;
    h.idle()?;
    let first = h.load(FIRST)?;
    let first_revision = match &first.result {
        Ok(Payload::File {
            path,
            text,
            revision,
        }) if path == FIRST && text == UPSTREAM => revision.clone(),
        _ => return Err("upstream report read differed"),
    };
    h.app.apply_event(first);
    let snapshot = h
        .app
        .test_report
        .snapshot
        .as_ref()
        .ok_or("report snapshot missing")?;
    require(
        snapshot.source.path == FIRST
            && snapshot.revision == first_revision
            && snapshot.report.counts.total == 1
            && snapshot.report.counts.errors == 1
            && snapshot.report.counts.passed == 0,
        "upstream report classification differed",
    )?;
    h.draft_unchanged()?;
    h.idle()?;

    let old = h.load(FIRST)?;
    let newer = h.load(SECOND)?;
    h.app.apply_event(newer);
    h.app.apply_event(old);
    let snapshot = h
        .app
        .test_report
        .snapshot
        .as_ref()
        .ok_or("newer report missing")?;
    require(
        snapshot.source.path == SECOND && snapshot.report.counts.passed == 1,
        "late old report replaced newer selection",
    )?;
    h.draft_unchanged()?;
    h.idle()?;

    let invalid = h.load(INVALID)?;
    h.app.apply_event(invalid);
    require(
        h.app.test_report.snapshot.is_none() && h.app.test_report.message.is_some(),
        "invalid XML appeared as successful report",
    )?;
    let missing = h.load("reports 雪/missing.xml")?;
    h.app.apply_event(missing);
    require(
        h.app.test_report.snapshot.is_none() && h.app.test_report.message.is_some(),
        "missing report appeared successful",
    )?;
    h.app.test_report.path = "../outside.xml".into();
    h.app.test_report.path_edited();
    h.app.load_test_report();
    h.idle()?;

    let stale_session = h.load(FIRST)?;
    h.app.disconnected("synthetic reconnect".into());
    h.connect()?;
    h.app.apply_event(stale_session);
    require(
        h.app.test_report.snapshot.is_none() && h.app.test_report.loading.is_none(),
        "old session report restored after reconnect",
    )?;
    let fresh = h.load(SECOND)?;
    h.app.apply_event(fresh);
    require(
        h.app
            .test_report
            .snapshot
            .as_ref()
            .is_some_and(|s| s.report.counts.passed == 1),
        "fresh reconnect read failed",
    )?;
    h.draft_unchanged()?;
    h.idle()?;
    for (path, expected) in [
        (FIRST, UPSTREAM),
        (SECOND, SECOND_XML),
        (
            INVALID,
            "<!DOCTYPE testsuite SYSTEM 'file:///absent'><testsuite/>",
        ),
        ("draft.txt", "original source\n"),
    ] {
        require(
            fs::read_to_string(h.root.as_ref().unwrap().path().join(path))
                .map_err(|_| "fixture reread failed")?
                == expected,
            "fixture source changed",
        )?;
    }
    h.client
        .take()
        .ok_or("client missing during cleanup")?
        .close_and_wait(Duration::from_secs(5))
        .map_err(|_| "agent cleanup failed")?;
    h.app.worker = None;
    h.root
        .take()
        .ok_or("root missing during cleanup")?
        .close()
        .map_err(|_| "fixture cleanup failed")?;
    println!(
        "{}",
        serde_json::json!({"kind":"test_report_read_acceptance","success":true,"trust_off":true,"ordinary_reads_only":true,"upstream_fixture_verified":true,"revision_bound":true,"stale_load_rejected":true,"stale_session_rejected":true,"malformed_and_missing_rejected":true,"dirty_draft_preserved":true,"source_unchanged":true,"client_reaped":true,"fixture_removed":true,"reads":h.reads})
    );
    Ok(())
}
