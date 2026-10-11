//! Finite report-keyboard triage proof through the normal shipping trust-off agent.
//! Generated Unicode reports, production keyboard frames, and an exact operation ledger.
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
const DRAFT: &str = "triage 😀/draft café.txt";
const REPORT: &str = "triage 😀/TEST-duplicate.xml";
const DRAFT_BASE: &str = "saved α😀e\u{301} café\r\nsecond Ω\r\n";
const DRAFT_TEXT: &str = "dirty α😀e\u{301} café\r\nsecond Ω\r\n";
const ROWS: usize = 192;
const REPORT_PANEL_HEIGHT: f32 = 480.0;
const CASES: [&str; 14] = [
    "explicit_trust_off_report_read",
    "filtered_xml_order_duplicate_identity",
    "arrow_boundary_clamp",
    "home_end_reveal",
    "enter_details_back_exact_row",
    "space_details_back_exact_row",
    "name_filter_empty_no_retarget",
    "stale_filter_capture_rejected",
    "ready_read_capture_rejected",
    "newer_load_rejects_late_read",
    "stale_session_capture_rejected",
    "fresh_reconnect_navigation",
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
            Operation::Write { .. } => serde_json::json!({"session": session, "phase": phase, "op": "Write"}),
            _ => serde_json::json!({"session": session, "phase": phase, "op": "Other"}),
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
    phase: &'static str,
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
            std::env::var_os("CEDAR_REPORT_TRIAGE_AGENT_BIN")
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
            .prefix("cedar-report-triage 😀-")
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
        fs::create_dir(root.path().join("triage 😀"))
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
            cancellation,
            watchdog: Some((stop, watchdog)),
        };
        for (path, text) in [(DRAFT, DRAFT_BASE.to_owned()), (REPORT, generated_report())] {
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
            env!("CARGO_PKG_VERSION") == "0.57.4"
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
                "stale race did not begin with a production-owned report key",
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
            "report triage enabled execution, language or recovery work",
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
        REPORT => "report",
        _ => "unexpected",
    }
}

fn generated_report() -> String {
    let mut xml = String::from(
        "<testsuite name=\"generated café 😀\" tests=\"192\" failures=\"64\" errors=\"0\" skipped=\"0\">\n",
    );
    for index in 0..ROWS {
        xml.push_str(&format!(
            "<testcase name=\"duplicate café 😀\" classname=\"fixture.Class{index:03}\" time=\"0.25\">"
        ));
        if index % 3 == 1 {
            xml.push_str(&format!(
                "<failure message=\"marker {index:03}\" type=\"AssertionError\">trace Ω marker {index:03}</failure>"
            ));
        }
        xml.push_str("</testcase>\n");
    }
    xml.push_str("</testsuite>\n");
    xml
}

fn text_rect(output: &egui::FullOutput, expected: &str) -> Check<egui::Rect> {
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
        .ok_or("expected rendered label absent")
}

impl Harness {
    fn report_panel_rect(&self) -> Check<egui::Rect> {
        let rect =
            egui::containers::panel::PanelState::load(&self.app.editor_ctx, egui::Id::new("tools"))
                .ok_or("rendered Tools panel state absent")?
                .rect;
        require(
            rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0,
            "rendered Tools panel geometry invalid",
        )?;
        Ok(rect)
    }

    fn resized_report_panel(&self) -> Check<()> {
        require(
            (self.report_panel_rect()?.height() - REPORT_PANEL_HEIGHT).abs() <= 1.0,
            "Tools panel did not retain the native 480-point resize",
        )
    }

