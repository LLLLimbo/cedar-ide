//! Explicit asynchronous commands: bounded full snapshots, no automatic restart.
use crate::{CedarApp, Job, Tool, AMBER, GREEN, MUTED, RED};
use cedar_protocol::Operation;
use cedar_tasks::{TaskSnapshot, TaskState};
use eframe::egui::{self, RichText};
use std::time::Duration;

const POLL_SECONDS: f64 = 0.25;
#[derive(Clone, Copy, Debug)]
pub(super) enum Kind {
    Start,
    Poll(u64),
    Cancel(u64),
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Action {
    pub epoch: u64,
    pub kind: Kind,
}
#[derive(Clone, Copy)]
pub(super) enum Transition {
    Reconnect,
    Close,
}
#[derive(Default)]
pub(super) struct RunPanel {
    epoch: u64,
    pub snapshot: Option<TaskSnapshot>,
    starting: bool,
    next_poll: f64,
    pub output: String,
    unknown: Option<String>,
    unknown_acknowledged: bool,
    transition: Option<Transition>,
}
impl RunPanel {
    fn active(&self) -> bool {
        self.starting
            || self
                .snapshot
                .as_ref()
                .is_some_and(|task| !task.state.is_terminal())
    }
    fn blocks_transition(&self) -> bool {
        if self.unknown.is_some() {
            !self.unknown_acknowledged
        } else {
            self.active()
        }
    }
    fn can_start(&self) -> bool {
        !self.active() && self.unknown.is_none()
    }
    pub fn reset(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.snapshot = None;
        self.starting = false;
        self.unknown = None;
        self.unknown_acknowledged = false;
        self.transition = None;
        self.next_poll = 0.0;
    }
    pub fn disconnected(&mut self) {
        if self.active() {
            self.unknown = Some("Connection lost while a command was active. Its outcome is unknown; it may still be running. Verify it on the workspace host before running it again".into());
            self.unknown_acknowledged = false;
        }
        // IDs are never reused to address a new agent session.
        self.snapshot = None;
        self.starting = false;
        self.epoch = self.epoch.wrapping_add(1);
    }
    fn mark_unknown(&mut self, message: String) {
        self.starting = false;
        self.unknown = Some(message);
        self.unknown_acknowledged = false;
    }
    fn accept(&mut self, action: Action, value: serde_json::Value, now: f64) -> Result<(), String> {
        if action.epoch != self.epoch {
            return Ok(());
        }
        let task: TaskSnapshot = serde_json::from_value(value)
            .map_err(|error| format!("Invalid command status: {error}"))?;
        let limit = cedar_tasks::MAX_OUTPUT_BYTES_PER_STREAM * 3;
        if task.id == 0
            || task.stdout.len() > limit
            || task.stderr.len() > limit
            || task.error.as_ref().is_some_and(|text| text.len() > 4096)
        {
            return Err("Command status exceeded supported bounds".into());
        }
        let expected = match action.kind {
            Kind::Start => None,
            Kind::Poll(id) | Kind::Cancel(id) => Some(id),
        };
        if expected.is_some_and(|id| id != task.id) {
            return Err("Command status belongs to a different task; it was ignored".into());
        }
        if !matches!(action.kind, Kind::Start)
            && self.snapshot.as_ref().is_none_or(|old| old.id != task.id)
        {
            return Ok(());
        }
        self.output = format!(
            "{}{}{}\n\n{}{}{}{}",
            task.stdout,
            if task.stderr.is_empty() {
                ""
            } else {
                "\n[stderr]\n"
            },
            task.stderr,
            state_label(task.state),
            task.exit_code
                .map(|code| format!(" · exit {code}"))
                .unwrap_or_default(),
            if task.truncated {
                " · output truncated"
            } else {
                ""
            },
            task.error
                .as_ref()
                .map(|error| format!("\n{error}"))
                .unwrap_or_default()
        );
        self.starting = false;
        self.unknown = None;
        self.unknown_acknowledged = false;
        self.next_poll = now + POLL_SECONDS;
        self.snapshot = Some(task);
        Ok(())
    }
}
fn state_label(state: TaskState) -> &'static str {
    match state {
        TaskState::Starting => "Starting",
        TaskState::Running => "Running",
        TaskState::Cancelling => "Cancelling (waiting for terminal status)",
        TaskState::Succeeded => "Succeeded",
        TaskState::Failed => "Failed",
        TaskState::Cancelled => "Cancelled",
        TaskState::TimedOut => "Timed out",
        TaskState::OutputLimit => "Stopped at output limit",
        TaskState::SpawnFailed => "Failed to start executable",
    }
}
impl CedarApp {
    fn run_request_pending(&self) -> bool {
        self.pending.values().any(|job| matches!(job, Job::Run(_)))
    }
    pub(super) fn run(&mut self) {
        if !self.run_state.can_start()
            || self.run_request_pending()
            || self.recovery.closing.is_some()
            || self.close_after_language_stop
        {
            return;
        }
        if !self.active_form.as_ref().is_some_and(|form| form.allow_run) {
            self.error = Some("Command execution is disabled for this connection".into());
            return;
        }
        let args: Vec<String> = match serde_json::from_str(&self.run_args) {
            Ok(args) => args,
            Err(_) => {
                self.error = Some("Arguments must be a JSON string array, for example [\"test\", \"--workspace\"]".into());
                return;
            }
        };
        let program = self.run_program.trim().to_owned();
        if program.is_empty()
            || program.len() > cedar_tasks::MAX_PROGRAM_BYTES
            || program.contains('\0')
            || args.len() > cedar_tasks::MAX_ARGUMENTS
            || args.iter().any(|arg| arg.contains('\0'))
            || args.iter().map(String::len).sum::<usize>() > cedar_tasks::MAX_ARGUMENT_BYTES
        {
            self.error = Some("Enter a valid executable and bounded literal arguments (256 arguments / 64 KiB maximum; no NUL)".into());
            return;
        }
        if !(1..=300).contains(&self.run_timeout) {
            self.error = Some("Command timeout must be from 1 to 300 seconds".into());
            return;
        }
        self.run_state.epoch = self.run_state.epoch.wrapping_add(1);
        let action = Action {
            epoch: self.run_state.epoch,
            kind: Kind::Start,
        };
        self.run_state.snapshot = None;
        let id = self.request(
            Operation::RunStart {
                program: program.clone(),
                args,
                timeout_secs: self.run_timeout,
            },
            Job::Run(action),
        );
        if id != 0 {
            self.run_state.starting = true;
            self.run_state.output = format!(
                "$ {program} {}\nWaiting for command acceptance...",
                self.run_args
            );
        }
    }
    fn cancel_run(&mut self) {
        if self.run_request_pending() || !self.ready() {
            return;
        }
        let Some(task) = self
            .run_state
            .snapshot
            .as_ref()
            .filter(|task| !task.state.is_terminal())
        else {
            return;
        };
        let id = task.id;
        let action = Action {
            epoch: self.run_state.epoch,
            kind: Kind::Cancel(id),
        };
        if self.request(Operation::RunCancel { task_id: id }, Job::Run(action)) != 0 {
            if let Some(task) = &mut self.run_state.snapshot {
                task.state = TaskState::Cancelling;
            }
            self.notice =
                "Cancellation requested. Waiting for the command's terminal status".into();
        }
    }
    fn poll_run(&mut self) {
        if self.run_request_pending() || !self.ready() {
            return;
        }
        let Some(task) = self
            .run_state
            .snapshot
            .as_ref()
            .filter(|task| !task.state.is_terminal())
        else {
            return;
        };
        let id = task.id;
        self.request(
            Operation::RunPoll { task_id: id },
            Job::Run(Action {
                epoch: self.run_state.epoch,
                kind: Kind::Poll(id),
            }),
        );
    }
    pub(super) fn run_tick(&mut self, ctx: &egui::Context) {
        if !self.ready()
            || self.run_state.unknown.is_some()
            || !self.run_state.active()
            || self.run_request_pending()
        {
            return;
        }
        let now = ctx.input(|input| input.time);
        if now >= self.run_state.next_poll {
            self.poll_run();
        }
        ctx.request_repaint_after(Duration::from_secs_f64(
            (self.run_state.next_poll - now).clamp(0.01, POLL_SECONDS),
        ));
    }
    pub(super) fn apply_run(&mut self, action: Action, value: serde_json::Value) {
        if action.epoch != self.run_state.epoch {
            return;
        }
        let now = self.editor_ctx.input(|input| input.time);
        if let Err(error) = self.run_state.accept(action, value, now) {
            self.run_state.mark_unknown(format!(
                "{error}. Command outcome is unknown; it will not be restarted"
            ));
            self.error = self.run_state.unknown.clone();
        } else if let Some(task) = self
            .run_state
            .snapshot
            .as_ref()
            .filter(|task| task.state.is_terminal())
        {
            self.notice = format!(
                "Command {}. Close or reconnect can now be retried",
                state_label(task.state).to_lowercase()
            );
        }
    }
    pub(super) fn run_error(&mut self, action: &Action, connected: bool, error: &str) {
        if action.epoch != self.run_state.epoch {
            return;
        }
        let rejected = matches!(action.kind, Kind::Start)
            && connected
            && [
                "run_disabled:",
                "invalid_command:",
                "invalid_timeout:",
                "unsupported_platform:",
                "invalid_root:",
                "task_capacity:",
            ]
            .iter()
            .any(|prefix| error.starts_with(prefix));
        if rejected {
            self.run_state.starting = false;
            self.run_state.output = format!("Command was not started: {error}");
        } else {
            self.run_state.mark_unknown(format!("Command status or cancellation could not be confirmed: {error}. It may still be running; no command was retried"));
        }
    }
    pub(super) fn guard_run_transition(&mut self, transition: Transition) -> bool {
        if self.run_state.blocks_transition() {
            self.run_state.transition = Some(transition);
            self.tool = Tool::Run;
            self.tools_open = true;
            false
        } else {
            self.run_state.transition = None;
            true
        }
    }
    pub(super) fn run_panel(&mut self, ui: &mut egui::Ui) {
        let allowed = self.active_form.as_ref().is_some_and(|form| form.allow_run);
        if !allowed {
            ui.colored_label(AMBER, "Command execution is off. Enable trust in Open workspace and reconnect only for a workspace you trust.");
        }
        ui.add_enabled_ui(allowed && self.ready(), |ui| {
            ui.horizontal(|ui| {
                ui.label("Executable");
                ui.add(
                    egui::TextEdit::singleline(&mut self.run_program)
                        .hint_text("cargo")
                        .desired_width(160.0),
                );
                ui.label("Arguments (JSON)");
                ui.add(
                    egui::TextEdit::singleline(&mut self.run_args)
                        .font(egui::TextStyle::Monospace)
                        .hint_text("[\"test\"]")
                        .desired_width((ui.available_width() - 235.0).max(100.0)),
                );
                ui.add(
                    egui::DragValue::new(&mut self.run_timeout)
                        .range(1..=300)
                        .suffix(" s"),
                );
                if ui
                    .add_enabled(
                        self.run_state.can_start()
                            && !self.run_request_pending()
                            && self.recovery.closing.is_none()
                            && !self.close_after_language_stop,
                        egui::Button::new("Run"),
                    )
                    .clicked()
                {
                    self.run();
                }
                if ui
                    .add_enabled(
                        self.run_state
                            .snapshot
                            .as_ref()
                            .is_some_and(|task| !task.state.is_terminal())
                            && !self.run_request_pending(),
                        egui::Button::new("Cancel"),
                    )
                    .clicked()
                {
                    self.cancel_run();
                }
            });
        });
        ui.label(RichText::new("Runs explicitly in the workspace with literal argv and no implicit shell. Live bounded output; editing and saving remain available. Commands are never automatically restarted.").small().color(MUTED));
        if self.run_state.starting {
            ui.colored_label(AMBER, "Waiting for command acceptance...");
        }
        if let Some(task) = &self.run_state.snapshot {
            ui.colored_label(
                if task.state == TaskState::Succeeded {
                    GREEN
                } else {
                    AMBER
                },
                format!("Task {} · {}", task.id, state_label(task.state)),
            );
        }
        if let Some(unknown) = self.run_state.unknown.clone() {
            ui.colored_label(RED, unknown);
            if ui
                .add_enabled(
                    self.ready()
                        && self.run_state.snapshot.is_some()
                        && !self.run_request_pending(),
                    egui::Button::new("Retry status only"),
                )
                .clicked()
            {
                self.poll_run();
            }
        }
        egui::ScrollArea::both()
            .id_salt("run_output")
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.run_state.output)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .interactive(false)
                        .frame(false),
                );
            });
    }
    pub(super) fn run_dialog(&mut self, ctx: &egui::Context) {
        let Some(transition) = self.run_state.transition else {
            return;
        };
        egui::Modal::new(egui::Id::new("command_transition")).show(ctx, |ui| {
            ui.set_max_width(530.0);
            ui.heading(match transition { Transition::Reconnect => "Command must finish before reconnecting", Transition::Close => "Command must finish before quitting" });
            if let Some(unknown) = &self.run_state.unknown {
                ui.colored_label(RED, unknown);
                ui.label("Continuing loses control of this task. It may still be running on the workspace host; check there before starting it again.");
                if ui.button("I understand; allow close or reconnect").clicked() {
                    self.run_state.unknown_acknowledged = true; self.run_state.transition = None;
                    self.notice = "Unknown command outcome acknowledged. Retry your close/reconnect action; dirty-draft checks still apply".into();
                }
            } else if self.run_state.active() {
                ui.label("Cancel explicitly and wait for a terminal status, then retry close or reconnect. You can keep editing and saving while cancellation finishes.");
                if self.run_state.starting { ui.label("Waiting for the task ID before cancellation is possible"); }
                if let Some(task) = &self.run_state.snapshot { ui.label(state_label(task.state)); }
                if ui.add_enabled(self.run_state.snapshot.is_some() && !self.run_request_pending() && self.ready(), egui::Button::new("Cancel command and wait")).clicked() { self.cancel_run(); }
            } else { ui.colored_label(GREEN, "Command finished. Retry close or reconnect; unsaved-draft checks still apply."); }
            if ui.button("Keep editing").clicked() { self.run_state.transition = None; }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Event;
    #[cfg(target_os = "linux")]
    use crate::{model::Document, ConnectForm, ConnectionState};
    use cedar_protocol::Payload;
    #[cfg(target_os = "linux")]
    use std::time::Instant;
    fn task(id: u64, state: TaskState, output: &str) -> TaskSnapshot {
        TaskSnapshot {
            id,
            state,
            stdout: output.into(),
            stderr: String::new(),
            exit_code: if state == TaskState::Succeeded {
                Some(0)
            } else {
                None
            },
            truncated: false,
            error: None,
        }
    }
    fn value(task: TaskSnapshot) -> serde_json::Value {
        serde_json::to_value(task).unwrap()
    }
    fn start(epoch: u64) -> Action {
        Action {
            epoch,
            kind: Kind::Start,
        }
    }
    #[test]
    fn full_snapshots_replace_output_and_terminal_stops_polling() {
        let mut panel = RunPanel {
            epoch: 7,
            ..Default::default()
        };
        panel
            .accept(start(7), value(task(2, TaskState::Running, "a")), 10.0)
            .unwrap();
        assert!(panel.active());
        assert_eq!(panel.next_poll, 10.25);
        panel
            .accept(
                Action {
                    epoch: 7,
                    kind: Kind::Poll(2),
                },
                value(task(2, TaskState::Running, "ab")),
                11.0,
            )
            .unwrap();
        assert!(panel.output.starts_with("ab\n"));
        assert!(!panel.output.starts_with("aab"));
        panel
            .accept(
                Action {
                    epoch: 7,
                    kind: Kind::Poll(2),
                },
                value(task(2, TaskState::Succeeded, "abc")),
                12.0,
            )
            .unwrap();
        assert!(!panel.active());
        assert!(panel.can_start());
        assert!(!panel.blocks_transition());
    }
    #[test]
    fn stale_session_and_wrong_id_cannot_replace_live_task() {
        let mut panel = RunPanel {
            epoch: 8,
            ..Default::default()
        };
        panel
            .accept(start(8), value(task(11, TaskState::Running, "new")), 0.0)
            .unwrap();
        panel
            .accept(start(7), value(task(2, TaskState::Succeeded, "old")), 0.0)
            .unwrap();
        assert_eq!(panel.snapshot.as_ref().unwrap().id, 11);
        assert!(panel
            .accept(
                Action {
                    epoch: 8,
                    kind: Kind::Poll(11)
                },
                value(task(12, TaskState::Succeeded, "wrong")),
                1.0
            )
            .is_err());
        assert_eq!(panel.snapshot.as_ref().unwrap().stdout, "new");
        panel.disconnected();
        assert!(panel.snapshot.is_none());
        assert!(panel.unknown.is_some());
        assert!(!panel.can_start());
        panel
            .accept(start(8), value(task(11, TaskState::Succeeded, "late")), 2.0)
            .unwrap();
        assert!(panel.snapshot.is_none());
        assert!(panel.blocks_transition());
        panel.unknown_acknowledged = true;
        assert!(!panel.blocks_transition());
        assert!(!panel.can_start());
        panel.reset();
        assert!(panel.snapshot.is_none());
        assert!(panel.can_start());
    }
    #[test]
    fn all_terminal_labels_are_distinct_and_payload_bounds_fail_closed() {
        let states = [
            TaskState::Succeeded,
            TaskState::Failed,
            TaskState::Cancelled,
            TaskState::TimedOut,
            TaskState::OutputLimit,
            TaskState::SpawnFailed,
        ];
        let labels: std::collections::HashSet<_> =
            states.iter().map(|state| state_label(*state)).collect();
        assert_eq!(labels.len(), states.len());
        let mut panel = RunPanel::default();
        let mut huge = task(1, TaskState::Running, "");
        huge.stdout = "x".repeat(cedar_tasks::MAX_OUTPUT_BYTES_PER_STREAM * 3 + 1);
        assert!(panel.accept(start(0), value(huge), 0.0).is_err());
        assert!(panel.snapshot.is_none());
    }
    #[test]
    fn active_commands_block_close_reconnect_but_not_save_mutation_checks() {
        let mut app = CedarApp::empty();
        app.run_state
            .accept(start(0), value(task(1, TaskState::Running, "")), 0.0)
            .unwrap();
        assert!(!app.mutation_pending());
        assert!(!app.guard_run_transition(Transition::Reconnect));
        assert!(!app.guard_run_transition(Transition::Close));
        app.run_state
            .accept(
                Action {
                    epoch: 0,
                    kind: Kind::Cancel(1),
                },
                value(task(1, TaskState::Cancelling, "")),
                0.0,
            )
            .unwrap();
        assert!(!app.guard_run_transition(Transition::Close));
        app.run_state
            .accept(
                Action {
                    epoch: 0,
                    kind: Kind::Poll(1),
                },
                value(task(1, TaskState::Cancelled, "")),
                1.0,
            )
            .unwrap();
        assert!(app.guard_run_transition(Transition::Close));
    }
    #[test]
    fn command_start_failures_and_unknown_cancel_never_retry() {
        let mut app = CedarApp::empty();
        app.run_state.starting = true;
        app.run_error(&start(0), true, "invalid_command: rejected");
        assert!(app.run_state.can_start());
        assert!(app.pending.is_empty());
        app.run_state
            .accept(start(0), value(task(2, TaskState::Running, "")), 0.0)
            .unwrap();
        app.run_error(
            &Action {
                epoch: 0,
                kind: Kind::Cancel(2),
            },
            true,
            "unknown_task: gone",
        );
        assert!(app.run_state.unknown.is_some());
        assert!(app.pending.is_empty());
        assert!(!app.guard_run_transition(Transition::Close));
    }
    #[test]
    fn outer_connection_generation_rejects_late_run_response() {
        let mut app = CedarApp::empty();
        app.generation = 3;
        app.pending.insert(2, Job::Run(start(0)));
        app.apply_event(Event {
            generation: 2,
            id: 2,
            connected: true,
            result: Ok(Payload::RunTask {
                snapshot: value(task(1, TaskState::Running, "late")),
            }),
        });
        assert!(app.run_state.snapshot.is_none());
    }
    #[cfg(target_os = "linux")]
    fn wait(app: &mut CedarApp, done: impl Fn(&CedarApp) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            app.poll();
            if done(app) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out: {:?}", app.error);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn actual_native_worker_streams_saves_while_running_and_cancels() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = CedarApp::empty();
        let form = ConnectForm {
            local_root: temp.path().to_string_lossy().into_owned(),
            allow_run: true,
            ..Default::default()
        };
        app.connect(&egui::Context::default(), form);
        wait(&mut app, |app| {
            app.state == ConnectionState::Ready && app.pending.is_empty()
        });
        app.run_program = "/bin/sh".into();
        app.run_args = r#"["-c", "printf first; sleep 0.05; printf second; sleep 10"]"#.into();
        app.run_timeout = 30;
        let started = Instant::now();
        app.run();
        assert!(started.elapsed() < Duration::from_secs(1));
        let after_start = app.next_request;
        app.run();
        assert_eq!(
            app.next_request, after_start,
            "duplicate start must be suppressed"
        );
        wait(&mut app, |app| {
            app.run_state.snapshot.is_some() && !app.run_request_pending()
        });
        assert!(app.run_state.active());
        let mut doc = Document::new(1, "while-running.txt".into(), String::new(), String::new());
        doc.revision = None;
        doc.text = "saved during command".into();
        app.documents.push(doc);
        app.active_document = Some(1);
        app.save();
        wait(&mut app, |app| !app.documents[0].dirty());
        assert_eq!(
            std::fs::read_to_string(temp.path().join("while-running.txt")).unwrap(),
            "saved during command"
        );
        assert!(app.run_state.active());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            app.poll_run();
            wait(&mut app, |app| !app.run_request_pending());
            if app.run_state.output.starts_with("firstsecond") {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!app.guard_run_transition(Transition::Reconnect));
        app.cancel_run();
        wait(&mut app, |app| !app.run_request_pending());
        loop {
            if !app.run_state.active() {
                break;
            }
            app.poll_run();
            wait(&mut app, |app| !app.run_request_pending());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            app.run_state.snapshot.as_ref().unwrap().state,
            TaskState::Cancelled
        );
        assert!(app.guard_run_transition(Transition::Reconnect));
        assert!(app.run_state.output.starts_with("firstsecond"));
    }
    #[test]
    fn command_panel_and_transition_dialog_layout() {
        let mut app = CedarApp::empty();
        app.run_state
            .accept(start(0), value(task(1, TaskState::Running, "live")), 0.0)
            .unwrap();
        app.run_state.transition = Some(Transition::Close);
        let ctx = egui::Context::default();
        for size in [[780.0, 540.0], [1320.0, 880.0]] {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(size[0], size[1]),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.run_panel(ui));
                    app.run_dialog(ctx);
                },
            );
            assert!(!output.shapes.is_empty());
        }
    }
}
