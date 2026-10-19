//! One finite twelve-operation candidate proof against the normal, untrusted shipping agent.
//! Agent-operation evidence is not evidence of operating-system file-open counts.
use super::*;
use crate::browse_tests::normal_shell_snapshot;
use crate::scoped_search_tests::{command_modifiers, key, pointer};
use crate::workspace_access_tests::{recorded_clip, recorded_rect, recorded_response};
use cedar_client::{Client, ConnectionCancellation};
use cedar_protocol::{CaseSearchRequest, ScopedSearchRequest};
use crate::search_keyboard::Route;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

const ACTIVE: Duration = Duration::from_secs(340);
const REQUEST: Duration = Duration::from_secs(30);
const CLEANUP: Duration = Duration::from_secs(30);
const WATCHDOG: Duration = Duration::from_secs(390);
const ORIGIN: &str = "src Ω/a origin.rs";
const TARGET: &str = "src Ω/b target.rs";
const NESTED: &str = "src Ω/nested/c note.md";
const ROOT: &str = "root.rs";
const OUTSIDE: &str = "outside/d untouched.txt";
const MASKED: &str = "src Ω/z masked.RS";
const QUERY: &str = "K [x].* café 😀";
const CASES: [&str; 16] = [
    "trust_off_normal_agent",
    "checked_legacy_root",
    "checked_filtered_route",
    "shared_native_case_controls_local",
    "unchecked_directory_echo",
    "local_grouping_and_movement",
    "dirty_preview_and_return",
    "unopened_preview",
    "toggle_back_retires_search_open",
    "fresh_root_cached_open_exact_back_undo",
    "nested_reply_retired_before_delivery",
    "root_basename_mask",
    "complete_no_match",
    "local_edit_retirement",
    "retained_shell_and_provider_state",
    "exact_ledger_disk_and_owner_cleanup",
];
type Check<T> = Result<T, &'static str>;
type Selection = egui::text::CCursorRange;
type SelectionKey = (usize, bool, usize, bool);
type History = [Vec<(SelectionKey, String)>; 2];
fn require(ok: bool, error: &'static str) -> Check<()> {
    if ok {
        Ok(())
    } else {
        Err(error)
    }
}
fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn selected_key(value: Selection) -> SelectionKey {
    (
        value.primary.index,
        value.primary.prefer_next_row,
        value.secondary.index,
        value.secondary.prefer_next_row,
    )
}
fn selected() -> Selection {
    Selection {
        primary: egui::text::CCursor {
            index: 9,
            prefer_next_row: false,
        },
        secondary: egui::text::CCursor {
            index: 2,
            prefer_next_row: true,
        },
    }
}
fn variant(label: &str) -> &'static str {
    match label { "origin" | "root" => "K", "target" | "masked" => "K", "nested" | "outside" => "k", _ => "invalid" }
}
fn snippet(label: &str) -> String { format!("{} [x].* café 😀 {label}", variant(label)) }
fn text(header: &str, suffix: &str) -> String {
    format!("{header} Ω😀\r\n{}\r\nK xZZ café 😀 decoy\r\n{}", snippet(suffix), if suffix == "origin" { "last\r\n" } else { "" })
}
fn fixture_sources() -> Vec<(&'static str, String)> {
    vec![(ORIGIN, text("saved", "origin")), (TARGET, text("target", "target")),
    (NESTED, text("nested", "nested")), (ROOT, text("root", "root")),
    (OUTSIDE, text("outside", "outside")), (MASKED, text("masked", "masked"))]
}
fn label(path: &str) -> &'static str {
    match path {
        "" => "root_directory",
        ORIGIN => "origin",
        TARGET => "target",
        NESTED => "nested",
        ROOT => "root",
        OUTSIDE => "outside",
        MASKED => "masked",
        _ => "unexpected",
    }
}
fn spec(scope: &str, masks: &[&str]) -> ScopedSearchRequest {
    ScopedSearchRequest {
        query: QUERY.into(),
        scope: scope.into(),
        masks: masks.iter().map(|mask| (*mask).into()).collect(),
        limit: 500,
    }
}
fn case_spec(scope: &str, masks: &[&str]) -> CaseSearchRequest {
    CaseSearchRequest { query: QUERY.into(), scope: scope.into(), masks: masks.iter().map(|s| (*s).into()).collect(), limit: 500, case_sensitive: false }
}
fn expected_ledger() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"phase":"setup","op":"Hello"}),
        serde_json::json!({"phase":"setup","op":"List","source":"root_directory"}),
        serde_json::json!({"phase":"setup","op":"Read","source":"origin"}),
        serde_json::json!({"phase":"legacy","op":"Search","query":QUERY,"limit":500}),
        serde_json::json!({"phase":"checked_directory","op":"SearchIn","request":spec("src Ω", &["*.rs"])}),
        serde_json::json!({"phase":"directory","op":"SearchCase","request":case_spec("src Ω", &["*.rs"])}),
        serde_json::json!({"phase":"preview","op":"Read","source":"target"}),
        serde_json::json!({"phase":"retired_open","op":"Read","source":"target"}),
        serde_json::json!({"phase":"root","op":"SearchCase","request":case_spec("", &[])}),
        serde_json::json!({"phase":"nested","op":"SearchCase","request":case_spec("src Ω/nested", &["*.rs", "*.md"])}),
        serde_json::json!({"phase":"root_mask","op":"SearchCase","request":case_spec("", &["*.rs"])}),
        serde_json::json!({"phase":"empty","op":"SearchCase","request":case_spec("src Ω", &["*.zz"])}),
    ]
}

