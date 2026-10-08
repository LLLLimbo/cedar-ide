//! Connection-scoped, unverified agent support claims. Trust stays user-owned.
use crate::{CedarApp, Operation};
use cedar_protocol::{
    supports_capability, JAVA_LANGUAGE_SESSION_CAPABILITIES, JAVA_STARTUP_CAPABILITIES,
    LANGUAGE_SESSION_CAPABILITIES, RUN_TASK_CAPABILITIES,
};

impl CedarApp {
    pub(super) fn backend_supports(&self, name: &str) -> bool {
        self.ready() && supports_capability(self.agent_info.as_ref(), name)
    }
    pub(super) fn backend_run_supported(&self) -> bool {
        RUN_TASK_CAPABILITIES
            .iter()
            .all(|name| self.backend_supports(name))
    }
    pub(super) fn backend_language_supported(&self) -> bool {
        self.backend_generic_language_supported() || self.backend_java_language_supported()
    }
    pub(super) fn backend_java_language_supported(&self) -> bool {
        JAVA_LANGUAGE_SESSION_CAPABILITIES
            .iter()
            .all(|name| self.backend_supports(name))
    }
    pub(super) fn backend_java_startup_supported(&self) -> bool {
        self.backend_java_language_supported()
            && JAVA_STARTUP_CAPABILITIES
                .iter()
                .all(|name| self.backend_supports(name))
    }
    pub(super) fn backend_generic_language_supported(&self) -> bool {
        LANGUAGE_SESSION_CAPABILITIES
            .iter()
            .all(|name| self.backend_supports(name))
    }
    pub(super) fn execution_trusted(&self) -> bool {
        self.active_form.as_ref().is_some_and(|form| form.allow_run)
    }
    pub(super) fn unsupported_message(&self, name: &str) -> String {
        if self.ready() && self.agent_info.is_none() {
            format!("This legacy agent supports basic file editing only. Upgrade the workspace agent to enable {name}")
        } else {
            format!("The workspace agent does not advertise {name}. Drafts remain editable; upgrade or use an agent with this capability")
        }
    }
    pub(super) fn operation_problem(&self, operation: &Operation) -> Option<String> {
        if let Operation::LanguageRefreshJavaDiagnostics { path, version } = operation {
            if let Some(problem) = self.java_diagnostics_operation_problem(path, *version) {
                return Some(problem);
            }
        }
        if matches!(
            operation,
            Operation::GitChanges { .. } | Operation::GitDiff { .. }
        ) && !self.typed_git_supported()
        {
            return Some(self.unsupported_message("git_changes/git_diff"));
        }
        if matches!(operation, Operation::RunStart { .. }) && !self.backend_run_supported() {
            return Some(self.unsupported_message("run_start/run_poll/run_cancel"));
        }
        if matches!(operation, Operation::LanguageStart { .. })
            && !self.backend_generic_language_supported()
        {
            return Some(self.unsupported_message("the complete language session lifecycle"));
        }
        if matches!(operation, Operation::LanguageStartJava { .. })
            && !self.backend_java_language_supported()
        {
            return Some(self.unsupported_message("the complete Java language session lifecycle"));
        }
        if matches!(
            operation,
            Operation::LanguageStartJavaBegin { .. }
                | Operation::LanguageStartJavaPoll { .. }
                | Operation::LanguageStartJavaCancel { .. }
        ) && !self.backend_java_startup_supported()
        {
            return Some(
                self.unsupported_message("the complete cancellable Java startup lifecycle"),
            );
        }
        if let Some(name) = operation.capability_name() {
            if !self.backend_supports(name) {
                return Some(self.unsupported_message(name));
            }
        }
        // Stop/cancel/poll remain reachable for existing sessions. A support
        // claim never enables trust, and changing a draft form never grants it.
        if matches!(
            operation,
            Operation::RunStart { .. }
                | Operation::Run { .. }
                | Operation::LanguageStart { .. }
                | Operation::LanguageStartJava { .. }
                | Operation::LanguageStartJavaBegin { .. }
                | Operation::GitStatus
                | Operation::GitChanges { .. }
                | Operation::GitDiff { .. }
        ) && !self.execution_trusted()
        {
            return Some("Command execution is disabled for this connection. Enable trust and reconnect only for a workspace you trust".into());
        }
        None
    }
    pub(super) fn agent_status(&self) -> String {
        match &self.agent_info {
            Some(agent) => format!(
                "Reported agent {} · {}/{}{}",
                agent.version,
                agent.os,
                agent.arch,
                if self.backend_supports("write") {
                    ""
                } else {
                    " · saves unavailable"
                }
            ),
            None => "Legacy agent · basic editing".into(),
        }
    }
    pub(super) fn agent_details(&self) -> String {
        match &self.agent_info {
            Some(_) => format!("Agent-reported information, not verified identity or permission.\nSave: {} · Search: {} · Tasks: {} · Language: {}\nExecution trust: {}",
                yes_no(self.backend_supports("write")), yes_no(self.backend_supports("search")),
                yes_no(self.backend_run_supported()), yes_no(self.backend_language_supported()),
                if self.execution_trusted() { "enabled by you" } else { "off" }),
            None => "This protocol-4 agent did not advertise capabilities. Files can be listed, opened, edited, saved, and searched. Upgrade the workspace agent for Git, commands, and language servers. Execution trust is separate.".into(),
        }
    }
}
fn yes_no(value: bool) -> &'static str {
    if value {
        "supported"
    } else {
        "unavailable"
    }
}

