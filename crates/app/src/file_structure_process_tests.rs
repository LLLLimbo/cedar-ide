//! Linux-only normal Client -> shipping agent -> existing compiled mock LSP.
//! The ordinary production popup emits every measured sync/query. This is a
//! protocol fixture proof, not native JDT, Windows provider, or desktop GUI evidence.
use super::*;
use crate::{
    worker::{Command, Event, Worker},
    ConnectForm, ConnectionState, Tool,
};
use cedar_client::{Client, ConnectionCancellation};
use cedar_protocol::Payload;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Instant,
};

const PATH: &str = "Hello.java";
const SOURCE: &str = "class Hello { /* café 😀 */ int count; }\n";
const DIRTY: &str = "class Hello { /* café 😀 dirty */ int count; }\n";
const REQUEST: Duration = Duration::from_secs(30);
const START: Duration = Duration::from_secs(75);
// Nine active requests, including Client's implicit Hello: 75 + 8*30 = 315.
// Fifteen seconds covers local frames. Cleanup reserves Close30 + Stop30 + Reap4.
const ACTIVE: Duration = Duration::from_secs(330);
const TOTAL: Duration = Duration::from_secs(394);
const REAP: Duration = Duration::from_secs(4);
type Check<T> = Result<T, String>;
static FAILURE_REPORTED: AtomicBool = AtomicBool::new(false);

fn failure_receipt(receipt: Value) {
    if FAILURE_REPORTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let encoded = receipt.to_string();
    // An exclusive, caller-selected sidecar preserves the fixed safe evidence
    // when the existing bounded driver rejects the nonzero libtest exit.
    if let Some(path) = std::env::var_os("CEDAR_FILE_STRUCTURE_FAILURE_FILE") {
        if let Ok(mut file) = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            let _ = writeln!(file, "{encoded}");
            let _ = file.flush();
        }
    }
    println!("file_structure_failure {encoded}");
}

fn stopped(result: &Check<Payload>) -> bool {
    matches!(result, Ok(Payload::Language { value }) if *value == json!({"stopped":true}))
}

