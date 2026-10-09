//! Finite open-buffer history acceptance through the normal trust-off agent.
//! Beyond Client's connection Hello, only explicit connection Lists and
//! initial/reopen Reads reach the process.
//! Back/Forward uses production input frames and existing editor state.
use super::*;
use cedar_client::{Client, ConnectionCancellation};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

const BUDGET: Duration = Duration::from_secs(60);
const CLEANUP: Duration = Duration::from_secs(5);
const FIRST: &str = "history 雪/first café.txt";
const SECOND: &str = "history 雪/second Ω.txt";
const THIRD: &str = "history 雪/third 草稿.txt";
const FIRST_BASE: &str = "alpha 雪 café\nsecond Ω\nthird line\n";
const FIRST_EDIT: &str = "draft 草稿 café\nsecond Ω\nthird line\n";
const FIRST_DRAFT: &str = "newer 草稿 café\nsecond Ω\nthird line\n";
const SECOND_BASE: &str = "beta Ω café\nsecond 雪\nthird line\n";
const SECOND_EDIT: &str = "beta edited 草稿\nsecond 雪\nthird line\n";
const THIRD_BASE: &str = "gamma 草稿\nsecond café\nthird Ω\n";
type Check<T> = Result<T, &'static str>;
type Selection = egui::text::CCursorRange;
type SelectionKey = (usize, bool, usize, bool);
type HistoryEntry = (u64, u64, u64, SelectionKey);

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
    lists: Vec<String>,
    reads: Vec<String>,
    writes: u32,
    other: u32,
}

