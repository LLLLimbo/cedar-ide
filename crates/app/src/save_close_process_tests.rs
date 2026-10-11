//! One acknowledged Save-and-close target through real stdio agents on both OSes.
//! Six fixed cases share one watchdog, including recovery and child cleanup.
//! Only the wrong-digest case uses the existing marked, nonshipping peer.
//! Cancellation revokes close consent; it cannot retract the submitted Write.
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
const ACTIVE_BUDGET: Duration = Duration::from_secs(115);
const REQUEST: Duration = Duration::from_secs(30);
const WAIT: Duration = Duration::from_secs(5);
const SOURCE: &str = "source Ω.txt";
const CONTROLLED_PATH: &str = "draft.txt";
const OTHER: &str = "other café.txt";
const BASE: &str = "original α\nsecond café\nthird line\n";
const DRAFT: &str = "unsaved draft β\nsecond café\nthird line\n";
const NEWER: &str = "newer source Ω\nsecond café\nthird line\n";
const EXTERNAL: &str = "independent on-disk change é\n";
const OTHER_DRAFT: &str = "unrelated unsaved café\nsecond line\n";
const MARKER: &str = ".cedar-interrupted-save-validation";
const MARKER_TEXT: &[u8] = b"cedar-interrupted-save-validation-v1\nsynthetic-data-only\n";
const ACK_MODE: &str = ".cedar-synthetic-save-ack-mode";
const OPERATIONS: &str = ".cedar-interrupted-save-operations";
const COMMITTED: &str = ".cedar-interrupted-save-committed";
const CASES: [&str; 6] = [
    "normal_existing_valid_close",
    "normal_new_file_valid_close",
    "normal_conflict_retained",
    "controlled_wrong_digest_unknown_retained",
    "normal_cancel_after_dispatch_retained",
    "normal_newer_edits_before_ack_retained",
];

