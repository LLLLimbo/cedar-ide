//! Copy to new draft through the ordinary Trust-off stdio agent on either OS.
//! Seven fixed cases share one watchdog, including every replacement agent.
//! The existing marked, nonshipping peer supplies two wrong-digest outcomes;
//! it is never described as shipping-agent evidence. The destination appearing
//! before Save is a pre-Save race, not the workspace's post-preparation race.
use super::*;
use cedar_client::{Client, ConnectionCancellation};
use cedar_recovery::{record_id, Store};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

const BUDGET: Duration = Duration::from_secs(120);
// Cancel foreground work at 115 s, leaving the final owned-child observation
// inside the same 120 s total budget. Neither clock restarts on reconnect.
const ACTIVE_BUDGET: Duration = Duration::from_secs(115);
const REQUEST: Duration = Duration::from_secs(30);
const WAIT: Duration = Duration::from_secs(5);
const SOURCE: &str = "source Ω.txt";
const DESTINATION: &str = "copy café.txt";
const CONTROLLED_PATH: &str = "draft.txt";
const BASE: &str = "original α\nsecond café\nthird line\n";
const DRAFT: &str = "unsaved draft β\nsecond café\nthird line\n";
const NEWER: &str = "newer source Ω\nsecond café\nthird line\n";
const EXTERNAL: &str = "independently created destination é\n";
const MARKER: &str = ".cedar-interrupted-save-validation";
const MARKER_TEXT: &[u8] = b"cedar-interrupted-save-validation-v1\nsynthetic-data-only\n";
const ACK_MODE: &str = ".cedar-synthetic-save-ack-mode";
const OPERATIONS: &str = ".cedar-interrupted-save-operations";
const COMMITTED: &str = ".cedar-interrupted-save-committed";
const CASES: [&str; 7] = [
    "normal_dirty_explicit_save",
    "normal_preexisting_destination",
    "normal_presave_destination_race",
    "normal_source_changed_reject",
    "normal_reconnect_reject",
    "controlled_unknown_source_copy",
    "controlled_copy_save_unknown",
];

