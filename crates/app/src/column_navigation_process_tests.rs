//! Finite line:column proof through the normal shipping trust-off agent.
//! Generated sources, actual Ctrl+G input frames, and a strict ordered ledger.
//! No Save, Run, language session, recovery actor or alternate remote protocol.
use super::*;
use cedar_client::{Client, ConnectionCancellation};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

const BUDGET: Duration = Duration::from_secs(60);
const TOTAL: Duration = Duration::from_secs(65);
const REQUEST: Duration = Duration::from_secs(30);
const CLEANUP: Duration = Duration::from_secs(5);
const FIRST: &str = "coordinates 😀/CRLF café.txt";
const SECOND: &str = "coordinates 😀/LF Ω.txt";
const FIRST_BASE: &str = "\tα😀e\u{301} café\r\nβ\tΩ\r\n\r\nlast\r\n";
const SECOND_BASE: &str = "\tα😀e\u{301} café\nβ\tΩ\n\nlast\n";
const FIRST_DRAFT: &str = "\tα😀e\u{301} café\r\nβ\tΩ\r\n\r\ndraft\r\n";
const SECOND_DRAFT: &str = "\tα😀e\u{301} café\nβ\tΩ\n\ndraft\n";
const CASES: [&str; 14] = [
    "unicode_crlf_coordinates",
    "unicode_lf_coordinates",
    "plain_line_compatibility",
    "invalid_coordinates_preserve_state",
    "cancel_preserves_full_selection",
    "same_location_preserves_history",
    "dirty_undo_redo",
    "stale_edit_before_release",
    "stale_edit_after_release",
    "stale_tab_before_release",
    "stale_navigation_after_release",
    "oversized_input_refused",
    "stale_reconnect",
    "history_after_reconnect",
];
type Check<T> = Result<T, &'static str>;
type Selection = egui::text::CCursorRange;
type SelectionKey = (usize, bool, usize, bool);
type HistoryEntry = (u64, u64, u64, SelectionKey);
type HistorySnapshot = [Vec<HistoryEntry>; 2];

fn require(condition: bool, message: &'static str) -> Check<()> {
    if condition {
        Ok(())
    } else {
        Err(message)
    }
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

// egui CCursor equality intentionally ignores affinity. Acceptance must not.
fn selection_key(selection: Selection) -> SelectionKey {
    (
        selection.primary.index,
        selection.primary.prefer_next_row,
        selection.secondary.index,
        selection.secondary.prefer_next_row,
    )
}

fn range(primary: usize, secondary: usize) -> Selection {
    Selection {
        primary: egui::text::CCursor {
            index: primary,
            prefer_next_row: true,
        },
        secondary: egui::text::CCursor {
            index: secondary,
            prefer_next_row: false,
        },
    }
}

#[derive(Default)]
struct Ledger {
    ordered: Vec<serde_json::Value>,
    lists: Vec<String>,
    reads: Vec<String>,
    writes: u32,
    other: u32,
}

impl Ledger {
    fn record(&mut self, operation: &Operation, session: u32) {
        self.ordered.push(match operation {
            Operation::List { path } => {
                serde_json::json!({"session": session, "op": "List", "path": path})
            }
            Operation::Read { path } => {
                serde_json::json!({"session": session, "op": "Read", "path": path})
            }
            Operation::Write { .. } => serde_json::json!({"session": session, "op": "Write"}),
            _ => serde_json::json!({"session": session, "op": "Other"}),
        });
        match operation {
            Operation::List { path } => self.lists.push(path.clone()),
            Operation::Read { path } => self.reads.push(path.clone()),
            Operation::Write { .. } => self.writes += 1,
            _ => self.other += 1,
        }
    }

    fn counts(&self) -> (usize, usize, u32, u32) {
        (self.lists.len(), self.reads.len(), self.writes, self.other)
    }
}

struct Harness {
    app: CedarApp,
    commands: Receiver<Command>,
    client: Option<Client>,
    root: Option<tempfile::TempDir>,
    agent: PathBuf,
    started: Instant,
    clock: f64,
    ledger: Ledger,
    sources: Vec<(String, [u8; 32])>,
    connections: u32,
    reaped: u32,
    cases: Vec<&'static str>,
    cleanup_uncertain: bool,
    cancellation: ConnectionCancellation,
    watchdog: Option<(Sender<()>, std::thread::JoinHandle<bool>)>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some((stop, watchdog)) = self.watchdog.take() {
            let _ = stop.send(());
            if watchdog.join().is_err() {
                self.cleanup_uncertain = true;
            }
        }
        if let Some(client) = self.client.take() {
            let allowance = TOTAL.saturating_sub(self.started.elapsed()).min(CLEANUP);
            if allowance.is_zero() || client.close_and_wait(allowance).is_err() {
                self.cleanup_uncertain = true;
            } else {
                self.reaped += 1;
            }
        }
        if self.cleanup_uncertain
            || self.reaped != self.connections
            || self.started.elapsed() >= TOTAL
        {
            self.retain_fixture();
        }
    }
}

