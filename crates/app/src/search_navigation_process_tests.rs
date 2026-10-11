//! Finite Project Search keyboard proof through the normal shipping trust-off agent.
//! Generated Unicode text fixtures, production keyboard frames, and an exact operation ledger.
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
const DRAFT: &str = "search 😀/a draft café.txt";
const UNOPENED: &str = "search 😀/b target Ω.txt";
const QUERY: &str = "needle café 😀";
const EMPTY_QUERY: &str = "absent search marker Ω";
const FILE_ROWS: usize = 96;
const ROWS: usize = FILE_ROWS * 2;
const CASES: [&str; 14] = [
    "explicit_trust_off_search",
    "exact_path_line_identity_order",
    "arrow_boundary_clamp",
    "home_end_reveal",
    "dirty_enter_zero_read_exact_back",
    "unopened_enter_exact_one_read",
    "ready_batch_capture_rejected",
    "newer_search_rejects_late_reply",
    "stale_session_capture_rejected",
    "fresh_reconnect_navigation",
    "stale_query_capture_rejected",
    "empty_results_inert",
    "dirty_full_selection_native_undo_redo",
    "exact_ledger_disk_and_owner_cleanup",
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
    searches: Vec<String>,
    writes: u32,
    other: u32,
}

impl Ledger {
    fn record(&mut self, operation: &Operation, session: u32, phase: &str) {
        self.ordered.push(match operation {
            Operation::List { path } => {
                serde_json::json!({"session": session, "phase": phase, "op": "List", "source": source_label(path)})
            }
            Operation::Read { path } => {
                serde_json::json!({"session": session, "phase": phase, "op": "Read", "source": source_label(path)})
            }
            Operation::Search { query, limit } => {
                serde_json::json!({"session": session, "phase": phase, "op": "Search", "query": query_label(query), "limit": limit})
            }
            Operation::Write { .. } => serde_json::json!({"session": session, "phase": phase, "op": "Write"}),
            _ => serde_json::json!({"session": session, "phase": phase, "op": "Other"}),
        });
        match operation {
            Operation::List { path } => self.lists.push(path.clone()),
            Operation::Read { path } => self.reads.push(path.clone()),
            Operation::Search { query, .. } => self.searches.push(query.clone()),
            Operation::Write { .. } => self.writes += 1,
            _ => self.other += 1,
        }
    }

    fn counts(&self) -> (usize, usize, usize, u32, u32) {
        (
            self.lists.len(),
            self.reads.len(),
            self.searches.len(),
            self.writes,
            self.other,
        )
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
    phase: &'static str,
    reaped: u32,
    cases: Vec<&'static str>,
    cleanup_uncertain: bool,
    receipt_emitted: bool,
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
        if !self.receipt_emitted {
            self.progress();
        }
    }
}

