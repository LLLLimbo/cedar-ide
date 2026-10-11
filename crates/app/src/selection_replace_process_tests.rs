//! Finite selection-replacement proof through the normal shipping trust-off agent.
//! Generated sources, actual Preview/Cancel/Apply frames, and an exact operation ledger.
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
const FIRST: &str = "selection 😀/CRLF café.txt";
const SECOND: &str = "selection 😀/LF Ω.txt";
const FIRST_BASE: &str = "out cat 😀\r\nα cat e\u{301} cat Ω\r\ntail cat\r\n";
const SECOND_BASE: &str = "out cat 😀\nα cat e\u{301} cat Ω\ntail cat\n";
const REPLACEMENT: &str = "$1\\😀";
const CASES: [&str; 16] = [
    "unicode_crlf_forward",
    "unicode_crlf_reversed",
    "unicode_lf_forward",
    "unicode_lf_reversed",
    "preview_cancel_preserves_full_history",
    "no_op_preserves_full_undo_redo",
    "empty_selection_refused",
    "crossing_boundaries_excluded",
    "stale_selection_rejected",
    "stale_affinity_rejected",
    "stale_edit_rejected",
    "stale_workspace_rejected",
    "stale_session_rejected",
    "stale_tab_rejected",
    "stale_reconnect_rejected",
    "replacement_after_reconnect",
];
type Check<T> = Result<T, &'static str>;
type Selection = egui::text::CCursorRange;
type SelectionKey = (usize, bool, usize, bool);
type HistorySnapshot = [Vec<(SelectionKey, String)>; 2];

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
            std::env::var_os("CEDAR_SELECTION_REPLACE_AGENT_BIN")
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
            .prefix("cedar-selection-replace 😀-")
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
        fs::create_dir(root.path().join("selection 😀"))
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

    fn frame(&mut self, events: Vec<egui::Event>) -> Check<egui::FullOutput> {
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
            focused: true,
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut self.app, &ctx, &mut input);
        let output = ctx.run(input, |ctx| {
            eframe::App::update(&mut self.app, ctx, &mut native)
        });
        self.idle()?;
        Ok(output)
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

    fn select(&mut self, id: u64, selected: Selection) -> Check<()> {
        require(
            self.app.active_document == Some(id),
            "selection target inactive",
        )?;
        let document = self
            .app
            .documents
            .iter_mut()
            .find(|doc| doc.id == id)
            .ok_or("selection target absent")?;
        editor_state::move_selection(&self.app.editor_ctx, document, selected);
        require(
            selection_key(self.selection(id)?) == selection_key(selected),
            "installed full selection differed",
        )
    }

    fn edit(&mut self, path: &str, text: &str) -> Check<()> {
        let id = self.document(path)?.id;
        require(self.app.active_document == Some(id), "edit target inactive")?;
        let document = self
            .app
            .documents
            .iter_mut()
            .find(|doc| doc.id == id)
            .ok_or("edit target absent")?;
        editor_state::commit(&self.app.editor_ctx, document, text.into(), 5);
        self.frame(vec![])?;
        Ok(())
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

    // Observe every reachable native Undo and Redo point on disposable clones.
    // Include both endpoint affinities; egui's ordinary equality ignores them.
    fn history(&self, id: u64) -> Check<HistorySnapshot> {
        let state = egui::TextEdit::load_state(&self.app.editor_ctx, egui::Id::new(("editor", id)))
            .ok_or("history state absent")?;
        let doc = self
            .app
            .documents
            .iter()
            .find(|doc| doc.id == id)
            .ok_or("history document absent")?;
        let current = (self.selection(id)?, doc.text.clone());
        let mut snapshots: HistorySnapshot = Default::default();
        for (index, redo) in [false, true].into_iter().enumerate() {
            let mut history = state.undoer();
            let mut current = current.clone();
            for _ in 0..=editor_state::MAX_UNDO_STATES + 1 {
                let next = if redo {
                    history.redo(&current)
                } else {
                    history.undo(&current)
                }
                .cloned();
                let Some(next) = next else {
                    break;
                };
                snapshots[index].push((selection_key(next.0), next.1.clone()));
                current = next;
            }
            require(
                snapshots[index].len() <= editor_state::MAX_UNDO_STATES + 1,
                "history observation exceeded native bound",
            )?;
        }
        Ok(snapshots)
    }

    fn settle(&mut self) -> Check<()> {
        for _ in 0..3 {
            self.frame(vec![])?;
        }
        Ok(())
    }

    fn configure(
        &mut self,
        id: u64,
        selected: Selection,
        query: &str,
        replacement: &str,
    ) -> Check<()> {
        self.app.find_open = true;
        self.app.find_focus = false;
        self.app.find_query = query.into();
        self.app.replace.replacement = replacement.into();
        self.select(id, selected)?;
        self.settle()?;
        require(
            self.app.find_open
                && !self.app.find_focus
                && !self.app.navigation.blocks_editor()
                && !self.app.navigation.restore_focus
                && !self.app.foreign_modal_owns_input(&self.app.editor_ctx),
            "Find setup retained an input or focus blocker",
        )?;
        require(
            selection_key(self.selection(id)?) == selection_key(selected),
            "Find setup changed full selection",
        )
    }

    fn press(&mut self, label: &str) -> Check<egui::Pos2> {
        let output = self.frame(vec![])?;
        let rect = text_rect(&output, label).ok_or("production button label absent")?;
        require(
            rect.is_finite() && rect.top() > 0.0 && rect.bottom() < 1000.0,
            "production button not visible",
        )?;
        let pos = rect.center();
        self.frame(pointer(pos, true))?;
        Ok(pos)
    }

    fn release(&mut self, pos: egui::Pos2) -> Check<()> {
        self.frame(pointer(pos, false))?;
        Ok(())
    }

    fn click(&mut self, label: &str) -> Check<()> {
        let pos = self.press(label)?;
        self.release(pos)
    }

    fn preview_visible(&mut self, visible: bool) -> Check<()> {
        let output = self.frame(vec![])?;
        require(
            text_rect(&output, "Apply").is_some() == visible
                && text_rect(&output, "Cancel").is_some() == visible,
            "selection preview visibility differed",
        )
    }

    fn local(&self) -> Check<()> {
        require(
            !self.app.execution_trusted()
                && !self.app.recovery.has_actor()
                && !self.app.language.running
                && !self.app.language.startup_active(),
            "local replacement enabled execution, language or recovery work",
        )
    }

    fn preserved(
        &self,
        buffers: &[BufferSnapshot],
        id: u64,
        selected: Selection,
        history: &HistorySnapshot,
    ) -> Check<()> {
        require(
            self.buffers() == buffers,
            "read-only action changed a draft or saved baseline",
        )?;
        require(
            self.app.active_document == Some(id)
                && selection_key(self.selection(id)?) == selection_key(selected),
            "read-only action changed selection orientation or affinity",
        )?;
        require(
            &self.history(id)? == history,
            "read-only action changed complete native history",
        )?;
        self.local()
    }

    fn preview(&mut self, id: u64) -> Check<()> {
        let buffers = self.buffers();
        let selected = self.selection(id)?;
        let history = self.history(id)?;
        let ledger = self.ledger.ordered.clone();
        self.click("Preview selection")?;
        self.preview_visible(true)?;
        self.preserved(&buffers, id, selected, &history)?;
        require(
            self.ledger.ordered == ledger,
            "Preview dispatched an operation",
        )
    }

    fn undo(&mut self, redo: bool, path: &str, expected: &str) -> Check<()> {
        let id = self.document(path)?.id;
        require(self.app.active_document == Some(id), "Undo target inactive")?;
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
        )?;
        self.local()
    }

    fn exhausted(&mut self, redo: bool, path: &str) -> Check<()> {
        let id = self.document(path)?.id;
        let before = self.buffers();
        self.app
            .editor_ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", id))));
        self.frame(key(
            egui::Key::Z,
            if redo {
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
            } else {
                egui::Modifiers::COMMAND
            },
        ))?;
        require(
            self.buffers() == before,
            "exhausted native history manufactured a text edit",
        )
    }

    fn apply_text(&mut self, path: &str, expected: &str) -> Check<()> {
        let before = self.buffers();
        let id = self.document(path)?.id;
        let ledger = self.ledger.ordered.clone();
        self.click("Apply")?;
        self.preview_visible(false)?;
        for old in &before {
            let doc = self
                .app
                .documents
                .iter()
                .find(|doc| doc.id == old.id)
                .ok_or("Apply removed an open document")?;
            require(
                doc.saved_text == old.saved_text
                    && doc.revision == old.revision
                    && doc.saving == old.saving
                    && doc.interrupted_save == old.interrupted
                    && doc.save_outcome_unverifiable == old.save_outcome_unverifiable,
                "Apply changed a saved baseline or save safety state",
            )?;
            require(
                if doc.id == id {
                    doc.text == expected && doc.edit_version == old.version + 1
                } else {
                    doc.text == old.text && doc.edit_version == old.version
                },
                "Apply changed wrong text, edit version or tab",
            )?;
        }
        require(
            self.ledger.ordered == ledger,
            "Apply dispatched an operation",
        )?;
        self.local()
    }

    fn stale(&mut self, id: u64, change: impl FnOnce(&mut Self) -> Check<()>) -> Check<()> {
        self.preview(id)?;
        let pos = self.press("Apply")?;
        change(self)?;
        let active = self
            .app
            .active_document
            .ok_or("stale case active document absent")?;
        let before = self.buffers();
        let selected = self.selection(active)?;
        let history = self.history(active)?;
        self.release(pos)?;
        self.preview_visible(false)?;
        self.preserved(&before, active, selected, &history)
    }

    fn case(&mut self, name: &'static str) -> Check<()> {
        self.idle()?;
        self.local()?;
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

fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

fn text_rect(output: &egui::FullOutput, expected: &str) -> Option<egui::Rect> {
    fn find(shape: &egui::epaint::Shape, expected: &str) -> Option<egui::Rect> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text == expected => {
                Some(text.visual_bounding_rect())
            }
            egui::epaint::Shape::Vec(shapes) => {
                shapes.iter().find_map(|shape| find(shape, expected))
            }
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, expected))
}