fn revision(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn binary(controlled: bool) -> PathBuf {
    let variable = if controlled {
        "CEDAR_INTERRUPTED_SAVE_AGENT_BIN"
    } else {
        "CEDAR_SAVE_CLOSE_AGENT_BIN"
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
struct EditorSnapshot {
    id: u64,
    path: String,
    text: String,
    edit_version: u64,
    cursor: (usize, usize),
    jump_to: Option<usize>,
    scroll_to: Option<usize>,
    has_cjk: bool,
    undo_initialized: bool,
    // CCursor equality omits affinity: inspect all four endpoint fields.
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
    case: usize,
    target: u64,
    other: u64,
    ledger: Vec<serde_json::Value>,
    controlled_ledger: Vec<&'static str>,
    connections: usize,
    reaped: usize,
    writes: usize,
    acknowledgements: usize,
    recovery_quiescent: bool,
    recovery_stopped: bool,
    clock: f64,
}

impl Drop for Harness<'_> {
    fn drop(&mut self) {
        // Recovery teardown and the sole live child share the final allowance.
        // This is an observation bound, not a claim of kernel I/O cancellation.
        let deadline = Instant::now() + self.budget.cleanup_allowance();
        let agent_stopped = self.client.take().is_none_or(|client| {
            client
                .close_and_wait(deadline.saturating_duration_since(Instant::now()))
                .is_ok()
        });
        let recovery_stopped = self.stop_recovery_until(deadline);
        if !agent_stopped || !recovery_stopped {
            self.retain_fixtures();
            eprintln!("save_close_acceptance cleanup unverified; owned fixtures retained; external test process deadline remains the only bound for blocked teardown");
        }
    }
}

impl<'a> Harness<'a> {
    fn new(budget: &'a Budget, case: usize) -> Self {
        budget.admit(REQUEST + WAIT);
        let root = tempfile::Builder::new()
            .prefix("cedar-save-close Ω-")
            .tempdir()
            .unwrap();
        let source = if case == 3 { CONTROLLED_PATH } else { SOURCE };
        if case != 1 {
            budget.admit(Duration::ZERO);
            fs::write(root.path().join(source), BASE).unwrap();
        }
        if case == 3 {
            budget.admit(Duration::ZERO);
            fs::write(root.path().join(MARKER), MARKER_TEXT).unwrap();
            budget.admit(Duration::ZERO);
            fs::write(root.path().join(ACK_MODE), b"synthetic-wrong-digest\n").unwrap();
        }
        budget.admit(Duration::ZERO);
        let recovery_root = tempfile::Builder::new()
            .prefix("cedar-save-close-recovery-")
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
            case,
            target: 0,
            other: 0,
            ledger: Vec::new(),
            controlled_ledger: Vec::new(),
            connections: 0,
            reaped: 0,
            writes: 0,
            acknowledgements: 0,
            recovery_quiescent: false,
            recovery_stopped: false,
            clock: 0.0,
        };
        h.connect();
        if case == 1 {
            h.new_draft(source);
            assert!(!h.root().join(source).exists());
        } else {
            h.app.open(source.into(), None);
            let command = h.next();
            let event = h.exchange(command);
            assert!(matches!(&event.result,
                Ok(Payload::File { path, text, revision: got })
                if path == source && text == BASE && got == &revision(BASE)));
            h.app.apply_event(event);
        }
        assert_eq!(h.app.documents.len(), 1);
        h.target = h.app.documents[0].id;
        h.edit(h.target, DRAFT);
        h.new_draft(OTHER);
        h.other = h.app.active_document.unwrap();
        assert_ne!(h.target, h.other);
        h.edit(h.other, OTHER_DRAFT);
        assert_eq!(h.app.active_document, Some(h.other));
        h.budget.admit(WAIT + WAIT);
        h.app
            .recovery
            .start(Ok(h.store_path.clone()), &h.app.editor_ctx);
        h.recovery_wait(|app| app.recovery.initialized);
        h.persist();
        h.trust_off();
        h.idle();
        h
    }

    fn root(&self) -> &std::path::Path {
        self.budget.admit(Duration::ZERO);
        self.root.as_ref().unwrap().path()
    }

    fn controlled(&self) -> bool {
        self.case == 3
    }

    fn target_doc(&self) -> &Document {
        self.app
            .documents
            .iter()
            .find(|doc| doc.id == self.target)
            .expect("the stable target must remain open")
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
        // The disposer owns only Recovery, never either fixture root. A timeout
        // retains the roots and prevents every success-shaped receipt.
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
            // Settlement only: recovery_tick would additionally observe tabs.
            let _ = self.app.recovery.poll();
            self.budget.admit(Duration::ZERO);
            assert!(started.elapsed() < WAIT, "recovery quiescence exceeded 5 s");
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
            assert!(started.elapsed() < WAIT, "recovery quiescence exceeded 5 s");
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

    fn connect(&mut self) {
        self.idle();
        assert!(self.client.is_none() && self.connections == 0);
        self.budget.admit(REQUEST + WAIT);
        let agent = binary(self.controlled());
        let root = self.root().to_owned();
        self.budget.admit(REQUEST + WAIT);
        let result = Client::spawn_agent_with_cancellation(
            &agent,
            &root,
            false,
            self.budget.cancellation.clone(),
        );
        // Failed Hello can have created a child without returning its owner.
        // This API cannot prove that asynchronous cleanup, so retain both roots.
        self.client = Some(match result {
            Ok(client) => client,
            Err(error) => {
                self.retain_fixtures();
                panic!("agent connection failed; child cleanup unverified and fixtures retained: {error}");
            }
        });
        self.budget.admit(Duration::ZERO);
        self.connections = 1;
        self.ledger
            .push(serde_json::json!({"session": 1, "op": "Hello"}));
        if self.controlled() {
            self.controlled_ledger.push("Hello");
        }
        let (worker, commands) = Worker::recording();
        self.commands = commands;
        self.app.worker = Some(worker);
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
            "Save-and-close or idle work dispatched an unsolicited request"
        );
    }

    fn exchange(&mut self, command: Command) -> Event {
        self.budget.admit(REQUEST + WAIT);
        assert!(self.ledger.len() < 5, "fixed per-case request limit");
        let (name, entry) = match &command.op {
            Operation::List { path } => {
                assert!(
                    path.is_empty(),
                    "only the ordinary root refresh is admitted"
                );
                (
                    "List",
                    serde_json::json!({"session": 1, "op": "List", "path": path}),
                )
            }
            Operation::Read { path } => {
                assert_ne!(self.case, 1, "new-file Save has no pre-Read");
                assert_eq!(path, self.source);
                assert!(!self.ledger.iter().any(|entry| entry["op"] == "Read"));
                (
                    "Read",
                    serde_json::json!({"session": 1, "op": "Read", "path": path}),
                )
            }
            Operation::Write {
                path,
                text,
                expected_revision,
            } => {
                assert_eq!(
                    path, self.source,
                    "the unrelated active tab must never be written"
                );
                assert_eq!(text, DRAFT);
                assert_eq!(expected_revision, &(self.case != 1).then(|| revision(BASE)));
                self.writes += 1;
                assert_eq!(self.writes, 1, "duplicate or replayed Write forbidden");
                (
                    "Write",
                    serde_json::json!({"session": 1, "op": "Write", "path": path,
                    "expected_revision": expected_revision, "text_sha256": revision(text)}),
                )
            }
            _ => panic!("Run, Language, search, and unrelated operations forbidden"),
        };
        self.ledger.push(entry);
        if self.controlled() {
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

    fn new_draft(&mut self, path: &str) {
        self.budget.admit(Duration::ZERO);
        self.app.new_file = true;
        self.app.new_path = path.into();
        self.app.create_new_file_draft();
        assert!(!self.app.new_file);
        let doc = self.app.active().unwrap();
        assert_eq!(doc.path, path);
        assert!(doc.text.is_empty() && doc.saved_text.is_empty() && doc.revision.is_none());
        self.idle();
    }

    fn edit(&mut self, id: u64, text: &str) {
        self.budget.admit(Duration::ZERO);
        let doc = self
            .app
            .documents
            .iter_mut()
            .find(|doc| doc.id == id)
            .unwrap();
        editor_state::commit_selection(
            &self.app.editor_ctx,
            doc,
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

    fn snapshot(&self, id: u64) -> EditorSnapshot {
        let doc = self.app.documents.iter().find(|doc| doc.id == id).unwrap();
        let selection =
            egui::TextEdit::load_state(&self.app.editor_ctx, egui::Id::new(("editor", id)))
                .unwrap()
                .cursor
                .char_range()
                .unwrap();
        EditorSnapshot {
            id,
            path: doc.path.clone(),
            text: doc.text.clone(),
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

    fn other_state(&self) -> (EditorSnapshot, String) {
        let doc = self
            .app
            .documents
            .iter()
            .find(|doc| doc.id == self.other)
            .unwrap();
        (self.snapshot(self.other), format!("{doc:?}"))
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

    fn recovery_bytes(&self, path: &str) -> Vec<u8> {
        self.budget.admit(Duration::ZERO);
        let identity = self.app.recovery_workspace().unwrap();
        fs::read(
            self.store_path
                .join(format!("{}.draft", record_id(&identity, path).unwrap())),
        )
        .unwrap()
    }

    fn final_frame(&mut self) {
        self.budget.admit(Duration::ZERO);
        self.app
            .finish_save_close_tab_frame(&self.app.editor_ctx.clone());
        self.budget.admit(Duration::ZERO);
    }

    fn dispatch(&mut self) -> (Event, interrupted_save::InterruptedSave) {
        self.idle();
        assert_eq!(self.app.active_document, Some(self.other));
        self.app.close_tab(self.target);
        assert!(matches!(self.app.confirm, Some(Confirm::CloseTab(id)) if id == self.target));
        self.app.queue_save_close_tab();
        self.app.queue_save_close_tab();
        self.idle(); // Confirmation only queues; the end-of-frame guard dispatches.
        self.final_frame();
        let command = self.next();
        assert!(matches!(&command.op, Operation::Write { path, .. } if path == self.source));
        let Some(Job::Save {
            document,
            snapshot,
            submission: Some(token),
        }) = self.app.pending.get(&command.id)
        else {
            panic!("ordinary Save must capture the stable document and submission identity")
        };
        assert_eq!(*document, self.target);
        assert_eq!(snapshot, DRAFT);
        let token = token.clone();
        assert!(self.target_doc().saving);
        assert_eq!(self.app.active_document, Some(self.other));
        self.app.queue_save_close_tab();
        self.final_frame();
        self.idle();
        let event = self.exchange(command);
        (event, token)
    }

    fn history(&mut self, redo: bool) {
        self.budget.admit(Duration::ZERO);
        let index = self
            .app
            .documents
            .iter()
            .position(|doc| doc.id == self.target)
            .unwrap();
        let ctx = self.app.editor_ctx.clone();
        let id = self.target;
        self.clock += 0.25;
        let _ = ctx.run(
            egui::RawInput {
                time: Some(self.clock),
                events: vec![egui::Event::Key {
                    key: egui::Key::Z,
                    physical_key: Some(egui::Key::Z),
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
                ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", id))));
                editor_state::history_shortcut(ctx, &mut self.app.documents[index]);
            },
        );
        self.idle();
    }

    fn retained_history(&mut self) {
        assert!(self.case >= 2);
        let before = self.snapshot(self.target);
        let baseline = (
            self.target_doc().saved_text.clone(),
            self.target_doc().revision.clone(),
            self.target_doc().interrupted_save.clone(),
            self.target_doc().save_outcome_unverifiable,
        );
        if self.case == 5 {
            self.history(false);
            assert_eq!(self.target_doc().text, DRAFT);
        }
        self.history(false);
        assert_eq!(self.target_doc().text, BASE);
        self.history(true);
        assert_eq!(self.target_doc().text, DRAFT);
        if self.case == 5 {
            self.history(true);
            assert_eq!(self.target_doc().text, NEWER);
        }
        let after = self.snapshot(self.target);
        assert_eq!(after.text, before.text);
        assert_eq!(after.selection, before.selection);
        assert_eq!(
            (
                self.target_doc().saved_text.clone(),
                self.target_doc().revision.clone(),
                self.target_doc().interrupted_save.clone(),
                self.target_doc().save_outcome_unverifiable
            ),
            baseline
        );
        self.final_frame();
        assert_eq!(
            self.app.documents.len(),
            2,
            "history must not resurrect cancelled close consent"
        );
        self.idle();
    }

    fn finish(
        mut self,
        other_before: (EditorSnapshot, String),
        other_recovery: Vec<u8>,
    ) -> serde_json::Value {
        self.app.cancel_tab_close();
        if self.case >= 2 {
            self.retained_history();
        }
        assert_eq!(self.other_state(), other_before);
        assert_eq!(self.app.active_document, Some(self.other));
        self.persist();
        assert_eq!(self.recovery_bytes(OTHER), other_recovery);
        self.trust_off();
        self.idle();
        self.reap();
        assert_eq!(self.connections, 1);
        assert_eq!(self.reaped, 1);
        self.quiesce_recovery();

        let expected: [&[&str]; 6] = [
            &["Hello", "List", "Read", "Write", "List"],
            &["Hello", "List", "Write", "List"],
            &["Hello", "List", "Read", "Write"],
            &["Hello", "List", "Read", "Write"],
            &["Hello", "List", "Read", "Write", "List"],
            &["Hello", "List", "Read", "Write", "List"],
        ];
        let names: Vec<_> = self
            .ledger
            .iter()
            .map(|entry| entry["op"].as_str().unwrap())
            .collect();
        assert_eq!(names, expected[self.case]);
        assert_eq!(self.writes, 1);
        if self.controlled() {
            let log = fs::read_to_string(self.root().join(OPERATIONS)).unwrap();
            assert_eq!(log.lines().collect::<Vec<_>>(), self.controlled_ledger);
            assert_eq!(fs::read(self.root().join(COMMITTED)).unwrap(), b"1\n");
        }
        let disk_text = fs::read_to_string(self.root().join(self.source)).unwrap();
        assert_eq!(disk_text, if self.case == 2 { EXTERNAL } else { DRAFT });
        assert!(
            !self.root().join(OTHER).exists(),
            "the unrelated draft must never reach disk"
        );
        let closed = self.case < 2;
        let retained = self.app.documents.iter().find(|doc| doc.id == self.target);
        assert_eq!(retained.is_none(), closed);
        assert_eq!(self.app.documents.len(), if closed { 1 } else { 2 });
        let source_recovery_state = match self.case {
            0 | 1 | 4 => "removed_after_valid_ack",
            2 | 3 => "retained_original_base",
            5 => "retained_newer_with_submitted_base",
            _ => unreachable!(),
        };
        let dirty_source = matches!(self.case, 2 | 3 | 5);
        let count = |name| names.iter().filter(|actual| **actual == name).count();
        // Split the maps to stay below the crate's ordinary macro recursion limit.
        let mut receipt = serde_json::json!({
            "schema": 1, "test": CASES[self.case], "os": std::env::consts::OS,
            "controlled_peer": self.controlled(),
            "controlled_fault": if self.controlled() { Some("synthetic-wrong-digest") } else { None },
            "execution_trusted": false, "source": self.source, "other": OTHER,
            "target_document": self.target, "other_document": self.other,
            "active_document_before": self.other, "active_document_after": self.app.active_document,
            "requests": self.ledger.len(), "hello": count("Hello"), "list": count("List"),
            "read": count("Read"), "write": count("Write"),
            "connections": self.connections, "reaped": self.reaped,
            "successful_ack_refresh_lists": self.acknowledgements,
            "operation_ledger": self.ledger, "controlled_operation_ledger": self.controlled_ledger,
        });
        let outcome = serde_json::json!({
            "session_ledger": [{"session": 1, "peer": if self.controlled() { "controlled" } else { "normal-release" }, "hello": 1, "reaped": true}],
            "source_initial_sha256": (self.case != 1).then(|| revision(BASE)),
            "submitted_sha256": revision(DRAFT), "source_final_sha256": revision(&disk_text),
            "source_final_text": disk_text,
            "ack_class": match self.case { 2 => "conflict", 3 => "wrong-digest", _ => "valid" },
            "target_closed": closed, "target_retained": !closed,
            "target_text_after": retained.map(|doc| doc.text.clone()),
            "target_saved_text_after": retained.map(|doc| doc.saved_text.clone()),
            "target_revision_after": retained.and_then(|doc| doc.revision.clone()),
        });
        let claims = serde_json::json!({
            "close_before_ack": false, "new_file_absence_precondition": self.case == 1,
            "other_state_preserved": true, "other_recovery_bytes_preserved": true,
            "other_disk_absent": true, "full_selection_preserved": (!closed).then_some(true),
            "undo_redo_preserved": (!closed).then_some(true),
            "source_recovery_state": source_recovery_state,
            "source_recovery_text_sha256": dirty_source.then(|| revision(if self.case == 5 { NEWER } else { DRAFT })),
            "source_recovery_base_sha256": dirty_source.then(|| revision(if self.case == 5 { DRAFT } else { BASE })),
            "source_recovery_revision": dirty_source.then(|| revision(if self.case == 5 { DRAFT } else { BASE })),
            "recovery_quiescence_verified": true, "recovery_workers_started": 1,
            "recovery_workers_stopped": 1, "owned_recovery_verified": true,
        });
        let bounds = serde_json::json!({
            "no_replay": true, "no_workspace_file_delete": true, "run": 0, "language": 0,
            "shared_watchdog_seconds": 115, "total_budget_seconds": 120,
            "request_admission_seconds": 35, "recovery_admission_seconds": 10,
            "recovery_timeout_seconds": 5, "cleanup_timeout_seconds": 5,
            "owned_children_reaped": true, "fixtures_removed": true,
            "cancelled_after_dispatch": self.case == 4, "newer_edits_before_ack": self.case == 5,
            "baseline_adopted": !matches!(self.case, 2 | 3),
            "wrong_digest_unknown_retained": self.case == 3, "source_disk_verified": true,
        });
        for details in [outcome, claims, bounds] {
            let serde_json::Value::Object(details) = details else {
                unreachable!()
            };
            receipt.as_object_mut().unwrap().extend(details);
        }
        let identity = self.app.recovery_workspace().unwrap();
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
            assert_eq!(listing.drafts.len(), if dirty_source { 2 } else { 1 });
            self.budget.admit(Duration::ZERO);
            let other = store.read(&record_id(&identity, OTHER).unwrap()).unwrap();
            assert_eq!(other.text, OTHER_DRAFT);
            assert!(other.base_text.is_empty() && other.base_revision.is_none());
            let source_id = record_id(&identity, self.source).unwrap();
            if dirty_source {
                self.budget.admit(Duration::ZERO);
                let source = store.read(&source_id).unwrap();
                assert_eq!(source.text, if self.case == 5 { NEWER } else { DRAFT });
                assert_eq!(source.base_text, if self.case == 5 { DRAFT } else { BASE });
                assert_eq!(
                    source.base_revision,
                    Some(revision(if self.case == 5 { DRAFT } else { BASE }))
                );
            } else {
                assert!(!listing.drafts.iter().any(|draft| draft.id == source_id));
            }
        }
        let root = self.root.take().unwrap();
        let path = root.path().to_owned();
        self.budget.admit(Duration::ZERO);
        root.close()
            .expect("owned fixture workspace cleanup must succeed");
        self.budget.admit(Duration::ZERO);
        assert!(!path.exists());
        let recovery = self.recovery_root.take().unwrap();
        let path = recovery.path().to_owned();
        self.budget.admit(Duration::ZERO);
        recovery
            .close()
            .expect("owned fixture recovery cleanup must succeed");
        self.budget.admit(Duration::ZERO);
        assert!(!path.exists());
        receipt
    }
}

#[test]
#[ignore = "requires explicit CEDAR_SAVE_CLOSE_AGENT_BIN and CEDAR_INTERRUPTED_SAVE_AGENT_BIN"]
fn save_and_close_is_acknowledged_bounded_and_targeted() {
    let mut budget = Budget::new();
    let mut receipts = Vec::with_capacity(CASES.len());
    for case in 0..CASES.len() {
        let mut h = Harness::new(&budget, case);
        let other = h.other_state();
        let other_recovery = h.recovery_bytes(OTHER);
        let protected_source = h.recovery_bytes(h.source);
        if case == 2 {
            // A deterministic external change creates an ordinary Write conflict.
            // It does not exercise the narrower post-preparation commit race.
            fs::write(h.root().join(h.source), EXTERNAL).unwrap();
        }
        let (event, token) = h.dispatch();
        assert!(event.connected);
        match case {
            2 => assert!(event.result.as_ref().unwrap_err().starts_with("conflict:")),
            3 => assert!(
                matches!(&event.result, Ok(Payload::Written { revision: got })
                if got.len() == 64 && got != &revision(DRAFT)
                && got.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
            ),
            _ => assert!(
                matches!(&event.result, Ok(Payload::Written { revision: got }) if got == &revision(DRAFT))
            ),
        }
        assert_eq!(
            h.app.documents.len(),
            2,
            "dispatch and even a received but undelivered ack cannot close the tab"
        );
        assert!(h.target_doc().saving);
        if case == 4 {
            let pending = h.app.pending.len();
            h.app.cancel_tab_close();
            assert!(h.target_doc().saving);
            assert_eq!(
                h.app.pending.len(),
                pending,
                "Keep editing cannot cancel the submitted Write"
            );
            h.final_frame();
            h.idle();
        }
        if case == 5 {
            h.edit(h.target, NEWER);
            h.persist();
        }
        let before_ack = h.snapshot(h.target);
        h.app.apply_event(event);
        assert!(!h.target_doc().saving);
        assert_eq!(
            h.snapshot(h.target),
            before_ack,
            "ack may adopt a baseline but must preserve the exact editor state"
        );
        assert_eq!(
            h.app.documents.len(),
            2,
            "ack only stages close for the final frame guard"
        );
        match case {
            2 => {
                assert_eq!(h.target_doc().saved_text, BASE);
                assert_eq!(h.target_doc().revision, Some(revision(BASE)));
                assert!(h.target_doc().dirty() && !h.target_doc().save_outcome_unknown());
            }
            3 => {
                assert_eq!(h.target_doc().saved_text, BASE);
                assert_eq!(h.target_doc().revision, Some(revision(BASE)));
                assert_eq!(h.target_doc().interrupted_save.as_ref(), Some(&token));
                assert!(h.target_doc().dirty() && !h.target_doc().save_outcome_unverifiable);
            }
            _ => {
                assert_eq!(h.target_doc().saved_text, DRAFT);
                assert_eq!(h.target_doc().revision, Some(revision(DRAFT)));
                assert_eq!(h.target_doc().dirty(), case == 5);
                assert!(!h.target_doc().save_outcome_unknown());
                h.acknowledgements += 1;
                h.list(); // Exactly the ordinary successful-ack flat Explorer refresh.
            }
        }
        h.final_frame();
        h.final_frame();
        h.idle();
        if case < 2 {
            assert_eq!(h.app.documents.len(), 1);
            assert!(!h.app.documents.iter().any(|doc| doc.id == h.target));
        } else {
            assert_eq!(h.snapshot(h.target), before_ack);
            assert_eq!(h.target_doc().text, if case == 5 { NEWER } else { DRAFT });
            assert_eq!(h.app.documents.len(), 2);
            if case == 3 {
                h.app.cancel_tab_close();
                h.app.active_document = Some(h.target);
                h.app.save(); // Unknown outcome blocks an explicit attempted replay.
                h.app.active_document = Some(h.other);
                h.idle();
            }
            h.persist();
            if matches!(case, 2 | 3) {
                assert_eq!(
                    h.recovery_bytes(h.source),
                    protected_source,
                    "refused acknowledgement must preserve original owned recovery bytes"
                );
            }
        }
        assert_eq!(h.other_state(), other);
        assert_eq!(h.app.active_document, Some(h.other));
        receipts.push(h.finish(other, other_recovery));
    }
    assert_eq!(receipts.len(), 6);
    for (key, expected) in [
        ("requests", 27),
        ("hello", 6),
        ("list", 10),
        ("read", 5),
        ("write", 6),
        ("connections", 6),
        ("reaped", 6),
    ] {
        assert_eq!(
            receipts
                .iter()
                .map(|receipt| receipt[key].as_u64().unwrap())
                .sum::<u64>(),
            expected
        );
    }
    budget.stop();
    // A partial, timed-out, or incompletely disposed suite prints no receipts.
    for receipt in receipts {
        println!("save_close_acceptance {receipt}");
    }
}
