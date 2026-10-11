//! Loaded-file chooser acceptance through the ordinary trust-off stdio agent.
//! Explicit Explorer actions load each fixture branch. Actual keyboard frames
//! open the chooser, select its scope, filter, and activate an admitted result.
//! The ledger observes production app dispatches; no alternate chooser driver,
//! protocol extension, agent fixture mode, filesystem walk, or runtime hook.
use super::*;
use cedar_client::{Client, ConnectionCancellation};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

// The existing client uses a 30-second ordinary-request limit. This one shared
// watchdog cancels the connection at 60 seconds, including across replacement;
// no request or connection renews the acceptance clock. Each owned agent has a
// separate five-second close observation, as in existing process acceptance.
const BUDGET: Duration = Duration::from_secs(60);
const CLEANUP: Duration = Duration::from_secs(5);
const ALPHA: &str = "alpha café";
const BETA: &str = "beta Ω";
const NESTED: &str = "alpha café/nested β";
const UNLOADED: &str = "beta Ω/unloaded π";
const SOURCE: &str = "alpha café/source λ.txt";
const SIBLING: &str = "beta Ω/picker é.txt";
const LATE: &str = "alpha café/nested β/late δ.txt";
const HIDDEN: &str = "beta Ω/unloaded π/hidden ü.txt";
const ROOT_FILE: &str = "root Ω.txt";
const BASE: &str = "original α\nsecond café\nthird line\n";
const DRAFT: &str = "unsaved draft β\nsecond café\nthird line\n";
type Check<T> = Result<T, &'static str>;
type Selection = egui::text::CCursorRange;

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

