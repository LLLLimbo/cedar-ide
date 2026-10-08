//! Backend implementation claims; neither tool discovery nor execution trust.
use cedar_protocol::{AgentInfo, AGENT_INFO_SCHEMA};
use cedar_tasks::BackendMode;

pub(super) fn agent_info(backend_mode: BackendMode) -> AgentInfo {
    let mut capabilities = vec!["list", "read", "write", "search"];
    // Git status and legacy synchronous Run still use run_bounded. Their
    // support is independent of isolated asynchronous task ownership.
    if cfg!(any(target_os = "linux", target_os = "macos")) {
        capabilities.extend(["git_status", "run"]);
    }
    if super::git_read::platform_supported(backend_mode) {
        capabilities.extend(["git_changes", "git_diff"]);
    }
    if backend_mode.supports_tasks() {
        capabilities.extend(["run_start", "run_poll", "run_cancel"]);
    }
    // Language startup has its own platform policy. Do not infer it from task
    // support or advertise executable availability (which Hello never probes).
    let generic_language = super::language::platform_supported();
    let java_language = super::language::java_platform_supported(backend_mode);
    if generic_language {
        capabilities.push("language_start");
    }
    if java_language {
        capabilities.push("language_start_java");
        capabilities.extend_from_slice(cedar_protocol::JAVA_STARTUP_CAPABILITIES);
        // A bridge implementation claim, never JDT/server-version support or
        // permission to execute it. Startup reports guarded session support.
        capabilities.push("java_diagnostics_refresh");
    }
    if generic_language || java_language {
        capabilities.extend([
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
            Operation::GitChanges {
                git_executable: "must-not-be-inspected".into(),
            },
            Operation::GitDiff {
                git_executable: "must-not-be-inspected".into(),
                path: "file".into(),
                kind: cedar_protocol::GitDiffKind::Staged,
            },
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
            Operation::LanguageStartJava {
                java_executable: "must-not-be-inspected".into(),
                distribution: "must-not-be-inspected".into(),
                data_directory: "must-not-be-inspected".into(),
            },
            Operation::LanguageStartJavaBegin {
                java_executable: "must-not-be-inspected".into(),
                distribution: "must-not-be-inspected".into(),
                data_directory: "must-not-be-inspected".into(),
            },
            Operation::LanguageStartJavaPoll { startup_id: 1 },
            Operation::LanguageStartJavaCancel { startup_id: 1 },
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
            Operation::LanguageRefreshJavaDiagnostics {
                path: "a.java".into(),
                version: 1,
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
        for backend_mode in [BackendMode::InProcess, BackendMode::IsolatedAgent] {
            let agent = agent_info(backend_mode);
            agent.validate().unwrap();
            assert_eq!(agent.version, env!("CARGO_PKG_VERSION"));
            assert_eq!(agent.os, std::env::consts::OS);
            assert_eq!(agent.arch, std::env::consts::ARCH);
            for capability in ["list", "read", "write", "search"] {
                assert!(agent.supports(capability));
            }
            let command_platform = cfg!(any(target_os = "linux", target_os = "macos"));
            for capability in ["git_status", "run"] {
                assert_eq!(agent.supports(capability), command_platform, "{capability}");
            }
            let task_platform =
                command_platform || (cfg!(windows) && backend_mode == BackendMode::IsolatedAgent);
            for capability in RUN_TASK_CAPABILITIES {
                assert_eq!(agent.supports(capability), task_platform, "{capability}");
            }
            // Generic start remains unsupported on Windows. Only an isolated
            // Windows agent advertises the narrow Java route and its lifecycle.
            let generic_language = !cfg!(windows);
            let java_language = cfg!(windows) && backend_mode == BackendMode::IsolatedAgent;
            let language_operation_supported = |name: &str| match name {
                "language_start" => generic_language,
                "language_start_java"
                | "java_diagnostics_refresh"
                | "language_start_java_begin"
                | "language_start_java_poll"
                | "language_start_java_cancel" => java_language,
                _ => generic_language || java_language,
            };
            for capability in LANGUAGE_SESSION_CAPABILITIES.iter().chain(&[
                "language_start_java",
                "language_start_java_begin",
                "language_start_java_poll",
                "language_start_java_cancel",
                "java_diagnostics_refresh",
                "language_query",
                "language_resolve_uri",
                "language_format",
                "language_references",
                "language_document_symbols",
                "language_resolve_completion",
            ]) {
                assert_eq!(
                    agent.supports(capability),
                    language_operation_supported(capability),
                    "{capability}"
                );
            }
            let mut expected = vec!["list", "read", "write", "search"];
            for operation in execution_operations() {
                let name = operation.capability_name().unwrap();
                let supported =
                    if name.starts_with("language_") || name == "java_diagnostics_refresh" {
                        language_operation_supported(name)
                    } else if name == "git_changes" || name == "git_diff" {
                        super::super::git_read::platform_supported(backend_mode)
                    } else if RUN_TASK_CAPABILITIES.contains(&name) {
                        task_platform
                    } else {
                        command_platform
                    };
                if supported {
                    expected.push(name);
                }
            }
            expected.sort_unstable();
            assert_eq!(agent.capabilities, expected);
            assert!(!agent.supports("terminal"));
        }
    }

    #[test]
    fn default_workspace_is_in_process_and_hello_has_no_execution_side_effects() {
        let root = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::open(root.path()).unwrap();
        assert_eq!(workspace.backend_mode, BackendMode::InProcess);
        assert_eq!(hello(&mut workspace), agent_info(BackendMode::InProcess));
        assert!(!workspace.allow_run);
        assert!(workspace.tasks.is_none());
        assert!(workspace.language.is_none());
    }

    #[test]
    fn hello_never_grants_trust_or_starts_a_tool_and_trust_does_not_change_support() {
        let root = tempfile::tempdir().unwrap();
        for backend_mode in [BackendMode::InProcess, BackendMode::IsolatedAgent] {
            let mut workspace = Workspace::with_backend_mode(root.path(), backend_mode).unwrap();
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
            assert_eq!(workspace.backend_mode, backend_mode);
            assert!(workspace.tasks.is_none());
            assert!(workspace.language.is_none());
            workspace.set_allow_run(false);
            assert_eq!(hello(&mut workspace), untrusted);
            assert_eq!(workspace.backend_mode, backend_mode);
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    #[test]
    fn unavailable_legacy_commands_reject_direct_requests_in_both_host_modes() {
        let root = tempfile::tempdir().unwrap();
        for backend_mode in [BackendMode::InProcess, BackendMode::IsolatedAgent] {
            let mut workspace = Workspace::with_backend_mode(root.path(), backend_mode).unwrap();
            workspace.set_allow_run(true);
            for operation in [
                Operation::GitStatus,
                Operation::Run {
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
    }

    #[cfg(windows)]
    #[test]
    fn unavailable_language_start_rejects_direct_requests_in_both_host_modes() {
        let root = tempfile::tempdir().unwrap();
        for backend_mode in [BackendMode::InProcess, BackendMode::IsolatedAgent] {
            let mut workspace = Workspace::with_backend_mode(root.path(), backend_mode).unwrap();
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
}