    fn resize_report_panel(&mut self) -> Check<()> {
        require(
            self.app.test_report.snapshot.is_none(),
            "initial Tools resize must precede the explicit report Read",
        )?;
        let id = egui::Id::new("tools").with("__resize");
        let handle = self
            .app
            .editor_ctx
            .read_response(id)
            .ok_or("native Tools resize handle absent")?;
        require(
            handle.enabled()
                && handle.sense.senses_drag()
                && handle.rect.is_finite()
                && handle.rect.width() > 0.0
                && handle.rect.height() > 0.0,
            "native Tools resize handle invalid",
        )?;
        let start = handle.rect.center();
        let target = egui::pos2(
            start.x,
            self.report_panel_rect()?.bottom() - REPORT_PANEL_HEIGHT,
        );
        self.frame(vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ])?;
        require(
            self.app
                .editor_ctx
                .read_response(id)
                .is_some_and(|response| response.is_pointer_button_down_on()),
            "native Tools resize handle did not receive the primary press",
        )?;
        self.frame(vec![egui::Event::PointerMoved(target)])?;
        require(
            self.app
                .editor_ctx
                .read_response(id)
                .is_some_and(|response| response.dragged_by(egui::PointerButton::Primary)),
            "native Tools resize handle did not own the drag",
        )?;
        self.frame(vec![egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }])?;
        require(
            self.app
                .editor_ctx
                .read_response(id)
                .is_some_and(|response| response.drag_stopped_by(egui::PointerButton::Primary)),
            "native Tools resize handle did not receive the primary release",
        )?;
        self.settle()?;
        self.resized_report_panel()
    }

    fn source(&self) -> Check<test_report_ui::Load> {
        self.app
            .test_report
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.source.clone())
            .ok_or("report snapshot absent")
    }

    fn focus_probe(&mut self, name: &str) -> Check<egui::Id> {
        let response = workspace_access_tests::recorded_response(&self.app, name);
        response.request_focus();
        self.settle()?;
        require(
            self.app.editor_ctx.memory(|memory| memory.focused()) == Some(response.id),
            "rendered report control did not receive focus",
        )?;
        Ok(response.id)
    }

    fn focus_row(&mut self, index: usize) -> Check<egui::Id> {
        self.focus_probe(&format!("test_report_case:{index}"))
    }

    fn visibility_context(&self, name: &str, initial: bool, rect: egui::Rect, clip: egui::Rect) {
        let (target, row_index) = match name {
            "test_report_back" => (
                "back",
                self.app.test_report.selected.filter(|index| *index < ROWS),
            ),
            "test_report_details" => (
                "details",
                self.app.test_report.selected.filter(|index| *index < ROWS),
            ),
            "test_report_load" => ("load", None),
            "test_report_filter" => ("filter", None),
            "test_report_path" => ("path", None),
            "test_report_refresh" => ("refresh", None),
            "test_report_suite" => ("suite", None),
            _ => name
                .strip_prefix("test_report_case:")
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|index| *index < ROWS)
                .map_or(("other", None), |index| ("case", Some(index))),
        };
        let loaded = self.app.test_report.snapshot.is_some();
        // An absent report must not reuse an earlier snapshot's scroll probe.
        let scroll = if loaded {
            self.app.editor_ctx.data(|data| {
                data.get_temp::<(egui::Id, egui::Vec2)>(egui::Id::new(
                    "workspace_access_report_scroll",
                ))
                .map(|(_, offset)| [offset.x, offset.y])
            })
        } else {
            None
        };
        let rect = [rect.min.x, rect.min.y, rect.max.x, rect.max.y];
        let clip = [clip.min.x, clip.min.y, clip.max.x, clip.max.y];
        let bounded = |values: &[f32]| {
            values
                .iter()
                .all(|value| value.is_finite() && value.abs() <= 100_000.0)
        };
        let finite = bounded(&rect)
            && bounded(&clip)
            && (!loaded || scroll.as_ref().is_some_and(|values| bounded(values)));
        let context = serde_json::json!({
            "schema": 1,
            "case_ordinal": self.cases.len().min(CASES.len() - 1),
            "reason": if initial { "loaded_viewport" } else { "target_visibility" },
            "target": target,
            "row_index": if loaded { row_index } else { None },
            "phase": if loaded { "loaded" } else { "before_load" },
            "geometry_state": if finite { "finite" } else { "invalid" },
            "rect": if finite { Some(rect) } else { None },
            "clip": if finite { Some(clip) } else { None },
            "scroll": if finite { scroll } else { None },
        });
        println!("report_triage_geometry {context}");
    }

    fn visible_probe(&self, name: &str) -> Check<()> {
        let rect = workspace_access_tests::recorded_rect(&self.app, name);
        let clip = workspace_access_tests::recorded_clip(&self.app, name);
        let visible = rect.is_finite()
            && clip.is_finite()
            && rect.height() > 0.0
            && rect.width() > 0.0
            && rect.min.y >= clip.min.y - 1.0
            && rect.max.y <= clip.max.y + 1.0
            && rect.intersect(clip).width() > 0.0;
        if !visible {
            self.visibility_context(name, false, rect, clip);
        }
        require(
            visible,
            "focused report target was not revealed in its real viewport",
        )
    }

    fn loaded_viewport(&self) -> Check<()> {
        let name = "test_report_case:0";
        let rect = workspace_access_tests::recorded_rect(&self.app, name);
        let clip = workspace_access_tests::recorded_clip(&self.app, name);
        let ready = rect.is_finite()
            && clip.is_finite()
            && rect.width() > 0.0
            && rect.height() > 0.0
            && clip.width() > 0.0
            && clip.height() > rect.height()
            && clip.contains_rect(rect);
        if !ready {
            self.visibility_context(name, true, rect, clip);
        }
        require(
            ready,
            "loaded report viewport does not fully contain its first row",
        )
    }

    fn expect_row(&mut self, index: usize) -> Check<()> {
        self.settle()?;
        let response = workspace_access_tests::recorded_response(
            &self.app,
            &format!("test_report_case:{index}"),
        );
        require(
            response.id == report_keyboard::row_id(&self.source()?, index)
                && self.app.editor_ctx.memory(|memory| memory.focused()) == Some(response.id),
            "keyboard focus did not retain the original XML row identity",
        )?;
        self.visible_probe(&format!("test_report_case:{index}"))
    }

    fn press(&mut self, key: egui::Key) -> Check<()> {
        self.frame(key_events(key))?;
        Ok(())
    }

    fn click_at(&mut self, at: egui::Pos2, request: bool) -> Check<()> {
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
            if request && !pressed {
                self.render_after_capture(events, false, |_| Ok(()))?;
            } else {
                self.frame(events)?;
            }
        }
        Ok(())
    }

    fn click_probe(&mut self, name: &str, request: bool) -> Check<()> {
        self.frame(vec![])?;
        self.visible_probe(name)?;
        let at = workspace_access_tests::recorded_rect(&self.app, name).center();
        self.click_at(at, request)
    }

    fn click_label(&mut self, label: &str) -> Check<()> {
        let output = self.frame(vec![])?;
        self.click_at(text_rect(&output, label)?.center(), false)?;
        self.settle()
    }

    fn read_report_command(&mut self) -> Check<Event> {
        let command = self.next()?;
        require(
            matches!(&command.op, Operation::Read { path } if path == REPORT)
                && matches!(
                    self.app.pending.get(&command.id),
                    Some(Job::TestReportRead(_))
                ),
            "explicit report action did not dispatch its exact ordinary Read",
        )?;
        let event = self.exchange(command)?;
        require(
            matches!(&event.result, Ok(Payload::File { path, text, .. })
                if path == REPORT && text == &generated_report()),
            "ordinary report Read returned different fixture bytes",
        )?;
        self.idle()?;
        Ok(event)
    }

    fn load_report_control(&mut self) -> Check<()> {
        self.resized_report_panel()?;
        self.click_probe("test_report_load", true)?;
        self.resized_report_panel()?;
        let event = self.read_report_command()?;
        self.app.apply_event(event);
        self.source()?;
        self.settle()?;
        self.resized_report_panel()?;
        self.loaded_viewport()
    }

    fn explicit_refresh_reply(&mut self) -> Check<Event> {
        self.phase = "refresh";
        // Race injection invokes the same explicit production action as the
        // Load/Refresh buttons. It does not inject a parsed report or fake reply.
        self.app.load_test_report();
        self.read_report_command()
    }

    fn details(&mut self, index: usize, space: bool) -> Check<()> {
        let mut events = key_events(if space {
            egui::Key::Space
        } else {
            egui::Key::Enter
        });
        if space {
            events.push(egui::Event::Text(" ".into()));
        }
        self.frame(events)?;
        self.settle()?;
        require(
            self.app.test_report.selected == Some(index)
                && self.app.editor_ctx.memory(|memory| memory.focused())
                    == Some(report_keyboard::back_id(&self.source()?, index)),
            "activation did not select exact duplicate-name row and focus Back",
        )?;
        self.visible_probe("test_report_back")?;
        self.visible_probe("test_report_details")?;
        let output = self.frame(vec![])?;
        text_rect(&output, &format!("trace Ω marker {index:03}"))?;
        Ok(())
    }

    fn filter_text(&mut self, text: &str) -> Check<()> {
        self.focus_probe("test_report_filter")?;
        self.frame(key(egui::Key::A, egui::Modifiers::COMMAND))?;
        self.press(egui::Key::Backspace)?;
        if !text.is_empty() {
            self.frame(vec![egui::Event::Text(text.into())])?;
        }
        self.settle()
    }
}