fn revision(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn binary(controlled: bool) -> PathBuf {
    let variable = if controlled {
        "CEDAR_INTERRUPTED_SAVE_AGENT_BIN"
    } else {
        "CEDAR_COPY_DRAFT_AGENT_BIN"
    };
    let path =
        PathBuf::from(std::env::var_os(variable).expect("explicit acceptance binary required"));
    let name = match (controlled, cfg!(windows)) {
        (false, false) => "cedar-agent",
        (false, true) => "cedar-agent.exe",
        (true, false) => "cedar-agent-interrupted-save-validation",
        (true, true) => "cedar-agent-interrupted-save-validation.exe",
    };
    assert!(path.is_absolute() && path.is_file());
    assert_eq!(path.file_name().unwrap(), name);
    path
}

struct Budget {
    started: Instant,
    cancellation: ConnectionCancellation,
    watchdog: Option<(Sender<()>, std::thread::JoinHandle<bool>)>,
}

impl Budget {
    fn new() -> Self {
        let started = Instant::now();
        let cancellation = ConnectionCancellation::new();
        let on_expiry = cancellation.clone();
        let (stop, stopped) = mpsc::channel();
        let remaining = ACTIVE_BUDGET.saturating_sub(started.elapsed());
        let watchdog = std::thread::spawn(move || {
            if matches!(
                stopped.recv_timeout(remaining),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                on_expiry.cancel();
                true
            } else {
                false
            }
        });
        Self {
            started,
            cancellation,
            watchdog: Some((stop, watchdog)),
        }
    }

    fn admit(&self, reserve: Duration) {
        assert!(
            !self.cancellation.is_cancelled() && self.started.elapsed() + reserve < ACTIVE_BUDGET,
            "shared acceptance budget exhausted or cancelled"
        );
    }

    fn cleanup_allowance(&self) -> Duration {
        BUDGET.saturating_sub(self.started.elapsed()).min(WAIT)
    }

    fn stop(&mut self) {
        self.admit(Duration::ZERO);
        let (stop, watchdog) = self.watchdog.take().unwrap();
        let _ = stop.send(());
        assert!(
            !watchdog.join().expect("watchdog joined"),
            "shared watchdog expired"
        );
    }
}

impl Drop for Budget {
    fn drop(&mut self) {
        if let Some((stop, watchdog)) = self.watchdog.take() {
            self.cancellation.cancel();
            let _ = stop.send(());
            let _ = watchdog.join();
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SourceSnapshot {
    id: u64,
    path: String,
    text: String,
    saved_text: String,
    revision: Option<String>,
    saving: bool,
    interrupted: Option<interrupted_save::InterruptedSave>,
    unverifiable: bool,
    edit_version: u64,
    cursor: (usize, usize),
    jump_to: Option<usize>,
    scroll_to: Option<usize>,
    has_cjk: bool,
    undo_initialized: bool,
    // CCursor equality omits affinity; compare all four endpoint fields.
    selection: (usize, bool, usize, bool),
}

struct Harness<'a> {
    budget: &'a Budget,
    app: CedarApp,
    commands: Receiver<Command>,
    client: Option<Client>,
    root: Option<tempfile::TempDir>,
    recovery_root: Option<tempfile::TempDir>,
    store_path: PathBuf,
    source: &'static str,
    destination: &'static str,
    controlled_connection: bool,
    ledger: Vec<serde_json::Value>,
    controlled_ledger: Vec<&'static str>,
    connections: usize,
    reaped: usize,
    source_writes: usize,
    destination_writes: usize,
    acknowledgements: usize,
    recovery_quiescent: bool,
    recovery_stopped: bool,
    clock: f64,
}

impl Drop for Harness<'_> {
    fn drop(&mut self) {
        // Only one child can be live. Failure cleanup has one final 5 s bound;
        // successful receipts require every close result and count below.
        let deadline = Instant::now() + self.budget.cleanup_allowance();
        let agent_stopped = self.client.take().is_none_or(|client| {
            client
                .close_and_wait(deadline.saturating_duration_since(Instant::now()))
                .is_ok()
        });
        let recovery_stopped = self.stop_recovery_until(deadline);
        if !agent_stopped || !recovery_stopped {
            self.retain_fixtures();
            eprintln!("copy_draft_acceptance cleanup unverified; owned fixtures retained; external test process deadline remains the only bound for blocked teardown");
        }
    }
}

impl<'a> Harness<'a> {
    fn new(budget: &'a Budget, case: usize) -> Self {
        budget.admit(REQUEST + WAIT);
        let root = tempfile::Builder::new()
            .prefix("cedar-copy-draft Ω-")
            .tempdir()
            .unwrap();
        let source = if case == 5 { CONTROLLED_PATH } else { SOURCE };
        let destination = if case == 6 {
            CONTROLLED_PATH
        } else {
            DESTINATION
        };
        budget.admit(Duration::ZERO);
        fs::write(root.path().join(source), BASE).unwrap();
        if case == 1 {
            budget.admit(Duration::ZERO);
            fs::write(root.path().join(destination), EXTERNAL).unwrap();
        }
        if case >= 5 {
            budget.admit(Duration::ZERO);
            fs::write(root.path().join(MARKER), MARKER_TEXT).unwrap();
            budget.admit(Duration::ZERO);
            fs::write(root.path().join(ACK_MODE), b"synthetic-wrong-digest\n").unwrap();
        }
        budget.admit(Duration::ZERO);
        let recovery_root = tempfile::Builder::new()
            .prefix("cedar-copy-recovery-")
            .tempdir()
            .unwrap();
        let store_path = recovery_root.path().join("private-recovery");
        budget.admit(Duration::ZERO);
        let canonical = root
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(!system_fonts::contains_cjk(&canonical));
        let (_, commands) = Worker::recording();
        let mut app = CedarApp::empty();
        app.open_form = false;
        app.form.local_root = canonical;
        let mut h = Self {
            budget,
            app,
            commands,
            client: None,
            root: Some(root),
            recovery_root: Some(recovery_root),
            store_path,
            source,
            destination,
            controlled_connection: false,
            ledger: Vec::new(),
            controlled_ledger: Vec::new(),
            connections: 0,
            reaped: 0,
            source_writes: 0,
            destination_writes: 0,
            acknowledgements: 0,
            recovery_quiescent: false,
            recovery_stopped: false,
            clock: 0.0,
        };
        h.connect(case == 5);
        h.app.open(source.into(), None);
        let command = h.next();
        assert!(matches!(&command.op, Operation::Read { path } if path == source));
        let event = h.exchange(command);
        assert!(
            matches!(&event.result, Ok(Payload::File { path, text, revision: got })
            if path == source && text == BASE && got == &revision(BASE))
        );
        h.app.apply_event(event);
        assert_eq!(h.app.documents.len(), 1);
        h.app
            .recovery
            .start(Ok(h.store_path.clone()), &h.app.editor_ctx);
        h.recovery_wait(|app| app.recovery.initialized);
        h.edit_source(DRAFT);
        h.frame(vec![]);
        h.frame(vec![]);
        h.persist();
        h.idle();
        h
    }

    fn root(&self) -> &std::path::Path {
        self.budget.admit(Duration::ZERO);
        self.root.as_ref().unwrap().path()
    }

    fn retain_fixtures(&mut self) {
        if let Some(root) = self.root.take() {
            let _ = root.keep();
        }
        if let Some(root) = self.recovery_root.take() {
            let _ = root.keep();
        }
    }

    fn stop_recovery_until(&mut self, deadline: Instant) -> bool {
        let recovery = std::mem::take(&mut self.app.recovery);
        if !recovery.has_actor() {
            return true;
        }
        // This owner receives only Recovery, never the fixtures or deletion
        // authority. Failed observation retains roots and emits no receipt.
        let cleanup = std::thread::spawn(move || drop(recovery));
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.retain_fixtures();
                return false;
            }
            if cleanup.is_finished() {
                break;
            }
            std::thread::sleep(remaining.min(Duration::from_millis(5)));
        }
        let stopped = cleanup.join().is_ok();
        if !stopped {
            self.retain_fixtures();
        }
        self.recovery_stopped = stopped;
        stopped
    }

    fn quiesce_recovery(&mut self) {
        self.budget.admit(WAIT + WAIT);
        let guard = self.app.recovery_close_guard();
        self.app.recovery.begin_close(guard);
        self.app.recovery.begin_quiescence(true);
        let started = Instant::now();
        loop {
            self.budget.admit(Duration::ZERO);
            // Poll settlement only. recovery_tick would also observe documents.
            let _ = self.app.recovery.poll();
            self.budget.admit(Duration::ZERO);
            assert!(
                started.elapsed() < WAIT,
                "recovery quiescence not observed within 5 s"
            );
            assert!(self.app.recovery.error.is_none());
            if self.app.recovery.closing.as_ref().is_some_and(|close| {
                matches!(
                    close.phase,
                    recovery::ClosePhase::AwaitingConfirmation { .. }
                )
            }) {
                self.recovery_quiescent = true;
                return;
            }
            assert!(
                started.elapsed() < WAIT,
                "recovery quiescence not observed within 5 s"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn reap(&mut self) {
        if self.client.is_some() {
            self.budget.admit(WAIT);
            let result = self.client.take().unwrap().close_and_wait(WAIT);
            if result.is_err() {
                self.retain_fixtures();
            }
            result.expect("owned child cleanup unverified; fixtures retained");
            self.reaped += 1;
            self.budget.admit(Duration::ZERO);
        }
    }

    fn connect(&mut self, controlled: bool) {
        self.idle();
        self.reap();
        self.budget.admit(REQUEST + WAIT);
        let agent = binary(controlled);
        let root = self.root().to_owned();
        self.budget.admit(REQUEST + WAIT);
        let result = Client::spawn_agent_with_cancellation(
            &agent,
            &root,
            false,
            self.budget.cancellation.clone(),
        );
        // A failed Hello can already have launched a child whose asynchronous
        // cleanup is not observable through this API. Preserve its owned root.
        self.client = Some(match result {
            Ok(client) => client,
            Err(error) => {
                self.retain_fixtures();
                panic!("agent connection failed; child cleanup unverified and fixtures retained: {error}");
            }
        });
        self.budget.admit(Duration::ZERO);
        self.connections += 1;
        self.controlled_connection = controlled;
        self.ledger.push(serde_json::json!({"op": "Hello"}));
        if controlled {
            self.controlled_ledger.push("Hello");
        }
        let (worker, commands) = Worker::recording();
        self.commands = commands;
        self.app.worker = Some(worker);
        self.app.generation += 1;
        self.app.state = ConnectionState::Connecting;
        self.app.connecting_form = Some(ConnectForm {
            local_root: self.root().to_string_lossy().into_owned(),
            allow_run: false,
            ..Default::default()
        });
        self.app.apply_event(Event {
            generation: self.app.generation,
            id: 0,
            connected: true,
            result: Ok(self.client.as_ref().unwrap().handshake().clone()),
        });
        self.list();
        assert!(self.app.ready());
        self.trust_off();
    }

    fn trust_off(&self) {
        assert!(!self.app.execution_trusted() && !self.app.language.running);
        assert!(self.app.documents.iter().all(|doc| !doc.has_cjk));
        assert!(!self.app.cjk_seen && !self.app.language.cjk_seen);
    }

    fn next(&self) -> Command {
        self.budget.admit(Duration::ZERO);
        self.commands
            .try_recv()
            .expect("one explicit frontend request required")
    }

    fn idle(&self) {
        self.budget.admit(Duration::ZERO);
        assert!(
            matches!(
                self.commands.try_recv(),
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected)
            ),
            "Copy or idle work dispatched an unsolicited request"
        );
    }

    fn exchange(&mut self, command: Command) -> Event {
        self.budget.admit(REQUEST + WAIT);
        assert!(self.ledger.len() < 8, "fixed per-case request limit");
        let (name, entry) = match &command.op {
            Operation::List { path } => {
                assert!(path.is_empty(), "only ordinary root refresh is admitted");
                ("List", serde_json::json!({"op": "List", "path": path}))
            }
            Operation::Read { path } => {
                assert_eq!(path, self.source, "destination pre-Read/probe forbidden");
                assert!(!self.ledger.iter().any(|entry| entry["op"] == "Read"));
                ("Read", serde_json::json!({"op": "Read", "path": path}))
            }
            Operation::Write {
                path,
                text,
                expected_revision,
            } => {
                assert_eq!(text, DRAFT);
                if path == self.source {
                    assert!(self.controlled_connection && self.source == CONTROLLED_PATH);
                    assert_eq!(expected_revision, &Some(revision(BASE)));
                    self.source_writes += 1;
                    assert_eq!(self.source_writes, 1);
                } else {
                    assert_eq!(path, self.destination);
                    assert_eq!(expected_revision, &None, "Copy Save must require absence");
                    self.destination_writes += 1;
                    assert_eq!(self.destination_writes, 1, "no replay or duplicate Write");
                }
                (
                    "Write",
                    serde_json::json!({"op": "Write", "path": path,
                    "expected_revision": expected_revision, "text_sha256": revision(text)}),
                )
            }
            _ => panic!("Run, Language, search, and unrelated operations forbidden"),
        };
        self.ledger.push(entry);
        if self.controlled_connection {
            self.controlled_ledger.push(name);
        }
        self.idle();
        self.budget.admit(REQUEST + WAIT);
        let client = self.client.as_mut().unwrap();
        let result = client.request(command.op);
        let event = Event {
            generation: self.app.generation,
            id: command.id,
            connected: client.is_connected(),
            result,
        };
        self.budget.admit(Duration::ZERO);
        event
    }

    fn list(&mut self) {
        let command = self.next();
        assert!(matches!(&command.op, Operation::List { path } if path.is_empty()));
        let event = self.exchange(command);
        assert!(event.connected && matches!(&event.result, Ok(Payload::Entries { .. })));
        self.app.apply_event(event);
        self.idle();
    }

    fn edit_source(&mut self, text: &str) {
        self.budget.admit(Duration::ZERO);
        editor_state::commit_selection(
            &self.app.editor_ctx,
            &mut self.app.documents[0],
            text.into(),
            egui::text::CCursorRange {
                primary: egui::text::CCursor {
                    index: 11,
                    prefer_next_row: true,
                },
                secondary: egui::text::CCursor {
                    index: 2,
                    prefer_next_row: false,
                },
            },
        );
    }

    fn snapshot(&self) -> SourceSnapshot {
        let doc = &self.app.documents[0];
        let selection =
            egui::TextEdit::load_state(&self.app.editor_ctx, egui::Id::new(("editor", doc.id)))
                .unwrap()
                .cursor
                .char_range()
                .unwrap();
        SourceSnapshot {
            id: doc.id,
            path: doc.path.clone(),
            text: doc.text.clone(),
            saved_text: doc.saved_text.clone(),
            revision: doc.revision.clone(),
            saving: doc.saving,
            interrupted: doc.interrupted_save.clone(),
            unverifiable: doc.save_outcome_unverifiable,
            edit_version: doc.edit_version,
            cursor: doc.cursor,
            jump_to: doc.jump_to,
            scroll_to: doc.scroll_to,
            has_cjk: doc.has_cjk,
            undo_initialized: doc.undo_initialized,
            selection: (
                selection.primary.index,
                selection.primary.prefer_next_row,
                selection.secondary.index,
                selection.secondary.prefer_next_row,
            ),
        }
    }

    fn recovery_wait(&mut self, done: impl Fn(&CedarApp) -> bool) {
        self.budget.admit(WAIT + WAIT);
        let started = Instant::now();
        loop {
            self.budget.admit(Duration::ZERO);
            self.app.recovery_tick(&self.app.editor_ctx.clone());
            self.budget.admit(Duration::ZERO);
            assert!(
                started.elapsed() < WAIT,
                "recovery observation exceeded 5 s"
            );
            assert!(
                self.app.recovery.error.is_none(),
                "owned recovery must acknowledge drafts"
            );
            if done(&self.app) {
                return;
            }
            assert!(
                started.elapsed() < WAIT,
                "recovery observation exceeded 5 s"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn persist(&mut self) {
        self.budget.admit(WAIT + WAIT);
        let identity = self.app.recovery_workspace().unwrap();
        self.app.recovery_tick(&self.app.editor_ctx.clone());
        self.app.recovery.flush();
        self.recovery_wait(|app| {
            app.recovery.removals_finished()
                && app
                    .documents
                    .iter()
                    .filter(|doc| doc.dirty())
                    .all(|doc| app.recovery.protected(&identity, doc))
        });
    }

    fn source_recovery(&self) -> Vec<u8> {
        self.budget.admit(Duration::ZERO);
        let identity = self.app.recovery_workspace().unwrap();
        fs::read(self.store_path.join(format!(
            "{}.draft",
            record_id(&identity, self.source).unwrap()
        )))
        .unwrap()
    }

    fn source_unchanged(&mut self, before: &SourceSnapshot, recovery: &[u8]) {
        assert_eq!(&self.snapshot(), before);
        self.persist();
        assert_eq!(
            self.source_recovery(),
            recovery,
            "Copy changed original owned recovery bytes"
        );
        self.idle();
    }

    fn frame(&mut self, events: Vec<egui::Event>) {
        self.budget.admit(Duration::ZERO);
        self.clock += 0.25;
        let ctx = self.app.editor_ctx.clone();
        let mut native = eframe::Frame::_new_kittest();
        let mut input = egui::RawInput {
            time: Some(self.clock),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 900.0),
            )),
            events,
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut self.app, &ctx, &mut input);
        let _ = ctx.run(input, |ctx| {
            eframe::App::update(&mut self.app, ctx, &mut native)
        });
        self.budget.admit(Duration::ZERO);
    }

    fn click(&mut self, name: &str) {
        let pos = workspace_access_tests::recorded_rect(&self.app, name).center();
        self.frame(vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            self.frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }]);
        }
    }

    fn begin_copy(&mut self) {
        self.idle();
        self.app.begin_copy_draft();
        assert!(self.app.copy_draft_open());
        self.app.copy_draft_set_path(self.destination);
        assert_eq!(self.app.copy_draft_path(), Some(self.destination));
        self.idle();
    }

    fn create_copy(&mut self, actual_modal: bool) {
        let before = self.ledger.clone();
        if actual_modal {
            self.app.begin_copy_draft();
            self.frame(vec![]);
            self.frame(vec![]);
            assert!(self.app.copy_draft_open());
            self.click("copy_draft_path");
            self.frame(key(egui::Key::A, egui::Modifiers::COMMAND));
            self.frame(vec![egui::Event::Text(self.destination.into())]);
            assert_eq!(self.app.copy_draft_path(), Some(self.destination));
            self.click("copy_draft_confirm");
        } else {
            self.begin_copy();
            self.app.confirm_copy_draft();
            self.idle();
            self.app
                .finish_copy_draft_frame(&self.app.editor_ctx.clone());
        }
        self.idle();
        assert_eq!(self.ledger, before, "Copy itself must send zero requests");
        assert!(!self.app.copy_draft_open());
        assert_eq!(self.app.documents.len(), 2);
        let copy = &self.app.documents[1];
        assert_eq!(copy.path, self.destination);
        assert_eq!(copy.text, DRAFT);
        assert!(copy.saved_text.is_empty() && copy.revision.is_none());
        assert!(copy.dirty() && !copy.saving && !copy.save_outcome_unknown());
        assert_ne!(copy.id, self.app.documents[0].id);
        assert_eq!(self.app.active_document, Some(copy.id));
        // A fresh copy must not inherit the source's pre-copy Undo history.
        self.history(1, false);
        assert_eq!(self.app.documents[1].text, DRAFT);
    }

    fn save_event(&mut self, source: bool) -> (Event, interrupted_save::InterruptedSave) {
        let index = usize::from(!source);
        let id = self.app.documents[index].id;
        self.app.active_document = Some(id);
        self.app.save();
        let command = self.next();
        assert!(matches!(&command.op, Operation::Write { path, .. }
            if path == if source { self.source } else { self.destination }));
        let Some(Job::Save {
            document,
            snapshot,
            submission: Some(token),
        }) = self.app.pending.get(&command.id)
        else {
            panic!("ordinary Save must capture its submission identity")
        };
        assert_eq!(*document, id);
        assert_eq!(snapshot, DRAFT);
        let token = token.clone();
        assert!(self.app.documents[index].saving);
        self.app.save();
        let event = self.exchange(command);
        (event, token)
    }

    fn acknowledge_copy(&mut self) {
        let (event, _) = self.save_event(false);
        assert!(!self.controlled_connection);
        assert!(
            event.connected
                && matches!(&event.result, Ok(Payload::Written { revision: got }) if got == &revision(DRAFT))
        );
        self.app.apply_event(event);
        self.acknowledgements += 1;
        self.list(); // Exactly the ordinary post-ack flat Explorer refresh.
        let copy = &self.app.documents[1];
        assert!(!copy.dirty() && !copy.saving && !copy.save_outcome_unknown());
        assert_eq!(copy.saved_text, DRAFT);
        assert_eq!(copy.revision, Some(revision(DRAFT)));
    }

    fn unknown_save(&mut self, source: bool) {
        let (event, token) = self.save_event(source);
        assert!(self.controlled_connection && event.connected);
        assert!(
            matches!(&event.result, Ok(Payload::Written { revision: got }) if got.len() == 64 && got != &revision(DRAFT))
        );
        assert_eq!(fs::read(self.root().join(COMMITTED)).unwrap(), b"1\n");
        self.app.apply_event(event);
        let doc = &self.app.documents[usize::from(!source)];
        assert_eq!(doc.interrupted_save.as_ref(), Some(&token));
        assert!(doc.dirty() && !doc.saving && !doc.save_outcome_unverifiable);
        assert_eq!(doc.saved_text, if source { BASE } else { "" });
        assert_eq!(doc.revision, source.then(|| revision(BASE)));
        self.app.save(); // Unknown outcome must block replay.
        self.idle();
    }

    fn history(&mut self, index: usize, redo: bool) {
        self.budget.admit(Duration::ZERO);
        let ctx = self.app.editor_ctx.clone();
        let id = self.app.documents[index].id;
        self.clock += 0.25;
        let _ = ctx.run(
            egui::RawInput {
                time: Some(self.clock),
                events: key(
                    egui::Key::Z,
                    if redo {
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
                    } else {
                        egui::Modifiers::COMMAND
                    },
                ),
                ..Default::default()
            },
            |ctx| {
                ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", id))));
                editor_state::history_shortcut(ctx, &mut self.app.documents[index]);
            },
        );
        self.idle();
    }

    fn source_history(&mut self, newer: bool) {
        let before = self.snapshot();
        if newer {
            self.history(0, false);
            assert_eq!(self.app.documents[0].text, DRAFT);
        }
        self.history(0, false);
        assert_eq!(self.app.documents[0].text, BASE);
        self.history(0, true);
        assert_eq!(self.app.documents[0].text, DRAFT);
        if newer {
            self.history(0, true);
            assert_eq!(self.app.documents[0].text, NEWER);
        }
        let after = self.snapshot();
        assert_eq!(after.text, before.text);
        assert_eq!(after.saved_text, before.saved_text);
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.interrupted, before.interrupted);
        assert_eq!(after.unverifiable, before.unverifiable);
        assert_eq!(after.selection, before.selection);
    }

    fn finish(mut self, case: usize) -> serde_json::Value {
        self.app.dismiss_copy_draft();
        self.source_history(case == 3);
        self.persist();
        self.trust_off();
        self.idle();
        self.reap();
        assert_eq!(self.connections, self.reaped);
        self.quiesce_recovery();
        let expected: [&[&str]; 7] = [
            &["Hello", "List", "Read", "Write", "List"],
            &["Hello", "List", "Read", "Write"],
            &["Hello", "List", "Read", "Write"],
            &["Hello", "List", "Read"],
            &["Hello", "List", "Read", "Hello", "List"],
            &[
                "Hello", "List", "Read", "Write", "Hello", "List", "Write", "List",
            ],
            &["Hello", "List", "Read", "Hello", "List", "Write"],
        ];
        let names: Vec<_> = self
            .ledger
            .iter()
            .map(|entry| entry["op"].as_str().unwrap())
            .collect();
        assert_eq!(names, expected[case]);
        if case >= 5 {
            let log = fs::read_to_string(self.root().join(OPERATIONS)).unwrap();
            assert_eq!(log.lines().collect::<Vec<_>>(), self.controlled_ledger);
        }
        assert_eq!(
            fs::read_to_string(self.root().join(self.source)).unwrap(),
            if case == 5 { DRAFT } else { BASE }
        );
        match case {
            0 | 5 | 6 => assert_eq!(
                fs::read_to_string(self.root().join(self.destination)).unwrap(),
                DRAFT
            ),
            1 | 2 => assert_eq!(
                fs::read_to_string(self.root().join(self.destination)).unwrap(),
                EXTERNAL
            ),
            3 | 4 => assert!(!self.root().join(self.destination).exists()),
            _ => unreachable!(),
        }
        let count = |name| names.iter().filter(|actual| **actual == name).count();
        // Keep macro expansion below the crate's ordinary recursion limit.
        // The two small maps form the same exact, flat receipt schema.
        let mut receipt = serde_json::json!({
            "schema": 1, "test": CASES[case], "os": std::env::consts::OS,
            "controlled_peer": case >= 5, "controlled_fault": if case >= 5 { Some("synthetic-wrong-digest") } else { None },
            "execution_trusted": false, "source": self.source, "destination": self.destination,
            "requests": self.ledger.len(), "hello": count("Hello"), "list": count("List"),
            "read": count("Read"), "write": count("Write"), "source_writes": self.source_writes,
            "destination_writes": self.destination_writes,
            "successful_ack_refresh_lists": self.acknowledgements,
            "connections": self.connections, "reaped": self.reaped,
            "operation_ledger": self.ledger, "controlled_operation_ledger": self.controlled_ledger,
            "copy_created": case != 3 && case != 4, "actual_modal_input": case == 0,
            "copy_requests": 0, "destination_prereads": 0, "run": 0, "language": 0,
        });
        let claims = serde_json::json!({
            "absence_precondition_verified": case != 3 && case != 4,
            "source_state_preserved": true, "full_selection_preserved": true,
            "undo_redo_preserved": true, "original_recovery_bytes_preserved": true,
            "owned_recovery_verified": true, "source_disk_verified": true,
            "recovery_quiescence_verified": true, "recovery_workers_started": 1,
            "recovery_workers_stopped": 1,
            "destination_disk_verified": true, "no_replay": true,
            "unknown_source_retained": case == 5, "unknown_copy_retained": case == 6,
            "same_frame_source_edit_rejected": case == 3, "reconnect_rejected": case == 4,
            "presave_destination_race": case == 2, "post_preparation_commit_race": false,
            "shared_watchdog_seconds": 115, "total_budget_seconds": 120,
            "request_admission_seconds": 35,
            "recovery_admission_seconds": 10, "recovery_timeout_seconds": 5,
            "cleanup_timeout_seconds": 5, "owned_children_reaped": true,
            "fixtures_removed": true,
        });
        let serde_json::Value::Object(claims) = claims else {
            unreachable!()
        };
        receipt.as_object_mut().unwrap().extend(claims);
        let identity = self.app.recovery_workspace().unwrap();
        let drafts: Vec<_> = self
            .app
            .documents
            .iter()
            .map(|doc| {
                (
                    doc.path.clone(),
                    doc.dirty(),
                    doc.text.clone(),
                    doc.saved_text.clone(),
                    doc.revision.clone(),
                )
            })
            .collect();
        // The production Paused fence prevents further actor I/O, including
        // from Drop. Observe termination before reopening the owned store.
        assert!(self.recovery_quiescent);
        self.budget.admit(WAIT);
        let deadline = Instant::now() + self.budget.cleanup_allowance();
        assert!(
            self.stop_recovery_until(deadline),
            "recovery cleanup unverified; fixtures retained"
        );
        assert!(self.recovery_stopped);
        self.budget.admit(Duration::ZERO);
        drop(std::mem::replace(&mut self.app, CedarApp::empty()));
        {
            self.budget.admit(Duration::ZERO);
            let store = Store::open(&self.store_path).unwrap();
            self.budget.admit(Duration::ZERO);
            let listing = store.list().unwrap();
            assert!(listing.issues.is_empty());
            assert_eq!(
                listing.drafts.len(),
                drafts.iter().filter(|draft| draft.1).count()
            );
            for (path, dirty, text, base, revision) in drafts {
                let id = record_id(&identity, &path).unwrap();
                if dirty {
                    self.budget.admit(Duration::ZERO);
                    let draft = store.read(&id).unwrap();
                    assert_eq!(draft.text, text);
                    assert_eq!(draft.base_text, base);
                    assert_eq!(draft.base_revision, revision);
                } else {
                    assert!(!listing.drafts.iter().any(|draft| draft.id == id));
                }
            }
        }
        let root = self.root.take().unwrap();
        let path = root.path().to_owned();
        self.budget.admit(Duration::ZERO);
        root.close().expect("owned workspace cleanup must succeed");
        self.budget.admit(Duration::ZERO);
        assert!(!path.exists());
        let recovery = self.recovery_root.take().unwrap();
        let path = recovery.path().to_owned();
        self.budget.admit(Duration::ZERO);
        recovery
            .close()
            .expect("owned recovery cleanup must succeed");
        self.budget.admit(Duration::ZERO);
        assert!(!path.exists());
        self.budget.admit(Duration::ZERO);
        receipt
    }
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers,
        })
        .collect()
}