fn require(condition: bool, message: &str) -> Check<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn binary(variable: &str, filename: &str) -> Check<PathBuf> {
    let path =
        PathBuf::from(std::env::var_os(variable).ok_or("explicit binary selection missing")?);
    require(
        path.is_absolute()
            && path.is_file()
            && path.file_name().is_some_and(|name| name == filename),
        "invalid selected binary",
    )?;
    path.canonicalize().map_err(|error| error.to_string())
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

struct Harness {
    app: CedarApp,
    commands: Receiver<Command>,
    client: Option<Client>,
    root: Option<tempfile::TempDir>,
    fixture_path: PathBuf,
    started: Instant,
    clock: f64,
    ledger: Vec<Value>,
    stage: &'static str,
    server_attempted: bool,
    server_owned: bool,
    document_open: bool,
    close_attempted: bool,
    stop_attempted: bool,
    reaped: bool,
    completed: bool,
    watchdog: Option<(Sender<()>, thread::JoinHandle<bool>)>,
}

impl Harness {
    fn new() -> Check<Self> {
        let agent = binary("CEDAR_AGENT_BIN", "cedar-agent")?;
        let started = Instant::now();
        let root = tempfile::Builder::new()
            .prefix("cedar-file-structure-")
            .tempdir()
            .map_err(|error| error.to_string())?;
        fs::write(root.path().join(PATH), SOURCE).map_err(|error| error.to_string())?;
        let fixture_path = root
            .path()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let cancellation = ConnectionCancellation::new();
        let expiry = cancellation.clone();
        let (stop, stopped) = mpsc::channel();
        let watchdog = thread::spawn(move || {
            if matches!(
                stopped.recv_timeout(TOTAL.saturating_sub(started.elapsed())),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                expiry.cancel();
                true
            } else {
                false
            }
        });
        let (worker, commands) = Worker::recording();
        let mut app = CedarApp::empty();
        app.open_form = false;
        app.worker = Some(worker);
        app.editor_ctx.set_os(egui::os::OperatingSystem::Nix);
        let mut h = Self {
            app,
            commands,
            client: None,
            root: Some(root),
            fixture_path,
            started,
            clock: 0.0,
            ledger: vec![json!({"op":"Hello"})],
            stage: "connect",
            server_attempted: false,
            server_owned: false,
            document_open: false,
            close_attempted: false,
            stop_attempted: false,
            reaped: false,
            completed: false,
            watchdog: Some((stop, watchdog)),
        };
        h.admit(REQUEST, false)?;
        // Client::from_process sends exactly one Hello. Retain ownership before
        // any post-connect budget or metadata assertion can fail.
        h.client = Some(Client::spawn_agent_with_cancellation(
            &agent,
            h.root.as_ref().unwrap().path(),
            true,
            cancellation,
        )?);
        h.check(false)?;
        let hello = h.client.as_ref().unwrap().handshake().clone();
        require(
            matches!(&hello, Payload::Hello { agent: Some(info), .. }
            if info.os == "linux" && info.version == env!("CARGO_PKG_VERSION")
            && info.supports("language_start") && info.supports("language_document_symbols")),
            "normal Linux agent metadata differed",
        )?;
        h.app.state = ConnectionState::Connecting;
        h.app.connecting_form = Some(ConnectForm {
            local_root: h
                .root
                .as_ref()
                .unwrap()
                .path()
                .to_string_lossy()
                .into_owned(),
            allow_run: true,
            ..Default::default()
        });
        h.app.apply_event(Event {
            generation: h.app.generation,
            id: 0,
            connected: true,
            result: Ok(hello),
        });
        h.exchange_next("List")?;
        require(
            h.app.ready() && h.app.execution_trusted(),
            "explicit fixture trust missing",
        )?;
        h.stage = "read";
        h.app.open(PATH.into(), None);
        h.exchange_next("Read")?;
        h.frame(vec![])?;
        h.idle()?;
        Ok(h)
    }

    fn check(&self, cleanup: bool) -> Check<()> {
        require(
            self.started.elapsed() < if cleanup { TOTAL } else { ACTIVE },
            "fixed acceptance deadline exceeded",
        )
    }

    fn admit(&self, cost: Duration, cleanup: bool) -> Check<()> {
        let limit = if cleanup { TOTAL } else { ACTIVE };
        let elapsed = self.started.elapsed();
        require(
            elapsed < limit && limit.saturating_sub(elapsed) >= cost,
            "request lacks its complete normal Client bound",
        )
    }

    fn record(&mut self, op: &Operation) -> Check<&'static str> {
        let (name, value) = match op {
            Operation::List { path } if path.is_empty() => ("List", json!({"op":"List","path":""})),
            Operation::Read { path } if path == PATH => ("Read", json!({"op":"Read","path":PATH})),
            Operation::LanguageStart { program, args }
                if program
                    == binary("CEDAR_MOCK_LSP_BIN", "cedar-mock-lsp")?
                        .to_str()
                        .unwrap()
                    && args
                        == &[
                            "normal".to_string(),
                            self.audit_path().to_string_lossy().into_owned(),
                        ] =>
            {
                (
                    "LanguageStart",
                    json!({"op":"LanguageStart","fixture":"cedar-mock-lsp","mode":"normal"}),
                )
            }
            Operation::LanguageOpen {
                path,
                language_id,
                version,
                text,
            } if path == PATH && language_id == "java" && *version == 1 && text == SOURCE => (
                "LanguageOpen",
                json!({"op":"LanguageOpen","path":PATH,"version":1}),
            ),
            Operation::LanguageChange {
                path,
                version,
                text,
            } if path == PATH && *version == 2 && text == DIRTY => (
                "LanguageChange",
                json!({"op":"LanguageChange","path":PATH,"version":2}),
            ),
            Operation::LanguageDocumentSymbols { path } if path == PATH => (
                "LanguageDocumentSymbols",
                json!({"op":"LanguageDocumentSymbols","path":PATH}),
            ),
            Operation::LanguageClose { path } if path == PATH => {
                ("LanguageClose", json!({"op":"LanguageClose","path":PATH}))
            }
            Operation::LanguageStop => ("LanguageStop", json!({"op":"LanguageStop"})),
            _ => return Err("unexpected operation or source snapshot".into()),
        };
        self.ledger.push(value);
        Ok(name)
    }

    fn audit_path(&self) -> PathBuf {
        self.root.as_ref().unwrap().path().join("audit.jsonl")
    }

    fn closed(&self, result: &Check<Payload>) -> bool {
        let Ok(uri) = url::Url::from_file_path(self.fixture_path.join(PATH)) else {
            return false;
        };
        matches!(result, Ok(Payload::Language { value }) if *value == json!({"closed":uri.as_str()}))
    }

    fn exchange(&mut self, command: Command, expected: &str) -> Check<Event> {
        let cleanup = matches!(
            command.op,
            Operation::LanguageClose { .. } | Operation::LanguageStop
        );
        let cost = if matches!(command.op, Operation::LanguageStart { .. }) {
            START
        } else {
            REQUEST
        };
        self.admit(cost, cleanup)?;
        require(
            self.record(&command.op)? == expected,
            "unexpected operation order",
        )?;
        // Conservatively retain the cleanup obligation before any result check.
        if matches!(command.op, Operation::LanguageStart { .. }) {
            self.server_attempted = true;
            self.server_owned = true;
        }
        if matches!(command.op, Operation::LanguageOpen { .. }) {
            self.document_open = true;
        }
        let close = matches!(command.op, Operation::LanguageClose { .. });
        let stop = matches!(command.op, Operation::LanguageStop);
        self.close_attempted |= close;
        self.stop_attempted |= stop;
        let client = self.client.as_mut().ok_or("normal Client absent")?;
        let result = client.request(command.op);
        let closed = self.closed(&result);
        let stopped = stopped(&result);
        if closed && close {
            self.document_open = false;
        }
        if stopped && stop {
            self.server_owned = false;
        }
        self.check(cleanup)?;
        require(result.is_ok(), "normal-agent operation failed")?;
        require(
            !close || closed,
            "exact LanguageClose acknowledgment absent",
        )?;
        require(
            !stop || stopped,
            "exact Linux generic Stop cleanup acknowledgment absent",
        )?;
        Ok(Event {
            generation: self.app.generation,
            id: command.id,
            connected: true,
            result,
        })
    }

    fn exchange_next(&mut self, expected: &str) -> Check<()> {
        let command = self
            .commands
            .try_recv()
            .map_err(|_| format!("{expected} request missing"))?;
        let event = self.exchange(command, expected)?;
        self.app.apply_event(event);
        Ok(())
    }

    fn idle(&self) -> Check<()> {
        require(
            self.commands.try_recv().is_err(),
            "local action dispatched an extra operation",
        )
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> Check<()> {
        self.check(false)?;
        self.clock += 0.25;
        let ctx = self.app.editor_ctx.clone();
        let mut input = egui::RawInput {
            time: Some(self.clock),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 960.0),
            )),
            events,
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut self.app, &ctx, &mut input);
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(input, |ctx| {
            eframe::App::update(&mut self.app, ctx, &mut native)
        });
        self.check(false)
    }

    fn local(&mut self, events: Vec<egui::Event>) -> Check<()> {
        self.frame(events)?;
        self.idle()
    }

    fn navigation_observation(&self, phase: &'static str) {
        let focused = self.app.editor_ctx.memory(|memory| memory.focused());
        let editor = self
            .app
            .active_document
            .map(|id| egui::Id::new(("editor", id)));
        let focus_role = if focused.is_none() {
            "none"
        } else if focused == editor {
            "editor"
        } else if focused == Some(egui::Id::new("file_structure_filter")) {
            "filter"
        } else {
            "other"
        };
        let doc = self.app.active();
        println!(
            "file_structure_observation {}",
            json!({
                "phase":phase, "popup_open":self.app.file_structure_open(),
            "activation_pending":self.app.file_structure_activation_pending(),
                "selected_row":self.app.file_structure_selected(), "focus_role":focus_role,
                "modal":self.app.editor_ctx.memory(|memory| memory.top_modal_layer().is_some()),
                "jump_pending":doc.is_some_and(|doc| doc.jump_to.is_some()),
                "scroll_pending":doc.is_some_and(|doc| doc.scroll_to.is_some()),
                "restore_focus":self.app.navigation.restore_focus, "selection":self.selection().ok(),
                "source_equal":doc.is_some_and(|doc| doc.text == SOURCE),
                "dirty_equal":doc.is_some_and(|doc| doc.text == DIRTY),
                "edit_version":doc.map(|doc| doc.edit_version),
                "history_depth":[self.app.location_history.back.len(),self.app.location_history.forward.len()],
                "history_pending":self.app.location_history.pending.is_some()
            })
        );
    }

    fn settle_navigation(&mut self) -> Check<()> {
        // Existing navigation deliberately waits for egui's preceding modal
        // layer to release before committing the editor jump/focus. Observe at
        // most three eventless frames, each with a zero-operation assertion.
        let pending = |app: &CedarApp| {
            app.file_structure_activation_pending()
                || app.navigation.restore_focus
                || app.active().is_some_and(|doc| doc.jump_to.is_some())
                || app
                    .editor_ctx
                    .memory(|memory| memory.top_modal_layer().is_some())
        };
        for _ in 0..3 {
            if !pending(&self.app) {
                return self.idle();
            }
            self.local(vec![])?;
            self.navigation_observation("settling");
        }
        require(
            !pending(&self.app),
            "native navigation did not settle within three local frames",
        )
    }

    fn select(&mut self, index: usize) -> Check<()> {
        let id = self.app.active_document.ok_or("active document absent")?;
        let editor = egui::Id::new(("editor", id));
        let mut state = egui::TextEdit::load_state(&self.app.editor_ctx, editor)
            .ok_or("editor state absent")?;
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(index),
            )));
        state.store(&self.app.editor_ctx, editor);
        self.app
            .editor_ctx
            .memory_mut(|memory| memory.request_focus(editor));
        self.local(vec![])
    }

    fn selection(&self) -> Check<(usize, usize)> {
        let state = egui::TextEdit::load_state(
            &self.app.editor_ctx,
            egui::Id::new((
                "editor",
                self.app.active_document.ok_or("active document absent")?,
            )),
        )
        .ok_or("editor state absent")?;
        let range = state.cursor.char_range().ok_or("selection absent")?;
        Ok((range.primary.index, range.secondary.index))
    }

    fn sources_unchanged(&self) -> Check<()> {
        require(
            fs::read(self.root.as_ref().unwrap().path().join(PATH))
                .map_err(|error| error.to_string())?
                == SOURCE.as_bytes(),
            "source bytes changed",
        )
    }

    fn cleanup(&mut self) -> Check<()> {
        self.app
            .close_language_document(self.app.active_document.ok_or("active document absent")?);
        self.exchange_next("LanguageClose")?;
        self.app.stop_language();
        self.exchange_next("LanguageStop")?;
        self.admit(REAP, true)?;
        let result = self
            .client
            .take()
            .ok_or("normal Client absent")?
            .close_and_wait(REAP);
        self.reaped = result.is_ok();
        result?;
        self.check(true)?;
        self.sources_unchanged()?;
        Ok(())
    }

    fn finish(&mut self) -> Check<()> {
        self.check(true)?;
        let (stop, watchdog) = self.watchdog.take().ok_or("watchdog absent")?;
        let _ = stop.send(());
        require(
            !watchdog.join().map_err(|_| "watchdog join failed")?,
            "watchdog expired",
        )?;
        require(
            self.reaped && !self.server_owned && !self.document_open,
            "owned cleanup incomplete",
        )?;
        let root = self.root.take().ok_or("fixture root absent")?;
        let path = root.path().to_path_buf();
        root.close().map_err(|error| error.to_string())?;
        require(!path.exists(), "fixture removal failed")?;
        self.check(true)?;
        self.completed = true;
        Ok(())
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let failed = !self.completed;
        let agent_owned_before = self.client.is_some();
        let server_owned_before = self.server_owned;
        let document_open_before = self.document_open;
        // Each cleanup operation has one attempt, a complete remaining bound,
        // and independent acknowledgment. A failed Stop is never replayed.
        if self.document_open && !self.close_attempted && self.admit(REQUEST, true).is_ok() {
            self.close_attempted = true;
            self.ledger.push(json!({"op":"LanguageClose","path":PATH}));
            if let Some(client) = self.client.as_mut() {
                let result = client.request(Operation::LanguageClose { path: PATH.into() });
                if self.closed(&result) {
                    self.document_open = false;
                }
            }
        }
        if self.server_owned && !self.stop_attempted && self.admit(REQUEST, true).is_ok() {
            self.stop_attempted = true;
            self.ledger.push(json!({"op":"LanguageStop"}));
            if let Some(client) = self.client.as_mut() {
                if stopped(&client.request(Operation::LanguageStop)) {
                    self.server_owned = false;
                }
            }
        }
        if let Some(client) = self.client.take() {
            if self.admit(REAP, true).is_ok() {
                self.reaped = client.close_and_wait(REAP).is_ok();
            } else {
                // Drop still asks the existing owner to close, but no expired
                // observation window can become verified reap evidence.
                drop(client);
            }
        }
        if let Some((stop, watchdog)) = self.watchdog.take() {
            let _ = stop.send(());
            let _ = watchdog.join();
        }
        if let Some(root) = self.root.take() {
            let _ = root.keep();
        }
        // finish() can consume TempDir before a failed removal. Report the
        // observed residual path; Option ownership alone does not prove absence.
        let fixture_retained = self.fixture_path.exists();
        if failed {
            failure_receipt(json!({
                "schema":1,"acceptance":"file_structure","success":false,"os":"linux",
                "kind":"native","category":"native_check_failed","stage":self.stage,
                "operations_recorded":self.ledger.len(),"agent_owned_before":agent_owned_before,
                "server_owned_before":server_owned_before,"document_open_before":document_open_before,
                "agent_reaped_after":self.reaped,"fixture_retained":fixture_retained,
                "server_cleanup_after":if !self.server_attempted {"not_started"}
                    else if !self.server_owned {"verified"} else {"unverified"}
            }));
        }
    }
}

