//! Finite Enter continuation acceptance through the normal trust-off agent.
//! Generated fixtures only; setup performs Hello, root List, and explicit Reads.
//! Measured Enter, Undo and Redo use production input frames and dispatch no
//! workspace operations. No native GUI, user files, or execution trust.
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
const FIRST: &str = "enter 😀/CRLF café.txt";
const SECOND: &str = "enter 😀/LF Ω.txt";
const FIRST_BASE: &str = " \talpha 😀 café\r\n \tsecond Ω\r\nthird line\r\n";
const SECOND_BASE: &str = " \talpha 😀 café\n \tsecond Ω\nthird line\n";
type Check<T> = Result<T, &'static str>;
type Selection = egui::text::CCursorRange;
type SelectionKey = (usize, bool, usize, bool);

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
    transactions: u32,
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
            std::env::var_os("CEDAR_ENTER_INDENT_AGENT_BIN")
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
            .prefix("cedar-enter-indent 😀-")
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
        fs::create_dir(root.path().join("enter 😀"))
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
            cases: 0,
            transactions: 0,
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

    fn action(
        &mut self,
        path: &str,
        key_code: egui::Key,
        modifiers: egui::Modifiers,
        expected: &str,
        selection: Selection,
        changed: bool,
    ) -> Check<()> {
        let id = self.document(path)?.id;
        let version = self.document(path)?.edit_version;
        let ledger = self.ledger.counts();
        let baseline = self.document(path)?.saved_text.clone();
        let revision = self.document(path)?.revision.clone();
        self.frame(key(key_code, modifiers))?;
        let doc = self.document(path)?;
        require(
            doc.text == expected,
            "Enter/history frame produced unexpected text",
        )?;
        require(
            doc.edit_version == version.saturating_add(u64::from(changed))
                && doc.saved_text == baseline
                && doc.revision == revision
                && doc.dirty() == (expected != baseline)
                && !doc.saving
                && doc.interrupted_save.is_none(),
            "shortcut changed draft version or saved baseline incorrectly",
        )?;
        require(
            selection_key(self.selection(id)?) == selection_key(selection),
            "shortcut lost selection direction, indices or affinity",
        )?;
        require(
            self.app.active_document == Some(id)
                && self
                    .app
                    .editor_ctx
                    .memory(|memory| memory.has_focus(egui::Id::new(("editor", id)))),
            "shortcut lost active editor or keyboard focus",
        )?;
        require(
            self.ledger.counts() == ledger,
            "shortcut dispatched a workspace operation",
        )?;
        require(
            !self.app.execution_trusted(),
            "shortcut enabled execution trust",
        )?;
        self.transactions += u32::from(changed);
        self.case()
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

fn caret(index: usize) -> Selection {
    Selection::one(egui::text::CCursor::new(index))
}

#[test]
#[ignore = "requires explicit CEDAR_ENTER_INDENT_AGENT_BIN selecting normal shipping agent"]
fn normal_agent_enter_continuation_is_local_and_preserves_history() -> Check<()> {
    let mut h = Harness::new()?;
    h.connect()?;
    h.open(FIRST, true)?;
    h.open(SECOND, true)?;
    let setup = h.ledger.counts();
    require(setup == (1, 2, 0, 0), "setup operation ledger differed")?;
    let first_line = " \talpha 😀 café";
    let first_end = first_line.chars().count();
    for (path, baseline, newline) in [(FIRST, FIRST_BASE, "\r\n"), (SECOND, SECOND_BASE, "\n")] {
        let id = h.open(path, false)?;
        // Plain Enter copies a mixed ASCII space/tab prefix and chooses this
        // line's original LF/CRLF without splitting Unicode scalar positions.
        let before = Selection::one(egui::text::CCursor {
            index: first_end,
            prefer_next_row: true,
        });
        let after = caret(first_end + newline.len() + 2);
        let entered =
            format!("{first_line}{newline} \t{newline} \tsecond Ω{newline}third line{newline}");
        h.select(id, before)?;
        h.action(
            path,
            egui::Key::Enter,
            egui::Modifiers::NONE,
            &entered,
            after,
            true,
        )?;
        h.action(
            path,
            egui::Key::Z,
            egui::Modifiers::COMMAND,
            baseline,
            before,
            true,
        )?;
        h.action(
            path,
            egui::Key::Z,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            &entered,
            after,
            true,
        )?;
        h.action(
            path,
            egui::Key::Z,
            egui::Modifiers::COMMAND,
            baseline,
            before,
            true,
        )?;

        // The lower endpoint cuts the original indentation after its first
        // space. The deleted tab must not be copied into the replacement.
        let before = range(1, first_end + newline.len() + 3);
        let after = caret(1 + newline.len() + 1);
        let replaced = format!(" {newline} econd Ω{newline}third line{newline}");
        h.select(id, before)?;
        h.action(
            path,
            egui::Key::Enter,
            egui::Modifiers::NONE,
            &replaced,
            after,
            true,
        )?;
        h.action(
            path,
            egui::Key::Z,
            egui::Modifiers::COMMAND,
            baseline,
            before,
            true,
        )?;
        h.action(
            path,
            egui::Key::Z,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            &replaced,
            after,
            true,
        )?;
        h.action(
            path,
            egui::Key::Z,
            egui::Modifiers::COMMAND,
            baseline,
            before,
            true,
        )?;

        // Replacing the existing newline+indentation produces identical bytes.
        // Collapse only the caret, keeping the text version and redo branch.
        let unchanged_end = first_end + newline.len() + 2;
        let unchanged_caret = caret(unchanged_end);
        h.select(id, range(first_end, unchanged_end))?;
        h.action(
            path,
            egui::Key::Enter,
            egui::Modifiers::NONE,
            baseline,
            unchanged_caret,
            false,
        )?;
        h.action(
            path,
            egui::Key::Z,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            &replaced,
            after,
            true,
        )?;
        h.action(
            path,
            egui::Key::Z,
            egui::Modifiers::COMMAND,
            baseline,
            unchanged_caret,
            true,
        )?;
        h.action(
            path,
            egui::Key::Z,
            egui::Modifiers::COMMAND,
            baseline,
            unchanged_caret,
            false,
        )?;
    }

    // A caret inside CRLF is refused through the production frame. Neither the
    // underlying widget nor this feature may split the pair after the refusal.
    let id = h.open(FIRST, false)?;
    let between_crlf = caret(first_end + 1);
    h.select(id, between_crlf)?;
    h.action(
        FIRST,
        egui::Key::Enter,
        egui::Modifiers::NONE,
        FIRST_BASE,
        between_crlf,
        false,
    )?;

    // Prefix planning has a finite 4096-byte limit. This synthetic dirty buffer
    // exists only in the already explicitly opened in-memory document.
    let id = h.open(SECOND, false)?;
    let over_prefix = format!("{}Ω\n", " ".repeat(4097));
    h.edit(SECOND, &over_prefix)?;
    let over_caret = caret(4097);
    h.select(id, over_caret)?;
    h.action(
        SECOND,
        egui::Key::Enter,
        egui::Modifiers::NONE,
        &over_prefix,
        over_caret,
        false,
    )?;

    // Saturated versions fail closed even for an otherwise valid Enter. This
    // is the final frame for this document; no version is reset or reused.
    let id = h.open(FIRST, false)?;
    h.app
        .documents
        .iter_mut()
        .find(|document| document.id == id)
        .ok_or("saturation target absent")?
        .edit_version = u64::MAX;
    let saturated_caret = caret(first_end);
    h.select(id, saturated_caret)?;
    h.action(
        FIRST,
        egui::Key::Enter,
        egui::Modifiers::NONE,
        FIRST_BASE,
        saturated_caret,
        false,
    )?;

    require(
        h.ledger.counts() == setup,
        "measured editor phase dispatched workspace operations",
    )?;
    require(
        h.ledger.lists.iter().map(String::as_str).eq([""])
            && h.ledger
                .reads
                .iter()
                .map(String::as_str)
                .eq([FIRST, SECOND]),
        "exact setup operation targets differed",
    )?;
    require(
        h.cases == 27 && h.transactions == 20,
        "acceptance counters differed",
    )?;
    require(
        !h.app.cjk_seen
            && !h.app.language.cjk_seen
            && h.app.documents.iter().all(|document| !document.has_cjk)
            && h.app.system_fonts.needs_probe(),
        "generated fixture unexpectedly triggered optional system-font work",
    )?;
    h.sources_unchanged()?;
    h.stop_watchdog()?;
    h.reap()?;
    require(
        h.connections == 1 && h.reaped == 1,
        "normal-agent owner was not fully reaped",
    )?;
    h.app.worker = None;
    let root = h.root.take().ok_or("fixture root absent during cleanup")?;
    let root_path = root.path().to_owned();
    root.close().map_err(|_| "fixture cleanup failed")?;
    require(!root_path.exists(), "fixture root remained after cleanup")?;
    println!(
        "{}",
        serde_json::json!({
            "acceptance": "enter_indent", "success": true,
            "cases_executed": h.cases, "editor_transactions": h.transactions,
            "connection_hellos": h.connections,
            "lists": h.ledger.lists.len(), "reads": h.ledger.reads.len(),
            "writes": h.ledger.writes, "other_operations": h.ledger.other,
            "measured_editor_operations": 0, "agents_reaped": h.reaped,
            "source_hashes_verified": h.sources.len(), "trust_off": true,
            "production_enter_frames": true,
            "unicode_reversed_selection_affinity_preserved": true,
            "surviving_ascii_prefix_only": true, "lf_crlf_preserved": true,
            "dirty_baselines_preserved": true, "one_effective_undo_redo_preserved": true,
            "identical_text_keeps_version_and_redo": true,
            "crlf_prefix_version_refusals_preserved": true,
            "exact_operation_ledger_verified": true, "fixture_removed": true,
            "watchdog_joined": true, "cleanup_verified": true,
            "optional_font_probe_untriggered": true
        })
    );
    Ok(())
}
