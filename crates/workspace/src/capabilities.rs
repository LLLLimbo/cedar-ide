//! Backend implementation claims; neither tool discovery nor execution trust.
use cedar_protocol::{AgentInfo, AGENT_INFO_SCHEMA};

pub(super) fn agent_info() -> AgentInfo {
    let mut capabilities = vec!["list", "read", "write", "search"];
    // TaskManager and run_bounded have narrower support than a generic Unix
    // target. Git status uses run_bounded and has the same platform restriction.
    if cfg!(any(target_os = "linux", target_os = "macos")) {
        capabilities.extend(["git_status", "run", "run_start", "run_poll", "run_cancel"]);
    }
    // Language startup has its own platform policy. Do not infer it from task
    // support or advertise executable availability (which Hello never probes).
    if super::language::platform_supported() {
        capabilities.extend([
            "language_start",
            "language_open",
            "language_change",
            "language_close",
            "language_query",
            "language_format",
            "language_references",
            "language_document_symbols",
            "language_resolve_uri",
            "language_resolve_completion",
            "language_events",
            "language_stop",
        ]);
    }
    capabilities.sort_unstable();
    AgentInfo {
        schema: AGENT_INFO_SCHEMA,
        version: env!("CARGO_PKG_VERSION").into(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        capabilities: capabilities.into_iter().map(str::to_owned).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workspace;
    use cedar_protocol::{
        LanguageQueryKind, Operation, Payload, LANGUAGE_SESSION_CAPABILITIES, PROTOCOL_VERSION,
        RUN_TASK_CAPABILITIES,
    };

    fn hello(workspace: &mut Workspace) -> AgentInfo {
        let Payload::Hello {
            protocol,
            root,
            agent,
        } = workspace.handle(Operation::Hello).unwrap()
        else {
            panic!("expected hello");
        };
        assert_eq!(protocol, PROTOCOL_VERSION);
        assert_eq!(root, workspace.root.to_string_lossy());
        let agent = agent.expect("current workspace reports its implementation");
        agent.validate().unwrap();
        agent
    }

    fn execution_operations() -> Vec<Operation> {
        vec![
            Operation::GitStatus,
            Operation::Run {
                program: "nonexistent-cedar-test-tool".into(),
                args: vec![],
                timeout_secs: 1,
            },
            Operation::RunStart {
                program: "nonexistent-cedar-test-tool".into(),
                args: vec![],
                timeout_secs: 1,
            },
            Operation::RunPoll { task_id: 1 },
            Operation::RunCancel { task_id: 1 },
            Operation::LanguageStart {
                program: "nonexistent-cedar-test-server".into(),
                args: vec![],
            },
            Operation::LanguageOpen {
                path: "a.rs".into(),
                language_id: "rust".into(),
                version: 1,
                text: String::new(),
            },
            Operation::LanguageChange {
                path: "a.rs".into(),
                version: 1,
                text: String::new(),
            },
            Operation::LanguageClose {
                path: "a.rs".into(),
            },
            Operation::LanguageQuery {
                path: "a.rs".into(),
                line: 0,
                character: 0,
                kind: LanguageQueryKind::Hover,
            },
            Operation::LanguageFormat {
                path: "a.rs".into(),
                version: 1,
                tab_size: 4,
                insert_spaces: true,
            },
            Operation::LanguageReferences {
                path: "a.rs".into(),
                line: 0,
                character: 0,
                include_declaration: true,
            },
            Operation::LanguageDocumentSymbols {
                path: "a.rs".into(),
            },
            Operation::LanguageResolveUri {
                uri: "file:///a.rs".into(),
            },
            Operation::LanguageResolveCompletion {
                item: serde_json::json!({}),
            },
            Operation::LanguageEvents,
            Operation::LanguageStop,
        ]
    }

    #[test]
    fn hello_reports_actual_build_platform_and_implemented_operations() {
        let agent = agent_info();
        agent.validate().unwrap();
        assert_eq!(agent.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(agent.os, std::env::consts::OS);
        assert_eq!(agent.arch, std::env::consts::ARCH);
        for capability in ["list", "read", "write", "search"] {
            assert!(agent.supports(capability));
        }
        let command_platform = cfg!(any(target_os = "linux", target_os = "macos"));
        for capability in ["git_status", "run"].iter().chain(RUN_TASK_CAPABILITIES) {
            assert_eq!(agent.supports(capability), command_platform, "{capability}");
        }
        // Language's existing stdio implementation has its own Windows guard;
        // it is not assumed to have the command-task implementation's targets.
        let language_platform = !cfg!(windows);
        for capability in LANGUAGE_SESSION_CAPABILITIES.iter().chain(&[
            "language_query",
            "language_resolve_uri",
            "language_format",
            "language_references",
            "language_document_symbols",
            "language_resolve_completion",
        ]) {
            assert_eq!(
                agent.supports(capability),
                language_platform,
                "{capability}"
            );
        }
        let mut expected = vec!["list", "read", "write", "search"];
        for operation in execution_operations() {
            let name = operation.capability_name().unwrap();
            if (name.starts_with("language_") && language_platform)
                || (!name.starts_with("language_") && command_platform)
            {
                expected.push(name);
            }
        }
        expected.sort_unstable();
        assert_eq!(agent.capabilities, expected);
        assert!(!agent.supports("terminal"));
    }

    #[test]
    fn hello_never_grants_trust_or_starts_a_tool_and_trust_does_not_change_support() {
        let root = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::open(root.path()).unwrap();
        let untrusted = hello(&mut workspace);
        assert!(!workspace.allow_run);
        assert!(workspace.tasks.is_none());
        assert!(workspace.language.is_none());
        for operation in execution_operations() {
            let name = operation.capability_name().unwrap();
            assert_eq!(
                workspace.handle(operation).unwrap_err().code,
                "run_disabled",
                "{name}"
            );
            assert!(workspace.tasks.is_none());
            assert!(workspace.language.is_none());
        }
        workspace.set_allow_run(true);
        assert_eq!(hello(&mut workspace), untrusted);
        assert!(workspace.tasks.is_none());
        assert!(workspace.language.is_none());
        workspace.set_allow_run(false);
        assert_eq!(hello(&mut workspace), untrusted);
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    #[test]
    fn unavailable_command_implementations_still_reject_direct_requests() {
        let root = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::open(root.path()).unwrap();
        workspace.set_allow_run(true);
        for operation in [
            Operation::GitStatus,
            Operation::Run {
                program: "nonexistent-cedar-test-tool".into(),
                args: vec![],
                timeout_secs: 1,
            },
            Operation::RunStart {
                program: "nonexistent-cedar-test-tool".into(),
                args: vec![],
                timeout_secs: 1,
            },
        ] {
            assert_eq!(
                workspace.handle(operation).unwrap_err().code,
                "unsupported_platform"
            );
            assert!(workspace.tasks.is_none());
        }
    }

    #[cfg(windows)]
    #[test]
    fn unavailable_language_start_still_rejects_direct_requests() {
        let root = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::open(root.path()).unwrap();
        workspace.set_allow_run(true);
        assert_eq!(
            workspace
                .handle(Operation::LanguageStart {
                    program: "nonexistent-cedar-test-server".into(),
                    args: vec![],
                })
                .unwrap_err()
                .code,
            "unsupported_platform"
        );
        assert!(workspace.language.is_none());
    }
}