#[cfg(test)]
pub(super) fn full_test_agent() -> cedar_protocol::AgentInfo {
    cedar_protocol::AgentInfo {
        schema: cedar_protocol::AGENT_INFO_SCHEMA,
        version: "test-agent".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        capabilities: [
            "list",
            "read",
            "write",
            "search",
            "git_status",
            "run_start",
            "run_poll",
            "run_cancel",
            "language_start",
            "language_open",
            "language_change",
            "language_close",
            "language_query",
            "language_resolve_uri",
            "language_events",
            "language_stop",
            "language_format",
            "language_references",
            "language_document_symbols",
            "language_resolve_completion",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::Document,
        worker::{Command, Event, Worker},
        ConnectForm, ConnectionState, Job, Payload,
    };
    use std::sync::mpsc::Receiver;

    fn connecting(
        trust: bool,
        agent: Option<cedar_protocol::AgentInfo>,
    ) -> (CedarApp, Receiver<Command>) {
        let mut app = CedarApp::empty();
        let form = ConnectForm {
            local_root: "/project".into(),
            allow_run: trust,
            ..Default::default()
        };
        let (worker, rx) = Worker::recording();
        app.worker = Some(worker);
        app.state = ConnectionState::Connecting;
        app.connecting_form = Some(form);
        hello(&mut app, 0, "/project", agent);
        (app, rx)
    }
    fn hello(
        app: &mut CedarApp,
        generation: u64,
        root: &str,
        agent: Option<cedar_protocol::AgentInfo>,
    ) {
        app.apply_event(Event {
            generation,
            id: 0,
            connected: true,
            result: Ok(Payload::Hello {
                protocol: cedar_protocol::PROTOCOL_VERSION,
                root: root.into(),
                agent,
            }),
        });
    }
    #[test]
    fn agent_claims_do_not_change_trust_and_platform_does_not_select_support() {
        for os in ["linux", "windows", "unknown"] {
            for trusted in [false, true] {
                let mut info = full_test_agent();
                info.os = os.into();
                let (mut app, rx) = connecting(trusted, Some(info.clone()));
                rx.try_recv().unwrap();
                assert_eq!(app.agent_info.as_ref(), Some(&info));
                assert_eq!(app.execution_trusted(), trusted);
                assert!(app.backend_run_supported());
                for ssh in [false, true] {
                    app.active_form.as_mut().unwrap().ssh = ssh;
                    assert!(app.backend_run_supported());
                    assert_eq!(
                        app.operation_problem(&Operation::RunStart {
                            program: "tool".into(),
                            args: vec![],
                            timeout_secs: 1
                        })
                        .is_none(),
                        trusted
                    );
                }
            }
        }
    }
    #[test]
    fn legacy_allows_basic_editing_but_never_execution_even_when_trusted() {
        let (mut app, rx) = connecting(true, None);
        rx.try_recv().unwrap();
        for name in ["list", "read", "write", "search"] {
            assert!(app.backend_supports(name));
        }
        for name in [
            "git_status",
            "run_start",
            "run_poll",
            "run_cancel",
            "language_start",
        ] {
            assert!(!app.backend_supports(name));
        }
        assert!(app.unsupported_message("run_start").contains("Upgrade"));
        app.request(Operation::GitStatus, Job::Git);
        assert!(rx.try_recv().is_err());
        app.documents.push(Document::new(
            1,
            "draft.rs".into(),
            "before".into(),
            "rev".into(),
        ));
        app.documents[0].text = "editable legacy draft".into();
        app.save_document(1);
        assert!(matches!(rx.try_recv().unwrap().op, Operation::Write { .. }));
    }
    #[test]
    fn explicit_read_only_and_missing_search_gate_dispatch_without_mutating_drafts() {
        let mut info = full_test_agent();
        info.capabilities
            .retain(|name| name != "write" && name != "search");
        let (mut app, rx) = connecting(true, Some(info));
        rx.try_recv().unwrap();
        app.documents.push(Document::new(
            1,
            "draft.rs".into(),
            "before".into(),
            "rev".into(),
        ));
        app.documents[0].text = "editable read-only draft".into();
        app.active_document = Some(1);
        let next = app.next_request;
        app.save();
        app.save_document(1);
        app.search_query = "draft".into();
        app.search();
        assert!(rx.try_recv().is_err());
        assert_eq!(app.next_request, next);
        assert_eq!(app.documents[0].text, "editable read-only draft");
        assert_eq!(app.documents[0].saved_text, "before");
        assert_eq!(app.documents[0].revision.as_deref(), Some("rev"));
        assert!(!app.documents[0].saving);
        assert!(app.backend_supports("read"));
    }
    #[test]
    fn search_and_write_support_are_independent() {
        for missing in ["write", "search"] {
            let mut info = full_test_agent();
            info.capabilities.retain(|name| name != missing);
            let (mut app, rx) = connecting(false, Some(info));
            rx.try_recv().unwrap();
            app.documents.push(Document::new(
                1,
                "main.rs".into(),
                "disk".into(),
                "r".into(),
            ));
            app.documents[0].text = "draft".into();
            app.search_query = "draft".into();
            if missing == "write" {
                assert!(!app.backend_supports("write"));
                app.search();
                assert!(matches!(
                    rx.try_recv().unwrap().op,
                    Operation::Search { .. }
                ));
            } else {
                assert!(!app.backend_supports("search"));
                app.save_document(1);
                assert!(matches!(rx.try_recv().unwrap().op, Operation::Write { .. }));
            }
        }
    }
    #[test]
    fn root_guard_rejects_new_metadata_and_preserves_existing_trust_and_draft() {
        let (mut app, rx) = connecting(false, Some(full_test_agent()));
        rx.try_recv().unwrap();
        app.documents.push(Document::new(
            1,
            "main.rs".into(),
            "disk".into(),
            "r".into(),
        ));
        app.documents[0].text = "draft".into();
        let mut form = app.active_form.clone().unwrap();
        form.allow_run = true;
        app.connecting_form = Some(form);
        app.state = ConnectionState::Connecting;
        let mut info = full_test_agent();
        info.version = "replacement-agent".into();
        hello(&mut app, 0, "/different/root", Some(info));
        assert!(app.state == ConnectionState::Disconnected);
        assert!(app.agent_info.is_none());
        assert_eq!(app.root, "/project");
        assert!(!app.execution_trusted());
        assert_eq!(app.documents[0].text, "draft");
        assert_eq!(app.documents[0].revision.as_deref(), Some("r"));
    }
    #[test]
    fn wrong_protocol_never_installs_metadata_or_trust() {
        let (mut app, rx) = connecting(false, Some(full_test_agent()));
        rx.try_recv().unwrap();
        app.connecting_form = Some(ConnectForm {
            allow_run: true,
            ..Default::default()
        });
        app.state = ConnectionState::Connecting;
        app.apply_event(Event {
            generation: 0,
            id: 0,
            connected: true,
            result: Ok(Payload::Hello {
                protocol: cedar_protocol::PROTOCOL_VERSION + 1,
                root: "/unaccepted".into(),
                agent: Some(full_test_agent()),
            }),
        });
        assert!(app.state == ConnectionState::Disconnected);
        assert!(app.agent_info.is_none());
        assert_eq!(app.root, "/project");
        assert!(!app.execution_trusted());
    }
    #[test]
    fn run_start_requires_complete_lifecycle_at_central_dispatch() {
        for missing in RUN_TASK_CAPABILITIES {
            let mut info = full_test_agent();
            info.capabilities.retain(|name| name != missing);
            let (mut app, rx) = connecting(true, Some(info));
            rx.try_recv().unwrap();
            let id = app.request(
                Operation::RunStart {
                    program: "tool".into(),
                    args: vec![],
                    timeout_secs: 1,
                },
                Job::Git,
            );
            assert_eq!(id, 0);
            assert!(rx.try_recv().is_err());
        }
    }
    #[test]
    fn all_handshake_duplicates_and_stale_errors_preserve_accepted_session() {
        let info = full_test_agent();
        let (mut app, rx) = connecting(false, Some(info.clone()));
        rx.try_recv().unwrap();
        app.documents.push(Document::new(
            1,
            "main.rs".into(),
            "draft".into(),
            "rev".into(),
        ));
        for generation in [0, 99] {
            hello(&mut app, generation, "/attacker", None);
            app.apply_event(Event {
                generation,
                id: 0,
                connected: false,
                result: Err("late connect failure".into()),
            });
            app.apply_event(Event {
                generation,
                id: 0,
                connected: true,
                result: Ok(Payload::Entries { entries: vec![] }),
            });
            assert!(app.ready());
            assert_eq!(app.root, "/project");
            assert_eq!(app.agent_info.as_ref(), Some(&info));
            assert!(!app.execution_trusted());
            assert_eq!(app.documents[0].text, "draft");
            assert!(app.error.is_none());
            assert!(rx.try_recv().is_err());
        }
    }
    #[test]
    fn failed_cancelled_and_disconnected_attempts_leave_no_usable_metadata() {
        let (mut app, _rx) = connecting(true, Some(full_test_agent()));
        app.disconnected("transport lost".into());
        assert!(app.agent_info.is_none());
        assert!(!app.backend_supports("read"));
        app.connecting_form = app.active_form.clone();
        app.state = ConnectionState::Connecting;
        app.apply_event(Event {
            generation: 0,
            id: 0,
            connected: false,
            result: Err("connect failed".into()),
        });
        assert!(app.agent_info.is_none());
        assert!(app.connecting_form.is_none());
        app.connecting_form = app.active_form.clone();
        app.state = ConnectionState::Connecting;
        app.cancel_connection();
        assert!(app.agent_info.is_none());
        assert!(app.connecting_form.is_none());
        hello(&mut app, 0, "/late", Some(full_test_agent()));
        assert!(!app.ready());
        assert_eq!(app.root, "/project");
    }
    #[test]
    fn reconnect_clears_old_support_before_waiting_for_new_handshake() {
        let (mut app, _rx) = connecting(false, Some(full_test_agent()));
        let temp = tempfile::tempdir().unwrap();
        app.connect(
            &eframe::egui::Context::default(),
            ConnectForm {
                local_root: temp.path().to_string_lossy().into_owned(),
                ..Default::default()
            },
        );
        assert!(app.state == ConnectionState::Connecting);
        assert!(app.agent_info.is_none());
        assert!(!app.backend_supports("read"));
        app.cancel_connection();
        assert!(app.agent_info.is_none());
    }
    #[test]
    fn malformed_metadata_or_missing_file_prerequisite_rejects_connection() {
        for missing in ["list", "read"] {
            let mut info = full_test_agent();
            info.capabilities.retain(|name| name != missing);
            let (app, rx) = connecting(true, Some(info));
            assert!(!app.ready());
            assert!(app.agent_info.is_none());
            assert!(app.active_form.is_none());
            assert!(rx.try_recv().is_err());
        }
        let mut malformed = full_test_agent();
        malformed.os = "bad\nplatform".into();
        let (app, _) = connecting(true, Some(malformed));
        assert!(!app.ready());
        assert!(app.agent_info.is_none());
        assert!(app.root.is_empty());
    }
    #[test]
    fn active_cleanup_does_not_require_execution_trust() {
        let (mut app, rx) = connecting(true, Some(full_test_agent()));
        rx.try_recv().unwrap();
        app.active_form.as_mut().unwrap().allow_run = false;
        for operation in [
            Operation::RunCancel { task_id: 3 },
            Operation::RunPoll { task_id: 3 },
            Operation::LanguageStop,
        ] {
            assert!(app.operation_problem(&operation).is_none());
            assert_ne!(app.request(operation, Job::Git), 0);
            rx.try_recv().unwrap();
        }
    }
    #[test]
    fn unknown_capability_cannot_enable_known_execution_or_language_operations() {
        let mut info = full_test_agent();
        info.capabilities = vec![
            "list".into(),
            "read".into(),
            "future_execute_everything".into(),
        ];
        let (app, _) = connecting(true, Some(info));
        assert!(!app.backend_run_supported());
        assert!(!app.backend_language_supported());
        assert!(!app.backend_supports("write"));
    }
}
