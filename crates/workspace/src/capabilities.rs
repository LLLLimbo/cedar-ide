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
        capabilities.push("language_organize_java_imports");
        capabilities.push("language_java_implementations");
    }
    // Windows retains its direct claims. Linux uses the exact versioned groups
    // so its existing flat inventory remains within the unchanged wire bound.
    if cfg!(windows) && backend_mode == BackendMode::IsolatedAgent {
        capabilities.extend_from_slice(cedar_protocol::JAVA_MAVEN_CAPABILITIES);
        capabilities.push(cedar_protocol::JAVA_MAVEN_DEPENDENCIES_CAPABILITY);
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
            "language_workspace_symbols",
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
        capability_groups: if cfg!(target_os = "linux")
            && backend_mode == BackendMode::IsolatedAgent
        {
            vec![
                cedar_protocol::JAVA_MAVEN_DEPENDENCIES_GROUP.into(),
                cedar_protocol::JAVA_MAVEN_LEAF_GROUP.into(),
            ]
        } else {
            Vec::new()
        },
    }
}

pub(super) fn remove_maven_claims(info: &mut AgentInfo) {
    info.capabilities.retain(|name| {
        !cedar_protocol::JAVA_MAVEN_CAPABILITIES.contains(&name.as_str())
            && name != cedar_protocol::JAVA_MAVEN_DEPENDENCIES_CAPABILITY
    });
    info.capability_groups.retain(|name| {
        name != cedar_protocol::JAVA_MAVEN_LEAF_GROUP
            && name != cedar_protocol::JAVA_MAVEN_DEPENDENCIES_GROUP
    });
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
            Operation::LanguageStartJavaMavenBegin {
                java_executable: "must-not-be-inspected".into(),
                distribution: "must-not-be-inspected".into(),
                data_directory: "must-not-be-inspected".into(),
                local_repository: "must-not-be-inspected".into(),
            },
            Operation::LanguageMavenModel,
            Operation::LanguageMavenDependencies {
                startup_id: 1,
                pom_sha256: "must-not-be-read".into(),
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
            Operation::LanguageOrganizeJavaImports {
                path: "a.java".into(),
                version: 1,
            },
            Operation::LanguageRefreshJavaDiagnostics {
                path: "a.java".into(),
                version: 1,
            },
            Operation::LanguageJavaImplementations {
                path: "a.java".into(),
                version: 1,
                line: 0,
                character: 0,
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
            Operation::LanguageWorkspaceSymbols {
                query: "Type".into(),
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

    // Independent test inventory: every platform can be checked on every host.
    // Keep this derived from the explicit operation list, not agent_info itself.
    fn expected_platform_capabilities(os: &str, backend_mode: BackendMode) -> Vec<&'static str> {
        let windows = os == "windows";
        let unix_commands = matches!(os, "linux" | "macos");
        let isolated_windows = windows && backend_mode == BackendMode::IsolatedAgent;
        let typed_java =
            matches!(os, "linux" | "windows") && backend_mode == BackendMode::IsolatedAgent;
        let tasks = unix_commands || isolated_windows;
        let generic_language = !windows;
        let mut expected = vec!["list", "read", "write", "search"];
        for operation in execution_operations() {
            let name = operation.capability_name().unwrap();
            let supported = match name {
                "language_start" => generic_language,
                "language_start_java"
                | "java_diagnostics_refresh"
                | "language_organize_java_imports"
                | "language_java_implementations"
                | "language_start_java_begin"
                | "language_start_java_poll"
                | "language_start_java_cancel" => typed_java,
                "language_start_java_maven_begin"
                | "language_maven_model"
                | "language_maven_dependencies" => isolated_windows,
                "git_changes" | "git_diff" => unix_commands || isolated_windows,
                name if RUN_TASK_CAPABILITIES.contains(&name) => tasks,
                name if name.starts_with("language_") => generic_language || typed_java,
                _ => unix_commands,
            };
            if supported {
                expected.push(name);
            }
        }
        expected.sort_unstable();
        expected
    }

    fn expected_platform_groups(os: &str, backend_mode: BackendMode) -> Vec<&'static str> {
        if os == "linux" && backend_mode == BackendMode::IsolatedAgent {
            vec!["java_maven_dependencies_v1", "java_maven_leaf_v1"]
        } else {
            Vec::new()
        }
    }

    fn expected_effective_capabilities(os: &str, backend_mode: BackendMode) -> Vec<&'static str> {
        let mut expected = expected_platform_capabilities(os, backend_mode);
        if os == "linux" && backend_mode == BackendMode::IsolatedAgent {
            expected.extend([
                "language_start_java_maven_begin",
                "language_maven_model",
                "language_maven_dependencies",
            ]);
        }
        expected.sort_unstable();
        expected
    }

    #[test]
    fn every_platform_inventory_is_unique_and_checked_against_wire_capacity_on_this_host() {
        assert_eq!(cedar_protocol::MAX_AGENT_CAPABILITIES, 32);
        assert_eq!(cedar_protocol::MAX_AGENT_CAPABILITY_GROUPS, 2);
        for os in ["linux", "macos", "windows", "other"] {
            let windows = os == "windows";
            let unix_commands = matches!(os, "linux" | "macos");
            for backend_mode in [BackendMode::InProcess, BackendMode::IsolatedAgent] {
                let expected = expected_platform_capabilities(os, backend_mode);
                let expected_groups = expected_platform_groups(os, backend_mode);
                let effective = expected_effective_capabilities(os, backend_mode);
                assert!(
                    expected.windows(2).all(|pair| pair[0] < pair[1]),
                    "{os}: duplicate capability"
                );
                assert!(expected_groups.windows(2).all(|pair| pair[0] < pair[1]));
                assert!(effective.windows(2).all(|pair| pair[0] < pair[1]));
                assert!(
                    expected.len() <= cedar_protocol::MAX_AGENT_CAPABILITIES,
                    "{os}: capability capacity exhausted"
                );
                let isolated_windows = windows && backend_mode == BackendMode::IsolatedAgent;
                let typed_java =
                    matches!(os, "linux" | "windows") && backend_mode == BackendMode::IsolatedAgent;
                if typed_java {
                    assert_eq!(expected.len(), 31, "{os}: raw flat inventory");
                    assert_eq!(effective.len(), if os == "linux" { 34 } else { 31 });
                }
                for capability in [
                    "language_start_java",
                    "language_start_java_begin",
                    "language_start_java_poll",
                    "language_start_java_cancel",
                    "java_diagnostics_refresh",
                    "language_organize_java_imports",
                    "language_java_implementations",
                ] {
                    assert_eq!(
                        expected.contains(&capability),
                        typed_java,
                        "{os}: {capability}"
                    );
                }
                for capability in [
                    "language_start_java_maven_begin",
                    "language_maven_model",
                    "language_maven_dependencies",
                ] {
                    assert_eq!(
                        expected.contains(&capability),
                        isolated_windows,
                        "{os}: direct {capability}"
                    );
                    assert_eq!(
                        effective.contains(&capability),
                        typed_java,
                        "{os}: effective {capability}"
                    );
                }
                assert_eq!(expected.contains(&"language_start"), !windows);
                assert_eq!(expected.contains(&"run"), unix_commands);
                assert_eq!(expected.contains(&"git_status"), unix_commands);
                let mut info = AgentInfo {
                    schema: AGENT_INFO_SCHEMA,
                    version: "inventory-test".into(),
                    os: os.into(),
                    arch: "x86_64".into(),
                    capabilities: expected.iter().copied().map(str::to_owned).collect(),
                    capability_groups: expected_groups.iter().copied().map(str::to_owned).collect(),
                };
                info.validate().unwrap();
                for operation in execution_operations() {
                    let name = operation.capability_name().unwrap();
                    assert_eq!(
                        info.supports(name),
                        effective.contains(&name),
                        "{os}: {name}"
                    );
                }
                // Reading the union must leave the wire vectors unchanged.
                assert_eq!(info.capabilities, expected);
                assert_eq!(info.capability_groups, expected_groups);
                // Exercise remaining capacity and one-over rejection with the
                // actual expected set, without a second hardcoded platform count.
                while info.capabilities.len() < cedar_protocol::MAX_AGENT_CAPABILITIES {
                    info.capabilities
                        .push(format!("fixture_capacity_{}", info.capabilities.len()));
                }
                info.validate().unwrap();
                info.capabilities.push("fixture_over_capacity".into());
                assert!(info.validate().is_err());
            }
        }
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
            // Typed Java and Maven require the normal Linux/Windows isolated
            // agent; support claims do not grant workspace execution trust.
            let generic_language = !cfg!(windows);
            let java_language = cfg!(any(target_os = "linux", windows))
                && backend_mode == BackendMode::IsolatedAgent;
            let maven_language = java_language;
            let language_operation_supported = |name: &str| match name {
                "language_start" => generic_language,
                "language_start_java"
                | "java_diagnostics_refresh"
                | "language_organize_java_imports"
                | "language_java_implementations"
                | "language_start_java_begin"
                | "language_start_java_poll"
                | "language_start_java_cancel" => java_language,
                "language_start_java_maven_begin"
                | "language_maven_model"
                | "language_maven_dependencies" => maven_language,
                _ => generic_language || java_language,
            };
            for capability in LANGUAGE_SESSION_CAPABILITIES.iter().chain(&[
                "language_start_java",
                "language_start_java_begin",
                "language_start_java_poll",
                "language_start_java_cancel",
                "language_start_java_maven_begin",
                "language_maven_model",
                "language_maven_dependencies",
                "java_diagnostics_refresh",
                "language_organize_java_imports",
                "language_java_implementations",
                "language_query",
                "language_resolve_uri",
                "language_format",
                "language_references",
                "language_document_symbols",
                "language_workspace_symbols",
                "language_resolve_completion",
            ]) {
                assert_eq!(
                    agent.supports(capability),
                    language_operation_supported(capability),
                    "{capability}"
                );
            }
            let expected = expected_platform_capabilities(std::env::consts::OS, backend_mode);
            // Exact set equality also proves the exact count. Never duplicate
            // its length in a cfg-only numeric assertion that host tests skip.
            assert_eq!(agent.capabilities, expected);
            assert_eq!(
                agent.capability_groups,
                expected_platform_groups(std::env::consts::OS, backend_mode)
            );
            assert!(agent.capabilities.len() <= cedar_protocol::MAX_AGENT_CAPABILITIES);
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
    fn shipping_hello_retains_raw_flat_inventory_and_linux_only_groups() {
        let root = tempfile::tempdir().unwrap();
        for mode in [BackendMode::InProcess, BackendMode::IsolatedAgent] {
            let mut workspace = Workspace::with_backend_mode(root.path(), mode).unwrap();
            let info = hello(&mut workspace);
            let groups = expected_platform_groups(std::env::consts::OS, mode);
            assert_eq!(info.capability_groups, groups);
            let wire = serde_json::to_value(&info).unwrap();
            if groups.is_empty() {
                assert!(wire.get("capability_groups").is_none());
            } else {
                assert_eq!(wire["capability_groups"], serde_json::json!(groups));
                assert_eq!(wire["capabilities"].as_array().unwrap().len(), 31);
            }
            assert_eq!(
                info.capabilities,
                expected_platform_capabilities(std::env::consts::OS, mode)
            );
            assert!(!workspace.allow_run);
            assert!(workspace.language.is_none());
            assert!(workspace.tasks.is_none());
        }
    }

    #[test]
    fn unsupported_profile_filter_removes_direct_and_group_maven_claims() {
        let mut info = agent_info(BackendMode::InProcess);
        info.capabilities = vec![
            "read".into(),
            "language_start_java_maven_begin".into(),
            "language_maven_model".into(),
            "language_maven_dependencies".into(),
        ];
        info.capability_groups = vec![
            cedar_protocol::JAVA_MAVEN_LEAF_GROUP.into(),
            cedar_protocol::JAVA_MAVEN_DEPENDENCIES_GROUP.into(),
        ];
        remove_maven_claims(&mut info);
        assert_eq!(info.capabilities, ["read"]);
        assert!(info.capability_groups.is_empty());
        for name in cedar_protocol::JAVA_MAVEN_CAPABILITIES
            .iter()
            .chain([&cedar_protocol::JAVA_MAVEN_DEPENDENCIES_CAPABILITY])
        {
            assert!(!info.supports(name));
        }
        remove_maven_claims(&mut info);
        assert_eq!(info.capabilities, ["read"]);
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
