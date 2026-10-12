//! Finite duplicate-lines proof through the normal shipping trust-off agent.
//! Generated sources, actual Ctrl/Cmd+D frames, and an exact operation ledger.
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
const FIRST: &str = "duplicate 😀/CRLF café.txt";
const SECOND: &str = "duplicate 😀/LF Ω.txt";
const FIRST_BASE: &str = "\tα😀e\u{301} café\r\nβ\tΩ\r\n\r\nlast\r\n";
const SECOND_BASE: &str = "\tα😀e\u{301} café\nβ\tΩ\n\nlast\n";
const CASES: [&str; 20] = [
    "unicode_crlf_collapsed",
    "unicode_lf_collapsed",
    "unicode_crlf_forward_block",
    "unicode_crlf_reversed_block",
    "unicode_lf_forward_block",
    "unicode_lf_reversed_block",
    "upper_endpoint_line_start_excluded",
    "mixed_line_endings_exact",
    "unterminated_nearest_separator",
    "empty_and_trailing_blank",
    "strict_modifiers_and_repeat_refused",
    "mixed_edit_frames_native_only",
    "focus_and_modal_guards",
    "stale_identity_and_selection_guards",
    "byte_and_line_bounds_refused",
    "dirty_baselines_and_other_tab_preserved",
    "atomic_full_selection_undo_redo",
    "stale_reconnect_capture_rejected",
    "history_after_reconnect",
    "duplication_after_reconnect",
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
            std::env::var_os("CEDAR_DUPLICATE_LINES_AGENT_BIN")
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
            .prefix("cedar-duplicate-lines 😀-")
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
        fs::create_dir(root.path().join("duplicate 😀"))
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
        require(
            env!("CARGO_PKG_VERSION") == "0.62.2"
                && self.app.agent_info.as_ref().is_some_and(|info| {
                    info.version == env!("CARGO_PKG_VERSION")
                        && info.os == if cfg!(windows) { "windows" } else { "linux" }
                }),
            "normal agent and source versions or platform differed",
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
        self.frame_after_capture(events, |_| Ok(()))
    }

    fn frame_after_capture(
        &mut self,
        events: Vec<egui::Event>,
        change: impl FnOnce(&mut Self) -> Check<()>,
    ) -> Check<egui::FullOutput> {
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
        change(self)?;
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

    fn local(&self) -> Check<()> {
        require(
            !self.app.execution_trusted()
                && !self.app.recovery.has_actor()
                && !self.app.language.running
                && !self.app.language.startup_active(),
            "local duplication enabled execution, language or recovery work",
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

    fn undo(&mut self, redo: bool, path: &str, expected: &str) -> Check<()> {
        let id = self.document(path)?.id;
        require(self.app.active_document == Some(id), "Undo target inactive")?;
        let version = self.document(path)?.edit_version;
        let saved = self.document(path)?.saved_text.clone();
        let revision = self.document(path)?.revision.clone();
        let saving = self.document(path)?.saving;
        let interrupted = self.document(path)?.interrupted_save.clone();
        let unknown = self.document(path)?.save_outcome_unverifiable;
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
                && doc.revision == revision
                && doc.saving == saving
                && doc.interrupted_save == interrupted
                && doc.save_outcome_unverifiable == unknown,
            "one native Undo or Redo lost text history, baseline or save safety state",
        )?;
        self.local()
    }

    fn prepare(&mut self, path: &str, text: &str, selected: Selection) -> Check<u64> {
        let id = self.open(path, false)?;
        if self.document(path)?.text != text {
            self.edit(path, text)?;
        }
        self.select(id, selected)?;
        self.settle()?;
        self.app.editor_ctx.memory_mut(|memory| {
            memory.request_focus(egui::Id::new(("editor", id)));
        });
        require(
            selection_key(self.selection(id)?) == selection_key(selected),
            "preparation changed full selection",
        )?;
        Ok(id)
    }

    fn duplicate(&mut self, path: &str, expected: &str, selected: Selection) -> Check<()> {
        let before = self.buffers();
        let id = self.document(path)?.id;
        let ledger = self.ledger.ordered.clone();
        self.frame(key(egui::Key::D, command()))?;
        for old in &before {
            let doc = self
                .app
                .documents
                .iter()
                .find(|doc| doc.id == old.id)
                .ok_or("Duplicate removed an open document")?;
            require(
                doc.saved_text == old.saved_text
                    && doc.revision == old.revision
                    && doc.saving == old.saving
                    && doc.interrupted_save == old.interrupted
                    && doc.save_outcome_unverifiable == old.save_outcome_unverifiable,
                "Duplicate changed saved baseline or save safety state",
            )?;
            require(
                if doc.id == id {
                    doc.text == expected
                        && doc.edit_version == old.version + 1
                        && doc.dirty() == (expected != old.saved_text)
                } else {
                    doc.text == old.text && doc.edit_version == old.version
                },
                "Duplicate changed wrong text, edit version or tab",
            )?;
        }
        require(
            selection_key(self.selection(id)?) == selection_key(selected),
            "Duplicate lost scalar endpoints, orientation or affinity",
        )?;
        require(
            self.ledger.ordered == ledger,
            "Duplicate dispatched an operation",
        )?;
        self.local()
    }

    fn cycle(
        &mut self,
        path: &str,
        before: &str,
        selected: Selection,
        after: &str,
        offset: usize,
    ) -> Check<()> {
        let id = self.prepare(path, before, selected)?;
        let mapped = shifted(selected, offset);
        self.duplicate(path, after, mapped)?;
        self.undo(false, path, before)?;
        require(
            selection_key(self.selection(id)?) == selection_key(selected),
            "one Undo did not restore complete original selection",
        )?;
        self.undo(true, path, after)?;
        require(
            selection_key(self.selection(id)?) == selection_key(mapped),
            "one Redo did not restore complete duplicate selection",
        )?;
        self.undo(false, path, before)?;
        require(
            selection_key(self.selection(id)?) == selection_key(selected),
            "second Undo lost original selection",
        )
    }

    fn refusal(&mut self, events: Vec<egui::Event>) -> Check<()> {
        let id = self.app.active_document.ok_or("refusal target absent")?;
        let buffers = self.buffers();
        let selected = self.selection(id)?;
        let history = self.history(id)?;
        self.frame(events)?;
        self.preserved(&buffers, id, selected, &history)
    }

    fn stale_capture(&mut self, change: impl FnOnce(&mut Self) -> Check<()>) -> Check<()> {
        let mut snapshot = None;
        self.frame_after_capture(key(egui::Key::D, command()), |h| {
            require(
                h.app.duplicate_input.is_some(),
                "stale case did not capture a real production grant",
            )?;
            change(h)?;
            let id = h.app.active_document.ok_or("stale target absent")?;
            snapshot = Some((h.buffers(), id, h.selection(id)?, h.history(id)?));
            Ok(())
        })?;
        let (buffers, id, selected, history) = snapshot.ok_or("stale observation absent")?;
        self.preserved(&buffers, id, selected, &history)?;
        self.settle()?;
        self.preserved(&buffers, id, selected, &history)
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
fn command() -> egui::Modifiers {
    egui::Modifiers {
        ctrl: true,
        command: true,
        ..egui::Modifiers::NONE
    }
}

fn shifted(mut selected: Selection, offset: usize) -> Selection {
    selected.primary.index += offset;
    selected.secondary.index += offset;
    selected
}

#[test]
#[ignore = "requires explicit CEDAR_DUPLICATE_LINES_AGENT_BIN selecting normal shipping agent"]
fn normal_agent_duplicate_lines_is_local_and_preserves_history() -> Check<()> {
    let mut h = Harness::new()?;
    h.connect()?;
    let first = h.open(FIRST, true)?;
    let second = h.open(SECOND, true)?;
    require(
        h.ledger.counts() == (1, 2, 0, 0) && h.ledger.ordered.len() == 4,
        "setup ledger differed",
    )?;
    let setup = h.ledger.ordered.clone();

    // Independent literal expectations and scalar offsets, never the planner's
    // own result: tab, emoji and the combining mark each occupy one scalar.
    let crlf_line = "\tα😀e\u{301} café\r\n\tα😀e\u{301} café\r\nβ\tΩ\r\n\r\nlast\r\n";
    let lf_line = "\tα😀e\u{301} café\n\tα😀e\u{301} café\nβ\tΩ\n\nlast\n";
    let crlf_block = "\tα😀e\u{301} café\r\nβ\tΩ\r\n\tα😀e\u{301} café\r\nβ\tΩ\r\n\r\nlast\r\n";
    let lf_block = "\tα😀e\u{301} café\nβ\tΩ\n\tα😀e\u{301} café\nβ\tΩ\n\nlast\n";
    for (index, path, base, selected, expected, offset) in [
        (0, FIRST, FIRST_BASE, range(2, 2), crlf_line, 12),
        (1, SECOND, SECOND_BASE, range(4, 4), lf_line, 11),
        (2, FIRST, FIRST_BASE, range(14, 2), crlf_block, 17),
        (3, FIRST, FIRST_BASE, range(2, 14), crlf_block, 17),
        (4, SECOND, SECOND_BASE, range(13, 2), lf_block, 15),
        (5, SECOND, SECOND_BASE, range(2, 13), lf_block, 15),
    ] {
        h.cycle(path, base, selected, expected, offset)?;
        h.case(CASES[index])?;
    }
    for selected in [range(12, 2), range(2, 12)] {
        h.cycle(FIRST, FIRST_BASE, selected, crlf_line, 12)?;
    }
    h.case(CASES[6])?;

    let mixed = "α\r\n😀\n\tΩ\r\nlast";
    for selected in [range(6, 1), range(1, 6)] {
        h.cycle(
            FIRST,
            mixed,
            selected,
            "α\r\n😀\n\tΩ\r\nα\r\n😀\n\tΩ\r\nlast",
            9,
        )?;
    }
    // Both sides of CRLF are valid scalar positions on the preceding line.
    h.cycle(FIRST, "α\r\nΩ", range(2, 2), "α\r\nα\r\nΩ", 3)?;
    h.case(CASES[7])?;

    for (before, selected, after, offset) in [
        ("α\r\n😀\nΩ", range(6, 6), "α\r\n😀\nΩ\nΩ", 2),
        ("α\n😀\r\nΩ", range(6, 6), "α\n😀\r\nΩ\r\nΩ", 3),
        ("α😀", range(1, 1), "α😀\nα😀", 3),
        ("α\rβ", range(2, 2), "α\rβ\nα\rβ", 4),
        ("α\n😀\r\nΩ", range(3, 6), "α\n😀\r\nΩ\r\n😀\r\nΩ", 6),
    ] {
        h.cycle(FIRST, before, selected, after, offset)?;
    }
    h.case(CASES[8])?;

    for (before, selected, after, offset) in [
        ("", range(0, 0), "\n", 1),
        ("α\n", range(2, 2), "α\n\n", 1),
        ("α\r\n", range(3, 3), "α\r\n\r\n", 2),
        ("α\n\nΩ", range(2, 2), "α\n\n\nΩ", 1),
    ] {
        h.cycle(FIRST, before, selected, after, offset)?;
    }
    h.case(CASES[9])?;

    h.prepare(FIRST, FIRST_BASE, range(2, 9))?;
    for modifiers in [
        egui::Modifiers::NONE,
        egui::Modifiers::COMMAND,
        egui::Modifiers::CTRL,
        command() | egui::Modifiers::ALT,
        command() | egui::Modifiers::SHIFT,
        egui::Modifiers {
            mac_cmd: true,
            ..command()
        },
    ] {
        h.refusal(key(egui::Key::D, modifiers))?;
    }
    let mut repeated = key(egui::Key::D, command());
    for event in &mut repeated {
        if let egui::Event::Key { repeat, .. } = event {
            *repeat = true;
        }
    }
    h.refusal(repeated)?;
    let mut twice = key(egui::Key::D, command());
    twice.extend(key(egui::Key::D, command()));
    h.refusal(twice)?;
    h.case(CASES[10])?;

    // A mixed native text event may edit once, but the same-frame D never gets
    // a line-copy grant. Both event orders have an independent literal result.
    for duplicate_first in [true, false] {
        h.prepare(FIRST, "α😀\nΩ", range(1, 1))?;
        let version = h.document(FIRST)?.edit_version;
        let mut events = key(egui::Key::D, command());
        if duplicate_first {
            events.push(egui::Event::Text("Q".into()));
        } else {
            events.insert(0, egui::Event::Text("Q".into()));
        }
        h.frame(events)?;
        require(
            h.document(FIRST)?.text == "αQ😀\nΩ" && h.document(FIRST)?.edit_version == version + 1,
            "mixed native text input gained duplication",
        )?;
        h.undo(false, FIRST, "α😀\nΩ")?;
    }
    h.prepare(FIRST, "α😀\nΩ", range(1, 1))?;
    let mut events = key(egui::Key::D, command());
    events.push(egui::Event::Paste("Q".into()));
    h.frame(events)?;
    require(
        h.document(FIRST)?.text == "αQ😀\nΩ",
        "mixed paste gained duplication",
    )?;
    h.undo(false, FIRST, "α😀\nΩ")?;
    h.case(CASES[11])?;

    h.prepare(FIRST, FIRST_BASE, range(2, 9))?;
    h.app
        .editor_ctx
        .memory_mut(|memory| memory.surrender_focus(egui::Id::new(("editor", first))));
    h.refusal(key(egui::Key::D, command()))?;
    for kind in 0..3 {
        h.prepare(FIRST, FIRST_BASE, range(2, 9))?;
        match kind {
            0 => h.app.open_form = true,
            1 => {
                h.app.find_open = true;
                h.app.find_focus = true;
            }
            _ => h.app.confirm = Some(Confirm::CloseTab(first)),
        }
        h.refusal(key(egui::Key::D, command()))?;
        h.app.open_form = false;
        h.app.find_open = false;
        h.app.find_focus = false;
        h.app.confirm = None;
        h.settle()?;
    }
    h.case(CASES[12])?;

    for kind in 0..8 {
        h.prepare(FIRST, FIRST_BASE, range(2, 9))?;
        let generation = h.app.generation;
        let session = h.app.language.session;
        let navigation = h.app.navigation_epoch;
        let workspace = h.app.workspace_key.clone();
        let root = h.app.root.clone();
        h.stale_capture(|h| {
            match kind {
                0 => h.select(first, range(3, 9))?,
                1 => {
                    let mut selected = range(2, 9);
                    selected.primary.prefer_next_row = false;
                    selected.secondary.prefer_next_row = true;
                    h.select(first, selected)?;
                }
                2 => h.app.generation += 1,
                3 => h.app.language.session += 1,
                4 => h.app.navigation_epoch += 1,
                5 => h.app.workspace_key = None,
                6 => h.app.root.push_str("-new-identity"),
                _ => h.app.activate_history_tab(second),
            }
            Ok(())
        })?;
        h.app.generation = generation;
        h.app.language.session = session;
        h.app.navigation_epoch = navigation;
        h.app.workspace_key = workspace;
        h.app.root = root;
    }
    h.case(CASES[13])?;

    // Only generated in-memory drafts are large. No additional Read, Write or
    // fixture is admitted; emoji keeps the glyph count below the byte count.
    for text in [
        "😀".repeat(131_073),
        "😀".repeat(262_145),
        "x\n".repeat(4097),
    ] {
        let end = text.chars().count();
        h.prepare(FIRST, &text, range(end, 0))?;
        h.refusal(key(egui::Key::D, command()))?;
    }
    h.prepare(FIRST, FIRST_BASE, range(2, 9))?;
    h.case(CASES[14])?;

    // Saving and uncertain-save metadata remain editable, matching ordinary
    // Enter semantics. Duplication cannot acknowledge, save or clear that state.
    h.prepare(SECOND, "other unsaved Ω\n", range(2, 2))?;
    h.prepare(FIRST, "dirty α😀\r\nΩ", range(7, 2))?;
    let token = interrupted_save::InterruptedSave::capture(&h.app, h.document(FIRST)?)
        .ok_or("synthetic interrupted-save token capture failed")?;
    for state in 0..3 {
        let doc = h
            .app
            .documents
            .iter_mut()
            .find(|doc| doc.id == first)
            .ok_or("save-state target absent")?;
        doc.saving = state == 0;
        doc.interrupted_save = (state == 1).then(|| token.clone());
        doc.save_outcome_unverifiable = state == 2;
        h.cycle(
            FIRST,
            "dirty α😀\r\nΩ",
            range(7, 2),
            "dirty α😀\r\ndirty α😀\r\nΩ",
            10,
        )?;
        let doc = h.document(FIRST)?;
        require(
            doc.saving == (state == 0)
                && doc.interrupted_save == (state == 1).then(|| token.clone())
                && doc.save_outcome_unverifiable == (state == 2),
            "duplicate Undo/Redo changed synthetic save safety state",
        )?;
    }
    let doc = h
        .app
        .documents
        .iter_mut()
        .find(|doc| doc.id == first)
        .ok_or("save-state target absent")?;
    doc.saving = false;
    doc.interrupted_save = None;
    doc.save_outcome_unverifiable = false;
    require(
        h.document(SECOND)?.text == "other unsaved Ω\n",
        "unrelated dirty tab changed",
    )?;
    h.case(CASES[15])?;

    // Explicitly checkpoint two earlier edits. Inspect every reachable native
    // point, including both affinities, before/after a full Undo/Redo cycle.
    h.prepare(FIRST, "one α😀\n", range(2, 6))?;
    let mut state = egui::TextEdit::load_state(&h.app.editor_ctx, egui::Id::new(("editor", first)))
        .ok_or("history state absent")?;
    let mut history = egui::util::undoer::Undoer::with_settings(egui::util::undoer::Settings {
        max_undos: editor_state::MAX_UNDO_STATES,
        ..Default::default()
    });
    for (selection, text) in [
        (range(1, 2), "old α\n"),
        (range(3, 1), "older 😀\n"),
        (range(2, 6), "one α😀\n"),
    ] {
        history.add_undo(&(selection, text.to_owned()));
    }
    state.set_undoer(history);
    state.store(&h.app.editor_ctx, egui::Id::new(("editor", first)));
    let past = h.history(first)?[0].clone();
    h.duplicate(FIRST, "one α😀\none α😀\n", range(9, 13))?;
    h.undo(false, FIRST, "one α😀\n")?;
    require(
        selection_key(h.selection(first)?) == selection_key(range(2, 6))
            && h.history(first)?[0] == past,
        "one Undo lost full selection or earlier native history",
    )?;
    h.undo(true, FIRST, "one α😀\none α😀\n")?;
    require(
        selection_key(h.selection(first)?) == selection_key(range(9, 13)),
        "one Redo lost full copied selection",
    )?;
    h.case(CASES[16])?;
    require(
        h.ledger.ordered == setup,
        "duplication phase changed exact setup ledger",
    )?;

    let before_reconnect = h.history(first)?;
    h.stale_capture(|h| {
        h.app
            .disconnected("synthetic explicit duplicate-lines reconnect".into());
        h.idle()?;
        h.connect()
    })?;
    require(
        h.history(first)? == before_reconnect,
        "reconnect changed complete native history",
    )?;
    h.case(CASES[17])?;

    let reconnected = h.ledger.ordered.clone();
    h.undo(false, FIRST, "one α😀\n")?;
    require(
        selection_key(h.selection(first)?) == selection_key(range(2, 6)),
        "reconnect Undo lost original full selection",
    )?;
    h.undo(true, FIRST, "one α😀\none α😀\n")?;
    require(
        selection_key(h.selection(first)?) == selection_key(range(9, 13)),
        "reconnect Redo lost duplicated full selection",
    )?;
    h.case(CASES[18])?;
    h.cycle(
        SECOND,
        "after α😀\n",
        range(7, 2),
        "after α😀\nafter α😀\n",
        9,
    )?;
    require(
        h.ledger.ordered == reconnected,
        "post-reconnect duplication dispatched an operation",
    )?;
    h.case(CASES[19])?;

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
    let mut receipt = serde_json::json!({
        "schema": 1, "acceptance": "duplicate_lines", "success": true,
        "os": if cfg!(windows) { "windows" } else { "linux" },
        "source_version": env!("CARGO_PKG_VERSION"), "source_agent_version_match": true,
        "cases": h.cases, "cases_executed": 20, "operation_ledger": h.ledger.ordered,
        "requests": 6, "hello": h.connections, "list": h.ledger.lists.len(), "read": h.ledger.reads.len(),
        "write": h.ledger.writes, "other_operations": h.ledger.other, "measured_duplicate_operations": 0,
        "connections": h.connections, "agents_reaped": h.reaped, "source_hashes": source_hashes,
    });
    let fields = receipt.as_object_mut().ok_or("receipt object absent")?;
    for name in [
        "trust_off",
        "no_language_session",
        "production_keyboard_frames",
        "unicode_scalar_endpoints",
        "lf_crlf_mixed_bytes_preserved",
        "forward_reversed_selection",
        "upper_line_start_excluded",
        "unterminated_nearest_separator",
        "empty_trailing_blank_supported",
        "strict_modifiers_repeat_refused",
        "mixed_input_not_duplicated",
        "focus_modal_stale_capture_guarded",
        "byte_and_line_bounds_refused",
        "dirty_baselines_and_save_state_preserved",
        "unrelated_tab_preserved",
        "atomic_full_selection_undo_redo",
        "complete_native_history_preserved",
        "reconnect_rejected_old_capture",
        "reconnect_undo_redo_preserved",
        "exact_operation_ledger_verified",
        "owned_children_reaped",
        "watchdog_joined",
        "fixture_removed",
        "optional_font_probe_untriggered",
    ] {
        require(
            fields
                .insert(name.into(), serde_json::Value::Bool(true))
                .is_none(),
            "duplicate receipt field",
        )?;
    }
    for (name, value) in [
        ("shared_watchdog_seconds", 60),
        ("total_budget_seconds", 65),
        ("request_admission_seconds", 30),
        ("cleanup_timeout_seconds", 5),
    ] {
        require(
            fields
                .insert(name.into(), serde_json::json!(value))
                .is_none(),
            "duplicate receipt field",
        )?;
    }
    require(fields.len() == 47, "receipt field count differed")?;
    println!("duplicate_lines_acceptance {receipt}");
    Ok(())
}
