//! One finite twelve-operation candidate proof against the normal, untrusted shipping agent.
//! Agent-operation evidence is not evidence of operating-system file-open counts.
use super::*;
use crate::browse_tests::normal_shell_snapshot;
use crate::scoped_search_tests::{command_modifiers, key, pointer};
use crate::search_keyboard::Route;
use crate::workspace_access_tests::{recorded_clip, recorded_rect, recorded_response};
use cedar_client::{Client, ConnectionCancellation};
use cedar_protocol::{CaseSearchRequest, ScopedSearchRequest};
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
    match label {
        "origin" | "root" => "K",
        "target" | "masked" => "K",
        "nested" | "outside" => "k",
        _ => "invalid",
    }
}
fn snippet(label: &str) -> String {
    format!("{} [x].* café 😀 {label}", variant(label))
}
fn text(header: &str, suffix: &str) -> String {
    format!(
        "{header} Ω😀\r\n{}\r\nK xZZ café 😀 decoy\r\n{}",
        snippet(suffix),
        if suffix == "origin" { "last\r\n" } else { "" }
    )
}
fn fixture_sources() -> Vec<(&'static str, String)> {
    vec![
        (ORIGIN, text("saved", "origin")),
        (TARGET, text("target", "target")),
        (NESTED, text("nested", "nested")),
        (ROOT, text("root", "root")),
        (OUTSIDE, text("outside", "outside")),
        (MASKED, text("masked", "masked")),
    ]
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
    CaseSearchRequest {
        query: QUERY.into(),
        scope: scope.into(),
        masks: masks.iter().map(|s| (*s).into()).collect(),
        limit: 500,
        case_sensitive: false,
    }
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
        require(
            self.app
                .case_search
                .as_ref()
                .is_some_and(|d| d.version == 1)
                && self.app.search_match_case,
            "case descriptor or checked default not applied",
        )?;
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
            Operation::SearchCase { request } => {
                serde_json::json!({"phase":self.phase,"op":"SearchCase","request":request})
            }
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
        let event = self.read_reply(path)?;
        self.app.apply_event(event);
        self.settle()
    }
    fn origin_snapshot(&self) -> Check<serde_json::Value> {
        let doc = self.document(ORIGIN)?;
        Ok(
            serde_json::json!({"id":doc.id,"text":doc.text,"saved_text":doc.saved_text,
            "revision":doc.revision,"dirty":doc.dirty(),"cursor":doc.cursor,
            "edit_version":doc.edit_version,"selection":selected_key(self.selection(doc.id)?),
            "history":self.history(doc.id)?}),
        )
    }
    fn preserve_origin(&self, snapshot: &serde_json::Value) -> Check<()> {
        require(
            &self.origin_snapshot()? == snapshot,
            "local operation changed exact origin state",
        )
    }
    fn location_snapshot(&self) -> serde_json::Value {
        let locations = |stack: &[location_history::Location]| {
            stack
                .iter()
                .map(|x| {
                    (
                        x.generation,
                        x.document,
                        x.edit_version,
                        selected_key(x.selection),
                    )
                })
                .collect::<Vec<_>>()
        };
        serde_json::json!({"back":locations(&self.app.location_history.back),
            "forward":locations(&self.app.location_history.forward),
            "pending":self.app.location_history.pending.is_some(),
            "message":self.app.location_history.message})
    }
    fn shell_snapshot(&self) -> serde_json::Value {
        // Use only after both documents have genuinely rendered native TextEdit state.
        serde_json::json!({"shell":normal_shell_snapshot(&self.app),"match_case":self.app.search_match_case,
            "accepted_specification":self.app.search_state.accepted.as_ref().map(|s|
                serde_json::json!({"request":s.specification.request,"route":format!("{:?}",s.specification.route),
                    "case_sensitive":s.specification.case_sensitive}))})
    }
    fn toggle_pointer(&mut self, probe: &str) -> Check<()> {
        let before = self.app.search_match_case;
        let ledger = self.ledger.len();
        let origin = self.origin_snapshot()?;
        self.click(probe, false)?;
        self.settle()?;
        require(
            self.app.search_match_case != before && self.ledger.len() == ledger,
            "native checkbox pointer did not toggle exactly locally",
        )?;
        self.preserve_origin(&origin)
    }
    fn toggle_key(&mut self, probe: &str, keycode: egui::Key) -> Check<()> {
        self.focus(probe)?;
        let before = self.app.search_match_case;
        let ledger = self.ledger.len();
        let origin = self.origin_snapshot()?;
        self.frame(key(keycode, egui::Modifiers::NONE))?;
        self.settle()?;
        require(
            self.app.search_match_case != before && self.ledger.len() == ledger,
            "native checkbox key did not toggle exactly locally",
        )?;
        self.preserve_origin(&origin)
    }
    fn read_reply(&mut self, path: &str) -> Check<Event> {
        let command = self.next()?;
        require(
            matches!(&command.op,Operation::Read {path:actual} if actual==path),
            "Read request target differed",
        )?;
        let event = self.exchange(command)?;
        let expected = fixture_sources()
            .into_iter()
            .find(|(p, _)| *p == path)
            .ok_or("unknown fixture Read")?
            .1;
        require(
            matches!(&event.result,Ok(Payload::File {path:actual,text,revision}) if actual==path && text==&expected && revision==&format!("{:x}",Sha256::digest(expected.as_bytes()))),
            "Read reply exact target or original bytes differed",
        )?;
        Ok(event)
    }
    fn search_reply(
        &mut self,
        route: Route,
        scope: &str,
        masks: &[&str],
        expected: &[&str],
        entries: Option<usize>,
        eligible: &[&str],
    ) -> Check<(Event, serde_json::Value)> {
        let command = self.next()?;
        match (&command.op, route) {
            (Operation::Search { query, limit: 500 }, Route::Legacy)
                if query == QUERY && scope.is_empty() && masks.is_empty() => {}
            (Operation::SearchIn { request }, Route::ScopedV1)
                if request == &spec(scope, masks) => {}
            (Operation::SearchCase { request }, Route::CaseV1)
                if request == &case_spec(scope, masks) => {}
            _ => return Err("explicit route or exact captured search request differed"),
        }
        let event = self.exchange(command)?;
        let (rows, examined, bytes) = match (&event.result, route) {
            (
                Ok(Payload::Matches {
                    matches,
                    truncated: false,
                }),
                Route::Legacy,
            ) => (
                serde_json::to_value(matches).map_err(|_| "legacy row serialization")?,
                None,
                None,
            ),
            (Ok(Payload::ScopedMatches { result }), Route::ScopedV1) => {
                result
                    .validate_for(&spec(scope, masks))
                    .map_err(|_| "scoped exact echo validation failed")?;
                require(
                    !result.incomplete && result.reasons.is_empty(),
                    "short scoped fixture incomplete",
                )?;
                (
                    serde_json::to_value(&result.matches)
                        .map_err(|_| "scoped row serialization")?,
                    Some(result.examined_entries),
                    Some(result.read_bytes),
                )
            }
            (Ok(Payload::CaseMatches { result }), Route::CaseV1) => {
                result
                    .validate_for(&case_spec(scope, masks))
                    .map_err(|_| "case exact five-field echo validation failed")?;
                require(
                    !result.incomplete && result.reasons.is_empty(),
                    "short case fixture incomplete",
                )?;
                (
                    serde_json::to_value(&result.matches).map_err(|_| "case row serialization")?,
                    Some(result.examined_entries),
                    Some(result.read_bytes),
                )
            }
            _ => return Err("normal agent search response kind differed"),
        };
        let expected_rows: Vec<_> = expected
            .iter()
            .map(|path| serde_json::json!({"path":path,"line":2,"text":snippet(label(path))}))
            .collect();
        require(
            rows == serde_json::json!(expected_rows),
            "exact original search rows order line or literal Unicode text differed",
        )?;
        let expected_bytes = entries.map(|_| {
            fixture_sources()
                .into_iter()
                .filter(|(p, _)| eligible.contains(p))
                .map(|(_, t)| t.len())
                .sum::<usize>()
        });
        require(
            examined == entries && bytes == expected_bytes,
            "exact eligible byte or examined-entry counters differed",
        )?;
        let observation = serde_json::json!({"phase":self.phase,"route":format!("{route:?}"),
            "returned_rows":expected.len(),"accepted_rows":expected.len(),"matches":rows,
            "examined_entries":examined,"read_bytes":bytes});
        Ok((event, observation))
    }
    fn search(
        &mut self,
        route: Route,
        scope: &str,
        masks: &[&str],
        expected: &[&str],
        entries: Option<usize>,
        eligible: &[&str],
    ) -> Check<()> {
        let (event, observation) =
            self.search_reply(route, scope, masks, expected, entries, eligible)?;
        let request = event.id;
        self.app.apply_event(event);
        self.settle()?;
        require(
            self.app.search_rows_active()
                && self.app.search_state.accepted.as_ref().is_some_and(|s| {
                    s.request == request
                        && s.specification.route == route
                        && s.specification.request == spec(scope, masks)
                        && s.specification.case_sensitive == (route != Route::CaseV1)
                }),
            "accepted App source specification differed",
        )?;
        require(
            serde_json::to_value(&self.app.search_results).map_err(|_| "App rows serialization")?
                == observation["matches"],
            "App did not accept exactly the validated rows",
        )?;
        self.observations.push(observation);
        Ok(())
    }
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