#[test]
#[ignore = "requires explicit CEDAR_SELECTION_REPLACE_AGENT_BIN selecting normal shipping agent"]
fn normal_agent_selection_replace_is_local_and_preserves_history() -> Check<()> {
    let mut h = Harness::new()?;
    h.connect()?;
    let first = h.open(FIRST, true)?;
    let second = h.open(SECOND, true)?;
    require(
        h.ledger.counts() == (1, 2, 0, 0) && h.ledger.ordered.len() == 4,
        "setup ledger differed",
    )?;
    let setup = h.ledger.ordered.clone();

    // Fixed scalar offsets include multibyte alpha/omega, emoji, and a separate
    // combining mark. Identical matches before and after the selection stay exact.
    for (index, path, baseline, selected, expected) in [
        (
            0,
            FIRST,
            FIRST_BASE,
            range(25, 11),
            "out cat 😀\r\nα $1\\😀 e\u{301} $1\\😀 Ω\r\ntail cat\r\n",
        ),
        (
            1,
            FIRST,
            FIRST_BASE,
            range(11, 25),
            "out cat 😀\r\nα $1\\😀 e\u{301} $1\\😀 Ω\r\ntail cat\r\n",
        ),
        (
            2,
            SECOND,
            SECOND_BASE,
            range(24, 10),
            "out cat 😀\nα $1\\😀 e\u{301} $1\\😀 Ω\ntail cat\n",
        ),
        (
            3,
            SECOND,
            SECOND_BASE,
            range(10, 24),
            "out cat 😀\nα $1\\😀 e\u{301} $1\\😀 Ω\ntail cat\n",
        ),
    ] {
        let id = h.open(path, false)?;
        h.configure(id, selected, "cat", REPLACEMENT)?;
        h.preview(id)?;
        h.apply_text(path, expected)?;
        h.undo(false, path, baseline)?;
        require(
            selection_key(h.selection(id)?) == selection_key(selected),
            "atomic Undo did not restore complete selected range",
        )?;
        h.exhausted(false, path)?;
        h.undo(true, path, expected)?;
        h.exhausted(true, path)?;
        h.undo(false, path, baseline)?;
        h.case(CASES[index])?;
    }

    // Multiple meaningful native checkpoints and a live multi-step redo branch.
    h.open(SECOND, false)?;
    let drafts: Vec<String> = (1..=4)
        .map(|index| format!("{SECOND_BASE}draft {index}\n"))
        .collect();
    for text in &drafts {
        h.edit(SECOND, text)?;
    }
    h.undo(false, SECOND, &drafts[2])?;
    h.undo(false, SECOND, &drafts[1])?;
    h.configure(second, range(10, 24), "cat", "dog")?;
    let buffers = h.buffers();
    let selected = h.selection(second)?;
    let history = h.history(second)?;
    require(
        history[0].len() >= 2 && history[1].len() == 2,
        "fixture lacks complete native undo/redo branches",
    )?;
    h.preview(second)?;
    h.click("Cancel")?;
    h.preview_visible(false)?;
    h.preserved(&buffers, second, selected, &history)?;
    h.case(CASES[4])?;

    h.configure(second, range(24, 10), "cat", "cat")?;
    let buffers = h.buffers();
    let selected = h.selection(second)?;
    let history = h.history(second)?;
    h.preview(second)?;
    h.click("Apply")?;
    h.preview_visible(false)?;
    h.preserved(&buffers, second, selected, &history)?;
    h.undo(true, SECOND, &drafts[2])?;
    h.undo(true, SECOND, &drafts[3])?;
    h.exhausted(true, SECOND)?;
    for expected in [&drafts[2], &drafts[1], &drafts[0]] {
        h.undo(false, SECOND, expected)?;
    }
    h.undo(false, SECOND, SECOND_BASE)?;
    h.exhausted(false, SECOND)?;
    for expected in &drafts {
        h.undo(true, SECOND, expected)?;
    }
    h.exhausted(true, SECOND)?;
    h.case(CASES[5])?;

    h.open(FIRST, false)?;
    h.configure(first, range(13, 13), "cat", "dog")?;
    let buffers = h.buffers();
    let selected = h.selection(first)?;
    let history = h.history(first)?;
    h.click("Preview selection")?;
    h.preview_visible(false)?;
    h.preserved(&buffers, first, selected, &history)?;
    h.case(CASES[6])?;

    // First and last cat cross the half-open selected interval. Only the two
    // fully contained middle matches may change, even with empty replacement.
    h.configure(first, range(34, 5), "cat", "")?;
    h.preview(first)?;
    h.apply_text(FIRST, "out cat 😀\r\nα  e\u{301}  Ω\r\ntail cat\r\n")?;
    h.undo(false, FIRST, FIRST_BASE)?;
    h.case(CASES[7])?;

    h.configure(first, range(25, 11), "cat", "dog")?;
    h.stale(first, |h| h.select(first, range(24, 12)))?;
    h.case(CASES[8])?;

    for primary in [true, false] {
        h.configure(first, range(25, 11), "cat", "dog")?;
        h.stale(first, |h| {
            let mut selected = h.selection(first)?;
            if primary {
                selected.primary.prefer_next_row = !selected.primary.prefer_next_row;
            } else {
                selected.secondary.prefer_next_row = !selected.secondary.prefer_next_row;
            }
            h.select(first, selected)
        })?;
    }
    h.case(CASES[9])?;

    h.configure(first, range(25, 11), "cat", "dog")?;
    h.stale(first, |h| {
        h.edit(FIRST, &format!("{FIRST_BASE}newer draft\r\n"))
    })?;
    h.case(CASES[10])?;

    // Change each independent workspace owner without relying on navigation
    // invalidation. Restore the owner only after the stale Apply was rejected.
    for variant in 0..3 {
        h.configure(first, range(25, 11), "cat", "dog")?;
        let root = h.app.root.clone();
        let workspace = h.app.workspace_key.clone();
        let form = h.app.active_form.clone();
        h.stale(first, |h| {
            match variant {
                0 => h.app.root.push_str("/changed-owner"),
                1 => {
                    h.app.workspace_key = Some(WorkspaceKey::Local {
                        root: "changed-workspace-owner".into(),
                    })
                }
                _ => h.app.active_form = None,
            }
            Ok(())
        })?;
        h.app.root = root;
        h.app.workspace_key = workspace;
        h.app.active_form = form;
    }
    h.case(CASES[11])?;

    for generation in [true, false] {
        h.configure(first, range(25, 11), "cat", "dog")?;
        let state = h.app.state;
        h.stale(first, |h| {
            if generation {
                h.app.generation += 1;
            } else {
                h.app.state = ConnectionState::Connecting;
            }
            Ok(())
        })?;
        h.app.state = state;
    }
    h.case(CASES[12])?;

    h.configure(first, range(25, 11), "cat", "dog")?;
    h.stale(first, |h| {
        h.app.activate_history_tab(second);
        Ok(())
    })?;
    h.case(CASES[13])?;
    require(
        h.ledger.ordered == setup,
        "local actions changed setup ledger",
    )?;

    h.configure(second, range(24, 10), "cat", "dog")?;
    h.preview(second)?;
    let pos = h.press("Apply")?;
    let buffers = h.buffers();
    let selected = h.selection(second)?;
    let history = h.history(second)?;
    h.app
        .disconnected("synthetic explicit selection replacement reconnect".into());
    h.idle()?;
    h.connect()?;
    h.release(pos)?;
    h.preview_visible(false)?;
    h.preserved(&buffers, second, selected, &history)?;
    h.case(CASES[14])?;

    let reconnected = h.ledger.ordered.clone();
    h.configure(second, range(10, 24), "cat", "dog")?;
    h.preview(second)?;
    let expected = "out cat 😀\nα dog e\u{301} dog Ω\ntail cat\ndraft 4\n";
    h.apply_text(SECOND, expected)?;
    h.undo(false, SECOND, &drafts[3])?;
    h.undo(true, SECOND, expected)?;
    require(
        h.ledger.ordered == reconnected,
        "post-reconnect replacement dispatched an operation",
    )?;
    h.case(CASES[15])?;

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
        "selection_replace_acceptance {}",
        serde_json::json!({
            "schema": 1, "acceptance": "selection_replace", "success": true,
            "os": if cfg!(windows) { "windows" } else { "linux" },
            "cases": h.cases, "cases_executed": 16, "operation_ledger": h.ledger.ordered,
            "requests": 6, "hello": h.connections, "list": h.ledger.lists.len(), "read": h.ledger.reads.len(),
            "write": h.ledger.writes, "other_operations": h.ledger.other, "measured_replacement_operations": 0,
            "connections": h.connections, "agents_reaped": h.reaped, "source_hashes": source_hashes,
            "trust_off": true, "no_language_session": true, "production_button_frames": true,
            "unicode_scalar_selection": true, "lf_crlf_outside_bytes_preserved": true,
            "forward_reversed_selection": true, "preview_cancel_preserved": true,
            "atomic_undo_redo": true, "full_history_and_no_op_preserved": true,
            "empty_selection_refused": true, "crossing_matches_excluded": true,
            "dirty_baselines_preserved": true, "stale_selection_and_affinity_rejected": true,
            "stale_edit_workspace_session_tab_rejected": true, "reconnect_rejected_old_preview": true,
            "exact_operation_ledger_verified": true, "shared_watchdog_seconds": 60,
            "total_budget_seconds": 65, "request_admission_seconds": 30, "cleanup_timeout_seconds": 5,
            "owned_children_reaped": true, "watchdog_joined": true, "fixture_removed": true,
            "optional_font_probe_untriggered": true,
        })
    );
    Ok(())
}