fn expected_ledger() -> Vec<Value> {
    vec![
        json!({"op":"Hello"}),
        json!({"op":"List","path":""}),
        json!({"op":"Read","path":PATH}),
        json!({"op":"LanguageStart","fixture":"cedar-mock-lsp","mode":"normal"}),
        json!({"op":"LanguageOpen","path":PATH,"version":1}),
        json!({"op":"LanguageDocumentSymbols","path":PATH}),
        json!({"op":"LanguageChange","path":PATH,"version":2}),
        json!({"op":"LanguageDocumentSymbols","path":PATH}),
        json!({"op":"LanguageDocumentSymbols","path":PATH}),
        json!({"op":"LanguageClose","path":PATH}),
        json!({"op":"LanguageStop"}),
    ]
}

#[test]
#[ignore = "requires explicit normal CEDAR_AGENT_BIN and compiled CEDAR_MOCK_LSP_BIN"]
fn normal_agent_file_structure_reuses_snapshot_and_retires_cancelled_reply() -> Check<()> {
    FAILURE_REPORTED.store(false, Ordering::SeqCst);
    let result = run_proof();
    if result.is_err() {
        failure_receipt(json!({
            "schema":1,"acceptance":"file_structure","success":false,"os":"linux",
            "kind":"native","category":"native_check_failed","stage":"setup",
            "operations_recorded":0,"agent_owned_before":false,"server_owned_before":false,
            "document_open_before":false,"agent_reaped_after":false,
            "server_cleanup_after":"not_started","fixture_retained":false
        }));
    }
    result
}