impl Ledger {
    fn record(&mut self, operation: &Operation) {
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

#[derive(PartialEq, Eq)]
struct BufferSnapshot {
    id: u64,
    text: String,
    saved_text: String,
    revision: Option<String>,
    version: u64,
    saving: bool,
    interrupted: bool,
    undo_initialized: bool,
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
    cases: u32,
    traversals: u32,
    cancellation: ConnectionCancellation,
    watchdog: Option<(Sender<()>, std::thread::JoinHandle<bool>)>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some((stop, watchdog)) = self.watchdog.take() {
            let _ = stop.send(());
            let _ = watchdog.join();
        }
        if let Some(client) = self.client.take() {
            let _ = client.close_and_wait(CLEANUP);
        }
    }
}

impl Harness {
    fn new() -> Check<Self> {
        let started = Instant::now();
        let agent = PathBuf::from(
            std::env::var_os("CEDAR_LOCATION_HISTORY_AGENT_BIN")
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
            .prefix("cedar-history 雪-")
            .tempdir()
            .map_err(|_| "fixture root creation failed")?;
        fs::create_dir(root.path().join("history 雪"))
            .map_err(|_| "fixture directory creation failed")?;
        let (_, commands) = Worker::recording();
        let mut app = CedarApp::empty();
        app.open_form = false;
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
            cases: 0,
            traversals: 0,
            cancellation,
            watchdog: Some((stop, watchdog)),
        };
        for (path, text) in [
            (FIRST, FIRST_BASE),
            (SECOND, SECOND_BASE),
            (THIRD, THIRD_BASE),
        ] {
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
        require(
            self.started.elapsed() < BUDGET,
            "acceptance budget exceeded",
        )
    }

    fn reap(&mut self) -> Check<()> {
        if let Some(client) = self.client.take() {
            client
                .close_and_wait(CLEANUP)
                .map_err(|_| "normal agent cleanup failed")?;
            self.reaped += 1;
        }
        Ok(())
    }

    fn connect(&mut self) -> Check<()> {
        self.budget()?;
        self.reap()?;
        let root = self.root.as_ref().ok_or("fixture root absent")?.path();
        let client = Client::spawn_agent_with_cancellation(
            &self.agent,
            root,
            false,
            self.cancellation.clone(),
        )
        .map_err(|_| "normal agent connection failed")?;
        self.connections += 1;
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
        self.idle()
    }

    fn next(&mut self) -> Check<Command> {
        self.budget()?;
        let command = self
            .commands
            .try_recv()
            .map_err(|_| "explicit request missing")?;
        self.ledger.record(&command.op);
        Ok(command)
    }

    fn exchange(&mut self, command: Command) -> Check<Event> {
        self.budget()?;
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
            self.ledger.record(&command.op);
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
        self.frame(vec![])?;
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

    fn undo(&mut self, redo: bool, path: &str, expected: &str) -> Check<()> {
        let id = self.document(path)?.id;
        require(
            self.app.active_document == Some(id),
            "Undo target was not active",
        )?;
        let version = self.document(path)?.edit_version;
        self.app.editor_ctx.memory_mut(|memory| {
            memory.request_focus(egui::Id::new(("editor", id)));
        });
        let modifiers = if redo {
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
        } else {
            egui::Modifiers::COMMAND
        };
        self.frame(key(egui::Key::Z, modifiers))?;
        require(
            self.document(path)?.text == expected
                && self.document(path)?.edit_version == version + 1,
            "native Undo or Redo did not preserve the complete text history",
        )
    }

    fn buffers(&self) -> Vec<BufferSnapshot> {
        self.app
            .documents
            .iter()
            .map(|doc| BufferSnapshot {
                id: doc.id,
                text: doc.text.clone(),
                saved_text: doc.saved_text.clone(),
                revision: doc.revision.clone(),
                version: doc.edit_version,
                saving: doc.saving,
                interrupted: doc.interrupted_save.is_some(),
                undo_initialized: doc.undo_initialized,
            })
            .collect()
    }

    fn entries(&self, forward: bool) -> Vec<HistoryEntry> {
        let stack = if forward {
            &self.app.location_history.forward
        } else {
            &self.app.location_history.back
        };
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
    }

    fn step(&mut self, forward: bool, id: u64, selection: Selection) -> Check<()> {
        let buffers = self.buffers();
        let ledger = self.ledger.counts();
        let button = if forward {
            egui::Key::CloseBracket
        } else {
            egui::Key::OpenBracket
        };
        // Match the native shortcut modifier shape on both CI operating systems.
        let modifiers = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        self.frame(key(button, modifiers))?;
        self.frame(vec![])?;
        require(
            self.app.active_document == Some(id),
            "history selected the wrong document",
        )?;
        require(
            selection_key(self.selection(id)?) == selection_key(selection),
            "history changed selection orientation or affinity",
        )?;
        require(
            self.app
                .editor_ctx
                .memory(|memory| memory.has_focus(egui::Id::new(("editor", id)))),
            "history failed to retain or restore the editor focus",
        )?;
        require(
            self.buffers() == buffers,
            "history mutated an existing document or its editor initialization",
        )?;
        require(
            self.ledger.counts() == ledger,
            "history dispatched a workspace operation",
        )?;
        self.traversals += 1;
        Ok(())
    }

    fn first_draft(&self, id: u64, version: u64) -> Check<()> {
        let doc = self.document(FIRST)?;
        require(
            doc.id == id
                && doc.text == FIRST_DRAFT
                && doc.saved_text == FIRST_BASE
                && doc.dirty()
                && !doc.saving
                && doc.interrupted_save.is_none()
                && doc.edit_version == version
                && doc.revision.as_deref()
                    == Some(format!("{:x}", Sha256::digest(FIRST_BASE.as_bytes())).as_str()),
            "dirty draft identity, version or disk baseline changed",
        )
    }

    fn sources_unchanged(&self) -> Check<()> {
        let root = self.root.as_ref().ok_or("fixture root absent")?.path();
        for (path, expected) in &self.sources {
            let actual = fs::read(root.join(path)).map_err(|_| "fixture source reread failed")?;
            require(&digest(&actual) == expected, "fixture source hash changed")?;
        }
        self.budget()
    }

    fn case(&mut self) -> Check<()> {
        self.idle()?;
        self.cases += 1;
        Ok(())
    }

    fn stop_watchdog(&mut self) -> Check<()> {
        let (stop, watchdog) = self.watchdog.take().ok_or("acceptance watchdog absent")?;
        let _ = stop.send(());
        let expired = watchdog
            .join()
            .map_err(|_| "acceptance watchdog join failed")?;
        require(!expired, "acceptance watchdog expired")
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
#[ignore = "requires explicit CEDAR_LOCATION_HISTORY_AGENT_BIN selecting normal shipping agent"]
fn normal_agent_open_buffer_history_is_local_and_preserves_draft_undo() -> Check<()> {
    let mut h = Harness::new()?;
    h.connect()?;
    let first = h.open(FIRST, true)?;
    h.edit(FIRST, FIRST_EDIT)?;
    h.edit(FIRST, FIRST_DRAFT)?;
    let first_selection = range(2, 19);
    h.select(first, first_selection)?;
    require(
        h.app.location_history.back.is_empty() && h.app.location_history.forward.is_empty(),
        "typing or selection admitted navigation history",
    )?;
    h.first_draft(first, 2)?;
    h.case()?;

    let second = h.open(SECOND, true)?;
    let second_selection = range(16, 1);
    h.select(second, second_selection)?;
    let third = h.open(THIRD, true)?;
    let third_selection = range(3, 15);
    h.select(third, third_selection)?;
    require(
        h.app.location_history.back.len() == 2 && h.app.location_history.forward.is_empty(),
        "completed initial Opens did not admit exactly two departures",
    )?;
    h.step(false, second, second_selection)?;
    h.step(false, first, first_selection)?;
    h.first_draft(first, 2)?;
    h.step(true, second, second_selection)?;
    h.step(true, third, third_selection)?;
    h.case()?;

    h.step(false, second, second_selection)?;
    require(
        h.app.location_history.forward.len() == 1,
        "Back did not retain Forward target",
    )?;
    let duplicate_back = h.entries(false);
    let duplicate_forward = h.entries(true);
    h.open(SECOND, false)?;
    require(
        h.entries(false) == duplicate_back && h.entries(true) == duplicate_forward,
        "same-location Open changed navigation history",
    )?;
    h.open(FIRST, false)?;
    require(
        h.app.location_history.forward.is_empty(),
        "new completed branch retained Forward",
    )?;
    let back = h.entries(false);
    h.step(true, first, first_selection)?;
    require(
        h.entries(false) == back,
        "Forward boundary changed Back history",
    )?;
    h.step(false, second, second_selection)?;
    h.step(true, first, first_selection)?;
    h.case()?;

    // Traversal preserved both text edits, their order, and the full redo chain.
    let back = h.entries(false);
    h.undo(false, FIRST, FIRST_EDIT)?;
    h.undo(false, FIRST, FIRST_BASE)?;
    h.undo(true, FIRST, FIRST_EDIT)?;
    h.undo(true, FIRST, FIRST_DRAFT)?;
    require(
        h.entries(false) == back,
        "Undo or Redo admitted a navigation departure",
    )?;
    let refreshed_first_selection = range(4, 20);
    h.select(first, refreshed_first_selection)?;
    h.first_draft(first, 6)?;
    h.case()?;

    // Earlier A@2 and B@0 entries remain bound to those edit versions. Editing
    // B and undoing to identical bytes must not revive its old B@0 location.
    h.open(SECOND, false)?;
    h.edit(SECOND, SECOND_EDIT)?;
    h.undo(false, SECOND, SECOND_BASE)?;
    h.select(second, second_selection)?;
    require(
        h.document(SECOND)?.edit_version == 2,
        "edit then Undo failed to advance version",
    )?;
    h.open(THIRD, false)?;
    h.step(false, second, second_selection)?;
    h.step(false, first, refreshed_first_selection)?;
    require(
        h.app.location_history.back.len() == 2,
        "all-stale case did not contain two old entries",
    )?;
    let opposite = h.entries(true);
    h.step(false, first, refreshed_first_selection)?;
    require(
        h.app.location_history.back.is_empty()
            && h.entries(true) == opposite
            && h.app
                .location_history
                .message
                .as_ref()
                .is_some_and(|message| message.contains('2')),
        "all-stale traversal changed opposite history or omitted the visible skip count",
    )?;
    h.step(true, second, second_selection)?;
    h.step(true, third, third_selection)?;
    h.case()?;

    // A second B edit/Undo makes just the older B@2 entry stale. Back must skip
    // it, reach valid A@6, and preserve the current C departure on Forward.
    h.open(SECOND, false)?;
    h.edit(SECOND, SECOND_EDIT)?;
    h.undo(false, SECOND, SECOND_BASE)?;
    h.select(second, second_selection)?;
    h.open(THIRD, false)?;
    h.step(false, second, second_selection)?;
    h.step(false, third, third_selection)?;
    h.step(false, first, refreshed_first_selection)?;
    require(
        h.app.location_history.back.is_empty()
            && h.app.location_history.forward.len() == 3
            && h.app
                .location_history
                .message
                .as_ref()
                .is_some_and(|message| message.contains('1')),
        "partial-stale traversal did not skip exactly the old edited entry",
    )?;
    h.first_draft(first, 6)?;
    h.case()?;

    h.app.close_tab(second);
    h.frame(vec![])?;
    require(
        h.app.documents.iter().all(|doc| doc.id != second)
            && h.app
                .location_history
                .back
                .iter()
                .chain(&h.app.location_history.forward)
                .all(|entry| entry.document != second)
            && h.app.location_history.forward.len() == 2,
        "closing a clean tab failed to prune its exact document identity",
    )?;
    let reopened = h.open(SECOND, true)?;
    require(
        reopened != second
            && h.app
                .location_history
                .back
                .iter()
                .chain(&h.app.location_history.forward)
                .all(|entry| entry.document != second)
            && h.app.location_history.forward.is_empty(),
        "same-path reopen revived an old document ID or Forward branch",
    )?;
    let reopened_selection = h.selection(reopened)?;
    h.step(false, first, refreshed_first_selection)?;
    h.step(true, reopened, reopened_selection)?;
    h.case()?;

    let buffers = h.buffers();
    let selection = h.selection(reopened)?;
    require(
        !h.app.location_history.back.is_empty(),
        "reconnect case had no history to clear",
    )?;
    h.app.disconnected("synthetic history reconnect".into());
    h.idle()?;
    h.connect()?;
    h.frame(vec![])?;
    require(
        h.app.location_history.back.is_empty()
            && h.app.location_history.forward.is_empty()
            && h.app.location_history.pending.is_none()
            && h.buffers() == buffers
            && h.app.active_document == Some(reopened)
            && selection_key(h.selection(reopened)?) == selection_key(selection),
        "reconnect failed to clear history or changed retained buffers",
    )?;
    h.step(false, reopened, selection)?;
    h.step(true, reopened, selection)?;
    h.case()?;

    h.open(FIRST, false)?;
    h.undo(false, FIRST, FIRST_EDIT)?;
    h.undo(false, FIRST, FIRST_BASE)?;
    h.undo(true, FIRST, FIRST_EDIT)?;
    h.undo(true, FIRST, FIRST_DRAFT)?;
    h.first_draft(first, 10)?;
    h.case()?;

    require(
        h.ledger.lists.iter().map(String::as_str).eq(["", ""])
            && h.ledger
                .reads
                .iter()
                .map(String::as_str)
                .eq([FIRST, SECOND, THIRD, SECOND])
            && h.ledger.writes == 0
            && h.ledger.other == 0,
        "exact connection List and explicit Open Read ledger differed",
    )?;
    h.sources_unchanged()?;
    h.case()?;
    require(
        h.cases == 10 && h.traversals == 20,
        "executed acceptance counters differed",
    )?;
    h.stop_watchdog()?;
    h.reap()?;
    require(
        h.reaped == h.connections && h.connections == 2,
        "normal-agent ownership was not fully reaped",
    )?;
    h.app.worker = None;
    let root = h.root.take().ok_or("fixture root absent during cleanup")?;
    let root_path = root.path().to_owned();
    root.close().map_err(|_| "fixture root cleanup failed")?;
    require(!root_path.exists(), "fixture root remained after cleanup")?;
    println!(
        "{}",
        serde_json::json!({
            "success": true, "cases_executed": h.cases,
            "history_steps": h.traversals, "traversal_operations": 0,
            "lists": h.ledger.lists.len(), "initial_open_reads": 3, "explicit_reopen_reads": 1,
            "reads": h.ledger.reads.len(), "writes": h.ledger.writes, "other_operations": h.ledger.other,
            "connections": h.connections, "connection_hellos": h.connections, "agents_reaped": h.reaped,
            "source_hashes_verified": h.sources.len(), "trust_off": true,
            "exact_operation_ledger_verified": true, "completed_open_admissions": true,
            "two_way_full_unicode_selection": true, "dirty_draft_preserved": true,
            "multi_step_undo_redo_preserved": true, "branch_truncated_forward": true,
            "partial_stale_skipped": true, "all_stale_preserved_current_and_opposite": true,
            "edit_then_undo_invalidated_old_entries": true, "closed_identity_pruned": true,
            "reopen_used_new_identity": true, "reconnect_cleared_history": true,
            "fixture_removed": true, "watchdog_joined": true, "cleanup_verified": true
        })
    );
    Ok(())
}