impl Harness {
    fn new() -> Check<Self> {
        let started = Instant::now();
        let agent = PathBuf::from(
            std::env::var_os("CEDAR_COLUMN_NAVIGATION_AGENT_BIN")
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
            .prefix("cedar-column-navigation 😀-")
            .tempdir()
            .map_err(|_| "fixture root creation failed")?;
        let canonical_root = root
            .path()
            .canonicalize()
            .map_err(|_| "fixture root canonicalization failed")?;
        require(
            !system_fonts::contains_cjk(&root.path().to_string_lossy())
                && !system_fonts::contains_cjk(&canonical_root.to_string_lossy()),
            "generated fixture temp path requires unsupported CJK font probing",
        )?;
        fs::create_dir(root.path().join("coordinates 😀"))
            .map_err(|_| "fixture directory creation failed")?;
        let (_, commands) = Worker::recording();
        let mut app = CedarApp::empty();
        app.open_form = false;
        app.form.local_root = canonical_root.to_string_lossy().into_owned();
        let cancellation = ConnectionCancellation::new();
        let cancel_on_expiry = cancellation.clone();
        let (stop, stopped) = mpsc::channel();
        let remaining = BUDGET.saturating_sub(started.elapsed());
        let watchdog = std::thread::spawn(move || {
            if matches!(
                stopped.recv_timeout(remaining),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                cancel_on_expiry.cancel();
                true
            } else {
                false
            }
        });
        let mut h = Self {
            app,
            commands,
            client: None,
            root: Some(root),
            agent,
            started,
            clock: 0.0,
            ledger: Ledger::default(),
            sources: Vec::new(),
            connections: 0,
            reaped: 0,
            cases: Vec::new(),
            cleanup_uncertain: false,
            cancellation,
            watchdog: Some((stop, watchdog)),
        };
        for (path, text) in [(FIRST, FIRST_BASE), (SECOND, SECOND_BASE)] {
            h.budget()?;
            fs::write(
                h.root
                    .as_ref()
                    .ok_or("fixture root absent")?
                    .path()
                    .join(path),
                text,
            )
            .map_err(|_| "fixture source creation failed")?;
            h.sources.push((path.into(), digest(text.as_bytes())));
        }
        Ok(h)
    }

    fn budget(&self) -> Check<()> {
        self.admit(Duration::ZERO)
    }

    fn admit(&self, reserve: Duration) -> Check<()> {
        require(
            !self.cancellation.is_cancelled() && self.started.elapsed() + reserve < BUDGET,
            "shared acceptance budget exhausted",
        )
    }

    fn retain_fixture(&mut self) {
        if let Some(root) = self.root.take() {
            let _ = root.keep();
        }
    }

    fn reap(&mut self) -> Check<()> {
        if let Some(client) = self.client.take() {
            let allowance = TOTAL.saturating_sub(self.started.elapsed()).min(CLEANUP);
            if allowance.is_zero() || client.close_and_wait(allowance).is_err() {
                self.cleanup_uncertain = true;
                self.retain_fixture();
                return Err("owned agent cleanup unverified; generated fixture retained");
            }
            self.reaped += 1;
        }
        Ok(())
    }

    fn connect(&mut self) -> Check<()> {
        self.budget()?;
        self.reap()?;
        self.admit(REQUEST)?;
        let root = self
            .root
            .as_ref()
            .ok_or("fixture root absent")?
            .path()
            .to_owned();
        let result = Client::spawn_agent_with_cancellation(
            &self.agent,
            &root,
            false,
            self.cancellation.clone(),
        );
        // Register the returned owner before any assertion or app setup. Failed
        // Hello may have created an owner hidden inside Client's cleanup path.
        self.client = Some(match result {
            Ok(client) => client,
            Err(_) => {
                self.cleanup_uncertain = true;
                self.retain_fixture();
                return Err("agent connection failed; cleanup unverified and fixture retained");
            }
        });
        self.connections += 1;
        self.ledger
            .ordered
            .push(serde_json::json!({"session": self.connections, "op": "Hello"}));
        self.budget()?;
        let handshake = self
            .client
            .as_ref()
            .ok_or("owned agent absent")?
            .handshake()
            .clone();
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
            result: Ok(handshake),
        });
        let command = self.next()?;
        require(
            matches!(&command.op, Operation::List { path } if path.is_empty()),
            "connection did not dispatch exactly the initial root List",
        )?;
        let event = self.exchange(command)?;
        require(
            matches!(&event.result, Ok(Payload::Entries { .. })),
            "initial root List failed",
        )?;
        self.app.apply_event(event);
        require(
            self.app.ready() && !self.app.execution_trusted(),
            "normal agent was not ready with execution disabled",
        )?;
        let form = self
            .app
            .active_form
            .as_ref()
            .ok_or("active local form absent")?;
        require(
            [
                self.app.root.as_str(),
                self.app.form.local_root.as_str(),
                self.app.form.remote_root.as_str(),
                form.local_root.as_str(),
                form.remote_root.as_str(),
            ]
            .iter()
            .all(|path| !system_fonts::contains_cjk(path)),
            "generated fixture root or form requires unsupported CJK font probing",
        )?;
        self.idle()
    }

    fn next(&mut self) -> Check<Command> {
        self.budget()?;
        let command = self
            .commands
            .try_recv()
            .map_err(|_| "explicit request missing")?;
        self.ledger.record(&command.op, self.connections);
        Ok(command)
    }

    fn exchange(&mut self, command: Command) -> Check<Event> {
        self.admit(REQUEST)?;
        require(
            matches!(&command.op, Operation::List { .. } | Operation::Read { .. }),
            "app dispatched an operation other than List or Read",
        )?;
        let client = self.client.as_mut().ok_or("normal agent absent")?;
        let result = client.request(command.op);
        require(
            client.is_connected(),
            "normal agent disconnected during request",
        )?;
        self.budget()?;
        Ok(Event {
            generation: self.app.generation,
            id: command.id,
            connected: true,
            result,
        })
    }

    fn idle(&mut self) -> Check<()> {
        self.budget()?;
        if let Ok(command) = self.commands.try_recv() {
            self.ledger.record(&command.op, self.connections);
            return Err("unsolicited or queued request was dispatched");
        }
        Ok(())
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> Check<()> {
        self.budget()?;
        self.clock += 0.25;
        let ctx = self.app.editor_ctx.clone();
        let mut native = eframe::Frame::_new_kittest();
        let mut input = egui::RawInput {
            time: Some(self.clock),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 1000.0),
            )),
            events,
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut self.app, &ctx, &mut input);
        let _ = ctx.run(input, |ctx| {
            eframe::App::update(&mut self.app, ctx, &mut native)
        });
        self.idle()
    }

    fn open(&mut self, path: &str, read: bool) -> Check<u64> {
        let before = self.ledger.counts();
        self.app.open(path.into(), None);
        if read {
            let command = self.next()?;
            require(
                matches!(&command.op, Operation::Read { path: actual } if actual == path),
                "explicit Open did not dispatch the exact ordinary Read",
            )?;
            let event = self.exchange(command)?;
            require(
                matches!(&event.result, Ok(Payload::File { path: actual, .. }) if actual == path),
                "ordinary Read reply differed",
            )?;
            self.app.apply_event(event);
        }
        self.frame(vec![])?;
        self.frame(vec![])?;
        let id = self.document(path)?.id;
        require(
            self.app.active_document == Some(id),
            "completed Open was not active",
        )?;
        require(
            self.app.location_history.pending.is_none(),
            "completed Open retained a pending admission",
        )?;
        require(
            self.ledger.counts() == (before.0, before.1 + usize::from(read), before.2, before.3),
            "Open operation ledger differed",
        )?;
        Ok(id)
    }

    fn document(&self, path: &str) -> Check<&Document> {
        self.app
            .documents
            .iter()
            .find(|document| document.path == path)
            .ok_or("expected open document absent")
    }

    fn selection(&self, id: u64) -> Check<Selection> {
        egui::TextEdit::load_state(&self.app.editor_ctx, egui::Id::new(("editor", id)))
            .and_then(|state| state.cursor.char_range())
            .ok_or("full editor selection absent")
    }

    fn select(&mut self, id: u64, selection: Selection) -> Check<()> {
        require(
            self.app.active_document == Some(id),
            "selection target was not active",
        )?;
        let editor_id = egui::Id::new(("editor", id));
        let mut state = egui::TextEdit::load_state(&self.app.editor_ctx, editor_id)
            .ok_or("editor state absent before selection")?;
        state.cursor.set_char_range(Some(selection));
        state.store(&self.app.editor_ctx, editor_id);
        self.app
            .editor_ctx
            .memory_mut(|memory| memory.request_focus(editor_id));
        // Install the complete selection before the next real input frame. An
        // artificial idle frame here could clear a pending native redo branch.
        require(
            selection_key(self.selection(id)?) == selection_key(selection),
            "installed full selection differed",
        )
    }

    fn edit(&mut self, path: &str, text: &str) -> Check<()> {
        let id = self.document(path)?.id;
        require(
            self.app.active_document == Some(id),
            "edit target was not active",
        )?;
        let document = self
            .app
            .documents
            .iter_mut()
            .find(|doc| doc.id == id)
            .ok_or("edit target absent")?;
        editor_state::commit(&self.app.editor_ctx, document, text.into(), 5);
        self.frame(vec![])
    }

    fn buffers(&self) -> Vec<BufferSnapshot> {
        self.app
            .documents
            .iter()
            .map(|doc| BufferSnapshot {
                id: doc.id,
                path: doc.path.clone(),
                text: doc.text.clone(),
                saved_text: doc.saved_text.clone(),
                revision: doc.revision.clone(),
                version: doc.edit_version,
                saving: doc.saving,
                interrupted: doc.interrupted_save.clone(),
                save_outcome_unverifiable: doc.save_outcome_unverifiable,
                undo_initialized: doc.undo_initialized,
            })
            .collect()
    }

    fn history(&self) -> HistorySnapshot {
        [
            &self.app.location_history.back,
            &self.app.location_history.forward,
        ]
        .map(|stack| {
            stack
                .iter()
                .map(|entry| {
                    (
                        entry.generation,
                        entry.document,
                        entry.edit_version,
                        selection_key(entry.selection),
                    )
                })
                .collect()
        })
    }

    fn settle(&mut self) -> Check<()> {
        self.frame(vec![])?;
        self.frame(vec![])?;
        self.frame(vec![])
    }

    fn chooser(&mut self) -> Check<()> {
        self.frame(key(egui::Key::G, egui::Modifiers::COMMAND))?;
        self.settle()?;
        require(
            self.app.navigation.dialog_open(),
            "Ctrl G did not open line dialog",
        )
    }

    fn enter_query(&mut self, input: &str) -> Check<()> {
        self.chooser()?;
        if input.is_empty() {
            // egui ignores an empty Paste; use actual editing keys to replace
            // the selected initial line with an empty input.
            self.frame(key(egui::Key::A, egui::Modifiers::COMMAND))?;
            self.frame(key(egui::Key::Backspace, egui::Modifiers::NONE))?;
        } else {
            self.frame(vec![egui::Event::Paste(input.into())])?;
        }
        self.frame(key(egui::Key::Enter, egui::Modifiers::NONE))
    }

    fn stage_coordinate(&mut self, release_frames: usize) -> Check<()> {
        self.enter_query("2:2")?;
        require(
            !self.app.navigation.dialog_open() && self.app.navigation.coordinate_jump_pending(),
            "Enter did not leave a delayed coordinate ticket",
        )?;
        for _ in 0..release_frames {
            self.frame(vec![])?;
            require(
                self.app.navigation.coordinate_jump_pending(),
                "coordinate consumed before the intended modal-release race",
            )?;
        }
        Ok(())
    }

    fn preserved(&self, buffers: &[BufferSnapshot], id: u64, selection: Selection) -> Check<()> {
        require(
            self.buffers() == buffers,
            "navigation changed a document or saved baseline",
        )?;
        require(
            self.app.active_document == Some(id)
                && selection_key(self.selection(id)?) == selection_key(selection),
            "navigation lost active document, selection orientation or affinity",
        )?;
        require(
            !self.app.execution_trusted() && !self.app.recovery.has_actor(),
            "navigation enabled execution trust or started recovery",
        )
    }

    fn step(&mut self, forward: bool, id: u64, expected: Selection) -> Check<()> {
        let buffers = self.buffers();
        let modifiers = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        self.frame(key(
            if forward {
                egui::Key::CloseBracket
            } else {
                egui::Key::OpenBracket
            },
            modifiers,
        ))?;
        self.settle()?;
        self.preserved(&buffers, id, expected)?;
        require(
            self.app
                .editor_ctx
                .memory(|memory| memory.has_focus(egui::Id::new(("editor", id)))),
            "history did not restore editor focus",
        )
    }

    fn navigate(&mut self, input: &str, target: usize) -> Check<()> {
        let id = self.app.active_document.ok_or("navigation target absent")?;
        let before = self.selection(id)?;
        let buffers = self.buffers();
        let ledger = self.ledger.ordered.clone();
        self.enter_query(input)?;
        self.settle()?;
        require(
            !self.app.navigation.dialog_open() && !self.app.navigation.coordinate_jump_pending(),
            "accepted navigation did not settle",
        )?;
        self.preserved(&buffers, id, caret(target))?;
        require(
            self.app
                .editor_ctx
                .memory(|memory| memory.has_focus(egui::Id::new(("editor", id)))),
            "Go to line and column did not restore editor focus",
        )?;
        require(
            self.app.location_history.pending.is_none(),
            "navigation history did not settle",
        )?;
        self.step(false, id, before)?;
        self.step(true, id, caret(target))?;
        require(
            self.ledger.ordered == ledger,
            "measured navigation dispatched an operation",
        )
    }

    fn refusal(&mut self, input: &str) -> Check<()> {
        let id = self.app.active_document.ok_or("refusal target absent")?;
        let selection = self.selection(id)?;
        let buffers = self.buffers();
        let history = self.history();
        self.enter_query(input)?;
        self.settle()?;
        require(
            self.app.navigation.dialog_open()
                && self.app.navigation.line_error().is_some()
                && !self.app.navigation.coordinate_jump_pending(),
            "invalid input did not remain a visible refusal",
        )?;
        self.preserved(&buffers, id, selection)?;
        require(
            self.history() == history && self.app.location_history.pending.is_none(),
            "refused input admitted history",
        )?;
        self.frame(key(egui::Key::Escape, egui::Modifiers::NONE))?;
        self.settle()?;
        self.preserved(&buffers, id, selection)?;
        require(
            !self.app.navigation.dialog_open() && self.history() == history,
            "Escape failed to preserve refused state",
        )
    }

    fn undo(&mut self, redo: bool, path: &str, expected: &str) -> Check<()> {
        let id = self.document(path)?.id;
        require(
            self.app.active_document == Some(id),
            "Undo target not active",
        )?;
        let version = self.document(path)?.edit_version;
        let saved = self.document(path)?.saved_text.clone();
        let revision = self.document(path)?.revision.clone();
        self.app
            .editor_ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", id))));
        let modifiers = if redo {
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
        } else {
            egui::Modifiers::COMMAND
        };
        self.frame(key(egui::Key::Z, modifiers))?;
        let doc = self.document(path)?;
        require(
            doc.text == expected
                && doc.edit_version == version + 1
                && doc.saved_text == saved
                && doc.revision == revision,
            "one native Undo or Redo lost text history or saved baseline",
        )
    }

    fn case(&mut self, name: &'static str) -> Check<()> {
        self.idle()?;
        require(
            CASES.get(self.cases.len()) == Some(&name),
            "case order changed",
        )?;
        self.cases.push(name);
        Ok(())
    }

    fn source_hashes(&self) -> Check<Vec<serde_json::Value>> {
        let root = self.root.as_ref().ok_or("fixture root absent")?.path();
        let mut hashes = Vec::new();
        for (path, expected) in &self.sources {
            self.budget()?;
            let actual = fs::read(root.join(path)).map_err(|_| "source reread failed")?;
            require(&digest(&actual) == expected, "source disk hash changed")?;
            hashes.push(serde_json::json!({"path": path, "sha256": format!("{:x}", Sha256::digest(&actual))}));
        }
        self.budget()?;
        Ok(hashes)
    }

    fn stop_watchdog(&mut self) -> Check<()> {
        let (stop, watchdog) = self.watchdog.take().ok_or("watchdog absent")?;
        let _ = stop.send(());
        require(
            !watchdog.join().map_err(|_| "watchdog join failed")?,
            "watchdog expired",
        )
    }
}

