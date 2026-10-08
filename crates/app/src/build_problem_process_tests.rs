//! Explicit, synthetic-only JDK 21 acceptance through the shipping stdio agent.
//! No GUI trust, SSH, project discovery, downloads, or user source is involved.
//! Only bounded scalar evidence is printed; compiler text and generated source
//! remain inside the test process. Missing tool selections fail the opt-in run.
use super::*;
use cedar_client::Client;
use cedar_tasks::{TaskSnapshot, TaskState};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const FILE: &str = "CedarBuildProblem.java";
const SOURCE: &str =
    "public class CedarBuildProblem {\n    int value = \"cedar synthetic compile error\";\n}\n";
const DRAFT: &str = "public class CedarBuildProblem {\n    int unsavedValue = \"cedar synthetic compile error\";\n}\n";
const NOTE: &str = "generated note\n";
const NOTE_DRAFT: &str = "generated unsaved note 雪\n";
type Check<T> = Result<T, &'static str>;

fn require(condition: bool, message: &'static str) -> Check<()> {
    if condition {
        Ok(())
    } else {
        Err(message)
    }
}

fn selected_executable(variable: &str, filename: &str) -> Check<PathBuf> {
    let path =
        PathBuf::from(std::env::var_os(variable).ok_or("explicit executable selection missing")?);
    require(
        path.is_absolute() && path.is_file(),
        "selected executable must be an existing absolute file",
    )?;
    require(
        path.file_name().is_some_and(|name| name == filename),
        "unexpected selected executable name",
    )?;
    // Keep the supplied ordinary spelling. Canonicalizing javac.exe to a
    // Windows verbatim path can crash the JVM before any compiler work starts.
    require(
        !path.to_string_lossy().starts_with(r"\\?\")
            && !path.to_string_lossy().starts_with(r"\\.\"),
        "javac acceptance requires ordinary executable spelling",
    )?;
    Ok(path)
}

fn bounded_file(path: &Path, limit: u64) -> Check<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "selected toolchain file could not be opened")?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "selected toolchain file could not be read")?;
    require(
        bytes.len() as u64 <= limit,
        "selected toolchain file exceeds its bound",
    )?;
    Ok(bytes)
}

fn bounded_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 64
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(&byte))
        && version.split(['.', '-', '+']).next() == Some("21")
}

#[derive(Default, Serialize)]
struct Evidence {
    kind: &'static str,
    route: &'static str,
    jdk_major: u8,
    jdk_release_version: String,
    javac_version: String,
    javac_sha256: String,
    explicit_toolchain: bool,
    environment_controlled: bool,
    literal_run_start: bool,
    terminal_run_poll: bool,
    failed_compile: bool,
    explicit_extraction: bool,
    exact_relative_location: bool,
    immutable_command: bool,
    normal_read_navigation: bool,
    dirty_target_preserved: bool,
    one_undo_exact: bool,
    one_redo_exact: bool,
    other_dirty_buffer_preserved: bool,
    stale_task_read_rejected: bool,
    stale_session_read_rejected: bool,
    source_files_unchanged: bool,
    output_directory_selected_and_root_inventory_verified: bool,
    agent_reaped: bool,
    synthetic_root_removed: bool,
    starts: u8,
    polls: u16,
    reads: u8,
    failure_stage: &'static str,
    primary_failed: bool,
    cleanup_failed: bool,
    success: bool,
}

struct Harness {
    client: Option<Client>,
    app: CedarApp,
    commands: Receiver<Command>,
    root: Option<tempfile::TempDir>,
    agent: PathBuf,
    javac: String,
    clock: f64,
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(client) = self.client.take() {
            let _ = client.close_and_wait(Duration::from_secs(5));
        }
    }
}