impl Harness {
    fn new() -> Check<Self> {
        let started = Instant::now();
        let agent = PathBuf::from(
            std::env::var_os("CEDAR_SEARCH_NAVIGATION_AGENT_BIN")
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
            .prefix("cedar-search-navigation 😀-")
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
        fs::create_dir(root.path().join("search 😀"))
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
            phase: "setup",
            reaped: 0,
            cases: Vec::new(),
            cleanup_uncertain: false,
            receipt_emitted: false,
            cancellation,
            watchdog: Some((stop, watchdog)),
        };
        for (path, text) in [
            (DRAFT, generated_text("saved")),
            (UNOPENED, generated_text("target")),
        ] {
            h.budget()?;
            fs::write(
                h.root
                    .as_ref()
                    .ok_or("fixture root absent")?
                    .path()
                    .join(path),
                &text,
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
            !self.cleanup_uncertain
                && !self.cancellation.is_cancelled()
                && self.started.elapsed() + reserve < BUDGET,
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
        self.phase = if self.connections == 0 {
            "setup"
        } else {
            "reconnect"
        };
        self.connections += 1;
        self.ledger.ordered.push(
            serde_json::json!({"session": self.connections, "phase": self.phase, "op": "Hello"}),
        );
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
            env!("CARGO_PKG_VERSION") == "0.57.2"
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
        self.ledger
            .record(&command.op, self.connections, self.phase);
        Ok(command)
    }

    fn exchange(&mut self, command: Command) -> Check<Event> {
        self.admit(REQUEST)?;
        require(
            matches!(
                &command.op,
                Operation::List { .. } | Operation::Read { .. } | Operation::Search { .. }
            ),
            "app dispatched an operation other than List Read or Search",
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
            self.ledger
                .record(&command.op, self.connections, self.phase);
            return Err("unsolicited or queued request was dispatched");
        }
        Ok(())
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> Check<egui::FullOutput> {
        let output = self.render_after_capture(events, false, |_| Ok(()))?;
        self.idle()?;
        Ok(output)
    }

    fn render_after_capture(
        &mut self,
        events: Vec<egui::Event>,
        require_claim: bool,
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
                egui::vec2(1000.0, 820.0),
            )),
            events,
            focused: true,
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut self.app, &ctx, &mut input);
        if require_claim {
            require(
                input.events.is_empty(),
                "stale race did not begin with a production-owned search key",
            )?;
        }
        change(self)?;
        let output = ctx.run(input, |ctx| {
            eframe::App::update(&mut self.app, ctx, &mut native)
        });
        self.budget()?;
        self.local()?;
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
            self.ledger.counts()
                == (
                    before.0,
                    before.1 + usize::from(read),
                    before.2,
                    before.3,
                    before.4,
                ),
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
            "search navigation enabled execution, language or recovery work",
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

    fn progress(&self) {
        let context = serde_json::json!({
            "schema": 1,
            "completed_cases": self.cases.len(),
            "requests": self.ledger.ordered.len(),
            "connections": self.connections,
            "agents_reaped": self.reaped,
            "cleanup_uncertain": self.cleanup_uncertain,
        });
        println!("search_navigation_progress {context}");
    }

    fn source_hashes(&self) -> Check<Vec<serde_json::Value>> {
        let root = self.root.as_ref().ok_or("fixture root absent")?.path();
        let mut hashes = Vec::new();
        for (path, expected) in &self.sources {
            self.budget()?;
            let actual = fs::read(root.join(path)).map_err(|_| "source reread failed")?;
            require(&digest(&actual) == expected, "source disk hash changed")?;
            hashes.push(serde_json::json!({"source": source_label(path), "sha256": format!("{:x}", Sha256::digest(&actual))}));
        }
        self.budget()?;
        Ok(hashes)
    }

    fn stop_watchdog(&mut self) -> Check<()> {
        let Some((stop, watchdog)) = self.watchdog.take() else {
            self.cleanup_uncertain = true;
            self.retain_fixture();
            return Err("watchdog ownership absent; generated fixture retained");
        };
        let _ = stop.send(());
        match watchdog.join() {
            Ok(false) => Ok(()),
            Ok(true) | Err(_) => {
                self.cleanup_uncertain = true;
                self.retain_fixture();
                Err("watchdog stop unverified; generated fixture retained")
            }
        }
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
fn source_label(path: &str) -> &'static str {
    match path {
        "" => "root",
        DRAFT => "draft",
        UNOPENED => "unopened",
        _ => "unexpected",
    }
}

fn query_label(query: &str) -> &'static str {
    match query {
        QUERY => "matches",
        EMPTY_QUERY => "empty",
        _ => "unexpected",
    }
}

fn generated_text(header: &str) -> String {
    let mut text = format!("{header} α😀e\u{301} café\r\n");
    for index in 0..FILE_ROWS {
        text.push_str(&format!("{QUERY} duplicate {index:03} Ω\r\n"));
    }
    text
}

impl Harness {
    fn source(&self) -> Check<search_keyboard::Source> {
        self.app
            .search_state
            .accepted
            .clone()
            .ok_or("accepted search source absent")
    }

    fn focus_probe(&mut self, name: &str) -> Check<egui::Id> {
        let response = workspace_access_tests::recorded_response(&self.app, name);
        response.request_focus();
        self.settle()?;
        require(
            self.app.editor_ctx.memory(|memory| memory.focused()) == Some(response.id),
            "rendered search control did not receive focus",
        )?;
        Ok(response.id)
    }

    fn focus_row(&mut self, index: usize) -> Check<egui::Id> {
        self.focus_probe(&format!("search_row:{index}"))
    }

