//! A finite, explicitly requested startup lifecycle. It does not parallelize LSP queries.
use super::{Action, ActionKind, View};
use crate::{CedarApp, Job, Operation, Payload, AMBER, MUTED};
use eframe::egui::{self, RichText};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

const POLL_SECONDS: f64 = 0.25;
// Preserve the existing Java startup envelope. Replies never extend this bound.
const STARTUP_SECONDS: f64 = 75.0;
const UNKNOWN: &str = "Java startup cleanup could not be verified. Reconnect before starting another Java session. Your draft is retained.";

pub(super) struct Startup {
    generation: u64,
    session: u64,
    id: Option<u64>,
    cancel_intent: bool,
    cancel_sent: bool,
    timed_out: bool,
    unverified: bool,
    next_poll: f64,
    deadline: f64,
}
impl Startup {
    pub(super) fn active(&self) -> bool {
        !self.unverified
    }
    fn cancellation_message(&self) -> &'static str {
        if self.timed_out {
            "Java startup timed out; cancelling and waiting for verified process cleanup."
        } else {
            "Cancelling Java startup; waiting for verified process cleanup."
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum Snapshot {
    Starting {
        startup_id: u64,
        process_id: Option<u32>,
    },
    Cancelling {
        startup_id: u64,
        process_id: Option<u32>,
    },
    Ready {
        startup_id: u64,
        language: Value,
    },
    Cancelled {
        startup_id: u64,
        cleanup_verified: bool,
    },
    Failed {
        startup_id: u64,
        cleanup_verified: bool,
        error: StartupError,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartupError {
    code: String,
    message: String,
}
impl StartupError {
    fn verified_failure_message(&self) -> &'static str {
        match self.code.as_str() {
            "language_startup_timeout" => "Java startup timed out; process cleanup verified.",
            "invalid_java_launch" => "Java startup configuration was rejected; process cleanup verified. Check the Java executable, JDT distribution and data directory on the workspace host.",
            "invalid_path" => "Java startup could not use the workspace path; process cleanup verified.",
            "language_startup_failed" => "Java startup could not launch the server; process cleanup verified.",
            "language_error" => "Java startup failed during language-server initialization; process cleanup verified.",
            _ => "Java startup failed; process cleanup verified.",
        }
    }
}
impl Snapshot {
    fn parse(value: Value) -> Result<Self, ()> {
        if matches!(
            value.get("state").and_then(Value::as_str),
            Some("starting" | "cancelling")
        ) && value.get("process_id").is_none()
        {
            return Err(());
        }
        let snapshot: Self = serde_json::from_value(value).map_err(|_| ())?;
        if snapshot.id() == 0 {
            return Err(());
        }
        match &snapshot {
            Self::Starting {
                process_id: Some(0),
                ..
            }
            | Self::Cancelling {
                process_id: Some(0),
                ..
            } => return Err(()),
            Self::Ready { language, .. }
                if language.get("started").and_then(Value::as_bool) != Some(true)
                    || !language
                        .get("initialize")
                        .and_then(|v| v.get("capabilities"))
                        .is_some_and(Value::is_object) =>
            {
                return Err(())
            }
            Self::Cancelled {
                cleanup_verified: false,
                ..
            } => return Err(()),
            Self::Failed { error, .. }
                if error.code.is_empty()
                    || error.code.len() > 128
                    || error.message.len() > 4096 =>
            {
                return Err(())
            }
            _ => {}
        }
        Ok(snapshot)
    }
    fn id(&self) -> u64 {
        match self {
            Self::Starting { startup_id, .. }
            | Self::Cancelling { startup_id, .. }
            | Self::Ready { startup_id, .. }
            | Self::Cancelled { startup_id, .. }
            | Self::Failed { startup_id, .. } => *startup_id,
        }
    }
}

impl CedarApp {
    pub(super) fn begin_java_startup(&mut self, operation: Operation) {
        let now = self.editor_ctx.input(|input| input.time);
        self.language.automatic = false;
        self.language.startup = Some(Startup {
            generation: self.generation,
            session: self.language.session,
            id: None,
            cancel_intent: false,
            cancel_sent: false,
            timed_out: false,
            unverified: false,
            next_poll: now + POLL_SECONDS,
            deadline: now + STARTUP_SECONDS,
        });
        self.language.output = "Starting Java server. Files and command tasks remain available; language features wait until it is ready.".into();
        if self.language_request(operation, ActionKind::JavaStartBegin) == 0 && self.ready() {
            self.java_startup_unknown();
        }
    }
    fn java_startup_request_pending(&self) -> bool {
        self.pending.values().any(|job| matches!(job, Job::Language(action) if action.session == self.language.session && action.is_java_startup()))
    }
    fn java_startup_current(&self, action: &Action) -> bool {
        let Some(startup) = &self.language.startup else {
            return false;
        };
        startup.active()
            && startup.generation == self.generation
            && startup.session == action.session
            && action.session == self.language.session
            && match action.kind {
                ActionKind::JavaStartBegin => startup.id.is_none(),
                ActionKind::JavaStartPoll { startup_id }
                | ActionKind::JavaStartCancel { startup_id } => startup.id == Some(startup_id),
                _ => false,
            }
    }
    pub(crate) fn apply_java_startup_event(
        &mut self,
        action: Action,
        result: Result<Payload, String>,
        connected: bool,
    ) {
        if !connected {
            self.close_after_language_stop = false;
            self.close_snapshot = None;
            self.disconnected(UNKNOWN.into());
            return;
        }
        if !self.java_startup_current(&action) {
            return;
        }
        match result {
            Ok(Payload::Language { value }) => self.apply_java_startup_action(action, value),
            _ => self.java_startup_unknown(),
        }
    }
    fn java_startup_unknown(&mut self) {
        if let Some(startup) = &mut self.language.startup {
            startup.unverified = true;
        }
        self.language.running = false;
        self.language.automatic = false;
        self.language.intent = None;
        self.language.restart_blocked = true;
        self.close_after_language_stop = false;
        self.close_snapshot = None;
        self.language.output = UNKNOWN.into();
        self.error = Some(UNKNOWN.into());
    }
    pub(super) fn cancel_java_startup(&mut self) {
        let Some(startup) = self
            .language
            .startup
            .as_mut()
            .filter(|startup| startup.active())
        else {
            return;
        };
        startup.cancel_intent = true;
        let message = startup.cancellation_message();
        self.language.automatic = false;
        self.language.intent = None;
        self.language.output = message.into();
        self.send_java_startup_cancel();
    }
    fn send_java_startup_cancel(&mut self) {
        if self.java_startup_request_pending() || !self.ready() {
            return;
        }
        let Some(startup) =
            self.language.startup.as_mut().filter(|startup| {
                startup.active() && startup.cancel_intent && !startup.cancel_sent
            })
        else {
            return;
        };
        let Some(startup_id) = startup.id else {
            return;
        };
        startup.cancel_sent = true;
        if self.language_request(
            Operation::LanguageStartJavaCancel { startup_id },
            ActionKind::JavaStartCancel { startup_id },
        ) == 0
            && self.ready()
        {
            self.java_startup_unknown();
        }
    }
    pub(super) fn java_startup_tick(&mut self, ctx: &egui::Context) {
        if !self.ready() {
            return;
        }
        let now = ctx.input(|input| input.time);
        let Some(startup) = self
            .language
            .startup
            .as_ref()
            .filter(|startup| startup.active())
        else {
            return;
        };
        if startup.generation != self.generation || startup.session != self.language.session {
            return;
        }
        if now >= startup.deadline && !startup.cancel_intent {
            // Expiry requests cleanup once; it is never evidence that cleanup finished.
            self.language.startup.as_mut().unwrap().timed_out = true;
            self.cancel_java_startup();
        }
        let Some(startup) = self
            .language
            .startup
            .as_ref()
            .filter(|startup| startup.active())
        else {
            return;
        };
        let delay = if now >= startup.next_poll {
            POLL_SECONDS
        } else {
            (startup.next_poll - now).clamp(0.016, POLL_SECONDS)
        };
        ctx.request_repaint_after(Duration::from_secs_f64(delay));
        self.send_java_startup_cancel();
        // Enqueue behind existing work, without starving cleanup when file/task
        // traffic continues. The worker still executes one request at a time.
        if self.java_startup_request_pending() {
            return;
        }
        let Some(startup) = self
            .language
            .startup
            .as_ref()
            .filter(|startup| startup.active())
        else {
            return;
        };
        if now < startup.next_poll {
            return;
        }
        let Some(startup_id) = startup.id else {
            return;
        };
        if self.language_request(
            Operation::LanguageStartJavaPoll { startup_id },
            ActionKind::JavaStartPoll { startup_id },
        ) == 0
            && self.ready()
        {
            self.java_startup_unknown();
        }
    }
    pub(super) fn apply_java_startup_action(&mut self, action: Action, value: Value) {
        if !self.java_startup_current(&action) {
            return;
        }
        let Ok(snapshot) = Snapshot::parse(value) else {
            self.java_startup_unknown();
            return;
        };
        let id = snapshot.id();
        if matches!(action.kind, ActionKind::JavaStartBegin)
            && !matches!(snapshot, Snapshot::Starting { .. })
        {
            self.java_startup_unknown();
            return;
        }
        let startup = self.language.startup.as_mut().unwrap();
        if startup.id.is_some_and(|expected| expected != id) {
            self.java_startup_unknown();
            return;
        }
        startup.id = Some(id);
        let now = self.editor_ctx.input(|input| input.time);
        if now >= startup.deadline {
            startup.timed_out |= !startup.cancel_intent;
            startup.cancel_intent = true;
            self.language.output = startup.cancellation_message().into();
        }
        startup.next_poll = now + POLL_SECONDS;
        match snapshot {
            Snapshot::Starting { .. } => {
                if matches!(action.kind, ActionKind::JavaStartCancel { .. }) || startup.cancel_sent
                {
                    self.java_startup_unknown();
                    return;
                }
            }
            Snapshot::Cancelling { .. } => {
                // Cancellation reported by the owner is also monotonic.
                startup.cancel_intent = true;
                startup.cancel_sent = true;
                self.language.output = startup.cancellation_message().into();
            }
            Snapshot::Ready { language, .. } => {
                if startup.cancel_intent {
                    if startup.cancel_sent {
                        self.java_startup_unknown();
                        return;
                    }
                    // Keep the owner ID through ready/cancel handoff. Do not activate or sync.
                    self.send_java_startup_cancel();
                    return;
                }
                if self.language.maven.enabled && !super::java_maven::valid_startup(&language) {
                    self.java_startup_unknown();
                    return;
                }
                self.language.startup = None;
                self.apply_language_action(
                    Action {
                        session: action.session,
                        kind: ActionKind::Start,
                    },
                    language,
                );
                return;
            }
            Snapshot::Cancelled { .. } => {
                let message = if startup.timed_out {
                    "Java startup timed out; process cleanup verified."
                } else {
                    "Java startup cancelled; process cleanup verified."
                };
                self.language.reset();
                self.language.automatic = false;
                self.language.output = message.into();
                self.notice = self.language.output.clone();
                return;
            }
            Snapshot::Failed {
                cleanup_verified,
                error,
                ..
            } => {
                if !cleanup_verified {
                    self.java_startup_unknown();
                    return;
                }
                self.language.reset();
                self.language.automatic = false;
                self.language.output = error.verified_failure_message().into();
                self.error = Some(self.language.output.clone());
                self.close_after_language_stop = false;
                self.close_snapshot = None;
                return;
            }
        }
        self.send_java_startup_cancel();
    }
    pub(super) fn java_startup_controls(&mut self, ui: &mut egui::Ui) {
        let Some(startup) = self
            .language
            .startup
            .as_ref()
            .filter(|startup| startup.active())
        else {
            return;
        };
        let cancelling = startup.cancel_intent;
        let timed_out = startup.timed_out;
        ui.horizontal_wrapped(|ui| {
            ui.spinner();
            ui.colored_label(
                AMBER,
                if cancelling && timed_out {
                    "Cancelling timed-out Java startup"
                } else if cancelling {
                    "Cancelling Java startup"
                } else {
                    "Starting Java server"
                },
            );
            if ui
                .add_enabled(!cancelling, egui::Button::new("Cancel startup"))
                .clicked()
            {
                self.cancel_java_startup();
            }
        });
        ui.label(RichText::new("Files and command tasks remain available. Language features wait for Ready; cancellation waits for verified cleanup.").small().color(MUTED));
        self.language.view = View::Activity;
    }
}

#[cfg(test)]
#[path = "java_startup_tests.rs"]
mod tests;