#[test]
#[ignore = "requires explicit CEDAR_CASE_SEARCH_AGENT_BIN selecting normal shipping agent"]
fn normal_agent_case_search_is_explicit_local_and_source_bound() -> Check<()> {
    let mut h = Harness::new()?;
    h.connect()?;
    h.app.open(ORIGIN.into(), None);
    h.read(ORIGIN)?;
    let origin = h.document(ORIGIN)?.id;
    editor_state::commit_selection(
        &h.app.editor_ctx,
        &mut h.app.documents[0],
        text("dirty", "origin"),
        selected(),
    );
    h.settle()?;
    let departure = h.selection(origin)?;
    let history = h.history(origin)?;
    require(
        selected_key(departure) == selected_key(selected()) && !history[0].is_empty(),
        "dirty reversed native history not prepared",
    )?;
    let original = h.origin_snapshot()?;
    h.preserve(origin, departure, &history)?;
    h.case(CASES[0])?;

    // The initial surface setup does not simulate a native operating-system window.
    h.app.tool = Tool::Search;
    h.app.tools_open = true;
    h.app.tools_height = Some(480.0);
    h.settle()?;
    h.phase = "legacy";
    h.replace("search_query", QUERY)?;
    require(
        h.app.search_match_case,
        "fresh Match case default was not checked",
    )?;
    h.click("search_submit", true)?;
    h.search(Route::Legacy, "", &[], &[ROOT, ORIGIN], None, &[])?;
    h.preserve_origin(&original)?;
    h.case(CASES[1])?;

    h.frame(key(
        egui::Key::F,
        egui::Modifiers {
            shift: true,
            ..command_modifiers()
        },
    ))?;
    h.settle()?;
    h.replace("browse_text_scope", "src Ω")?;
    h.replace("browse_masks", "*.rs")?;
    h.phase = "checked_directory";
    h.click("browse_submit", true)?;
    h.search(
        Route::ScopedV1,
        "src Ω",
        &["*.rs"],
        &[ORIGIN],
        Some(5),
        &[ORIGIN, TARGET],
    )?;
    h.preserve_origin(&original)?;
    h.case(CASES[2])?;

    let before_toggles = h.ledger.len();
    h.toggle_pointer("browse_match_case")?;
    h.toggle_key("browse_match_case", egui::Key::Space)?;
    h.toggle_key("browse_match_case", egui::Key::Enter)?;
    require(
        !h.app.search_match_case && !h.app.search_rows_active(),
        "Browse toggles revived checked snapshot",
    )?;
    h.frame(key(egui::Key::Escape, egui::Modifiers::NONE))?;
    h.settle()?;
    h.click("find_results_entry", false)?;
    h.settle()?;
    require(
        !h.app.search_match_case && h.app.search_scope == "src Ω" && h.app.search_masks == "*.rs",
        "bottom controls did not share case/filter draft",
    )?;
    h.toggle_key("search_match_case", egui::Key::Space)?;
    h.toggle_key("search_match_case", egui::Key::Enter)?;
    require(
        !h.app.search_match_case && h.ledger.len() == before_toggles && !h.app.search_rows_active(),
        "shared controls dispatched or revived rows",
    )?;
    h.frame(key(
        egui::Key::F,
        egui::Modifiers {
            shift: true,
            ..command_modifiers()
        },
    ))?;
    h.settle()?;
    require(
        !h.app.search_match_case && h.app.search_scope == "src Ω" && h.app.search_masks == "*.rs",
        "Browse lost shared draft",
    )?;
    h.preserve_origin(&original)?;
    h.case(CASES[3])?;

    h.phase = "directory";
    h.click("browse_submit", true)?;
    h.search(
        Route::CaseV1,
        "src Ω",
        &["*.rs"],
        &[ORIGIN, TARGET],
        Some(5),
        &[ORIGIN, TARGET],
    )?;
    h.preserve_origin(&original)?;
    h.case(CASES[4])?;
    let before_local = h.ledger.len();
    require(
        h.app.search_state.group_expanded(0),
        "origin group initially collapsed",
    )?;
    h.click("browse_group:0", false)?;
    h.settle()?;
    require(
        !h.app.search_state.group_expanded(0) && !h.app.search_state.visible_indices().contains(&0),
        "group did not collapse origin",
    )?;
    h.click("browse_group:0", false)?;
    h.settle()?;
    require(
        h.app.search_state.group_expanded(0) && h.app.search_state.visible_indices().contains(&0),
        "group did not restore origin",
    )?;
    h.focus("browse_row:0")?;
    for (k, index) in [
        (egui::Key::ArrowDown, 1),
        (egui::Key::ArrowUp, 0),
        (egui::Key::Home, 0),
    ] {
        h.frame(key(k, egui::Modifiers::NONE))?;
        h.settle()?;
        require(
            h.app.browse.selected_index() == Some(index)
                && h.app.editor_ctx.memory(|m| m.focused())
                    == Some(recorded_response(&h.app, &format!("browse_row:{index}")).id),
            "native movement did not select and focus exact row",
        )?;
    }
    require(
        h.ledger.len() == before_local,
        "grouping or movement dispatched operations",
    )?;
    h.preserve_origin(&original)?;
    h.case(CASES[5])?;
    h.frame(key(egui::Key::Space, egui::Modifiers::NONE))?;
    h.settle()?;
    require(
        h.app.browse.selected_index() == Some(0)
            && h.app.browse.preview_visible()
            && h.app.browse_preview_text() == Some(text("dirty", "origin").as_str()),
        "dirty preview exact content or owner differed",
    )?;
    let pane = recorded_rect(&h.app, "browse_preview_content");
    require(
        pane.is_positive() && recorded_clip(&h.app, "browse_preview_content").contains_rect(pane),
        "dirty preview not visible",
    )?;
    h.preserve_origin(&original)?;
    h.frame(key(egui::Key::Escape, egui::Modifiers::NONE))?;
    h.settle()?;
    require(
        !h.app.browse.preview_visible()
            && h.app.browse_preview_text().is_none()
            && h.app.editor_ctx.memory(|m| m.focused())
                == Some(recorded_response(&h.app, "browse_row:0").id),
        "preview Escape did not restore exact row",
    )?;
    require(
        h.ledger.len() == before_local,
        "dirty preview or Escape dispatched operations",
    )?;
    h.preserve_origin(&original)?;
    h.case(CASES[6])?;

    h.phase = "preview";
    h.focus("browse_row:1")?;
    h.render(key(egui::Key::Space, egui::Modifiers::NONE))?;
    let preview = h.read_reply(TARGET)?;
    h.app.apply_event(preview);
    h.settle()?;
    require(
        h.app.documents.len() == 1
            && h.app.active_document == Some(origin)
            && h.app.browse.selected_index() == Some(1)
            && h.app.browse_preview_text() == Some(text("target", "target").as_str()),
        "unopened preview created document or differed",
    )?;
    h.preserve_origin(&original)?;
    h.case(CASES[7])?;
    h.frame(key(egui::Key::Escape, egui::Modifiers::NONE))?;
    h.settle()?;
    let history_before_open = h.location_snapshot();
    h.phase = "retired_open";
    h.render(key(egui::Key::Enter, egui::Modifiers::NONE))?;
    let open_reply = h.read_reply(TARGET)?;
    let open_id = open_reply.id;
    h.settle()?;
    // Reopen only the bottom Find panel. Browse entry changes the navigation epoch
    // and would make an unrelated cause retire this Open before the case toggle.
    h.click("find_results_entry", false)?;
    h.settle()?;
    let open_epoch = h.app.navigation_epoch;
    let accepted = h.app.search_state.accepted.clone();
    require(
        h.app.location_history.pending.is_some()
            && h.app.pending.get(&open_id).is_some_and(|j| match j {
                Job::SearchOpen {
                    path,
                    line,
                    navigation,
                    source,
                } => {
                    path == TARGET
                        && *line == Some(2)
                        && *navigation == open_epoch
                        && accepted.as_ref() == Some(&source.snapshot)
                        && h.app.search_open_source_current(source, path, *line)
                }
                _ => false,
            }),
        "pending SearchOpen was not fully current before checkbox retirement",
    )?;
    h.toggle_pointer("search_match_case")?;
    h.toggle_pointer("search_match_case")?;
    require(
        !h.app.search_match_case
            && h.app.navigation_epoch == open_epoch
            && !h.app.search_rows_active()
            && h.app
                .pending
                .get(&open_id)
                .is_some_and(|j| matches!(j,Job::SearchOpen {source,..} if !source.active))
            && h.location_snapshot() == history_before_open,
        "toggle/back did not irreversibly retire exact Open and history ticket",
    )?;
    let error = h.app.error.clone();
    h.app.apply_event(open_reply);
    h.settle()?;
    let cached = h.document(TARGET)?;
    require(
        !h.app.pending.contains_key(&open_id)
            && h.app.documents.len() == 2
            && h.app.active_document == Some(origin)
            && cached.text == text("target", "target")
            && cached.saved_text == cached.text
            && !cached.dirty()
            && cached.revision.as_deref()
                == Some(
                    format!("{:x}", Sha256::digest(text("target", "target").as_bytes())).as_str(),
                )
            && cached.edit_version == 0
            && !cached.undo_initialized
            && cached.cursor == (1, 1)
            && cached.jump_to.is_none()
            && cached.scroll_to.is_none()
            && egui::TextEdit::load_state(&h.app.editor_ctx, egui::Id::new(("editor", cached.id)))
                .is_none()
            && h.location_snapshot() == history_before_open
            && h.app.error == error
            && !h.app.has_search_preview()
            && !h.app.browse.preview_visible(),
        "retired real Read did not drain into safe inactive cache only",
    )?;
    h.preserve_origin(&original)?;
    h.case(CASES[8])?;

    h.phase = "root";
    h.replace("search_scope", "")?;
    h.replace("search_masks", "")?;
    h.click("search_submit", true)?;
    h.search(
        Route::CaseV1,
        "",
        &[],
        &[OUTSIDE, ROOT, ORIGIN, TARGET, NESTED, MASKED],
        Some(9),
        &[ORIGIN, TARGET, NESTED, ROOT, OUTSIDE, MASKED],
    )?;
    let before_cached = h.ledger.len();
    let index = h
        .app
        .search_results
        .iter()
        .position(|row| row.path == TARGET && row.line == 2)
        .ok_or("fresh root target absent")?;
    require(index == 3, "root target path order differed")?;
    h.click(&format!("search_row:{index}"), false)?;
    h.settle()?;
    require(
        h.app.search_state.selected_index() == Some(index),
        "fresh cached target not selected",
    )?;
    h.frame(key(egui::Key::Enter, egui::Modifiers::NONE))?;
    h.settle()?;
    require(
        h.app
            .active()
            .is_some_and(|d| d.path == TARGET && d.cursor == (2, 1)),
        "fresh cached Open did not reach exact path/line",
    )?;
    h.frame(key(egui::Key::OpenBracket, command_modifiers()))?;
    h.settle()?;
    require(
        h.app.active_document == Some(origin) && h.ledger.len() == before_cached,
        "cached Open or Back dispatched or missed origin",
    )?;
    h.preserve_origin(&original)?;
    h.frame(key(egui::Key::Z, command_modifiers()))?;
    require(
        h.document(ORIGIN)?.text == text("saved", "origin"),
        "native Undo lost exact saved bytes",
    )?;
    h.frame(key(
        egui::Key::Z,
        egui::Modifiers {
            shift: true,
            ..command_modifiers()
        },
    ))?;
    require(
        h.document(ORIGIN)?.text == text("dirty", "origin"),
        "native Redo lost exact dirty bytes",
    )?;
    h.preserve(origin, departure, &history)?;
    let after_edit = h.origin_snapshot()?;
    require(
        h.document(ORIGIN)?.edit_version
            > original["edit_version"]
                .as_u64()
                .ok_or("missing original version")?,
        "native Undo/Redo did not advance version",
    )?;
    h.case(CASES[9])?;

    h.click("find_results_entry", false)?;
    h.settle()?;
    h.phase = "nested";
    h.replace("search_scope", "src Ω/nested")?;
    h.replace("search_masks", "*.rs;*.md")?;
    h.click("search_submit", true)?;
    let (nested_reply, mut nested_observation) = h.search_reply(
        Route::CaseV1,
        "src Ω/nested",
        &["*.rs", "*.md"],
        &[NESTED],
        Some(1),
        &[NESTED],
    )?;
    let nested_id = nested_reply.id;
    require(
        h.app.search_busy()
            && h.app
                .search_state
                .pending
                .as_ref()
                .is_some_and(|s| s.request == nested_id),
        "nested App job not pending before delivery hold",
    )?;
    let retained = h.app.search_state.accepted.clone();
    let historical =
        serde_json::to_value(&h.app.search_results).map_err(|_| "historical serialization")?;
    let history_before = h.location_snapshot();
    h.toggle_pointer("search_match_case")?;
    h.toggle_pointer("search_match_case")?;
    require(
        !h.app.search_match_case
            && !h.app.search_rows_active()
            && h.app.search_state.pending.is_none()
            && h.app.search_busy()
            && h.app.pending.contains_key(&nested_id)
            && !recorded_response(&h.app, "search_submit").enabled(),
        "case retirement cancelled or prematurely released App job",
    )?;
    let before_error = h.app.error.clone();
    h.app.apply_event(nested_reply);
    h.settle()?;
    require(
        !h.app.search_busy()
            && !h.app.pending.contains_key(&nested_id)
            && !h.app.search_rows_active()
            && h.app.search_state.accepted == retained
            && serde_json::to_value(&h.app.search_results)
                .map_err(|_| "retained rows serialization")?
                == historical
            && h.app.error == before_error
            && h.location_snapshot() == history_before
            && !h.app.has_search_preview()
            && !h.app.browse.preview_visible(),
        "retired real Case reply revived authority or failed to drain",
    )?;
    nested_observation["accepted_rows"] = serde_json::json!(0);
    h.observations.push(nested_observation);
    h.preserve_origin(&after_edit)?;
    h.case(CASES[10])?;

    h.phase = "root_mask";
    h.replace("search_scope", "")?;
    h.replace("search_masks", "*.rs")?;
    h.click("search_submit", true)?;
    h.search(
        Route::CaseV1,
        "",
        &["*.rs"],
        &[ROOT, ORIGIN, TARGET],
        Some(9),
        &[ROOT, ORIGIN, TARGET],
    )?;
    h.preserve_origin(&after_edit)?;
    h.case(CASES[11])?;
    h.phase = "empty";
    h.replace("search_scope", "src Ω")?;
    h.replace("search_masks", "*.zz")?;
    h.click("search_submit", true)?;
    h.search(Route::CaseV1, "src Ω", &["*.zz"], &[], Some(5), &[])?;
    require(
        h.app.search_results.is_empty(),
        "complete empty result kept old rows",
    )?;
    h.preserve_origin(&after_edit)?;
    h.case(CASES[12])?;
    let before_edits = h.ledger.len();
    h.replace("search_scope", "outside")?;
    h.replace("search_scope", "src Ω")?;
    h.replace("search_masks", "*")?;
    h.replace("search_masks", "*.zz")?;
    h.replace("search_query", "changed")?;
    h.replace("search_query", QUERY)?;
    require(
        h.ledger.len() == before_edits && !h.app.search_rows_active() && !h.app.search_match_case,
        "local edits dispatched or revived old authority",
    )?;
    h.preserve_origin(&after_edit)?;
    h.case(CASES[13])?;

    let before_shell = h.ledger.len();
    let shell_state = h.shell_snapshot();
    h.frame(key(
        egui::Key::F,
        egui::Modifiers {
            shift: true,
            ..command_modifiers()
        },
    ))?;
    h.settle()?;
    h.click("browse_files", false)?;
    h.settle()?;
    require(
        h.app.browse.query().is_empty() && !h.app.search_match_case,
        "Files provider reset shared case draft",
    )?;
    h.click("browse_text", false)?;
    h.settle()?;
    require(
        h.app.browse.query() == QUERY && !h.app.search_match_case,
        "Text provider reset shared case draft",
    )?;
    h.frame(key(egui::Key::Escape, egui::Modifiers::NONE))?;
    h.settle()?;
    require(
        h.shell_snapshot() == shell_state,
        "provider roundtrip changed exact shell/editor state",
    )?;
    let editor = recorded_response(&h.app, "editor").id;
    let project_probe = h
        .app
        .entries
        .first()
        .ok_or("root Project row absent")?
        .name
        .clone();
    h.frame(key(egui::Key::Num1, egui::Modifiers::ALT))?;
    h.settle()?;
    let project = recorded_response(&h.app, &project_probe);
    require(
        project.enabled()
            && project.sense.is_focusable()
            && recorded_clip(&h.app, &project_probe).contains_rect(project.rect)
            && h.app.editor_ctx.memory(|m| m.focused()) == Some(project.id)
            && h.app.tools_open
            && h.app.tool == Tool::Search,
        "Alt+1 did not focus useful Project retaining Find",
    )?;
    require(
        h.shell_snapshot() == shell_state,
        "Project focus changed retained exact state",
    )?;
    h.frame(key(egui::Key::J, command_modifiers()))?;
    h.settle()?;
    require(
        !h.app.tools_open
            && h.app.editor_ctx.memory(|m| m.focused()) == Some(editor)
            && recorded_response(&h.app, "editor").id == editor,
        "hide did not restore exact editor",
    )?;
    require(
        h.shell_snapshot() == shell_state,
        "hide changed retained exact state",
    )?;
    h.click("find_results_entry", false)?;
    h.settle()?;
    require(
        h.app.tools_open
            && h.app.tool == Tool::Search
            && !h.app.search_rows_active()
            && !h.app.search_match_case
            && h.app.editor_ctx.memory(|m| m.focused())
                == Some(recorded_response(&h.app, "search_query").id),
        "Find reopen did not retain false and query fallback",
    )?;
    require(
        h.shell_snapshot() == shell_state && h.ledger.len() == before_shell,
        "shell transitions mutated state or dispatched",
    )?;
    h.preserve_origin(&after_edit)?;
    h.case(CASES[14])?;

    require(
        h.ledger == expected_ledger(),
        "exact twelve-operation ledger differed",
    )?;
    require(
        h.observations.len() == 7 && h.connection_attempts == 1,
        "search or connection count differed",
    )?;
    let hashes = h.source_hashes()?;
    h.preserve(origin, departure, &history)?;
    h.finish()?;
    require(
        CASES.get(h.cases.len()) == Some(&CASES[15]),
        "final cleanup witness order differed",
    )?;
    h.cases.push(CASES[15]);
    let mut receipt = serde_json::json!({"schema":1,"acceptance":"case_search","success":true,
        "os":if cfg!(windows) {"windows"} else {"linux"},"source_version":env!("CARGO_PKG_VERSION"),"source_agent_version_match":true,
        "cases":h.cases,"cases_executed":16,"operation_ledger":h.ledger,"source_hashes":hashes,"search_observations":h.observations,
        "requests":12,"hello":1,"list":1,"read":3,"search":1,"search_in":1,"search_case":5,"write":0,"other_operations":0,
        "connection_attempts":h.connection_attempts,"connections":h.connected,"agents_reaped":h.reaped,"fixture_files":6});
    let facts = [
        serde_json::json!({
            "movement_operations":0,"filter_edit_operations":0,"toggle_operations":0,"dirty_preview_reads":0,"unopened_preview_reads":1,
            "retired_unopened_open_reads":1,"fresh_cached_open_reads":0,"shell_transition_operations":h.ledger.len()-before_shell,
            "trust_off":true,"no_language_session":true,"no_recovery_actor":true,"production_native_frames":true,"recording_worker_client_bridge":true,
            "default_match_case_checked":true,"shared_match_case_controls":true,"native_pointer_space_enter_toggle":true,
            "exact_scoped_and_case_echo_validated":true,"literal_metacharacters_preserved":true,"original_unicode_snippets_preserved":true,
            "mask_case_sensitive":true,"complete_no_match":true,"toggle_back_does_not_revive":true,"retired_open_cached_inactive":true,
            "retired_open_no_activation_or_history":true,"retired_case_app_job_drained":true,"retired_case_no_active_rows_preview_or_error":true,
        }),
        serde_json::json!({
        "dirty_text_and_baseline_preserved":true,"edit_version_preserved_between_edits":true,"full_selection_affinity_preserved":true,
        "complete_native_history_preserved":true,"native_undo_redo_preserved":true,"source_disk_unchanged":true,"exact_operation_ledger_verified":true,
        "optional_font_probe_untriggered":true,"shell_project_focus_retains_find":true,"shell_find_reopen_query_fallback":true,
        "shell_hide_exact_editor_return":true,"shell_provider_retains_match_case":true,"shell_state_preserved":true,
        "shared_active_seconds":340,"request_admission_seconds":30,"cleanup_timeout_seconds":30,"proof_watchdog_seconds":390,
        "owned_children_reaped":true,"watchdog_joined":true,"fixture_removed":true}),
    ];
    for facts in facts {
        receipt
            .as_object_mut()
            .ok_or("receipt object absent")?
            .extend(facts.as_object().ok_or("facts object absent")?.clone());
    }
    h.receipt_emitted = true;
    println!("case_search_acceptance {receipt}");
    Ok(())
}