fn run_proof() -> Check<()> {
    let mut h = Harness::new()?;
    h.stage = "start";
    h.app.language.program = binary("CEDAR_MOCK_LSP_BIN", "cedar-mock-lsp")?
        .to_string_lossy()
        .into_owned();
    h.app.language.args = json!(["normal", h.audit_path()]).to_string();
    h.app.language.language_id = "java".into();
    h.app.start_language();
    h.exchange_next("LanguageStart")?;
    // The ordinary user-facing setting isolates explicit popup work from the
    // independently tested automatic background-sync/diagnostics policy.
    h.app.language.automatic = false;
    h.app.tool = Tool::Search;
    h.app.tools_open = false;
    h.local(vec![])?;
    h.select(5)?;
    let popup = || {
        key(
            egui::Key::F12,
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        )
    };
    h.stage = "initial_outline";
    h.frame(popup())?;
    require(
        h.app.file_structure_open(),
        "Ctrl+F12 did not open File Structure",
    )?;
    h.exchange_next("LanguageOpen")?;
    h.frame(vec![])?;
    h.exchange_next("LanguageDocumentSymbols")?;
    h.local(vec![])?;
    require(
        h.app.file_structure_rows() == vec![(0, "Hello"), (1, "count")],
        "ordinary mock outline did not populate the popup",
    )?;
    require(
        h.app.tool == Tool::Search && !h.app.tools_open,
        "popup response retargeted Language tool",
    )?;
    let snapshot_operations = h.ledger.len();
    h.stage = "cache_navigation";
    h.local(key(egui::Key::Escape, egui::Modifiers::NONE))?;
    h.local(popup())?;
    h.local(vec![egui::Event::Text("co".into())])?;
    require(
        h.app.file_structure_query() == Some("co")
            && h.app.file_structure_rows() == vec![(1, "count")],
        "literal filter did not use cached rows",
    )?;
    h.navigation_observation("filtered");
    h.local(key(egui::Key::Enter, egui::Modifiers::NONE))?;
    h.navigation_observation("entered");
    h.settle_navigation()?;
    h.navigation_observation("settled");
    let selection = h.selection()?;
    require(
        !h.app.file_structure_open()
            && selection.0.min(selection.1) == 0
            && selection.0.max(selection.1) == 1,
        "local symbol selection differed",
    )?;
    h.local(key(
        egui::Key::OpenBracket,
        egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
    ))?;
    h.settle_navigation()?;
    require(
        h.selection()? == (5, 5),
        "Back did not restore the exact editor location",
    )?;
    require(
        h.ledger.len() == snapshot_operations && snapshot_operations == 6,
        "cache filter and local navigation changed the fixed ledger",
    )?;

    let insert = SOURCE[..SOURCE.find(" */").ok_or("fixture insertion absent")?]
        .chars()
        .count();
    h.select(insert)?;
    h.stage = "dirty_outline";
    h.local(vec![egui::Event::Text(" dirty".into())])?;
    require(
        h.app.documents[0].text == DIRTY && h.app.documents[0].dirty(),
        "native dirty edit differed",
    )?;
    h.frame(popup())?;
    h.exchange_next("LanguageChange")?;
    h.frame(vec![])?;
    let command = h
        .commands
        .try_recv()
        .map_err(|_| "dirty outline request missing")?;
    let late = h.exchange(command, "LanguageDocumentSymbols")?;
    h.stage = "cancelled_reply";
    // This is a real completed Client response deliberately delivered after
    // dismissal. The production app decides whether to admit the stale event.
    h.local(key(egui::Key::Escape, egui::Modifiers::NONE))?;
    h.app.apply_event(late);
    h.local(vec![])?;
    require(
        !h.app.file_structure_open() && h.app.tool == Tool::Search && !h.app.tools_open,
        "cancelled response reopened popup or retargeted Language tool",
    )?;
    let dirty_cursor = h.selection()?;
    h.frame(popup())?;
    h.exchange_next("LanguageDocumentSymbols")?;
    h.local(vec![])?;
    require(
        h.app.file_structure_rows() == vec![(0, "Hello"), (1, "count")],
        "explicit retry did not populate current dirty snapshot",
    )?;
    let local_operations = h.ledger.len();
    h.stage = "dirty_navigation";
    h.local(vec![egui::Event::Text("count".into())])?;
    h.local(key(egui::Key::Enter, egui::Modifiers::NONE))?;
    h.settle_navigation()?;
    h.local(key(
        egui::Key::OpenBracket,
        egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
    ))?;
    h.settle_navigation()?;
    require(
        h.selection()? == dirty_cursor,
        "dirty Back location differed",
    )?;
    require(
        h.app.documents[0].text == DIRTY && h.app.documents[0].saved_text == SOURCE,
        "symbol navigation changed the dirty draft or baseline",
    )?;
    h.local(key(
        egui::Key::Z,
        egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
    ))?;
    require(
        h.app.documents[0].text == SOURCE && !h.app.documents[0].dirty(),
        "symbol navigation consumed native dirty Undo",
    )?;
    require(
        h.ledger.len() == local_operations && local_operations == 9,
        "typing, navigation or Undo emitted a query",
    )?;
    require(
        !h.app.language.automatic && h.app.execution_trusted(),
        "popup changed explicit execution settings",
    )?;
    h.sources_unchanged()?;
    require(
        h.app.system_fonts.needs_probe(),
        "fixture triggered optional system-font work",
    )?;
    h.stage = "cleanup";
    h.cleanup()?;

    h.stage = "audit";
    let audit = fs::read_to_string(h.audit_path()).map_err(|error| error.to_string())?;
    require(audit.len() < 64 * 1024, "fixture audit exceeded its bound")?;
    let messages: Vec<Value> = audit
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|_| "fixture audit was malformed")?;
    let methods: Vec<&str> = messages
        .iter()
        .map(|value| value["method"].as_str().unwrap_or(""))
        .collect();
    let expected_methods = [
        "initialize",
        "initialized",
        "textDocument/didOpen",
        "textDocument/documentSymbol",
        "textDocument/didChange",
        "textDocument/documentSymbol",
        "textDocument/documentSymbol",
        "textDocument/didClose",
        "shutdown",
        "exit",
    ];
    require(
        methods == expected_methods,
        "LSP audit was not the fixed ten-message sequence",
    )?;
    require(
        messages[2]["params"]["textDocument"]["text"] == SOURCE
            && messages[2]["params"]["textDocument"]["version"] == 1
            && messages[4]["params"]["contentChanges"][0]["text"] == DIRTY
            && messages[4]["params"]["textDocument"]["version"] == 2,
        "LSP sync did not receive the exact source snapshots",
    )?;
    require(
        h.ledger == expected_ledger(),
        "exact eleven-operation ledger differed",
    )?;
    h.stage = "finish";
    h.finish()?;
    let mut receipt = json!({
            "schema":1, "acceptance":"file_structure", "success":true, "os":"linux",
            "route":"normal_client_normal_agent_mock_lsp", "operation_ledger":h.ledger,
            "requests":11, "hello":1, "list":1, "read":1, "write":0, "run":0,
            "language_start":1, "language_open":1, "language_change":1,
            "language_document_symbols":3, "language_close":1, "language_stop":1,
            "dynamic_operations":0, "measured_local_operations":0, "lsp_methods":expected_methods,
            "source_sha256":format!("{:x}", Sha256::digest(SOURCE.as_bytes()))
    });
    let checks = json!({
            "exact_sync_snapshots":true, "cache_filter_zero_operations":true,
            "cancelled_response_retired":true, "popup_response_kept_tool":true,
            "local_selection_and_back":true, "dirty_undo_preserved":true,
            "source_bytes_unchanged":true, "production_keyboard_frames":true,
            "explicit_fixture_start_only":true, "automatic_sync_disabled":true,
            "agent_reaped":true, "generic_stop_cleanup_verified":true,
            "watchdog_joined":true, "fixture_removed":true,
            "active_seconds":330, "cleanup_reserve_seconds":64, "total_seconds":394,
            "request_seconds":30, "start_seconds":75, "client_reap_seconds":4,
            "native_jdt":"unmeasured", "windows_provider":"unmeasured", "native_gui":"unmeasured"
    });
    receipt
        .as_object_mut()
        .unwrap()
        .extend(checks.as_object().unwrap().clone());
    println!("file_structure_acceptance {receipt}");
    Ok(())
}