impl Harness {
    fn new(agent: PathBuf, javac: PathBuf) -> Check<Self> {
        let root = tempfile::tempdir().map_err(|_| "synthetic root creation failed")?;
        fs::write(root.path().join(FILE), SOURCE)
            .map_err(|_| "synthetic source creation failed")?;
        fs::write(root.path().join("draft.txt"), NOTE)
            .map_err(|_| "synthetic note creation failed")?;
        fs::create_dir(root.path().join("classes"))
            .map_err(|_| "synthetic output directory creation failed")?;
        let (_, commands) = Worker::recording();
        Ok(Self {
            client: None,
            app: CedarApp::empty(),
            commands,
            root: Some(root),
            agent,
            javac: javac
                .to_str()
                .ok_or("selected javac path must be UTF-8")?
                .to_owned(),
            clock: 0.0,
        })
    }

    fn connect(&mut self) -> Check<()> {
        if let Some(client) = self.client.take() {
            client
                .close_and_wait(Duration::from_secs(5))
                .map_err(|_| "previous agent did not reap")?;
        }
        let root = self.root.as_ref().ok_or("synthetic root missing")?.path();
        let client = Client::spawn_agent(&self.agent, root, true)
            .map_err(|_| "shipping agent connection failed")?;
        let (worker, commands) = Worker::recording();
        self.commands = commands;
        self.app.worker = Some(worker);
        self.app.generation += 1;
        self.app.state = ConnectionState::Connecting;
        self.app.connecting_form = Some(ConnectForm {
            local_root: root.to_string_lossy().into_owned(),
            ssh: false,
            allow_run: true,
            ..Default::default()
        });
        self.app.apply_event(Event {
            generation: self.app.generation,
            id: 0,
            connected: true,
            result: Ok(client.handshake().clone()),
        });
        self.client = Some(client);
        let command = self.next()?;
        require(
            matches!(&command.op, Operation::List { path } if path.is_empty()),
            "initial request was not ordinary List",
        )?;
        let event = self.exchange(command)?;
        self.app.apply_event(event);
        require(
            self.app.ready() && self.app.execution_trusted(),
            "synthetic connection did not become ready",
        )?;
        self.idle()
    }

    fn next(&self) -> Check<Command> {
        self.commands
            .try_recv()
            .map_err(|_| "expected frontend request missing")
    }

