//! Explicit Windows Maven leaf profile and a finite, session-bound model snapshot.
//! Checking a model never saves a buffer, runs a build, or reimports the project.
use super::{safe_relative_path, Action, ActionKind, ServerMode, View};
use crate::{java_language::JavaConfiguration, CedarApp, Operation, Payload, AMBER, MUTED};
use eframe::egui::{self, RichText};
use serde::Deserialize;
use serde_json::Value;

const RESTART: &str = "Root pom.xml changed on disk. Stop and restart this Maven session to use it. Unsaved drafts are retained.";
const UNAVAILABLE: &str = "Maven model unavailable. This snapshot does not establish that the project imported successfully.";
const SERVER_EXITED: &str = "The Maven language server exited. Its model snapshot is unavailable. Stop this session and restart it explicitly; unsaved drafts are retained.";
const DIRTY_POM: &str = "pom.xml has unsaved edits. Maven uses the on-disk POM; this action does not save it. Save explicitly, then stop and restart to use POM changes.";

#[derive(Default)]
pub(super) struct MavenConfiguration {
    pub enabled: bool,
    pub local_repository: String,
}
impl MavenConfiguration {
    pub(super) fn operation(&self, java: &JavaConfiguration) -> Result<Operation, String> {
        let Operation::LanguageStartJava {
            java_executable,
            distribution,
            data_directory,
        } = java.operation()?
        else {
            unreachable!()
        };
        if !data_directory.is_ascii() {
            return Err("Maven mode requires an ASCII JDT data/control directory for the JDK user.home argument. Workspace, JDT distribution and Maven cache paths may contain Unicode.".into());
        }
        if !bounded_text(self.local_repository.trim(), 4096) {
            return Err("Enter an existing local Maven repository directory on the workspace host. Maven mode does not discover or download a cache.".into());
        }
        Ok(Operation::LanguageStartJavaMavenBegin {
            java_executable,
            distribution,
            data_directory,
            local_repository: self.local_repository.trim().into(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ModelContext {
    generation: u64,
    session: u64,
    sequence: u64,
    pom_sha256: String,
}
#[derive(Default, PartialEq, Eq)]
enum ModelStatus {
    #[default]
    Unchecked,
    Checking,
    Imported,
    Unresolved,
    Unavailable,
    RestartRequired,
    ServerExited,
}
#[derive(Default)]
pub(super) struct ModelState {
    pom_sha256: Option<String>,
    observation_request_floor: u64,
    supported: bool,
    sequence: u64,
    pending: Option<ModelContext>,
    status: ModelStatus,
    model: Option<Model>,
}
impl ModelState {
    pub(super) fn active(&self) -> bool {
        self.pom_sha256.is_some()
    }
    pub(super) fn cancel_pending(&mut self) {
        self.sequence = self.sequence.wrapping_add(1);
        self.pending = None;
        if self.status == ModelStatus::Checking {
            self.status = ModelStatus::Unchecked;
        }
    }
    pub(super) fn pom_sha256(&self) -> Option<&str> {
        self.pom_sha256.as_deref()
    }
    pub(super) fn restart_required(&self) -> bool {
        self.status == ModelStatus::RestartRequired
    }
    pub(super) fn require_restart(&mut self) {
        if self.status == ModelStatus::ServerExited {
            return;
        }
        self.cancel_pending();
        self.model = None;
        self.status = ModelStatus::RestartRequired;
    }
    pub(super) fn server_exited(&mut self) -> bool {
        if !self.active() || self.status == ModelStatus::ServerExited {
            return false;
        }
        // Retire adoption ownership, not the already-sent worker request. Its
        // response still drains in order and can report transport loss.
        self.cancel_pending();
        self.model = None;
        self.status = ModelStatus::ServerExited;
        true
    }
    pub(super) fn message(&self) -> &'static str {
        match self.status {
            ModelStatus::Unchecked => "Maven model not checked. Use Check Maven model for one read-only snapshot.",
            ModelStatus::Checking => "Checking the Maven model once; no build or reimport is requested.",
            ModelStatus::Imported => "Maven model imported. Dependency and source information is a snapshot, not a build result.",
            ModelStatus::Unresolved => "Maven model has unresolved entries. The local cache may be incomplete; no dependencies are downloaded.",
            ModelStatus::Unavailable => UNAVAILABLE,
            ModelStatus::RestartRequired => RESTART,
            ModelStatus::ServerExited => SERVER_EXITED,
        }
    }
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ImportedStatus {
    Imported,
    Unresolved,
    Unavailable,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Compiler {
    source: Option<String>,
    compliance: Option<String>,
    target: Option<String>,
    release_enabled: Option<bool>,
}
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum EntryKind {
    Source,
    Library,
    Container,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EntryOrigin {
    Model,
    Declared,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClasspathEntry {
    kind: EntryKind,
    path: String,
    resolved: bool,
    origin: EntryOrigin,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Model {
    profile: String,
    status: ImportedStatus,
    pom_path: String,
    pom_sha256: String,
    restart_required: bool,
    maven_nature: bool,
    compiler: Compiler,
    source_paths: Vec<String>,
    classpath: Vec<ClasspathEntry>,
    unresolved_count: usize,
    message: Option<String>,
}
fn bounded_text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn startup_hash(value: &Value) -> Option<&str> {
    let initialize = value.get("initialize")?;
    if initialize.get("cedar_java_profile")?.as_str()? != "maven_leaf" {
        return None;
    }
    initialize.get("cedar_java_maven_model")?.as_bool()?;
    initialize
        .get("cedar_java_maven_pom_sha256")?
        .as_str()
        .filter(|hash| valid_hash(hash))
}
pub(super) fn valid_startup(value: &Value) -> bool {
    startup_hash(value).is_some()
}
impl Model {
    fn parse(value: Value, expected_hash: &str) -> Result<Self, ()> {
        // Bound the collections and strings before deserializing owned copies.
        let sources = value
            .get("source_paths")
            .and_then(Value::as_array)
            .ok_or(())?;
        let classpath = value.get("classpath").and_then(Value::as_array).ok_or(())?;
        if sources.len() > 64 || classpath.len() > 256 {
            return Err(());
        }
        let compiler = value.get("compiler").and_then(Value::as_object).ok_or(())?;
        if ["source", "compliance", "target", "release_enabled"]
            .iter()
            .any(|name| !compiler.contains_key(*name))
        {
            return Err(());
        }
        if value.get("profile").and_then(Value::as_str) != Some("maven_leaf")
            || value.get("pom_path").and_then(Value::as_str) != Some("pom.xml")
            || value.get("pom_sha256").and_then(Value::as_str) != Some(expected_hash)
            || ["source", "compliance", "target"].iter().any(|name| {
                let level = &compiler[*name];
                !level.is_null()
                    && !level
                        .as_str()
                        .is_some_and(|value| bounded_text(value, 32) && value.is_ascii())
            })
            || value.get("message").is_some_and(|message| {
                !message.is_null()
                    && !message
                        .as_str()
                        .is_some_and(|message| bounded_text(message, 256))
            })
        {
            return Err(());
        }
        let mut bytes = 0usize;
        for path in sources.iter().map(Value::as_str).chain(
            classpath
                .iter()
                .map(|entry| entry.get("path").and_then(Value::as_str)),
        ) {
            let path = path.ok_or(())?;
            if !bounded_text(path, 4096) {
                return Err(());
            }
            bytes = bytes.checked_add(path.len()).ok_or(())?;
        }
        if bytes > 256 * 1024 {
            return Err(());
        }
        let model: Self = serde_json::from_value(value).map_err(|_| ())?;
        let unresolved_libraries = model
            .classpath
            .iter()
            .filter(|entry| entry.kind == EntryKind::Library && !entry.resolved)
            .count();
        if model.profile != "maven_leaf"
            || model.pom_path != "pom.xml"
            || !valid_hash(&model.pom_sha256)
            || model.pom_sha256 != expected_hash
            || model.restart_required
            || model.unresolved_count > 256
            || model.unresolved_count != unresolved_libraries
            || model
                .source_paths
                .iter()
                .any(|path| path != "." && !safe_relative_path(path))
            || model
                .message
                .as_ref()
                .is_some_and(|message| !bounded_text(message, 256))
            || [
                &model.compiler.source,
                &model.compiler.compliance,
                &model.compiler.target,
            ]
            .into_iter()
            .flatten()
            .any(|value| !bounded_text(value, 32) || !value.is_ascii())
            || model.status == ImportedStatus::Imported && model.unresolved_count != 0
            || model.status != ImportedStatus::Unavailable && !model.maven_nature
            || model.status == ImportedStatus::Unresolved && model.unresolved_count == 0
        {
            return Err(());
        }
        Ok(model)
    }
}

impl CedarApp {
    pub(super) fn activate_maven_model(&mut self, value: &Value) {
        if self.language.mode != ServerMode::Java || !self.language.maven.enabled {
            return;
        }
        let Some(hash) = startup_hash(value) else {
            return;
        };
        self.language.maven_model.pom_sha256 = Some(hash.into());
        // Existing tabs and already-submitted reads/saves may describe an older
        // disk state than the POM captured by the server. Only acknowledgements
        // of requests submitted after activation are new local evidence.
        self.language.maven_model.observation_request_floor = self.next_request;
        self.language.maven_model.supported =
            value["initialize"]["cedar_java_maven_model"].as_bool() == Some(true);
        if !self.language.maven_model.supported {
            self.language.maven_model.status = ModelStatus::Unavailable;
        }
    }
    fn maven_dirty_pom(&self) -> bool {
        self.documents
            .iter()
            .any(|doc| doc.path == "pom.xml" && doc.dirty())
    }
    pub(crate) fn observe_maven_pom_acknowledgement(
        &mut self,
        request: u64,
        path: &str,
        revision: &str,
    ) {
        let state = &mut self.language.maven_model;
        if self.language.running
            && !self.language.diagnostics_exited
            && path == "pom.xml"
            && request >= state.observation_request_floor
            && valid_hash(revision)
            && state
                .pom_sha256
                .as_deref()
                .is_some_and(|hash| hash != revision)
            && state.status != ModelStatus::RestartRequired
        {
            state.require_restart();
            self.language.maven_dependencies.reset();
        }
    }
    pub(crate) fn maven_model_problem(&self) -> Option<String> {
        if self.language.maven_model.status == ModelStatus::ServerExited {
            return Some(SERVER_EXITED.into());
        }
        if !self.ready()
            || !self.language.running
            || self.language.diagnostics_exited
            || !self.language.maven_model.active()
        {
            return Some("Start the explicit Maven Java profile first".into());
        }
        if !self.execution_trusted() {
            return Some(
                "Maven model checks require trusted tool permission for this connection".into(),
            );
        }
        if !self.backend_java_maven_supported() {
            return Some(self.unsupported_message("the complete typed Maven Java lifecycle"));
        }
        if self.language.maven_model.status == ModelStatus::RestartRequired {
            return Some(RESTART.into());
        }
        if !self.language.maven_model.supported {
            return Some(UNAVAILABLE.into());
        }
        if self.language_busy() || self.close_after_language_stop || self.recovery.closing.is_some()
        {
            return Some(
                "Wait for the current language request or close operation to finish".into(),
            );
        }
        None
    }
    fn check_maven_model(&mut self) {
        if let Some(problem) = self.maven_model_problem() {
            self.error = Some(problem);
            return;
        }
        let state = &mut self.language.maven_model;
        state.sequence = state.sequence.wrapping_add(1);
        let context = ModelContext {
            generation: self.generation,
            session: self.language.session,
            sequence: state.sequence,
            pom_sha256: state.pom_sha256.clone().unwrap(),
        };
        state.pending = Some(context.clone());
        state.model = None;
        state.status = ModelStatus::Checking;
        self.language.view = View::Maven;
        if self.language_request(
            Operation::LanguageMavenModel,
            ActionKind::MavenModel { context },
        ) == 0
        {
            self.language.maven_model.cancel_pending();
            self.language.maven_model.status = ModelStatus::Unavailable;
        }
    }
    pub(crate) fn apply_maven_model_event(
        &mut self,
        action: Action,
        result: Result<Payload, String>,
        connected: bool,
    ) {
        if !connected {
            self.disconnected("The connection closed while checking the Maven model. Your unsaved drafts are retained.".into());
            return;
        }
        let ActionKind::MavenModel { context } = action.kind else {
            return;
        };
        if context.generation != self.generation
            || context.session != self.language.session
            || action.session != context.session
        {
            return;
        }
        let state = &mut self.language.maven_model;
        if !self.language.running
            || self.language.diagnostics_exited
            || state.status == ModelStatus::ServerExited
            || state.pending.as_ref() != Some(&context)
            || state.pom_sha256.as_deref() != Some(context.pom_sha256.as_str())
        {
            return;
        }
        state.pending = None;
        match result {
            Ok(Payload::Language { value }) => match Model::parse(value, &context.pom_sha256) {
                Ok(model) => {
                    self.language.cjk_seen |= model
                        .source_paths
                        .iter()
                        .chain(model.classpath.iter().map(|entry| &entry.path))
                        .any(|path| crate::system_fonts::contains_cjk(path));
                    state.status = match model.status {
                        ImportedStatus::Imported => ModelStatus::Imported,
                        ImportedStatus::Unresolved => ModelStatus::Unresolved,
                        ImportedStatus::Unavailable => ModelStatus::Unavailable,
                    };
                    state.model = Some(model);
                }
                Err(()) => {
                    state.model = None;
                    state.status = ModelStatus::Unavailable;
                }
            },
            Err(error) if error.starts_with("language_maven_restart_required:") => {
                state.require_restart();
                self.language.maven_dependencies.reset();
            }
            _ => {
                state.model = None;
                state.status = ModelStatus::Unavailable;
            }
        }
        self.language.output = state.message().into();
    }
    pub(super) fn maven_configuration_controls(&mut self, ui: &mut egui::Ui) {
        let supported = self.backend_java_maven_supported();
        ui.add_enabled(
            supported && self.execution_trusted(),
            egui::Checkbox::new(
                &mut self.language.maven.enabled,
                "Import root Maven pom.xml (Windows, trusted leaf project)",
            ),
        );
        if !supported {
            ui.label(RichText::new("Maven mode needs the agent's typed Maven startup and model capabilities; there is no fallback.").small().color(MUTED));
            if self.language.maven.enabled
                && ui
                    .button("Use ordinary Java with imports disabled")
                    .clicked()
            {
                self.language.maven.enabled = false;
            }
        }
        if self.language.maven.enabled {
            ui.horizontal(|ui| {
                ui.label("Local Maven repository");
                ui.add(
                    egui::TextEdit::singleline(&mut self.language.maven.local_repository)
                        .hint_text("Existing absolute directory on the workspace host")
                        .desired_width(390.0),
                );
            });
            ui.label(RichText::new("Root pom.xml only. JDT data/control path must be ASCII; workspace, distribution and cache paths may contain Unicode.").small().color(MUTED));
            ui.label(RichText::new("Uses offline Maven dependencies in your selected cache. JDT may write cache/index data and run trusted configurator code; it may fetch public Gradle metadata. Offline mode is not network isolation or a code sandbox.").small().color(MUTED));
            ui.label(RichText::new("No dependency downloads, wrappers, automatic builds or automatic saves. Stop and restart after changing the on-disk POM. JDK class-file viewing is unavailable.").small().color(MUTED));
            if self.maven_dirty_pom() {
                ui.colored_label(AMBER, DIRTY_POM);
            }
        } else {
            ui.label("Language: Java. Maven and Gradle project imports are disabled; JDK class-file viewing is unavailable.");
        }
    }
    pub(super) fn maven_model_controls(&mut self, ui: &mut egui::Ui) {
        if !self.language.maven_model.active() {
            return;
        }
        let problem = self.maven_model_problem();
        let check = ui.add_enabled(problem.is_none(), egui::Button::new("Check Maven model"));
        let clicked = check.clicked();
        if let Some(problem) = problem {
            check.on_disabled_hover_text(problem);
        }
        if clicked {
            self.check_maven_model();
        }
        ui.label(
            RichText::new(self.language.maven_model.message())
                .small()
                .color(MUTED),
        );
        if self.maven_dirty_pom() {
            ui.colored_label(AMBER, DIRTY_POM);
        }
    }
    pub(super) fn maven_model_view(&mut self, ui: &mut egui::Ui) {
        let state = &self.language.maven_model;
        ui.label(state.message());
        if state.status == ModelStatus::ServerExited {
            return;
        }
        let Some(model) = &state.model else {
            return;
        };
        ui.label(format!(
            "Root POM: {} · SHA-256 {}",
            model.pom_path,
            &model.pom_sha256[..12]
        ));
        ui.label("This model uses the on-disk POM captured at startup. Open buffers and unsaved edits may differ.");
        ui.label(format!(
            "{} source paths · {} classpath entries · {} unresolved dependencies",
            model.source_paths.len(),
            model.classpath.len(),
            model.unresolved_count
        ));
        let compiler = &model.compiler;
        ui.label(format!(
            "Compiler source: {} · compliance: {} · target: {} · --release: {}",
            compiler.source.as_deref().unwrap_or("unavailable"),
            compiler.compliance.as_deref().unwrap_or("unavailable"),
            compiler.target.as_deref().unwrap_or("unavailable"),
            match compiler.release_enabled {
                Some(true) => "enabled",
                Some(false) => "disabled",
                None => "unavailable",
            }
        ));
        egui::ScrollArea::vertical()
            .id_salt("maven_model")
            .max_height(240.0)
            .show(ui, |ui| {
                for path in &model.source_paths {
                    ui.label(format!("Source: {}", display_path(path)));
                }
                for entry in &model.classpath {
                    let kind = match entry.kind {
                        EntryKind::Source => "Source",
                        EntryKind::Library => "Library",
                        EntryKind::Container => "Container",
                    };
                    let origin = match entry.origin {
                        EntryOrigin::Model => "model",
                        EntryOrigin::Declared => "declared",
                    };
                    ui.label(format!(
                        "{kind} ({origin}, {}): {}",
                        if entry.resolved {
                            "resolved"
                        } else if entry.kind == EntryKind::Source {
                            "folder absent"
                        } else {
                            "unresolved"
                        },
                        display_path(&entry.path)
                    ));
                }
            });
    }
}
fn display_path(path: &str) -> String {
    let mut display: String = path.chars().take(160).collect();
    if path.chars().count() > 160 {
        display.push('…');
    }
    display
}

#[cfg(test)]
#[path = "java_maven_tests.rs"]
mod tests;