#[test]
#[ignore = "requires explicit CEDAR_COPY_DRAFT_AGENT_BIN and CEDAR_INTERRUPTED_SAVE_AGENT_BIN"]
fn copy_to_new_draft_is_local_and_explicit_save_requires_absence() {
    let mut budget = Budget::new();
    let mut receipts = Vec::with_capacity(CASES.len());
    for case in 0..CASES.len() {
        let mut h = Harness::new(&budget, case);
        if case == 5 {
            h.unknown_save(true);
            h.persist();
        }
        if case == 6 {
            h.connect(true);
        }
        let mut source = h.snapshot();
        let mut recovery = h.source_recovery();
        match case {
            0 | 1 | 2 | 5 | 6 => {
                h.create_copy(case == 0);
                h.source_unchanged(&source, &recovery);
                if case != 1 {
                    assert!(!h.root().join(h.destination).exists());
                }
                match case {
                    0 => h.acknowledge_copy(),
                    1 | 2 => {
                        if case == 2 {
                            fs::write(h.root().join(h.destination), EXTERNAL).unwrap();
                        }
                        let (event, _) = h.save_event(false);
                        assert!(
                            event.connected
                                && event.result.as_ref().unwrap_err().starts_with("conflict:")
                        );
                        h.app.apply_event(event);
                        let copy = &h.app.documents[1];
                        assert_eq!(copy.text, DRAFT);
                        assert!(copy.saved_text.is_empty() && copy.revision.is_none());
                        assert!(copy.dirty() && !copy.saving && !copy.save_outcome_unknown());
                        h.idle();
                    }
                    5 => {
                        h.connect(false);
                        h.acknowledge_copy();
                    }
                    6 => h.unknown_save(false),
                    _ => unreachable!(),
                }
            }
            3 => {
                h.begin_copy();
                h.app.confirm_copy_draft();
                // Same-frame input changes the captured source after confirmation.
                h.edit_source(NEWER);
                source = h.snapshot();
                h.persist();
                recovery = h.source_recovery();
                h.app.finish_copy_draft_frame(&h.app.editor_ctx.clone());
                assert_eq!(h.app.documents.len(), 1);
                h.idle();
            }
            4 => {
                h.begin_copy();
                h.app.confirm_copy_draft();
                let generation = h.app.generation;
                h.connect(false);
                assert_ne!(h.app.generation, generation);
                h.app.finish_copy_draft_frame(&h.app.editor_ctx.clone());
                assert_eq!(h.app.documents.len(), 1);
                h.idle();
            }
            _ => unreachable!(),
        }
        h.source_unchanged(&source, &recovery);
        receipts.push(h.finish(case));
    }
    assert_eq!(receipts.len(), 7);
    assert_eq!(
        receipts
            .iter()
            .map(|r| r["requests"].as_u64().unwrap())
            .sum::<u64>(),
        35
    );
    assert_eq!(
        receipts
            .iter()
            .map(|r| r["connections"].as_u64().unwrap())
            .sum::<u64>(),
        10
    );
    assert_eq!(
        receipts
            .iter()
            .map(|r| r["reaped"].as_u64().unwrap())
            .sum::<u64>(),
        10
    );
    budget.stop();
    // A partial or timed-out suite never prints success-shaped case receipts.
    for receipt in receipts {
        println!("copy_draft_acceptance {receipt}");
    }
}