    fn visible_probe(&self, name: &str) -> Check<()> {
        let rect = workspace_access_tests::recorded_rect(&self.app, name);
        let clip = workspace_access_tests::recorded_clip(&self.app, name);
        require(
            rect.is_finite()
                && clip.is_finite()
                && rect.height() > 0.0
                && rect.width() > 0.0
                && rect.min.y >= clip.min.y - 1.0
                && rect.max.y <= clip.max.y + 1.0
                && rect.intersect(clip).width() > 0.0,
            "focused search target was not revealed in its real viewport",
        )
    }

    fn expect_row(&mut self, index: usize) -> Check<()> {
        self.settle()?;
        let response =
            workspace_access_tests::recorded_response(&self.app, &format!("search_row:{index}"));
        require(
            self.app.search_rows_active()
                && response.id == search_keyboard::row_id(&self.source()?, index)
                && self.app.editor_ctx.memory(|memory| memory.focused()) == Some(response.id),
            "keyboard focus did not retain exact search row identity",
        )?;
        self.visible_probe(&format!("search_row:{index}"))
    }

    fn press(&mut self, key_value: egui::Key) -> Check<()> {
        self.frame(key(key_value, egui::Modifiers::NONE))?;
        Ok(())
    }

    fn query_text(&mut self, text: &str) -> Check<()> {
        self.focus_probe("search_query")?;
        self.frame(key(egui::Key::A, egui::Modifiers::COMMAND))?;
        self.press(egui::Key::Backspace)?;
        if !text.is_empty() {
            self.frame(vec![egui::Event::Text(text.into())])?;
        }
        self.settle()?;
        require(self.app.search_query == text, "query input text differed")
    }

    fn click_submit(&mut self) -> Check<()> {
        self.frame(vec![])?;
        self.visible_probe("search_submit")?;
        let at = workspace_access_tests::recorded_rect(&self.app, "search_submit").center();
        for pressed in [true, false] {
            let events = vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            if pressed {
                self.frame(events)?;
            } else {
                self.render_after_capture(events, false, |_| Ok(()))?;
            }
        }
        Ok(())
    }

    fn search_reply(&mut self, query: &str) -> Check<Event> {
        let command = self.next()?;
        require(
            matches!(&command.op, Operation::Search { query: actual, limit } if actual == query && *limit == 500),
            "explicit search did not dispatch the exact finite Search",
        )?;
        let event = self.exchange(command)?;
        match &event.result {
            Ok(Payload::Matches { matches, truncated }) => {
                require(!truncated, "finite fixture Search was truncated")?;
                let count = if query == QUERY { ROWS } else { 0 };
                require(matches.len() == count, "real Search result count differed")?;
                for (index, result) in matches.iter().enumerate() {
                    let path = if index < FILE_ROWS { DRAFT } else { UNOPENED };
                    let ordinal = index % FILE_ROWS;
                    require(
                        result.path == path
                            && result.line == ordinal + 2
                            && result.text == format!("{QUERY} duplicate {ordinal:03} Ω"),
                        "real Search path line order or Unicode preview differed",
                    )?;
                }
            }
            _ => return Err("ordinary Search reply differed"),
        }
        self.idle()?;
        Ok(event)
    }

    fn search_control(&mut self, query: &str) -> Check<()> {
        self.query_text(query)?;
        self.click_submit()?;
        let reply = self.search_reply(query)?;
        self.app.apply_event(reply);
        self.settle()
    }

    // Race injection dispatches the same production explicit Search method,
    // returns the real normal-agent reply, and defers only owner delivery.
    fn refresh_reply(&mut self) -> Check<Event> {
        self.phase = "refresh";
        self.app.search();
        self.search_reply(QUERY)
    }

    fn exact_jump(&self, path: &str, line: usize) -> Check<()> {
        let document = self.document(path)?;
        let expected = line_start(&document.text, line);
        let selection = self.selection(document.id)?;
        require(
            self.app.active_document == Some(document.id)
                && selection.primary.index == expected
                && selection.secondary.index == expected
                && document.jump_to.is_none()
                && self.app.location_history.pending.is_none(),
            "search activation did not complete the intentional line jump",
        )
    }

