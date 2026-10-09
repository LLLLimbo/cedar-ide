//! Explicit lazy-tree acceptance through the ordinary trust-off stdio agent.
//! The ledger contains production app dispatches, never a second tree driver.
//! All filesystem changes are generated fixtures; the app sends no Write.
use super::*;
use cedar_client::{Client, ConnectionCancellation};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

// One wall-clock budget covers fixture preparation, protocol/UI work and source
// verification. Each owned agent also has a separate bounded cleanup wait.
const BUDGET: Duration = Duration::from_secs(60);
const CLEANUP: Duration = Duration::from_secs(5);
const ALPHA: &str = "alpha 雪";
const NESTED: &str = "alpha 雪/nested 草稿";
const BETA: &str = "beta café";
const WIDE: &str = "wide Ω";
const FILE: &str = "alpha 雪/nested 草稿/source λ.txt";
const PICKER_FILE: &str = "beta café/picker Ω.txt";
const ADDED: &str = "alpha 雪/added 外部.txt";
const BASE: &str = "original α\nsecond 雪\nthird line\n";
const DRAFT: &str = "unsaved 草稿\nsecond 雪\nthird line\n";
type Check<T> = Result<T, &'static str>;

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
            std::env::var_os("CEDAR_EXPLORER_TREE_AGENT_BIN")
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
            .prefix("cedar-tree 雪-")
            .tempdir()
            .map_err(|_| "fixture root creation failed")?;
        for path in [NESTED, BETA, WIDE] {
            fs::create_dir_all(root.path().join(path))
                .map_err(|_| "fixture directory creation failed")?;
        }
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
            cancellation,
            watchdog: Some((stop, watchdog)),
        };
        for (path, text) in [
            (FILE, BASE),
            ("alpha 雪/direct.txt", "alpha direct\n"),
            (PICKER_FILE, "picker source Ω\n"),
            ("wide Ω/original.txt", "original wide source\n"),
            ("root.txt", "root source\n"),
        ] {
            h.fixture(path, text)?;
        }
        Ok(h)
    }

    fn budget(&self) -> Check<()> {
        require(
            self.started.elapsed() < BUDGET,
            "acceptance budget exceeded",
        )
    }

    fn fixture(&mut self, path: &str, text: &str) -> Check<()> {
        self.budget()?;
        fs::write(
            self.root
                .as_ref()
                .ok_or("fixture root absent")?
                .path()
                .join(path),
            text,
        )
        .map_err(|_| "fixture file creation failed")?;
        self.sources.push((path.into(), digest(text.as_bytes())));
        Ok(())
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
        let flat = self.app.explorer.mode == explorer_tree::Mode::Flat;
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
        if flat {
            self.list_reply("", true)?;
        }
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

    fn idle(&mut self) -> Check<()> {
        self.budget()?;
        if let Ok(command) = self.commands.try_recv() {
            self.ledger.record(&command.op);
            return Err("unsolicited or queued request was dispatched");
        }
        Ok(())
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

    fn list_event(&mut self, path: &str, success: bool) -> Check<Event> {
        let command = self.next()?;
        require(
            matches!(&command.op, Operation::List { path: actual } if actual == path),
            "List did not target the explicitly selected branch",
        )?;
        let event = self.exchange(command)?;
        require(event.result.is_ok() == success, "List outcome differed")?;
        if success {
            require(
                matches!(&event.result, Ok(Payload::Entries { .. })),
                "List reply type differed",
            )?;
        } else {
            require(
                event
                    .result
                    .as_ref()
                    .err()
                    .is_some_and(|error| error.starts_with("directory_too_large:")),
                "oversized directory did not fail as a whole",
            )?;
        }
        Ok(event)
    }

    fn list_reply(&mut self, path: &str, success: bool) -> Check<()> {
        let event = self.list_event(path, success)?;
        self.app.apply_event(event);
        self.idle()
    }

    fn read_reply(&mut self, path: &str) -> Check<()> {
        let command = self.next()?;
        require(
            matches!(&command.op, Operation::Read { path: actual } if actual == path),
            "row Open did not dispatch the exact ordinary Read",
        )?;
        let event = self.exchange(command)?;
        require(
            matches!(&event.result, Ok(Payload::File { path: actual, .. }) if actual == path),
            "ordinary Read reply differed",
        )?;
        self.app.apply_event(event);
        self.idle()
    }

    fn row(&self, path: &str) -> Check<explorer_tree::Row> {
        self.app
            .explorer_visible_rows()
            .into_iter()
            .find(|row| row.entry.path == path)
            .ok_or("expected Explorer row absent")
    }

    fn visible(&self, path: &str) -> bool {
        self.app
            .explorer_visible_rows()
            .iter()
            .any(|row| row.entry.path == path)
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
        Ok(())
    }

    fn click_row(&mut self, path: &str) -> Check<()> {
        self.frame(vec![])?;
        self.frame(vec![])?;
        let response = self
            .app
            .editor_ctx
            .read_response(explorer_tree::row_id(path))
            .ok_or("production Explorer row response absent")?;
        require(response.enabled(), "production Explorer row disabled")?;
        let position = response.rect.center();
        self.frame(vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ])?;
        self.frame(vec![egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }])
    }

    fn document(&self) -> Check<&Document> {
        self.app
            .documents
            .iter()
            .find(|document| document.path == FILE)
            .ok_or("opened source document absent")
    }

    fn selection(&self) -> Check<egui::text::CCursorRange> {
        let id = egui::Id::new(("editor", self.document()?.id));
        egui::TextEdit::load_state(&self.app.editor_ctx, id)
            .and_then(|state| state.cursor.char_range())
            .ok_or("full editor selection absent")
    }

    fn preserve_draft(
        &self,
        selection: egui::text::CCursorRange,
        id: u64,
        version: u64,
    ) -> Check<()> {
        let document = self.document()?;
        require(
            document.id == id
                && document.text == DRAFT
                && document.saved_text == BASE
                && document.dirty()
                && !document.saving
                && document.edit_version == version
                && document.revision.as_deref()
                    == Some(format!("{:x}", Sha256::digest(BASE.as_bytes())).as_str())
                && document.interrupted_save.is_none(),
            "existing dirty Document changed during Explorer navigation",
        )?;
        let actual = self.selection()?;
        require(
            actual.primary.index == selection.primary.index
                && actual.primary.prefer_next_row == selection.primary.prefer_next_row
                && actual.secondary.index == selection.secondary.index
                && actual.secondary.prefer_next_row == selection.secondary.prefer_next_row,
            "full editor selection changed during Explorer navigation",
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
#[ignore = "requires explicit CEDAR_EXPLORER_TREE_AGENT_BIN selecting normal shipping agent"]
fn normal_agent_lazy_tree_is_explicit_bounded_and_preserves_dirty_editor() -> Check<()> {
    let mut h = Harness::new()?;
    h.connect()?;
    require(
        h.app.explorer.mode == explorer_tree::Mode::Flat,
        "Explorer did not default to flat mode",
    )?;
    h.app.explorer_set_mode(explorer_tree::Mode::Tree);
    h.idle()?;
    h.app.explorer_expand("");
    h.list_reply("", true)?;
    require(
        h.visible(ALPHA) && h.visible(BETA) && h.visible(WIDE) && !h.visible(NESTED),
        "root expansion walked descendants",
    )?;
    h.frame(vec![])?;
    h.frame(vec![])?;
    h.case()?;

    h.app.explorer_expand(ALPHA);
    let alpha = h.list_event(ALPHA, true)?;
    require(
        h.app.explorer_busy(),
        "explicit expansion did not own browser slot",
    )?;
    h.app.explorer_expand(BETA);
    h.app.explorer_refresh("");
    h.idle()?;
    h.app.apply_event(alpha);
    require(
        h.visible(NESTED) && h.visible(BETA) && !h.visible(FILE),
        "single branch expansion lost siblings or walked descendants",
    )?;
    h.app.explorer_expand(ALPHA);
    h.case()?;

    h.app.explorer_expand(NESTED);
    h.list_reply(NESTED, true)?;
    h.app.explorer_expand(BETA);
    h.list_reply(BETA, true)?;
    require(
        h.visible(FILE) && h.visible(PICKER_FILE),
        "independent expanded branch was not retained",
    )?;
    h.case()?;

    h.app.explorer_refresh(NESTED);
    let collapsed_reply = h.list_event(NESTED, true)?;
    h.app.explorer_collapse(ALPHA);
    require(
        !h.visible(NESTED) && !h.visible(FILE) && h.visible(PICKER_FILE),
        "collapse failed to release descendants or lost sibling cache",
    )?;
    h.app.explorer_expand(ALPHA);
    h.idle()?;
    h.app.apply_event(collapsed_reply);
    require(
        !h.visible(NESTED) && !h.visible(FILE) && !h.app.explorer_busy(),
        "collapsed epoch accepted a stale result or queued expansion",
    )?;
    h.case()?;

    h.app.explorer_expand(ALPHA);
    h.list_reply(ALPHA, true)?;
    h.app.explorer_expand(NESTED);
    h.list_reply(NESTED, true)?;
    h.click_row(FILE)?;
    h.read_reply(FILE)?;
    require(
        h.document()?.text == BASE && h.document()?.saved_text == BASE,
        "row Read did not reach editor",
    )?;
    h.frame(vec![])?;
    let id = h.document()?.id;
    let document = h
        .app
        .documents
        .iter_mut()
        .find(|document| document.id == id)
        .ok_or("edit target absent")?;
    editor_state::commit(&h.app.editor_ctx, document, DRAFT.into(), 9);
    let selection = egui::text::CCursorRange {
        primary: egui::text::CCursor {
            index: 11,
            prefer_next_row: true,
        },
        secondary: egui::text::CCursor {
            index: 2,
            prefer_next_row: false,
        },
    };
    let editor_id = egui::Id::new(("editor", id));
    let mut state = egui::TextEdit::load_state(&h.app.editor_ctx, editor_id)
        .ok_or("editor state absent after edit")?;
    state.cursor.set_char_range(Some(selection));
    state.store(&h.app.editor_ctx, editor_id);
    let version = h.document()?.edit_version;
    h.click_row(FILE)?;
    h.preserve_draft(selection, id, version)?;
    h.case()?;

    // The chooser uses only the selected branch snapshot plus existing tabs.
    h.app.explorer_select(PICKER_FILE);
    h.app
        .editor_ctx
        .memory_mut(|memory| memory.request_focus(explorer_tree::row_id(PICKER_FILE)));
    require(
        h.app.explorer_scope() == BETA
            && h.app
                .explorer_scope_entries()
                .iter()
                .map(|entry| entry.path.as_str())
                .eq([PICKER_FILE]),
        "selected file did not scope chooser entries to its immediate parent",
    )?;
    h.frame(key(egui::Key::P, egui::Modifiers::COMMAND))?;
    h.frame(vec![])?;
    h.frame(vec![egui::Event::Text("picker Ω.txt".into())])?;
    h.frame(key(egui::Key::Enter, egui::Modifiers::NONE))?;
    h.read_reply(PICKER_FILE)?;
    require(
        h.app
            .active()
            .is_some_and(|document| document.path == PICKER_FILE),
        "Ctrl P did not open a file from the selected branch",
    )?;
    h.frame(vec![])?;
    h.frame(vec![])?;
    h.app.explorer_select(NESTED);
    h.app.explorer_new_file();
    require(
        h.app.new_file && h.app.new_path == format!("{NESTED}/"),
        "New File did not inherit selected directory scope",
    )?;
    h.frame(key(egui::Key::Escape, egui::Modifiers::NONE))?;
    require(
        !h.app.new_file,
        "New File cancellation did not close draft dialog",
    )?;
    h.app.explorer_activate(FILE);
    h.preserve_draft(selection, id, version)?;
    h.case()?;

    // A known external fixture addition must remain invisible until Refresh.
    h.fixture(ADDED, "external fixture addition\n")?;
    h.frame(vec![])?;
    require(
        !h.visible(ADDED),
        "external addition appeared without explicit Refresh",
    )?;
    h.app.explorer_refresh(ALPHA);
    let refreshed = h.list_event(ALPHA, true)?;
    require(
        h.visible(FILE) && h.visible(PICKER_FILE) && !h.visible(ADDED),
        "pending refresh replaced valid cache",
    )?;
    h.app.apply_event(refreshed);
    require(
        h.visible(ADDED) && h.visible(FILE) && h.visible(PICKER_FILE),
        "explicit Refresh lost retained branches or omitted addition",
    )?;
    h.preserve_draft(selection, id, version)?;
    h.case()?;

    h.app.explorer_expand(WIDE);
    h.list_reply(WIDE, true)?;
    for index in 0..4096 {
        h.fixture(&format!("{WIDE}/overflow-{index:04}.txt"), "")?;
    }
    h.app.explorer_refresh(WIDE);
    let oversized = h.list_event(WIDE, false)?;
    require(
        h.visible("wide Ω/original.txt"),
        "oversized pending refresh discarded old cache",
    )?;
    h.app.apply_event(oversized);
    let wide = h.row(WIDE)?;
    require(
        wide.loaded
            && wide.stale
            && wide.error.is_some()
            && h.visible("wide Ω/original.txt")
            && !h.visible("wide Ω/overflow-0000.txt"),
        "oversized refresh accepted a prefix or hid retained stale cache",
    )?;
    h.preserve_draft(selection, id, version)?;
    h.case()?;

    // A genuine old-session List is retained until the replacement handshake.
    h.app.explorer_refresh(NESTED);
    let old_session = h.list_event(NESTED, true)?;
    h.app.disconnected("synthetic tree reconnect".into());
    h.connect()?;
    h.app.explorer_refresh("");
    let replacement = h.list_event("", true)?;
    h.app.apply_event(old_session);
    require(
        !h.visible(FILE) && !h.visible(PICKER_FILE) && h.app.explorer_busy(),
        "old generation restored stale cache or released replacement request",
    )?;
    h.app.apply_event(replacement);
    require(
        !h.app.explorer_busy(),
        "replacement root request did not finish",
    )?;
    h.preserve_draft(selection, id, version)?;
    h.case()?;

    h.app
        .editor_ctx
        .memory_mut(|memory| memory.request_focus(editor_id));
    h.frame(key(egui::Key::Z, egui::Modifiers::COMMAND))?;
    require(
        h.document()?.text == BASE && h.document()?.saved_text == BASE,
        "one Undo did not restore pre-edit text",
    )?;
    h.frame(key(
        egui::Key::Z,
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
    ))?;
    require(
        h.document()?.text == DRAFT && h.document()?.saved_text == BASE && h.document()?.dirty(),
        "one Redo did not restore retained draft",
    )?;
    h.case()?;

    let expected_lists = [
        "", "", ALPHA, NESTED, BETA, NESTED, ALPHA, NESTED, ALPHA, WIDE, WIDE, NESTED, "",
    ];
    require(
        h.ledger.lists.iter().map(String::as_str).eq(expected_lists),
        "List ledger included a branch without an explicit action",
    )?;
    require(
        h.ledger
            .reads
            .iter()
            .map(String::as_str)
            .eq([FILE, PICKER_FILE])
            && h.ledger.writes == 0
            && h.ledger.other == 0,
        "non-List request ledger differed",
    )?;
    h.sources_unchanged()?;
    h.case()?;
    require(h.cases == 11, "executed acceptance case count differed")?;
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
            "lists": h.ledger.lists.len(), "reads": h.ledger.reads.len(),
            "writes": h.ledger.writes, "other_operations": h.ledger.other,
            "connections": h.connections, "agents_reaped": h.reaped,
            "source_hashes_verified": h.sources.len(), "trust_off": true,
            "explicit_branch_ledger_verified": true, "busy_requests_not_queued": true,
            "collapse_stale_reply_rejected": true, "reconnect_stale_reply_rejected": true,
            "whole_branch_overflow_rejected": true, "failed_refresh_retained_stale_cache": true,
            "row_open_used_read": true, "dirty_document_and_full_selection_retained": true,
            "selected_scope_ctrl_p_and_new_file": true,
            "undo_redo_retained": true, "fixture_removed": true,
            "watchdog_joined": true, "cleanup_verified": true
        })
    );
    Ok(())
}