    fn idle(&self) -> Check<()> {
        require(
            matches!(
                self.commands.try_recv(),
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected)
            ),
            "unexpected frontend request",
        )
    }

    fn exchange(&mut self, command: Command) -> Check<Event> {
        // A new feature must not quietly add writes, language launches, Git, or
        // discovery operations to this source-read/command-only pipeline.
        require(
            matches!(
                &command.op,
                Operation::List { .. }
                    | Operation::Read { .. }
                    | Operation::RunStart { .. }
                    | Operation::RunPoll { .. }
            ),
            "unexpected operation in javac acceptance",
        )?;
        let client = self.client.as_mut().ok_or("shipping agent missing")?;
        let result = client.request(command.op);
        require(
            result.is_ok() && client.is_connected(),
            "shipping agent request failed",
        )?;
        Ok(Event {
            generation: self.app.generation,
            id: command.id,
            connected: true,
            result,
        })
    }

    fn read_event(&mut self, path: &str, evidence: &mut Evidence) -> Check<Event> {
        let command = self.next()?;
        require(
            matches!(&command.op, Operation::Read { path: actual } if actual == path),
            "navigation did not use the expected ordinary Read",
        )?;
        evidence.reads = evidence
            .reads
            .checked_add(1)
            .ok_or("read counter exceeded")?;
        self.exchange(command)
    }

    fn start(&mut self, args: Vec<String>, evidence: &mut Evidence) -> Check<()> {
        self.app.profiles.draft.program.clone_from(&self.javac);
        self.app.profiles.draft.args = args.clone();
        self.app.profiles.draft.timeout_secs = 30;
        self.app.run();
        let command = self.next()?;
        require(
            matches!(&command.op, Operation::RunStart { program, args: actual, timeout_secs: 30 } if program == &self.javac && actual == &args),
            "RunStart changed literal executable or arguments",
        )?;
        require(
            self.app.run_state.problems.is_none(),
            "new task retained old extracted locations",
        )?;
        // Edit the command form before the actual acceptance response arrives.
        // Neither its identity nor subsequent extracted provenance may follow it.
        self.app.profiles.draft.program = "not-the-running-compiler".into();
        self.app.profiles.draft.args = vec!["not-the-running-arguments".into()];
        self.app.profiles.changed();
        require(
            self.app
                .run_state
                .submitted
                .as_ref()
                .is_some_and(|submitted| {
                    submitted.program == self.javac
                        && submitted.args == args
                        && submitted.backend_os == std::env::consts::OS
                }),
            "submitted command identity followed the editable form",
        )?;
        let event = self.exchange(command)?;
        self.app.apply_event(event);
        evidence.starts = evidence
            .starts
            .checked_add(1)
            .ok_or("start counter exceeded")?;
        evidence.literal_run_start = true;
        self.idle()
    }

    fn finish(&mut self, evidence: &mut Evidence) -> Check<TaskSnapshot> {
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            let task = self
                .app
                .run_state
                .snapshot
                .as_ref()
                .ok_or("task snapshot missing")?;
            if task.state.is_terminal() {
                require(
                    !task.truncated && task.error.is_none(),
                    "compiler output or outcome incomplete",
                )?;
                return Ok(task.clone());
            }
            require(
                Instant::now() < deadline,
                "compiler did not reach a terminal state",
            )?;
            let task_id = task.id;
            std::thread::sleep(Duration::from_millis(250));
            self.clock += 0.5;
            let ctx = self.app.editor_ctx.clone();
            let _ = ctx.run(
                egui::RawInput {
                    time: Some(self.clock),
                    ..Default::default()
                },
                |_| {},
            );
            self.app.run_tick(&ctx);
            let command = self.next()?;
            require(
                matches!(&command.op, Operation::RunPoll { task_id: id } if *id == task_id),
                "task did not use its original RunPoll identity",
            )?;
            let event = self.exchange(command)?;
            self.app.apply_event(event);
            evidence.polls = evidence
                .polls
                .checked_add(1)
                .ok_or("poll counter exceeded")?;
            evidence.terminal_run_poll = self
                .app
                .run_state
                .snapshot
                .as_ref()
                .is_some_and(|task| task.state.is_terminal());
        }
    }

    fn document(&self, path: &str) -> Check<&Document> {
        self.app
            .documents
            .iter()
            .find(|doc| doc.path == path)
            .ok_or("expected document missing")
    }

    fn edit(&mut self, path: &str, text: &str) -> Check<()> {
        let doc = self
            .app
            .documents
            .iter_mut()
            .find(|doc| doc.path == path)
            .ok_or("edit target missing")?;
        editor_state::commit(&self.app.editor_ctx, doc, text.into(), 4);
        Ok(())
    }

    fn history(&mut self, path: &str, redo: bool) -> Check<()> {
        let ctx = self.app.editor_ctx.clone();
        let doc = self
            .app
            .documents
            .iter_mut()
            .find(|doc| doc.path == path)
            .ok_or("history target missing")?;
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Z,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: if redo {
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
                    } else {
                        egui::Modifiers::COMMAND
                    },
                }],
                ..Default::default()
            },
            |ctx| {
                ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", doc.id))));
                editor_state::history_shortcut(ctx, doc);
            },
        );
        Ok(())
    }

    fn sources_unchanged(&self) -> Check<()> {
        let root = self.root.as_ref().ok_or("synthetic root missing")?.path();
        require(
            fs::read(root.join(FILE)).ok().as_deref() == Some(SOURCE.as_bytes())
                && fs::read(root.join("draft.txt")).ok().as_deref() == Some(NOTE.as_bytes()),
            "synthetic source bytes changed",
        )
    }

    fn root_inventory(&self) -> Check<()> {
        let root = self.root.as_ref().ok_or("synthetic root missing")?.path();
        let entries = fs::read_dir(root).map_err(|_| "synthetic root inspection failed")?;
        for entry in entries {
            let entry = entry.map_err(|_| "synthetic root entry inspection failed")?;
            require(
                [FILE, "draft.txt", "classes"]
                    .iter()
                    .any(|name| entry.file_name() == *name),
                "unexpected synthetic root entry",
            )?;
        }
        require(
            root.join("classes").is_dir(),
            "synthetic compiler output directory missing",
        )
    }

    fn cleanup(&mut self, evidence: &mut Evidence) -> Check<()> {
        let reaped = self
            .client
            .take()
            .is_none_or(|client| client.close_and_wait(Duration::from_secs(5)).is_ok());
        evidence.agent_reaped = reaped;
        if let Some(root) = self.root.take() {
            let path = root.path().to_owned();
            evidence.synthetic_root_removed = root.close().is_ok() && !path.exists();
        }
        require(
            evidence.agent_reaped && evidence.synthetic_root_removed,
            "synthetic acceptance cleanup failed",
        )
    }
}