    fn back(&mut self, id: u64, selection: Selection) -> Check<()> {
        self.frame(key(egui::Key::OpenBracket, egui::Modifiers::COMMAND))?;
        self.settle()?;
        require(
            self.app.active_document == Some(id)
                && selection_key(self.selection(id)?) == selection_key(selection),
            "Back did not restore the exact departure selection",
        )
    }
}

#[test]
#[ignore = "requires explicit CEDAR_SEARCH_NAVIGATION_AGENT_BIN selecting normal shipping agent"]
fn normal_agent_search_keyboard_navigation_is_local_and_source_bound() -> Check<()> {
    let mut h = Harness::new()?;
    h.connect()?;
    let draft = h.open(DRAFT, true)?;
    h.select(draft, range(2, 10))?;
    editor_state::commit_selection(
        &h.app.editor_ctx,
        &mut h.app.documents[0],
        generated_text("dirty"),
        range(10, 2),
    );
    h.settle()?;
    let selection = h.selection(draft)?;
    let history = h.history(draft)?;
    require(
        h.document(DRAFT)?.dirty()
            && selection_key(selection) == selection_key(range(10, 2))
            && !history[0].is_empty(),
        "dirty draft with full selection and native Undo was not prepared",
    )?;
    h.app.tools_open = true;
    h.app.tool = Tool::Search;
    h.settle()?;
    let original_buffers = h.buffers();
    h.search_control(QUERY)?;
    require(
        h.ledger.counts() == (1, 1, 1, 0, 0)
            && h.ledger.ordered.len() == 4
            && h.app.search_results.len() == ROWS
            && h.app.search_rows_active(),
        "initial explicit search or setup ledger differed",
    )?;
    h.case(CASES[0])?;

    let first_id = h.focus_row(0)?;
    let second_id = workspace_access_tests::recorded_response(&h.app, "search_row:1").id;
    require(
        first_id != second_id,
        "different search lines shared one row identity",
    )?;
    h.expect_row(0)?;
    h.case(CASES[1])?;
    for (key_value, index) in [
        (egui::Key::ArrowUp, 0),
        (egui::Key::ArrowDown, 1),
        (egui::Key::ArrowUp, 0),
    ] {
        h.press(key_value)?;
        h.expect_row(index)?;
    }
    h.case(CASES[2])?;
    for (key_value, index) in [
        (egui::Key::End, ROWS - 1),
        (egui::Key::ArrowDown, ROWS - 1),
        (egui::Key::Home, 0),
        (egui::Key::ArrowDown, 1),
    ] {
        h.press(key_value)?;
        h.expect_row(index)?;
    }
    h.preserved(&original_buffers, draft, selection, &history)?;
    require(
        h.ledger.ordered.len() == 4,
        "row movement dispatched an operation",
    )?;
    h.case(CASES[3])?;

    h.press(egui::Key::Enter)?;
    h.settle()?;
    h.exact_jump(DRAFT, 3)?;
    require(
        h.buffers() == original_buffers,
        "dirty activation changed draft or baseline",
    )?;
    h.back(draft, selection)?;
    h.preserved(&original_buffers, draft, selection, &history)?;
    require(
        h.ledger.ordered.len() == 4,
        "dirty activation or Back dispatched an operation",
    )?;
    h.case(CASES[4])?;

    h.focus_row(1)?;
    h.press(egui::Key::End)?;
    h.expect_row(ROWS - 1)?;
    h.phase = "activation";
    h.render_after_capture(key(egui::Key::Enter, egui::Modifiers::NONE), true, |_| {
        Ok(())
    })?;
    let read = h.next()?;
    require(
        matches!(&read.op, Operation::Read { path } if path == UNOPENED),
        "unopened activation did not dispatch its one exact Read",
    )?;
    let reply = h.exchange(read)?;
    require(
        matches!(&reply.result, Ok(Payload::File { path, text, .. }) if path == UNOPENED && text == &generated_text("target")),
        "unopened Read returned different fixture bytes",
    )?;
    h.app.apply_event(reply);
    h.settle()?;
    h.exact_jump(UNOPENED, FILE_ROWS + 1)?;
    require(
        h.ledger.counts() == (1, 2, 1, 0, 0) && h.app.documents.len() == 2,
        "unopened activation did not admit exactly one new document and Read",
    )?;
    h.back(draft, selection)?;
    let buffers = h.buffers();
    require(
        buffers.first() == original_buffers.first(),
        "unopened activation changed the original dirty draft",
    )?;
    h.preserved(&buffers, draft, selection, &history)?;
    h.case(CASES[5])?;

    h.focus_row(1)?;
    let old_source = h.source()?;
    let mut late = None;
    h.render_after_capture(key(egui::Key::Enter, egui::Modifiers::NONE), true, |h| {
        late = Some(h.refresh_reply()?);
        let ready = h.refresh_reply()?;
        h.app
            .result_tx
            .send(WorkerEvent::Response(ready))
            .map_err(|_| "ready Search reply could not enter the owner event queue")?;
        Ok(())
    })?;
    h.idle()?;
    h.settle()?;
    let fresh_source = h.source()?;
    require(
        fresh_source.generation == old_source.generation
            && fresh_source.request > old_source.request
            && search_keyboard::row_id(&old_source, 1) != search_keyboard::row_id(&fresh_source, 1),
        "replacement Search reused the old batch identity",
    )?;
    h.preserved(&buffers, draft, selection, &history)?;
    h.case(CASES[6])?;
    h.app
        .apply_event(late.take().ok_or("late Search reply absent")?);
    h.settle()?;
    require(
        h.source()? == fresh_source,
        "late old Search replaced the newer accepted batch",
    )?;
    h.preserved(&buffers, draft, selection, &history)?;
    h.case(CASES[7])?;

    h.focus_row(0)?;
    h.render_after_capture(key(egui::Key::Enter, egui::Modifiers::NONE), true, |h| {
        let old_session = h.refresh_reply()?;
        h.app.disconnected("acceptance reconnect".into());
        h.connect()?;
        h.app.apply_event(old_session);
        Ok(())
    })?;
    h.idle()?;
    h.settle()?;
    require(
        !h.app.search_rows_active(),
        "old session Search or captured activation restored an active target",
    )?;
    h.preserved(&buffers, draft, selection, &history)?;
    h.case(CASES[8])?;
    h.search_control(QUERY)?;
    require(
        h.source()?.generation > fresh_source.generation
            && h.source()?.request > fresh_source.request,
        "reconnect reused a search generation or request identity",
    )?;
    h.focus_row(0)?;
    h.press(egui::Key::End)?;
    h.expect_row(ROWS - 1)?;
    h.press(egui::Key::Home)?;
    h.expect_row(0)?;
    h.preserved(&buffers, draft, selection, &history)?;
    h.case(CASES[9])?;

    h.render_after_capture(key(egui::Key::Enter, egui::Modifiers::NONE), true, |h| {
        // Only this stale-race injection bypasses the rendered query control.
        h.app.search_query = EMPTY_QUERY.into();
        Ok(())
    })?;
    h.idle()?;
    h.settle()?;
    require(
        !h.app.search_rows_active(),
        "captured old-query Enter retargeted an edited query",
    )?;
    h.preserved(&buffers, draft, selection, &history)?;
    h.case(CASES[10])?;
    h.phase = "empty";
    h.search_control(EMPTY_QUERY)?;
    require(
        h.app.search_results.is_empty(),
        "empty Search retained a result target",
    )?;
    // A retired row ID cannot make empty results actionable. Focus is not put
    // in the query field, whose Enter intentionally starts a new Search.
    h.app
        .editor_ctx
        .memory_mut(|memory| memory.request_focus(first_id));
    h.press(egui::Key::Enter)?;
    h.settle()?;
    h.preserved(&buffers, draft, selection, &history)?;
    h.case(CASES[11])?;

    h.undo(false, DRAFT, &generated_text("saved"))?;
    require(
        selection_key(h.selection(draft)?) == selection_key(range(2, 10)),
        "native Undo lost original full selection",
    )?;
    h.undo(true, DRAFT, &generated_text("dirty"))?;
    require(
        selection_key(h.selection(draft)?) == selection_key(selection)
            && h.document(DRAFT)?.dirty(),
        "native Redo lost dirty text or complete selection",
    )?;
    h.case(CASES[12])?;

    let expected = serde_json::json!([
        {"session": 1, "phase": "setup", "op": "Hello"},
        {"session": 1, "phase": "setup", "op": "List", "source": "root"},
        {"session": 1, "phase": "setup", "op": "Read", "source": "draft"},
        {"session": 1, "phase": "setup", "op": "Search", "query": "matches", "limit": 500},
        {"session": 1, "phase": "activation", "op": "Read", "source": "unopened"},
        {"session": 1, "phase": "refresh", "op": "Search", "query": "matches", "limit": 500},
        {"session": 1, "phase": "refresh", "op": "Search", "query": "matches", "limit": 500},
        {"session": 1, "phase": "refresh", "op": "Search", "query": "matches", "limit": 500},
        {"session": 2, "phase": "reconnect", "op": "Hello"},
        {"session": 2, "phase": "reconnect", "op": "List", "source": "root"},
        {"session": 2, "phase": "reconnect", "op": "Search", "query": "matches", "limit": 500},
        {"session": 2, "phase": "empty", "op": "Search", "query": "empty", "limit": 500},
    ]);
    h.idle()?;
    require(
        serde_json::json!(h.ledger.ordered) == expected && h.ledger.counts() == (2, 2, 6, 0, 0),
        "final exact Search Read and zero-write ledger differed",
    )?;
    require(
        !h.app.cjk_seen
            && !h.app.language.cjk_seen
            && h.app.documents.iter().all(|document| !document.has_cjk)
            && h.app.system_fonts.needs_probe(),
        "fixture triggered optional system-font work",
    )?;
    let source_hashes = h.source_hashes()?;
    h.reap()?;
    require(
        h.connections == 2 && h.reaped == 2 && !h.cleanup_uncertain,
        "both owned normal agents were not observed reaped",
    )?;
    h.case(CASES[13])?;
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
        !root_path.exists() && h.started.elapsed() < TOTAL && h.cases == CASES,
        "fixture removal case list or total cleanup budget unverified",
    )?;
    let mut receipt = serde_json::json!({
        "schema": 1, "acceptance": "search_navigation", "success": true,
        "os": if cfg!(windows) { "windows" } else { "linux" },
        "source_version": env!("CARGO_PKG_VERSION"), "source_agent_version_match": true,
        "cases": h.cases, "cases_executed": 14, "operation_ledger": h.ledger.ordered,
        "source_hashes": source_hashes,
    });
    let fields = receipt.as_object_mut().ok_or("receipt object absent")?;
    for name in [
        "trust_off",
        "no_language_session",
        "production_keyboard_frames",
        "exact_path_line_identity_order",
        "arrows_clamp_without_wrap",
        "home_end_reveal_visible_row",
        "dirty_enter_zero_read_exact_back",
        "unopened_enter_exact_one_read",
        "ready_batch_capture_rejected",
        "newer_search_rejects_late_reply",
        "stale_session_capture_rejected",
        "fresh_reconnect_navigation",
        "stale_query_capture_rejected",
        "empty_results_inert",
        "dirty_text_and_baseline_preserved",
        "full_selection_affinity_preserved",
        "complete_native_history_preserved",
        "native_undo_redo_preserved",
        "source_disk_unchanged",
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
        ("requests", 12_u64),
        ("hello", u64::from(h.connections)),
        ("list", h.ledger.lists.len() as u64),
        ("read", h.ledger.reads.len() as u64),
        ("search", h.ledger.searches.len() as u64),
        ("write", u64::from(h.ledger.writes)),
        ("other_operations", u64::from(h.ledger.other)),
        ("movement_operations", 0),
        ("dirty_activation_reads", 0),
        ("unopened_activation_reads", 1),
        ("setup_requests", 4),
        ("activation_requests", 1),
        ("refresh_requests", 3),
        ("reconnect_requests", 3),
        ("empty_requests", 1),
        ("connections", u64::from(h.connections)),
        ("agents_reaped", u64::from(h.reaped)),
        ("result_rows", ROWS as u64),
        ("fixture_files", 2),
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
    require(fields.len() == 57, "receipt field count differed")?;
    println!("search_navigation_acceptance {receipt}");
    h.receipt_emitted = true;
    Ok(())
}
