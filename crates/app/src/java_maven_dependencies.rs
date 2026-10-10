//! Explicit dependency evidence for one verified typed Maven startup.
//! Declarations and observed libraries have separate provenance; paths stay inert.
use super::{Action, ActionKind, ServerMode, View};
use crate::{CedarApp, Operation, Payload, MUTED};
use cedar_protocol::{
    MavenDependenciesSnapshot, MavenDependencyObservation, MavenDependencyScope, MavenLibraryRoot,
    JAVA_MAVEN_DEPENDENCIES_CAPABILITY,
};
use eframe::egui::{self, RichText};

const UNKNOWN: &str = "Dependency observation unavailable. JDT library membership is unknown; this does not establish an empty classpath or successful resolution.";
const EVIDENCE: &str = "Captured declarations and JDT library observations are separate evidence. File presence does not establish resolution, artifact integrity, or a dependency tree. No transitive coordinates or scopes are inferred.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DependencyContext {
    generation: u64,
    session: u64,
    startup_id: u64,
    sequence: u64,
    request: u64,
    pom_sha256: String,
}

#[derive(Default, PartialEq, Eq)]
enum Status {
    #[default]
    Unchecked,
    Checking,
    Ready,
    Unavailable,
}
struct Snapshot {
    context: DependencyContext,
    evidence: MavenDependenciesSnapshot,
}
#[derive(Default)]
pub(super) struct DependencyState {
    sequence: u64,
    pending: Option<DependencyContext>,
    snapshot: Option<Snapshot>,
    status: Status,
}
impl DependencyState {
    pub(super) fn reset(&mut self) {
        // Clearing both identities makes even the terminal sequence harmless.
        self.sequence = self.sequence.saturating_add(1);
        self.pending = None;
        self.snapshot = None;
        self.status = Status::Unchecked;
    }
    fn message(&self) -> &'static str {
        match self.status {
            Status::Unchecked => {
                "Dependencies not inspected. Inspect dependencies captures one read-only snapshot."
            }
            Status::Checking => {
                "Inspecting dependencies once; no build, download, save or reimport is requested."
            }
            Status::Ready => {
                "Dependency evidence captured for the on-disk POM used by this startup."
            }
            Status::Unavailable => UNKNOWN,
        }
    }
}
impl CedarApp {
    fn maven_dependencies_session_problem(&self) -> Option<String> {
        if !self.ready()
            || !self.language.running
            || self.language.diagnostics_exited
            || self.language.mode != ServerMode::Java
            || !self.language.maven_model.active()
            || self.language.running_startup_id.is_none()
        {
            return Some("Start the explicit Maven Java profile and wait for Ready first".into());
        }
        if !self.execution_trusted() {
            return Some(
                "Dependency inspection requires trusted tool permission for this connection".into(),
            );
        }
        if !self.backend_java_maven_supported() {
            return Some(self.unsupported_message("the complete typed Maven Java lifecycle"));
        }
        if !self.backend_supports(JAVA_MAVEN_DEPENDENCIES_CAPABILITY) {
            return Some(self.unsupported_message("optional Maven dependency inspection"));
        }
        if self.language.maven_model.restart_required() {
            return Some("Root pom.xml changed on disk. Stop and restart this Maven session before inspecting dependencies.".into());
        }
        if !self.language.capabilities["executeCommandProvider"]["commands"]
            .as_array()
            .is_some_and(|commands| {
                commands
                    .iter()
                    .any(|command| command.as_str() == Some("java.project.getSettings"))
            })
        {
            return Some("The running Java server does not advertise java.project.getSettings; dependency observation is unavailable.".into());
        }
        if self.close_after_language_stop || self.recovery.closing.is_some() {
            return Some("Finish closing the current session first".into());
        }
        None
    }
    pub(crate) fn maven_dependencies_operation_problem(
        &self,
        startup_id: u64,
        pom_sha256: &str,
    ) -> Option<String> {
        self.maven_dependencies_session_problem().or_else(|| {
            (self.language.running_startup_id != Some(startup_id)
                || self.language.maven_model.pom_sha256() != Some(pom_sha256))
            .then(|| {
                "Dependency request no longer matches the running Maven startup and POM".into()
            })
        })
    }
    fn maven_dependency_context_current(&self, context: &DependencyContext) -> bool {
        self.maven_dependencies_operation_problem(context.startup_id, &context.pom_sha256)
            .is_none()
            && context.generation == self.generation
            && context.session == self.language.session
            && context.sequence == self.language.maven_dependencies.sequence
    }
    pub(super) fn invalidate_maven_dependencies(&mut self) {
        let state = &self.language.maven_dependencies;
        if state
            .pending
            .as_ref()
            .is_some_and(|context| !self.maven_dependency_context_current(context))
            || state
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| !self.maven_dependency_context_current(&snapshot.context))
        {
            self.language.maven_dependencies.reset();
        }
    }
    fn inspect_maven_dependencies(&mut self) {
        self.invalidate_maven_dependencies();
        if let Some(problem) = self.maven_dependencies_session_problem() {
            self.error = Some(problem);
            return;
        }
        if self.language_busy() {
            return;
        }
        let Some(sequence) = self.language.maven_dependencies.sequence.checked_add(1) else {
            self.language.maven_dependencies.reset();
            self.language.maven_dependencies.status = Status::Unavailable;
            return;
        };
        if self.next_request == 0 || self.next_request == u64::MAX {
            self.language.maven_dependencies.reset();
            self.language.maven_dependencies.status = Status::Unavailable;
            return;
        }
        let context = DependencyContext {
            generation: self.generation,
            session: self.language.session,
            startup_id: self.language.running_startup_id.unwrap(),
            sequence,
            request: self.next_request,
            pom_sha256: self.language.maven_model.pom_sha256().unwrap().into(),
        };
        let state = &mut self.language.maven_dependencies;
        state.sequence = sequence;
        state.pending = Some(context.clone());
        state.snapshot = None;
        state.status = Status::Checking;
        self.language.view = View::MavenDependencies;
        let id = self.language_request(
            Operation::LanguageMavenDependencies {
                startup_id: context.startup_id,
                pom_sha256: context.pom_sha256.clone(),
            },
            ActionKind::MavenDependencies {
                context: context.clone(),
            },
        );
        if id == 0 || id != context.request {
            self.language.maven_dependencies.reset();
            self.language.maven_dependencies.status = Status::Unavailable;
        }
    }
    pub(crate) fn apply_maven_dependencies_event(
        &mut self,
        request: u64,
        action: Action,
        result: Result<Payload, String>,
        connected: bool,
    ) {
        if !connected {
            self.disconnected("The connection closed while inspecting Maven dependencies. Your unsaved drafts are retained.".into());
            return;
        }
        self.invalidate_maven_dependencies();
        let ActionKind::MavenDependencies { context } = action.kind else {
            return;
        };
        if action.session != context.session
            || request != context.request
            || !self.maven_dependency_context_current(&context)
            || self.language.maven_dependencies.pending.as_ref() != Some(&context)
        {
            return;
        }
        let state = &mut self.language.maven_dependencies;
        state.pending = None;
        match result {
            Ok(Payload::MavenDependencies { snapshot })
                if snapshot
                    .validate_for(context.startup_id, &context.pom_sha256, true)
                    .is_ok() =>
            {
                self.language.cjk_seen |= snapshot.declarations.iter().any(|declaration| {
                    crate::system_fonts::contains_cjk(&declaration.expected_jar_path)
                }) || match &snapshot.observation {
                    MavenDependencyObservation::Available { libraries } => libraries
                        .iter()
                        .any(|library| crate::system_fonts::contains_cjk(&library.relative_path)),
                    MavenDependencyObservation::Unavailable { .. } => false,
                };
                state.status = Status::Ready;
                state.snapshot = Some(Snapshot {
                    context,
                    evidence: snapshot,
                });
            }
            Err(error) if error.starts_with("language_maven_restart_required:") => {
                state.reset();
                self.language.maven_model.require_restart();
            }
            _ => {
                state.snapshot = None;
                state.status = Status::Unavailable;
            }
        }
        self.language.output = state.message().into();
    }
    pub(super) fn maven_dependencies_controls(&mut self, ui: &mut egui::Ui) {
        self.invalidate_maven_dependencies();
        if !self.language.maven_model.active() {
            return;
        }
        let problem = self.maven_dependencies_session_problem();
        let button = ui.add_enabled(
            problem.is_none() && !self.language_busy(),
            egui::Button::new("Inspect dependencies"),
        );
        let clicked = button.clicked();
        if let Some(problem) = problem {
            button.on_disabled_hover_text(problem);
        }
        if clicked {
            self.inspect_maven_dependencies();
        }
    }
    pub(super) fn maven_dependencies_view(&mut self, ui: &mut egui::Ui) {
        self.invalidate_maven_dependencies();
        if let Some(problem) = self.maven_dependencies_session_problem() {
            ui.label(problem);
        }
        let state = &self.language.maven_dependencies;
        ui.label(state.message());
        ui.label(RichText::new(EVIDENCE).small().color(MUTED));
        let Some(snapshot) = &state.snapshot else {
            return;
        };
        let evidence = &snapshot.evidence;
        ui.label(format!(
            "Startup {} · captured pom.xml SHA-256 {}",
            evidence.startup_id, evidence.pom_sha256
        ));
        ui.label("On-disk POM captured at startup. Open buffers and unsaved edits may differ; inspection does not save them.");
        egui::ScrollArea::vertical().id_salt("maven_dependencies").max_height(420.0).show(ui, |ui| {
            ui.strong(format!("Captured POM declarations ({})", evidence.declarations.len()));
            if evidence.declarations.is_empty() {
                ui.label("No direct dependency declarations in the captured POM.");
            }
            for (index, declaration) in evidence.declarations.iter().enumerate() {
                ui.group(|ui| {
                    ui.label(format!("Declaration #{} · {}:{}:{} · classifier: {}", index + 1, declaration.group_id, declaration.artifact_id, declaration.version, declaration.classifier.as_deref().unwrap_or("none")));
                    ui.label(format!("Scope: {} ({}) · optional: {} ({})", scope_name(&declaration.scope), provenance(declaration.scope_explicit), declaration.optional, provenance(declaration.optional_explicit)));
                    ui.label(format!("Expected JAR · local repository relative: {}", declaration.expected_jar_path));
                    ui.label(format!("Expected regular file: {}", presence(declaration.regular_file_present)));
                });
            }
            ui.separator();
            ui.strong("Observed JDT library rows");
            match &evidence.observation {
                MavenDependencyObservation::Unavailable { .. } => { ui.label(UNKNOWN); }
                MavenDependencyObservation::Available { libraries } => {
                    ui.label(format!("{} library rows observed in this snapshot", libraries.len()));
                    for library in libraries {
                        ui.group(|ui| {
                            ui.label(format!("Observed library · {} relative: {}", match library.root { MavenLibraryRoot::Workspace => "workspace", MavenLibraryRoot::LocalRepository => "local repository" }, library.relative_path));
                            ui.label(format!("Observed regular file: {}", presence(library.regular_file_present)));
                            if library.declaration_indices.is_empty() {
                                ui.label("No declaration path match; coordinates and scope are unknown.");
                            } else {
                                let indices = library.declaration_indices.iter().map(|index| format!("#{}", usize::from(*index) + 1)).collect::<Vec<_>>().join(", ");
                                ui.label(format!("Declaration path matches: {indices}{}", if library.declaration_indices.len() > 1 { " (ambiguous; no unique dependency identity)" } else { " (path evidence only)" }));
                            }
                        });
                    }
                }
            }
        });
    }
}
fn scope_name(scope: &MavenDependencyScope) -> &'static str {
    match scope {
        MavenDependencyScope::Compile => "compile",
        MavenDependencyScope::Provided => "provided",
        MavenDependencyScope::Runtime => "runtime",
        MavenDependencyScope::Test => "test",
    }
}
fn provenance(explicit: bool) -> &'static str {
    if explicit {
        "explicit"
    } else {
        "default"
    }
}
fn presence(present: bool) -> &'static str {
    if present {
        "present"
    } else {
        "absent"
    }
}

#[cfg(test)]
#[path = "java_maven_dependency_tests.rs"]
mod tests;
#[cfg(all(test, target_os = "linux"))]
pub(crate) use tests::verify_native_linux_maven_dependencies;
#[cfg(all(test, windows))]
pub(crate) use tests::verify_native_maven_dependencies;