fn compile_args() -> Vec<String> {
    [
        "-J-Duser.language=en",
        "-J-Duser.country=US",
        "-proc:none",
        "-classpath",
        ".",
        "-sourcepath",
        ".",
        "-d",
        "classes",
        FILE,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn version_args() -> Vec<String> {
    ["-J-Duser.language=en", "-J-Duser.country=US", "-version"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn extract(h: &mut Harness, evidence: &mut Evidence) -> Check<()> {
    require(
        h.app.run_state.problems.is_none(),
        "locations were extracted without an explicit action",
    )?;
    let output = h.app.run_state.output.clone();
    h.app.extract_build_problems();
    h.idle()?;
    require(
        h.app.run_state.output == output,
        "extraction changed raw command output",
    )?;
    let parsed = h
        .app
        .run_state
        .problems
        .as_ref()
        .ok_or("explicit extraction produced no snapshot")?;
    require(
        parsed.rows.len() == 1,
        "expected one generated javac location",
    )?;
    let row = &parsed.rows[0];
    require(
        row.path == FILE
            && row.navigation_path.as_deref() == Some(FILE)
            && row.line == 2
            && row.severity == build_problems::Severity::Error
            && row.stream == build_problems::Stream::Stderr
            && row.message == "incompatible types: String cannot be converted to int"
            && row.disabled_reason.is_none()
            && !row.message_truncated
            && !parsed.summary.row_limit_reached
            && !parsed.summary.text_limit_reached
            && parsed.summary.unscanned_bytes == 0,
        "generated javac location did not match its exact source",
    )?;
    evidence.explicit_extraction = true;
    evidence.exact_relative_location = true;
    Ok(())
}

fn pipeline(h: &mut Harness, evidence: &mut Evidence) -> Check<()> {
    evidence.failure_stage = "connect";
    h.connect()?;
    evidence.failure_stage = "javac_version";
    h.start(version_args(), evidence)?;
    let version = h.finish(evidence)?;
    require(
        version.state == TaskState::Succeeded && version.exit_code == Some(0),
        "selected javac version command failed",
    )?;
    let output = format!("{}{}", version.stdout, version.stderr);
    let actual = output
        .trim()
        .strip_prefix("javac ")
        .ok_or("javac version output was not recognized")?;
    require(
        bounded_version(actual),
        "selected executable is not bounded JDK 21 javac",
    )?;
    evidence.javac_version = actual.into();

    evidence.failure_stage = "dirty_note";
    h.app.open("draft.txt".into(), None);
    let event = h.read_event("draft.txt", evidence)?;
    h.app.apply_event(event);
    h.edit("draft.txt", NOTE_DRAFT)?;
    let note_id = h.document("draft.txt")?.id;

    evidence.failure_stage = "compile";
    h.start(compile_args(), evidence)?;
    let terminal = h.finish(evidence)?;
    require(
        terminal.state == TaskState::Failed && terminal.exit_code == Some(1),
        "generated compile did not fail normally with exit one",
    )?;
    evidence.failed_compile = true;
    evidence.failure_stage = "extract";
    extract(h, evidence)?;
    // The pending form no longer names javac. Successful retained extraction
    // and navigation must belong to the run that actually produced the output.
    require(
        h.app.profiles.draft.program == "not-the-running-compiler",
        "test form mutation was lost",
    )?;
    evidence.immutable_command = true;

    evidence.failure_stage = "read_navigation";
    h.app.open_build_problem(0);
    let event = h.read_event(FILE, evidence)?;
    h.app.apply_event(event);
    let doc = h.document(FILE)?;
    require(
        doc.text == SOURCE
            && h.app.active_document == Some(doc.id)
            && doc.jump_to == Some(model::line_start(SOURCE, 2)),
        "Read result did not select the exact generated source line",
    )?;
    evidence.normal_read_navigation = true;

    evidence.failure_stage = "dirty_history";
    h.edit(FILE, DRAFT)?;
    let before = (
        h.document(FILE)?.id,
        h.document(FILE)?.edit_version,
        h.document(FILE)?.revision.clone(),
    );
    h.app.open_build_problem(0);
    h.idle()?;
    let doc = h.document(FILE)?;
    require(
        doc.text == DRAFT
            && doc.saved_text == SOURCE
            && doc.dirty()
            && (doc.id, doc.edit_version, doc.revision.clone()) == before
            && doc.jump_to == Some(model::line_start(DRAFT, 2)),
        "result navigation changed the dirty target",
    )?;
    evidence.dirty_target_preserved = true;
    h.history(FILE, false)?;
    require(
        h.document(FILE)?.text == SOURCE && !h.document(FILE)?.dirty(),
        "one Undo did not restore source exactly",
    )?;
    evidence.one_undo_exact = true;
    h.history(FILE, true)?;
    require(
        h.document(FILE)?.text == DRAFT && h.document(FILE)?.dirty(),
        "one Redo did not restore draft exactly",
    )?;
    evidence.one_redo_exact = true;
    require(
        h.document("draft.txt")?.text == NOTE_DRAFT
            && h.document("draft.txt")?.dirty()
            && h.document("draft.txt")?.id == note_id,
        "unrelated dirty note was replaced",
    )?;
    h.history("draft.txt", false)?;
    require(
        h.document("draft.txt")?.text == NOTE,
        "unrelated note history was lost",
    )?;
    h.history("draft.txt", true)?;
    require(
        h.document("draft.txt")?.text == NOTE_DRAFT,
        "unrelated note redo was lost",
    )?;
    evidence.other_dirty_buffer_preserved = true;

    evidence.failure_stage = "stale_task";
    h.history(FILE, false)?;
    require(
        !h.document(FILE)?.dirty(),
        "target must be clean before closing",
    )?;
    let id = h.document(FILE)?.id;
    h.app.remove_tab(id);
    h.app.open_build_problem(0);
    let stale = h.read_event(FILE, evidence)?;
    h.start(version_args(), evidence)?;
    h.app.apply_event(stale);
    require(
        h.app.documents.iter().all(|doc| doc.path != FILE),
        "old task Read opened after task replacement",
    )?;
    require(
        h.finish(evidence)?.state == TaskState::Succeeded,
        "replacement command failed",
    )?;
    evidence.stale_task_read_rejected = true;

    evidence.failure_stage = "stale_session";
    h.start(compile_args(), evidence)?;
    require(
        h.finish(evidence)?.state == TaskState::Failed,
        "second generated compile did not fail normally",
    )?;
    extract(h, evidence)?;
    h.app.open_build_problem(0);
    let stale = h.read_event(FILE, evidence)?;
    h.app.disconnected("synthetic reconnect boundary".into());
    h.connect()?;
    h.app.apply_event(stale);
    h.app.open_build_problem(0);
    h.idle()?;
    require(
        h.app.run_state.problems.is_none()
            && h.app.run_state.snapshot.is_none()
            && h.app.documents.iter().all(|doc| doc.path != FILE)
            && h.document("draft.txt")?.text == NOTE_DRAFT,
        "old session retained navigation authority",
    )?;
    evidence.stale_session_read_rejected = true;

    require(
        evidence.terminal_run_poll && evidence.polls > 0,
        "terminal RunPoll witness missing",
    )?;
    h.idle()
}

fn acceptance(evidence: &mut Evidence) -> Check<()> {
    evidence.failure_stage = "toolchain";
    let agent = selected_executable(
        "CEDAR_AGENT_BIN",
        if cfg!(windows) {
            "cedar-agent.exe"
        } else {
            "cedar-agent"
        },
    )?;
    let javac = selected_executable(
        "CEDAR_JAVAC_BIN",
        if cfg!(windows) { "javac.exe" } else { "javac" },
    )?;
    let jdk = javac
        .parent()
        .and_then(Path::parent)
        .ok_or("JDK installation root missing")?;
    let release = String::from_utf8(bounded_file(&jdk.join("release"), 64 * 1024)?)
        .map_err(|_| "JDK release metadata is not UTF-8")?;
    let version = release
        .lines()
        .find_map(|line| {
            line.strip_prefix("JAVA_VERSION=\"")
                .and_then(|value| value.strip_suffix('"'))
        })
        .ok_or("JDK release version missing")?;
    require(
        bounded_version(version),
        "JDK release metadata must identify version 21",
    )?;
    evidence.jdk_major = 21;
    evidence.jdk_release_version = version.into();
    evidence.javac_sha256 = format!(
        "{:x}",
        Sha256::digest(bounded_file(&javac, 32 * 1024 * 1024)?)
    );
    evidence.explicit_toolchain = true;
    require(
        [
            "JDK_JAVA_OPTIONS",
            "JDK_JAVAC_OPTIONS",
            "JAVA_TOOL_OPTIONS",
            "_JAVA_OPTIONS",
            "CLASSPATH",
        ]
        .iter()
        .all(|name| std::env::var_os(name).is_none()),
        "remove inherited Java option and classpath variables from the test parent",
    )?;
    evidence.environment_controlled = true;
    evidence.failure_stage = "fixture";
    let mut harness = Harness::new(agent, javac)?;
    let primary = pipeline(&mut harness, evidence);
    // Retain independent source evidence even when extraction or navigation
    // fails. Neither invariant failure nor its inspection is cleanup itself.
    let sources = harness.sources_unchanged();
    let inventory = harness.root_inventory();
    evidence.source_files_unchanged = sources.is_ok();
    evidence.output_directory_selected_and_root_inventory_verified =
        evidence.failed_compile && inventory.is_ok();
    let integrity = sources.and(inventory);
    if primary.is_ok() && integrity.is_err() {
        evidence.failure_stage = "source_integrity";
    }
    let primary = primary.and(integrity);
    evidence.primary_failed = primary.is_err();
    let cleanup = harness.cleanup(evidence);
    evidence.cleanup_failed = cleanup.is_err();
    if primary.is_ok() && cleanup.is_err() {
        evidence.failure_stage = "cleanup";
    }
    primary.and(cleanup)
}

#[test]
#[ignore = "requires explicit CEDAR_AGENT_BIN and JDK 21 CEDAR_JAVAC_BIN; synthetic CI acceptance"]
fn real_javac_run_extract_read_navigation_preserves_draft_history() {
    let mut evidence = Evidence {
        kind: "javac_build_problem_acceptance",
        route: "normal_agent_client_frontend",
        ..Default::default()
    };
    let result = acceptance(&mut evidence);
    evidence.primary_failed |= result.is_err() && !evidence.cleanup_failed;
    evidence.success = result.is_ok();
    if evidence.success {
        evidence.failure_stage = "none";
    }
    println!(
        "CEDAR_BUILD_PROBLEM_RECEIPT={}",
        serde_json::to_string(&evidence).expect("bounded receipt serialization")
    );
    assert!(
        result.is_ok(),
        "synthetic javac acceptance failed at {}",
        evidence.failure_stage
    );
}