fn key_events(key_value: egui::Key) -> Vec<egui::Event> {
    key(key_value, egui::Modifiers::NONE)
}

#[test]
#[ignore = "requires explicit CEDAR_REPORT_TRIAGE_AGENT_BIN selecting normal shipping agent"]
fn normal_agent_report_keyboard_triage_is_local_and_revision_bound() -> Check<()> {
    let mut h = Harness::new()?;
    h.connect()?;
    let draft = h.open(DRAFT, true)?;
    h.select(draft, range(2, 10))?;
    editor_state::commit_selection(
        &h.app.editor_ctx,
        &mut h.app.documents[0],
        DRAFT_TEXT.into(),
        range(10, 2),
    );
    h.settle()?;
    let buffers = h.buffers();
    let selection = h.selection(draft)?;
    let history = h.history(draft)?;
    require(
        h.document(DRAFT)?.dirty()
            && selection_key(selection) == selection_key(range(10, 2))
            && !history[0].is_empty(),
        "dirty draft with full selection and native Undo was not prepared",
    )?;
    h.app.tools_open = true;
    h.app.tool = Tool::Tests;
    h.app.test_report.path = REPORT.into();
    h.app.test_report.path_edited();
    h.settle()?;
    h.resize_report_panel()?;
    h.load_report_control()?;
    require(
        h.ledger.counts() == (1, 2, 0, 0)
            && h.ledger.ordered.len() == 4
            && h.app.test_report.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.report.cases.len() == ROWS
                    && snapshot.report.counts.failed == 64
                    && snapshot.report.counts.passed == 128
            }),
        "initial explicit report or setup ledger differed",
    )?;
    h.case(CASES[0])?;

    h.click_label("Failed")?;
    require(
        h.app.test_report.visible_cases() == (1..ROWS).step_by(3).collect::<Vec<_>>(),
        "filter did not retain original XML indices in order",
    )?;
    let first_id = h.focus_row(1)?;
    let second_id = workspace_access_tests::recorded_response(&h.app, "test_report_case:4").id;
    require(
        first_id != second_id,
        "duplicate names shared one row identity",
    )?;
    h.expect_row(1)?;
    h.case(CASES[1])?;

    for (key_value, index) in [
        (egui::Key::ArrowUp, 1),
        (egui::Key::ArrowDown, 4),
        (egui::Key::ArrowUp, 1),
    ] {
        h.press(key_value)?;
        h.expect_row(index)?;
    }
    h.case(CASES[2])?;
    for (key_value, index) in [
        (egui::Key::End, 190),
        (egui::Key::ArrowDown, 190),
        (egui::Key::Home, 1),
        (egui::Key::End, 190),
    ] {
        h.press(key_value)?;
        h.expect_row(index)?;
    }
    h.case(CASES[3])?;

    h.details(190, false)?;
    h.press(egui::Key::Enter)?;
    h.expect_row(190)?;
    h.case(CASES[4])?;
    h.press(egui::Key::Home)?;
    h.press(egui::Key::ArrowDown)?;
    h.expect_row(4)?;
    h.details(4, true)?;
    h.click_probe("test_report_back", false)?;
    h.expect_row(4)?;
    h.case(CASES[5])?;

    h.filter_text("absent Ω")?;
    require(
        h.app.test_report.visible_cases().is_empty() && h.app.test_report.selected.is_none(),
        "empty name filter retained a selectable detail target",
    )?;
    h.press(egui::Key::Enter)?;
    require(
        h.app.test_report.selected.is_none(),
        "empty filter activated a row",
    )?;
    h.filter_text("")?;
    require(
        h.app.test_report.visible_cases().len() == 64,
        "cleared name filter differed",
    )?;
    h.case(CASES[6])?;

    h.focus_row(1)?;
    h.render_after_capture(key_events(egui::Key::End), true, |h| {
        // Only the stale-race injection changes the filter directly. All normal
        // filtering above used rendered controls and native text input.
        h.app.test_report.filter = test_report_ui::Filter::Passed;
        h.app.test_report.filter_changed();
        Ok(())
    })?;
    h.idle()?;
    h.settle()?;
    require(
        h.app.test_report.selected.is_none()
            && h.app.editor_ctx.memory(|memory| memory.focused())
                != Some(report_keyboard::row_id(&h.source()?, 191)),
        "captured old-filter movement retargeted a new visible row",
    )?;
    h.case(CASES[7])?;

    h.click_label("Failed")?;
    h.focus_row(1)?;
    let old_source = h.source()?;
    let mut late_read = None;
    h.render_after_capture(key_events(egui::Key::Enter), true, |h| {
        late_read = Some(h.explicit_refresh_reply()?);
        let ready_read = h.explicit_refresh_reply()?;
        h.app
            .result_tx
            .send(WorkerEvent::Response(ready_read))
            .map_err(|_| "ready ordinary Read could not enter the owner event queue")?;
        Ok(())
    })?;
    h.idle()?;
    h.settle()?;
    let new_source = h.source()?;
    require(
        new_source.generation == old_source.generation
            && new_source.id > old_source.id
            && h.app.test_report.selected.is_none()
            && report_keyboard::row_id(&old_source, 1) != report_keyboard::row_id(&new_source, 1),
        "ready Read allowed captured activation to target a replacement snapshot",
    )?;
    h.case(CASES[8])?;
    h.app
        .apply_event(late_read.take().ok_or("late ordinary Read absent")?);
    h.settle()?;
    require(
        h.source()? == new_source && h.app.test_report.selected.is_none(),
        "late old load replaced or selected the newer snapshot",
    )?;
    h.case(CASES[9])?;

    h.focus_row(0)?;
    h.render_after_capture(key_events(egui::Key::Enter), true, |h| {
        let old_session = h.explicit_refresh_reply()?;
        h.app.disconnected("acceptance reconnect".into());
        h.connect()?;
        h.app.apply_event(old_session);
        Ok(())
    })?;
    h.idle()?;
    h.settle()?;
    require(
        h.app.test_report.snapshot.is_none()
            && h.app.test_report.loading.is_none()
            && h.app.test_report.selected.is_none(),
        "old session Read or captured activation restored a target after reconnect",
    )?;
    h.case(CASES[10])?;
    h.load_report_control()?;
    require(
        h.source()?.generation > new_source.generation && h.source()?.id > new_source.id,
        "reconnect reused a report generation or load identity",
    )?;
    h.click_label("Failed")?;
    h.focus_row(1)?;
    h.press(egui::Key::End)?;
    h.expect_row(190)?;
    h.details(190, false)?;
    h.press(egui::Key::Space)?;
    h.expect_row(190)?;
    h.case(CASES[11])?;

    h.preserved(&buffers, draft, selection, &history)?;
    h.undo(false, DRAFT, DRAFT_BASE)?;
    require(
        selection_key(h.selection(draft)?) == selection_key(range(2, 10)),
        "native Undo lost original full selection",
    )?;
    h.undo(true, DRAFT, DRAFT_TEXT)?;
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
        {"session": 1, "phase": "setup", "op": "Read", "source": "report"},
        {"session": 1, "phase": "refresh", "op": "Read", "source": "report"},
        {"session": 1, "phase": "refresh", "op": "Read", "source": "report"},
        {"session": 1, "phase": "refresh", "op": "Read", "source": "report"},
        {"session": 2, "phase": "reconnect", "op": "Hello"},
        {"session": 2, "phase": "reconnect", "op": "List", "source": "root"},
        {"session": 2, "phase": "reconnect", "op": "Read", "source": "report"},
    ]);
    h.idle()?;
    require(
        serde_json::json!(h.ledger.ordered) == expected && h.ledger.counts() == (2, 6, 0, 0),
        "final exact setup, refresh, reconnect or zero-navigation ledger differed",
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
        "fixture removal, case list or total cleanup budget unverified",
    )?;
    let mut receipt = serde_json::json!({
        "schema": 1, "acceptance": "report_triage", "success": true,
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
        "filtered_original_xml_indices",
        "duplicate_names_distinct_identity",
        "arrows_clamp_without_wrap",
        "home_end_reveal_visible_row",
        "enter_space_details_back_exact_row",
        "empty_filter_cannot_retarget",
        "stale_filter_capture_rejected",
        "ready_read_capture_rejected",
        "newer_load_rejects_late_read",
        "stale_session_capture_rejected",
        "fresh_reconnect_navigation",
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
        ("requests", 10_u64),
        ("hello", u64::from(h.connections)),
        ("list", h.ledger.lists.len() as u64),
        ("read", h.ledger.reads.len() as u64),
        ("write", u64::from(h.ledger.writes)),
        ("other_operations", u64::from(h.ledger.other)),
        ("navigation_operations", 0),
        ("setup_requests", 4),
        ("refresh_requests", 3),
        ("reconnect_requests", 3),
        ("connections", u64::from(h.connections)),
        ("agents_reaped", u64::from(h.reaped)),
        ("report_rows", 192),
        ("filtered_rows", 64),
        ("first_filtered_index", 1),
        ("last_filtered_index", 190),
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
    require(fields.len() == 54, "receipt field count differed")?;
    println!("report_triage_acceptance {receipt}");
    Ok(())
}