struct Harness {
    app: CedarApp,
    commands: Receiver<Command>,
    client: Option<Client>,
    root: Option<tempfile::TempDir>,
    agent: PathBuf,
    started: Instant,
    clock: f64,
    phase: &'static str,
    ledger: Vec<serde_json::Value>,
    sources: Vec<(String, [u8; 32])>,
    cases: Vec<&'static str>,
    observations: Vec<serde_json::Value>,
    connected: usize,
    connection_attempts: usize,
    reaped: usize,
    uncertain: bool,
    receipt_emitted: bool,
    cleanup_deadline: Option<Instant>,
    failure_stage: &'static str,
    cancellation: ConnectionCancellation,
    watchdog: Option<(Sender<()>, std::thread::JoinHandle<bool>)>,
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.cancellation.cancel();
        let deadline = *self
            .cleanup_deadline
            .get_or_insert_with(|| Instant::now() + CLEANUP);
        if let Some(client) = self.client.take() {
            let allowance = deadline
                .saturating_duration_since(Instant::now())
                .min(WATCHDOG.saturating_sub(self.started.elapsed()));
            if allowance.is_zero() || client.close_and_wait(allowance).is_err() {
                self.uncertain = true;
            } else {
                self.reaped += 1;
            }
        }
        if let Some((stop, watchdog)) = self.watchdog.take() {
            let _ = stop.send(());
            // The watcher only waits on this channel and cancels one token.
            // Always join this proof-owned worker; never detach it on timeout.
            if !matches!(watchdog.join(), Ok(false)) {
                self.uncertain = true;
            }
        }
        if self.uncertain
            || self.reaped != self.connected
            || Instant::now() >= deadline
            || self.started.elapsed() >= WATCHDOG
        {
            self.retain();
        }
        if !self.receipt_emitted {
            println!(
                "case_search_progress {}",
                serde_json::json!({"schema":1,"completed_cases":self.cases.len(),"requests":self.ledger.len(),"connection_attempts":self.connection_attempts,"connections":self.connected,"agents_reaped":self.reaped,"cleanup_uncertain":self.uncertain,"failure_stage":self.failure_stage})
            );
        }
    }
}
impl Harness {
    fn new() -> Check<Self> {
        let started = Instant::now();
        let agent = PathBuf::from(
            std::env::var_os("CEDAR_CASE_SEARCH_AGENT_BIN")
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
            .prefix("cedar-case-search Ω-")
            .tempdir()
            .map_err(|_| "fixture root creation failed")?;
        let canonical = root
            .path()
            .canonicalize()
            .map_err(|_| "fixture root canonicalization failed")?;
        require(
            !system_fonts::contains_cjk(&canonical.to_string_lossy())
                && !system_fonts::contains_cjk(&root.path().to_string_lossy()),
            "fixture path would start optional font I/O",
        )?;
        let mut app = CedarApp::empty();
        app.open_form = false;
        app.form.local_root = canonical.to_string_lossy().into_owned();
        let (_, commands) = Worker::recording();
        let cancellation = ConnectionCancellation::new();
        let cancel = cancellation.clone();
        let (stop, stopped) = mpsc::channel();
        let remaining = WATCHDOG.saturating_sub(started.elapsed());
        let watchdog = std::thread::spawn(move || {
            if matches!(
                stopped.recv_timeout(remaining),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                cancel.cancel();
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
            phase: "setup",
            ledger: vec![],
            sources: vec![],
            cases: vec![],
            observations: vec![],
            connected: 0,
            connection_attempts: 0,
            reaped: 0,
            uncertain: false,
            receipt_emitted: false,
            cleanup_deadline: None,
            failure_stage: "fixture_setup",
            cancellation,
            watchdog: Some((stop, watchdog)),
        };
        for (path, contents) in fixture_sources() {
            h.budget()?;
            let full = h.root.as_ref().ok_or("fixture missing")?.path().join(path);
            fs::create_dir_all(full.parent().ok_or("fixture parent missing")?)
                .map_err(|_| "fixture directory creation failed")?;
            fs::write(full, &contents).map_err(|_| "fixture write failed")?;
            h.sources.push((path.into(), digest(contents.as_bytes())));
        }
        Ok(h)
    }
    fn budget(&self) -> Check<()> {
        self.admit(Duration::ZERO)
    }
    fn admit(&self, reserve: Duration) -> Check<()> {
        require(
            !self.uncertain
                && !self.cancellation.is_cancelled()
                && self.started.elapsed() + reserve < ACTIVE,
            "shared active budget exhausted",
        )
    }
    fn retain(&mut self) {
        if let Some(root) = self.root.take() {
            let _ = root.keep();
        }
    }
    fn connect(&mut self) -> Check<()> {
        self.admit(REQUEST)?;
        let root = self
            .root
            .as_ref()
            .ok_or("fixture absent")?
            .path()
            .to_owned();
        self.connection_attempts += 1;
        self.failure_stage = "connection_spawn_or_hello_cleanup_unverified";
        let result = Client::spawn_agent_with_cancellation(
            &self.agent,
            &root,
            false,
            self.cancellation.clone(),
        );
        // Before return, Client ownership is inaccessible and errors contain only text.
        // A failed attempt cannot prove spawn/Hello or cleanup: retain the fixture
        // and emit failure progress only. Register a returned owner immediately.
        self.client = Some(match result {
            Ok(client) => client,
            Err(_) => {
                self.uncertain = true;
                self.retain();
                return Err("connection failed; cleanup unverified and fixture retained");
            }
        });
        self.failure_stage = "handshake_validation";
        self.connected += 1;
        self.ledger
            .push(serde_json::json!({"phase":"setup","op":"Hello"}));
        self.budget()?;
        let hello = self
            .client
            .as_ref()
            .ok_or("owner absent")?
            .handshake()
            .clone();
        require(
            matches!(&hello, Payload::Hello { scoped_search: Some(descriptor), case_search: Some(case), .. } if descriptor.version == 1 && case.version == 1),
            "normal agent independent search descriptors missing",
        )?;
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
            result: Ok(hello),
            connected: true,
        });
        let command = self.next()?;
        require(
            matches!(&command.op, Operation::List { path } if path.is_empty()),
            "initial root List differed",
        )?;
        let event = self.exchange(command)?;
        require(
            matches!(&event.result, Ok(Payload::Entries { .. })),
            "initial root List failed",
        )?;
        self.app.apply_event(event);
        require(
            self.app.ready()
                && self
                    .app
                    .scoped_search
                    .as_ref()
                    .is_some_and(|d| d.version == 1),
            "normal-agent search handshake not applied",
        )?;
        require(
            env!("CARGO_PKG_VERSION") == "0.79.0"
                && self.app.agent_info.as_ref().is_some_and(|info| {
                    info.version == env!("CARGO_PKG_VERSION")
                        && info.os == if cfg!(windows) { "windows" } else { "linux" }
                }),
            "source agent version or OS differed",
        )?;
        require(self.app.case_search.as_ref().is_some_and(|d| d.version == 1) && self.app.search_match_case, "case descriptor or checked default not applied")?;
        self.local()?;
        self.idle()
    }
    fn next(&mut self) -> Check<Command> {
        self.budget()?;
        let command = self
            .commands
            .try_recv()
            .map_err(|_| "explicit request missing")?;
        let entry = match &command.op {
            Operation::List { path } => {
                serde_json::json!({"phase":self.phase,"op":"List","source":label(path)})
            }
            Operation::Read { path } => {
                serde_json::json!({"phase":self.phase,"op":"Read","source":label(path)})
            }
            Operation::Search { query, limit } => {
                serde_json::json!({"phase":self.phase,"op":"Search","query":query,"limit":limit})
            }
            Operation::SearchIn { request } => {
                serde_json::json!({"phase":self.phase,"op":"SearchIn","request":request})
            }
            Operation::SearchCase { request } => serde_json::json!({"phase":self.phase,"op":"SearchCase","request":request}),
            _ => serde_json::json!({"phase":self.phase,"op":"Other"}),
        };
        self.ledger.push(entry);
        Ok(command)
    }
    fn exchange(&mut self, command: Command) -> Check<Event> {
        self.admit(REQUEST)?;
        self.failure_stage = "agent_request";
        require(
            matches!(
                &command.op,
                Operation::List { .. }
                    | Operation::Read { .. }
                    | Operation::Search { .. }
                    | Operation::SearchIn { .. }
                    | Operation::SearchCase { .. }
            ),
            "unexpected agent operation",
        )?;
        let client = self.client.as_mut().ok_or("normal-agent owner absent")?;
        let result = client.request(command.op);
        require(client.is_connected(), "normal agent disconnected")?;
        self.budget()?;
        Ok(Event {
            generation: self.app.generation,
            id: command.id,
            result,
            connected: true,
        })
    }
    fn idle(&mut self) -> Check<()> {
        self.budget()?;
        if self.commands.try_recv().is_ok() {
            return Err("unrequested operation was dispatched");
        }
        Ok(())
    }
    fn local(&self) -> Check<()> {
        require(
            !self.app.execution_trusted()
                && !self.app.recovery.has_actor()
                && !self.app.language.running
                && !self.app.language.startup_active()
                && self.app.system_fonts.needs_probe(),
            "Find started execution recovery language or optional font I/O",
        )
    }
    fn render(&mut self, events: Vec<egui::Event>) -> Check<egui::FullOutput> {
        self.budget()?;
        self.clock += 0.25;
        let ctx = self.app.editor_ctx.clone();
        let mut input = egui::RawInput {
            time: Some(self.clock),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 850.0),
            )),
            events,
            focused: true,
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut self.app, &ctx, &mut input);
        let mut native = eframe::Frame::_new_kittest();
        let output = ctx.run(input, |ctx| {
            eframe::App::update(&mut self.app, ctx, &mut native)
        });
        require(
            output
                .shapes
                .iter()
                .all(|shape| shape.clip_rect.is_finite()),
            "nonfinite rendered geometry",
        )?;
        self.budget()?;
        self.local()?;
        Ok(output)
    }
    fn frame(&mut self, events: Vec<egui::Event>) -> Check<egui::FullOutput> {
        let output = self.render(events)?;
        self.idle()?;
        Ok(output)
    }
    fn settle(&mut self) -> Check<()> {
        for _ in 0..3 {
            self.frame(vec![])?;
        }
        Ok(())
    }
    fn focus(&mut self, probe: &str) -> Check<()> {
        let response = recorded_response(&self.app, probe);
        response.request_focus();
        self.settle()?;
        require(
            self.app.editor_ctx.memory(|m| m.focused()) == Some(response.id),
            "rendered target did not receive focus",
        )
    }
    fn replace(&mut self, probe: &str, text: &str) -> Check<()> {
        self.focus(probe)?;
        self.frame(key(egui::Key::A, command_modifiers()))?;
        self.frame(key(egui::Key::Backspace, egui::Modifiers::NONE))?;
        if !text.is_empty() {
            self.frame(vec![egui::Event::Text(text.into())])?;
        }
        self.settle()
    }
    fn click(&mut self, probe: &str, submits: bool) -> Check<()> {
        let rect = recorded_rect(&self.app, probe).intersect(recorded_clip(&self.app, probe));
        require(
            rect.is_finite() && rect.is_positive(),
            "native click target not visible",
        )?;
        let pos = rect.center();
        self.frame(vec![egui::Event::PointerMoved(pos)])?;
        self.frame(pointer(pos, true))?;
        require(
            recorded_response(&self.app, probe).is_pointer_button_down_on(),
            "native pointer press missed target",
        )?;
        if submits {
            self.render(pointer(pos, false))?;
        } else {
            self.frame(pointer(pos, false))?;
        }
        require(
            recorded_response(&self.app, probe).clicked(),
            "native click missed target",
        )
    }
    fn document(&self, path: &str) -> Check<&Document> {
        self.app
            .documents
            .iter()
            .find(|d| d.path == path)
            .ok_or("expected document absent")
    }
    fn selection(&self, id: u64) -> Check<Selection> {
        egui::TextEdit::load_state(&self.app.editor_ctx, egui::Id::new(("editor", id)))
            .and_then(|s| s.cursor.char_range())
            .ok_or("native selection absent")
    }
    fn history(&self, id: u64) -> Check<History> {
        let state = egui::TextEdit::load_state(&self.app.editor_ctx, egui::Id::new(("editor", id)))
            .ok_or("native history absent")?;
        let doc = self
            .app
            .documents
            .iter()
            .find(|d| d.id == id)
            .ok_or("history document absent")?;
        let mut snapshots: History = Default::default();
        for (index, redo) in [false, true].into_iter().enumerate() {
            let mut history = state.undoer();
            let mut current = (self.selection(id)?, doc.text.clone());
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
                snapshots[index].push((selected_key(next.0), next.1.clone()));
                current = next;
            }
            require(
                snapshots[index].len() <= editor_state::MAX_UNDO_STATES + 1,
                "native history observation exceeded bound",
            )?;
        }
        Ok(snapshots)
    }
    fn preserve(&self, id: u64, selection: Selection, history: &History) -> Check<()> {
        let origin = self.document(ORIGIN)?;
        require(
            origin.text == text("dirty", "origin")
                && origin.saved_text == text("saved", "origin")
                && origin.dirty(),
            "Find changed dirty text or saved baseline",
        )?;
        require(
            selected_key(self.selection(id)?) == selected_key(selection)
                && &self.history(id)? == history,
            "Find changed full selection or complete native Undo history",
        )
    }
    fn read(&mut self, path: &str) -> Check<()> {
        let command = self.next()?;
        require(
            matches!(&command.op, Operation::Read { path: actual } if actual == path),
            "ordinary Read target differed",
        )?;
        let event = self.exchange(command)?;
        require(
            matches!(&event.result, Ok(Payload::File { path: actual, .. }) if actual == path),
            "ordinary Read reply differed",
        )?;
        self.app.apply_event(event);
        self.settle()
    }
    // Exact real-response validation and journey are added in this checkpoint's next chunk.
    fn case(&mut self, name: &'static str) -> Check<()> {
        self.idle()?;
        self.local()?;
        require(
            CASES.get(self.cases.len()) == Some(&name),
            "case order differed",
        )?;
        self.cases.push(name);
        self.failure_stage = "journey_assertion";
        Ok(())
    }
    fn source_hashes(&self) -> Check<Vec<serde_json::Value>> {
        let root = self.root.as_ref().ok_or("fixture root absent")?.path();
        let mut hashes = vec![];
        for (path, expected) in &self.sources {
            self.budget()?;
            let bytes = fs::read(root.join(path)).map_err(|_| "source reread failed")?;
            require(digest(&bytes) == *expected, "source disk hash changed")?;
            hashes.push(serde_json::json!({"source":label(path),"sha256":format!("{:x}",Sha256::digest(&bytes))}));
        }
        Ok(hashes)
    }
    fn cleanup_budget(&mut self, deadline: Instant) -> Check<()> {
        if Instant::now() >= deadline || self.started.elapsed() >= WATCHDOG {
            self.uncertain = true;
            self.retain();
            return Err("shared cleanup deadline exhausted; fixture retained");
        }
        Ok(())
    }
    fn finish(&mut self) -> Check<()> {
        self.budget()?;
        self.failure_stage = "owner_and_fixture_cleanup";
        let deadline = Instant::now() + CLEANUP;
        self.cleanup_deadline = Some(deadline);
        self.cleanup_budget(deadline)?;
        let client = self.client.take().ok_or("normal-agent owner missing")?;
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .min(WATCHDOG.saturating_sub(self.started.elapsed()));
        if remaining.is_zero() || client.close_and_wait(remaining).is_err() {
            self.uncertain = true;
            self.retain();
            return Err("agent cleanup unverified; fixture retained");
        }
        self.reaped += 1;
        self.cleanup_budget(deadline)?;
        // This proves the direct child was reaped and this proof's watchdog was
        // joined. Client does not expose proof of transport-reader thread joins.
        let (stop, watchdog) = self.watchdog.take().ok_or("watchdog owner missing")?;
        let _ = stop.send(());
        // Joining this stop-only worker is cooperative. The shared deadline is
        // checked before and after it; an overrun fails without a success receipt.
        if !matches!(watchdog.join(), Ok(false)) {
            self.uncertain = true;
            self.retain();
            return Err("watchdog cleanup unverified; fixture retained");
        }
        self.cleanup_budget(deadline)?;
        require(
            self.reaped == self.connected && self.connected == 1,
            "direct-child ownership differed",
        )?;
        // Stop TempDir's implicit Drop deletion before the explicit final step.
        // On deletion error, any remaining generated files stay at this path.
        let root = self
            .root
            .take()
            .ok_or("fixture absent before cleanup")?
            .keep();
        fs::remove_dir_all(&root)
            .map_err(|_| "generated fixture cleanup failed; remaining fixture retained")?;
        self.cleanup_budget(deadline)?;
        require(!root.exists(), "generated fixture removal unverified")?;
        self.failure_stage = "complete";
        Ok(())
    }
}