fn selection_key(selection: Selection) -> (usize, bool, usize, bool) {
    (
        selection.primary.index,
        selection.primary.prefer_next_row,
        selection.secondary.index,
        selection.secondary.prefer_next_row,
    )
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
            std::env::var_os("CEDAR_LOADED_FILES_AGENT_BIN")
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
            .prefix("cedar-loaded-files Ω-")
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
        for path in [NESTED, UNLOADED] {
            fs::create_dir_all(root.path().join(path))
                .map_err(|_| "fixture directory creation failed")?;
        }
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
            cancellation,
            watchdog: Some((stop, watchdog)),
        };
        for (path, text) in [
            (SOURCE, BASE),
            (SIBLING, "cached sibling é\n"),
            (LATE, "late cached file δ\n"),
            (HIDDEN, "unloaded descendant ü\n"),
            (ROOT_FILE, "root source Ω\n"),
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
            .map_err(|_| "fixture file creation failed")?;
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
        self.budget()?;
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
            self.list_reply("")?;
        }
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

    fn list_event(&mut self, path: &str) -> Check<Event> {
        let command = self.next()?;
        require(
            matches!(&command.op, Operation::List { path: actual } if actual == path),
            "List did not target the explicitly selected branch",
        )?;
        let event = self.exchange(command)?;
        require(
            matches!(&event.result, Ok(Payload::Entries { .. })),
            "List reply differed",
        )?;
        Ok(event)
    }

    fn list_reply(&mut self, path: &str) -> Check<()> {
        let event = self.list_event(path)?;
        self.app.apply_event(event);
        self.idle()
    }

    fn read_reply(&mut self, path: &str) -> Check<()> {
        let command = self.next()?;
        require(
            matches!(&command.op, Operation::Read { path: actual } if actual == path),
            "Open did not dispatch the exact ordinary Read",
        )?;
        let event = self.exchange(command)?;
        require(
            matches!(&event.result, Ok(Payload::File { path: actual, .. }) if actual == path),
            "ordinary Read reply differed",
        )?;
        self.app.apply_event(event);
        self.idle()
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
        self.budget()
    }

    fn settle(&mut self) -> Check<()> {
        self.frame(vec![])?;
        self.frame(vec![])?;
        self.idle()
    }

    fn chooser(&mut self) -> Check<()> {
        let before = self.ledger.counts();
        self.frame(key(egui::Key::P, egui::Modifiers::COMMAND))?;
        self.settle()?;
        require(
            self.app.navigation.dialog_open() && !self.app.navigation.loaded_scope(),
            "new chooser did not default to Current directory",
        )?;
        // The production controls precede the query. Shift+Tab, then Space is
        // the ordinary keyboard-only path; no direct scope setter is used.
        self.frame(key(egui::Key::Tab, egui::Modifiers::SHIFT))?;
        let button =
            workspace_access_tests::recorded_response(&self.app, "navigation_scope_loaded");
        require(
            button.has_focus(),
            "Shift Tab did not focus Loaded scope control",
        )?;
        self.frame(key(egui::Key::Space, egui::Modifiers::NONE))?;
        self.settle()?;
        require(
            self.app.navigation.loaded_scope(),
            "Space did not activate Loaded scope",
        )?;
        require(
            self.ledger.counts() == before,
            "opening or changing chooser scope dispatched work",
        )
    }

    fn filter(&mut self, path: &str, present: bool) -> Check<()> {
        let before = self.ledger.counts();
        self.frame(key(egui::Key::A, egui::Modifiers::COMMAND))?;
        self.frame(vec![egui::Event::Text(path.into())])?;
        self.idle()?;
        let paths = self.app.navigation.visible_paths();
        require(
            if present {
                paths == [path]
            } else {
                paths.is_empty()
            },
            "chooser filtered candidates differed",
        )?;
        require(
            self.app.navigation.selected_file_path() == present.then_some(path),
            "chooser selected candidate identity differed",
        )?;
        require(
            self.ledger.counts() == before,
            "filtering chooser dispatched work",
        )
    }

    fn dismiss(&mut self) -> Check<()> {
        self.frame(key(egui::Key::Escape, egui::Modifiers::NONE))?;
        self.settle()?;
        require(
            !self.app.navigation.dialog_open(),
            "Escape did not dismiss chooser",
        )
    }

    fn document(&self) -> Check<&Document> {
        self.app
            .documents
            .iter()
            .find(|doc| doc.path == SOURCE)
            .ok_or("source document absent")
    }

    fn selection(&self, id: u64) -> Check<Selection> {
        egui::TextEdit::load_state(&self.app.editor_ctx, egui::Id::new(("editor", id)))
            .and_then(|state| state.cursor.char_range())
            .ok_or("full editor selection absent")
    }

    fn preserve_draft(&self, selection: Selection, id: u64, version: u64) -> Check<()> {
        let doc = self.document()?;
        require(
            doc.id == id
                && doc.text == DRAFT
                && doc.saved_text == BASE
                && doc.dirty()
                && !doc.saving
                && doc.edit_version == version
                && doc.revision.as_deref()
                    == Some(format!("{:x}", Sha256::digest(BASE.as_bytes())).as_str())
                && doc.interrupted_save.is_none(),
            "chooser changed dirty Document identity, text, version, or baseline",
        )?;
        require(
            selection_key(self.selection(id)?) == selection_key(selection),
            "chooser changed full editor selection",
        )
    }

    fn case(&mut self) -> Check<()> {
        self.idle()?;
        self.cases += 1;
        Ok(())
    }

    fn sources_unchanged(&self) -> Check<()> {
        let root = self.root.as_ref().ok_or("fixture root absent")?.path();
        for (path, expected) in &self.sources {
            let actual = fs::read(root.join(path)).map_err(|_| "fixture source reread failed")?;
            require(&digest(&actual) == expected, "fixture source hash changed")?;
        }
        self.budget()
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
#[ignore = "requires explicit CEDAR_LOADED_FILES_AGENT_BIN selecting normal shipping agent"]
fn normal_agent_loaded_files_chooser_is_local_bounded_and_preserves_dirty_editor() -> Check<()> {
    let mut h = Harness::new()?;
    h.connect()?;
    h.app.explorer_set_mode(explorer_tree::Mode::Tree);
    for path in ["", ALPHA, BETA] {
        h.app.explorer_expand(path);
        h.list_reply(path)?;
    }
    // This is the only setup Read. Every later activation uses the chooser.
    h.app.open(SOURCE.into(), None);
    h.read_reply(SOURCE)?;
    h.settle()?;
    let id = h.document()?.id;
    let document = h
        .app
        .documents
        .iter_mut()
        .find(|doc| doc.id == id)
        .ok_or("edit target absent")?;
    editor_state::commit(&h.app.editor_ctx, document, DRAFT.into(), 9);
    h.settle()?;
    let selection = Selection {
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
    let mut state =
        egui::TextEdit::load_state(&h.app.editor_ctx, editor_id).ok_or("editor state absent")?;
    state.cursor.set_char_range(Some(selection));
    state.store(&h.app.editor_ctx, editor_id);
    h.app
        .editor_ctx
        .memory_mut(|memory| memory.request_focus(editor_id));
    let version = h.document()?.edit_version;
    h.app.explorer_select(ALPHA);
    require(
        h.app.explorer_scope() == ALPHA,
        "explicit current scope differed",
    )?;
    h.case()?;

    h.chooser()?;
    h.filter(HIDDEN, false)?;
    h.frame(key(egui::Key::Enter, egui::Modifiers::NONE))?;
    h.idle()?;
    require(
        h.app.navigation.dialog_open() && h.app.active_document == Some(id),
        "no-match Loaded Enter opened an unloaded path or dismissed the chooser",
    )?;
    h.preserve_draft(selection, id, version)?;
    h.case()?;

    h.filter(SIBLING, true)?;
    let before = h.ledger.counts();
    h.frame(key(egui::Key::Enter, egui::Modifiers::NONE))?;
    h.read_reply(SIBLING)?;
    h.settle()?;
    require(
        h.ledger.counts() == (before.0, before.1 + 1, 0, 0)
            && h.app.active().is_some_and(|doc| doc.path == SIBLING)
            && !h.app.navigation.dialog_open(),
        "unopened cached sibling did not use exactly one ordinary Read",
    )?;
    h.preserve_draft(selection, id, version)?;
    h.case()?;

    let before = h.ledger.counts();
    h.chooser()?;
    h.filter(SOURCE, true)?;
    h.frame(key(egui::Key::Enter, egui::Modifiers::NONE))?;
    h.settle()?;
    require(
        h.app.active_document == Some(id) && h.ledger.counts() == before,
        "dirty open-buffer activation dispatched work or replaced its identity",
    )?;
    h.preserve_draft(selection, id, version)?;
    h.case()?;

    // A real, explicitly requested List completes after snapshot admission.
    // Deferring delivery is local event scheduling, not an invented peer reply.
    h.app.explorer_expand(NESTED);
    let late = h.list_event(NESTED)?;
    h.chooser()?;
    h.filter(LATE, false)?;
    h.app.apply_event(late);
    h.settle()?;
    h.filter(LATE, false)?;
    h.frame(key(egui::Key::Enter, egui::Modifiers::NONE))?;
    h.idle()?;
    require(
        h.app.active_document == Some(id),
        "late cache retargeted chooser activation",
    )?;
    h.dismiss()?;
    h.preserve_draft(selection, id, version)?;
    h.case()?;

    h.chooser()?;
    h.filter(LATE, true)?;
    h.app.explorer_collapse(ALPHA);
    h.frame(key(egui::Key::Enter, egui::Modifiers::NONE))?;
    h.idle()?;
    require(
        h.app.active_document == Some(id) && h.app.documents.len() == 2,
        "collapsed candidate read or activated a different result",
    )?;
    h.dismiss()?;
    h.preserve_draft(selection, id, version)?;
    h.case()?;

    h.chooser()?;
    h.filter(ROOT_FILE, true)?;
    h.app.explorer_set_mode(explorer_tree::Mode::Flat);
    h.frame(key(egui::Key::Enter, egui::Modifiers::NONE))?;
    h.idle()?;
    require(
        !h.app.navigation.dialog_open() && h.app.active_document == Some(id),
        "old Explorer mode admitted chooser input",
    )?;
    h.settle()?;
    h.preserve_draft(selection, id, version)?;
    h.case()?;

    h.app.explorer_set_mode(explorer_tree::Mode::Tree);
    h.app.explorer_expand("");
    h.list_reply("")?;
    h.chooser()?;
    h.filter(ROOT_FILE, true)?;
    let generation = h.app.generation;
    h.connect()?;
    require(
        h.app.generation != generation,
        "replacement agent generation was unchanged",
    )?;
    h.frame(key(egui::Key::Enter, egui::Modifiers::NONE))?;
    h.idle()?;
    require(
        !h.app.navigation.dialog_open() && h.app.active_document == Some(id),
        "old connection generation admitted chooser input",
    )?;
    h.settle()?;
    h.preserve_draft(selection, id, version)?;
    h.case()?;

    h.app
        .editor_ctx
        .memory_mut(|memory| memory.request_focus(editor_id));
    h.frame(key(egui::Key::Z, egui::Modifiers::COMMAND))?;
    require(
        h.document()?.text == BASE && h.document()?.saved_text == BASE,
        "one Undo did not restore the pre-edit text",
    )?;
    h.frame(key(
        egui::Key::Z,
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
    ))?;
    require(
        h.document()?.text == DRAFT && h.document()?.saved_text == BASE && h.document()?.dirty(),
        "one Redo did not restore the retained draft",
    )?;
    h.case()?;

    require(
        h.ledger
            .lists
            .iter()
            .map(String::as_str)
            .eq(["", "", ALPHA, BETA, NESTED, ""]),
        "List ledger included work without an explicit setup action",
    )?;
    require(
        h.ledger
            .reads
            .iter()
            .map(String::as_str)
            .eq([SOURCE, SIBLING])
            && h.ledger.writes == 0
            && h.ledger.other == 0,
        "Read, Write, Run, or language-operation ledger differed",
    )?;
    require(
        !h.app.cjk_seen
            && !h.app.language.cjk_seen
            && h.app.documents.iter().all(|document| !document.has_cjk)
            && h.app.system_fonts.needs_probe(),
        "generated fixture unexpectedly triggered optional system-font work",
    )?;
    h.sources_unchanged()?;
    h.case()?;
    require(h.cases == 10, "executed acceptance case count differed")?;
    h.stop_watchdog()?;
    h.reap()?;
    require(
        h.reaped == h.connections && h.connections == 2,
        "owned agents were not fully reaped",
    )?;
    h.app.worker = None;
    let root = h.root.take().ok_or("fixture root absent during cleanup")?;
    let root_path = root.path().to_owned();
    root.close().map_err(|_| "fixture root cleanup failed")?;
    require(!root_path.exists(), "fixture root remained after cleanup")?;
    println!(
        "{}",
        serde_json::json!({
            "acceptance": "loaded_files", "success": true,
            "cases_executed": h.cases, "lists": h.ledger.lists.len(),
            "reads": h.ledger.reads.len(), "writes": h.ledger.writes,
            "other_operations": h.ledger.other, "connections": h.connections,
            "agents_reaped": h.reaped, "source_hashes_verified": h.sources.len(),
            "setup_lists": 6, "setup_reads": 1, "chooser_reads": 1,
            "chooser_lists": 0, "dirty_buffer_reads": 0,
            "watchdog_seconds": 60, "cleanup_observation_seconds": 5,
            "trust_off": true, "explicit_branch_ledger_verified": true,
            "current_scope_default": true, "keyboard_scope_filter_enter": true,
            "scope_and_filter_dispatched_nothing": true,
            "unloaded_descendant_not_scanned": true, "no_match_enter_did_not_read": true,
            "late_cache_did_not_retarget": true, "collapsed_candidate_did_not_read": true,
            "mode_change_rejected_old_dialog": true, "reconnect_rejected_old_dialog": true,
            "dirty_document_and_full_selection_retained": true, "undo_redo_retained": true,
            "fixture_removed": true, "watchdog_joined": true, "cleanup_verified": true,
            "optional_font_probe_untriggered": true
        })
    );
    Ok(())
}