#[derive(PartialEq, Eq)]
struct BufferSnapshot {
    id: u64,
    path: String,
    text: String,
    saved_text: String,
    revision: Option<String>,
    version: u64,
    saving: bool,
    interrupted: Option<interrupted_save::InterruptedSave>,
    save_outcome_unverifiable: bool,
    undo_initialized: bool,
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

fn caret(index: usize) -> Selection {
    Selection::one(egui::text::CCursor::new(index))
}

#[test]
#[ignore = "requires explicit CEDAR_COLUMN_NAVIGATION_AGENT_BIN selecting normal shipping agent"]
fn normal_agent_line_column_navigation_is_local_and_preserves_history() -> Check<()> {
    let mut h = Harness::new()?;
    h.connect()?;
    let first = h.open(FIRST, true)?;
    let second = h.open(SECOND, true)?;
    require(
        h.ledger.counts() == (1, 2, 0, 0) && h.ledger.ordered.len() == 4,
        "setup ledger differed",
    )?;
    let setup = h.ledger.ordered.clone();

    for (path, draft, case, second_start, blank_start, last_end, eof) in [
        (FIRST, FIRST_DRAFT, CASES[0], 12, 17, 24, 26),
        (SECOND, SECOND_DRAFT, CASES[1], 11, 15, 21, 22),
    ] {
        let id = h.open(path, false)?;
        h.edit(path, draft)?;
        // Independent scalar positions: tab and emoji each occupy one index;
        // the combining mark occupies its own index. CR in CRLF is not a column.
        for (input, target) in [
            ("1:1", 0),
            ("1:2", 1),
            ("1:3", 2),
            ("1:4", 3),
            ("1:5", 4),
            ("1:6", 5),
            ("1:11", 10),
            ("2:1", second_start),
            ("2:2", second_start + 1),
            ("2:4", second_start + 3),
            ("3:1", blank_start),
            ("4:6", last_end),
            (" 5:1 ", eof),
        ] {
            h.select(id, range(9, 2))?;
            h.navigate(input, target)?;
        }
        h.case(case)?;
    }

    for (path, second_start) in [(FIRST, 12), (SECOND, 11)] {
        let id = h.open(path, false)?;
        h.select(id, range(2, 9))?;
        h.navigate(" 2 ", second_start)?;
    }
    h.case(CASES[2])?;

    for path in [FIRST, SECOND] {
        let id = h.open(path, false)?;
        h.select(id, range(9, 2))?;
        for invalid in [
            "",
            "0",
            "0:1",
            "1:0",
            "6:1",
            "1:12",
            "2:5",
            "3:2",
            "5:2",
            "1:",
            ":1",
            "1:2:3",
            "-1:2",
            "+1:2",
            "1 :2",
            "1: 2",
            "1.0:2",
            "1:2x",
            "18446744073709551616:1",
            "1:18446744073709551616",
        ] {
            h.refusal(invalid)?;
        }
    }
    h.case(CASES[3])?;

    for path in [FIRST, SECOND] {
        let id = h.open(path, false)?;
        let selection = range(2, 9);
        h.select(id, selection)?;
        let buffers = h.buffers();
        let history = h.history();
        h.chooser()?;
        h.frame(vec![egui::Event::Text("2:2".into())])?;
        h.frame(key(egui::Key::Escape, egui::Modifiers::NONE))?;
        h.settle()?;
        h.preserved(&buffers, id, selection)?;
        require(h.history() == history, "cancellation changed history")?;
    }
    h.case(CASES[4])?;

    for path in [FIRST, SECOND] {
        let id = h.open(path, false)?;
        h.select(id, caret(3))?;
        let buffers = h.buffers();
        let history = h.history();
        h.enter_query("1:4")?;
        h.settle()?;
        h.preserved(&buffers, id, caret(3))?;
        require(
            h.history() == history,
            "same-location coordinate changed history",
        )?;
    }
    h.case(CASES[5])?;

    for (path, base, draft) in [
        (FIRST, FIRST_BASE, FIRST_DRAFT),
        (SECOND, SECOND_BASE, SECOND_DRAFT),
    ] {
        h.open(path, false)?;
        h.undo(false, path, base)?;
        // A coordinate after Undo must preserve the pending native redo branch.
        let id = h.document(path)?.id;
        h.select(id, range(9, 2))?;
        h.navigate("1:4", 3)?;
        h.undo(true, path, draft)?;
        h.undo(false, path, base)?;
        h.undo(true, path, draft)?;
    }
    h.case(CASES[6])?;

    // Hold the real coordinate ticket across each modal-release delay. A local
    // edit invalidates it before consumption; no old target may reach the new
    // text or add a historical departure on a later idle frame.
    for (release_frames, case, replacement) in [
        (0, CASES[7], "new α😀e\u{301}\r\nreplacement Ω\r\n"),
        (1, CASES[8], "later α😀e\u{301}\r\nreplacement Ω\r\n"),
    ] {
        h.open(FIRST, false)?;
        h.select(first, range(9, 2))?;
        h.stage_coordinate(release_frames)?;
        let history = h.history();
        let doc = h
            .app
            .documents
            .iter_mut()
            .find(|doc| doc.id == first)
            .ok_or("edit target absent")?;
        editor_state::commit(&h.app.editor_ctx, doc, replacement.into(), 5);
        let buffers = h.buffers();
        let selection = h.selection(first)?;
        h.settle()?;
        h.preserved(&buffers, first, selection)?;
        require(
            !h.app.navigation.coordinate_jump_pending() && h.history() == history,
            "edited source consumed a stale coordinate or history",
        )?;
        h.case(case)?;
    }

    h.open(FIRST, false)?;
    h.select(first, range(9, 2))?;
    h.stage_coordinate(0)?;
    h.app.activate_history_tab(second);
    let buffers = h.buffers();
    let other_selection = h.selection(second)?;
    let original_selection = h.selection(first)?;
    let history = h.history();
    h.settle()?;
    h.preserved(&buffers, second, other_selection)?;
    require(
        selection_key(h.selection(first)?) == selection_key(original_selection)
            && !h.app.navigation.coordinate_jump_pending()
            && h.history() == history,
        "tab switch consumed stale coordinate",
    )?;
    h.case(CASES[9])?;

    // A newer real cursor key cancels the ticket even on the blocked first
    // release frame, before that input could disappear from a later frame.
    for release_frames in [0, 1] {
        h.open(SECOND, false)?;
        h.select(second, range(9, 2))?;
        h.stage_coordinate(release_frames)?;
        let buffers = h.buffers();
        let history = h.history();
        h.frame(key(egui::Key::ArrowRight, egui::Modifiers::NONE))?;
        require(
            !h.app.navigation.coordinate_jump_pending(),
            "new cursor key failed to cancel delayed coordinate immediately",
        )?;
        let selection = h.selection(second)?;
        require(
            selection.primary.index <= 9 && selection.secondary.index <= 9,
            "new cursor key consumed the old coordinate destination",
        )?;
        if release_frames == 0 {
            require(
                selection_key(selection) == selection_key(range(9, 2)),
                "blocked modal-release cursor key changed full source selection",
            )?;
        }
        h.settle()?;
        h.preserved(&buffers, second, selection)?;
        require(
            h.history() == history,
            "new cursor key admitted stale history",
        )?;
    }
    h.open(SECOND, false)?;
    h.select(second, range(9, 2))?;
    h.stage_coordinate(1)?;
    let buffers = h.buffers();
    let history = h.history();
    h.app.navigation_changed();
    h.settle()?;
    h.preserved(&buffers, second, range(9, 2))?;
    require(
        !h.app.navigation.coordinate_jump_pending() && h.history() == history,
        "newer navigation consumed stale coordinate",
    )?;
    h.case(CASES[10])?;

    h.refusal(&"1".repeat(33))?;
    h.refusal(&format!("2:2{}", " ".repeat(4097)))?;
    h.case(CASES[11])?;
    require(
        h.ledger.ordered == setup,
        "navigation phase changed exact setup ledger",
    )?;

    h.select(second, range(9, 2))?;
    h.stage_coordinate(0)?;
    let buffers = h.buffers();
    let selection = h.selection(second)?;
    h.app
        .disconnected("synthetic explicit coordinate reconnect".into());
    h.idle()?;
    h.connect()?;
    h.settle()?;
    h.preserved(&buffers, second, selection)?;
    require(
        !h.app.navigation.coordinate_jump_pending()
            && h.app.location_history.back.is_empty()
            && h.app.location_history.forward.is_empty()
            && h.app.location_history.pending.is_none(),
        "real reconnect retained old coordinate or location history",
    )?;
    h.case(CASES[12])?;

    let reconnected = h.ledger.ordered.clone();
    h.select(second, range(2, 9))?;
    h.navigate("2:2", 12)?;
    h.undo(false, SECOND, SECOND_BASE)?;
    h.undo(true, SECOND, SECOND_DRAFT)?;
    require(
        h.ledger.ordered == reconnected,
        "post-reconnect navigation dispatched an operation",
    )?;
    h.case(CASES[13])?;

    let expected = serde_json::json!([
        {"session": 1, "op": "Hello"}, {"session": 1, "op": "List", "path": ""},
        {"session": 1, "op": "Read", "path": FIRST}, {"session": 1, "op": "Read", "path": SECOND},
        {"session": 2, "op": "Hello"}, {"session": 2, "op": "List", "path": ""},
    ]);
    h.idle()?;
    require(
        serde_json::json!(h.ledger.ordered) == expected
            && h.ledger.counts() == (2, 2, 0, 0)
            && h.cases == CASES,
        "final exact ledger or case contract differed",
    )?;
    require(
        !h.app.cjk_seen
            && !h.app.language.cjk_seen
            && h.app.documents.iter().all(|doc| !doc.has_cjk)
            && h.app.system_fonts.needs_probe(),
        "fixture triggered optional system-font work",
    )?;
    let source_hashes = h.source_hashes()?;
    h.reap()?;
    require(
        h.connections == 2 && h.reaped == 2 && !h.cleanup_uncertain,
        "both owned normal agents were not observed reaped",
    )?;
    h.stop_watchdog()?;
    h.app.worker = None;
    require(
        h.started.elapsed() < TOTAL,
        "total cleanup budget exhausted",
    )?;
    let root = h.root.take().ok_or("fixture root absent during cleanup")?;
    let root_path = root.path().to_owned();
    root.close()
        .map_err(|_| "generated fixture cleanup failed")?;
    require(
        !root_path.exists() && h.started.elapsed() < TOTAL,
        "fixture removal or total cleanup budget unverified",
    )?;
    println!(
        "column_navigation_acceptance {}",
        serde_json::json!({
            "schema": 1, "acceptance": "column_navigation", "success": true,
            "os": if cfg!(windows) { "windows" } else { "linux" },
            "cases": h.cases, "cases_executed": 14, "operation_ledger": h.ledger.ordered,
            "requests": 6, "hello": h.connections, "list": h.ledger.lists.len(), "read": h.ledger.reads.len(),
            "write": h.ledger.writes, "other_operations": h.ledger.other, "measured_navigation_operations": 0,
            "connections": h.connections, "agents_reaped": h.reaped, "source_hashes": source_hashes,
            "trust_off": true, "production_keyboard_frames": true, "unicode_scalar_columns": true,
            "lf_crlf_logical_end": true, "trailing_empty_line": true, "plain_line_compatible": true,
            "invalid_and_oversized_preserved": true, "dirty_baselines_preserved": true,
            "full_selection_and_affinity_preserved": true, "back_forward_preserved": true,
            "undo_redo_preserved": true, "stale_delayed_coordinates_rejected": true,
            "reconnect_rejected_old_coordinate": true, "exact_operation_ledger_verified": true,
            "shared_watchdog_seconds": 60, "total_budget_seconds": 65, "request_admission_seconds": 30,
            "cleanup_timeout_seconds": 5, "owned_children_reaped": true, "watchdog_joined": true,
            "fixture_removed": true, "optional_font_probe_untriggered": true,
        })
    );
    Ok(())
}
