//! Feature-gated test driver. Copy this executable beside the exact agent under
//! test: `Client::connect(Local)` resolves a sibling of this process, not cargo.
//! `portable ROOT` exercises the normal distributed agent with trust off. The
//! parent creates the marked synthetic workspace; this fixture is never shipped
//! in the portable ZIP and does not exercise or attest to GUI behavior.
#[cfg(windows)]
fn main() {
    use cedar_client::{Client, ConnectionSpec};
    use cedar_protocol::{
        Operation, Payload, JAVA_LANGUAGE_SESSION_CAPABILITIES, PROTOCOL_VERSION,
        RUN_TASK_CAPABILITIES,
    };
    use std::{
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let mode = args.first().and_then(|arg| arg.to_str());
    let valid_args = match mode {
        Some("success") => args.len() >= 3 && args[2].to_str().is_some(),
        // The existing Windows suite also passes its unused task fixture to
        // both negative modes. Keep accepting that established invocation.
        Some("missing" | "invalid") => args.len() >= 2,
        Some("portable") => args.len() == 2,
        _ => false,
    };
    if !valid_args {
        eprintln!("usage: cedar-client-bundle-probe success ROOT FIXTURE | missing ROOT [FIXTURE] | invalid ROOT [FIXTURE] | portable ROOT");
        std::process::exit(2);
    }
    let mode = mode.unwrap();
    let root = PathBuf::from(&args[1]);
    // Bound even a regression in the production client's 30s request deadline.
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(12));
        eprintln!("bundle probe watchdog expired");
        std::process::exit(126);
    });
    if mode == "portable" {
        if let Err(stage) = portable::verify(&root) {
            // Errors from the protocol or filesystem can contain workspace
            // paths or file contents. Only fixed stage names leave this mode.
            eprintln!("portable-error:{stage}");
            std::process::exit(1);
        }
        println!(
            "{}",
            concat!(
                "{\"kind\":\"cedar_windows_bundle_probe\",",
                "\"schema_version\":1,\"status\":\"success\",",
                "\"metadata_verified\":true,\"trust_off\":true,",
                "\"list_verified\":true,\"read_verified\":true,",
                "\"write_verified\":true,\"readback_verified\":true,",
                "\"search_verified\":true,\"stale_write_rejected\":true,",
                "\"execution_rejected\":true,\"java_rejected\":true,\"maven_rejected\":true,\"maven_dependencies_rejected\":true,\"workspace_symbols_rejected\":true,\"java_implementations_rejected\":true,",
                "\"client_reaped\":true}"
            )
        );
        return;
    }
    let connect = |allow_run| {
        Client::connect(ConnectionSpec::Local {
            root: root.clone(),
            allow_run,
        })
    };
    if mode == "missing" || mode == "invalid" {
        let error = match connect(true) {
            Ok(_) => panic!("{mode}: Local unexpectedly connected using a fallback"),
            Err(error) => error,
        };
        assert!(
            error.starts_with(if mode == "missing" {
                "bundled_agent_missing:"
            } else {
                "bundled_agent_start_failed:"
            }),
            "{mode}: {error}"
        );
        assert!(
            error.contains("cedar-agent"),
            "{mode}: actionable agent error: {error}"
        );
        println!("expected-error:{error}");
        return;
    }
    assert_eq!(mode, "success");
    let fixture = args[2].to_str().unwrap().to_owned();
    let mut untrusted = connect(false).unwrap();
    let Payload::Hello {
        protocol,
        root: reported,
        agent: Some(info),
    } = untrusted.handshake()
    else {
        panic!("local Windows agent omitted metadata");
    };
    assert_eq!(*protocol, PROTOCOL_VERSION);
    assert_eq!(*reported, root.canonicalize().unwrap().to_string_lossy());
    info.validate().unwrap();
    assert_eq!(info.os, "windows");
    assert_eq!(info.arch, std::env::consts::ARCH);
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    for cap in ["list", "read", "write", "search"]
        .iter()
        .chain(RUN_TASK_CAPABILITIES)
        .chain(JAVA_LANGUAGE_SESSION_CAPABILITIES)
    {
        assert!(info.supports(cap), "missing {cap}");
    }
    for cap in ["run", "git_status", "language_start", "terminal"] {
        assert!(!info.supports(cap), "unexpected {cap}");
    }
    let saved_info = info.clone();
    assert!(untrusted
        .request(Operation::Read {
            path: "absent.txt".into()
        })
        .is_err());
    let task = || Operation::RunStart {
        program: fixture.clone(),
        args: vec!["exit".into(), "0".into()],
        timeout_secs: 3,
    };
    assert!(untrusted
        .request(task())
        .unwrap_err()
        .starts_with("run_disabled:"));
    assert!(untrusted
        .request(Operation::LanguageStartJava {
            java_executable: String::new(),
            distribution: String::new(),
            data_directory: String::new(),
        })
        .unwrap_err()
        .starts_with("run_disabled:"));
    assert!(untrusted
        .request(Operation::Run {
            program: fixture.clone(),
            args: vec![],
            timeout_secs: 1,
        })
        .unwrap_err()
        .starts_with("unsupported_operation:"));
    assert!(untrusted.is_connected());
    let Payload::Hello {
        agent: Some(after), ..
    } = untrusted.request(Operation::Hello).unwrap()
    else {
        panic!("hello payload");
    };
    assert_eq!(after, saved_info);
    drop(untrusted);

    let mut trusted = connect(true).unwrap();
    let Payload::RunTask { snapshot } = trusted.request(task()).unwrap() else {
        panic!("task payload")
    };
    let id = snapshot["id"].as_u64().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let Payload::RunTask { snapshot } =
            trusted.request(Operation::RunPoll { task_id: id }).unwrap()
        else {
            panic!("poll payload")
        };
        if !matches!(
            snapshot["state"].as_str(),
            Some("starting" | "running" | "cancelling")
        ) {
            assert_eq!(snapshot["state"], "succeeded", "{snapshot}");
            assert_eq!(snapshot["windows_exit_code"], 0);
            assert_eq!(snapshot["exit_code"], 0);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Local task routing timed out: {snapshot}"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert!(trusted.is_connected());
    println!("bundle-ok:metadata,trust,file-errors,async-tasks");
}

#[cfg(windows)]
mod portable {
    use cedar_client::{Client, ConnectionSpec};
    use cedar_protocol::{Operation, Payload, PROTOCOL_VERSION};
    use std::{fs, os::windows::fs::MetadataExt, path::Path, time::Duration};

    const MARKER: &str = ".cedar-portable-probe";
    const MARKER_TEXT: &[u8] = b"cedar-portable-probe-v1\n";
    const DIRECTORY: &str = "portable files 雪";
    const FILE_NAME: &str = "hello café.txt";
    const FILE: &str = "portable files 雪/hello café.txt";
    const ORIGINAL: &str = "cedar-portable-original-v1\n";
    const SAVED: &str = "cedar-portable-saved-v1\n";
    const SAVED_QUERY: &str = "cedar-portable-saved-v1";
    const STALE: &str = "cedar-portable-stale-v1\n";
    const CLEANUP_BOUND: Duration = Duration::from_secs(5);

    // Fixed-stage errors deliberately discard diagnostics supplied by the
    // process, filesystem, and protocol. This probe's receipt is safe to retain.
    type ProbeResult<T> = Result<T, &'static str>;

    pub fn verify(root: &Path) -> ProbeResult<()> {
        if !root.is_absolute() {
            return Err("synthetic_root");
        }
        ordinary_path(root, true)?;
        ordinary_path(&root.join(MARKER), false)?;
        let marker_metadata = fs::metadata(root.join(MARKER)).map_err(|_| "synthetic_marker")?;
        if marker_metadata.len() != MARKER_TEXT.len() as u64
            || fs::read(root.join(MARKER)).map_err(|_| "synthetic_marker")? != MARKER_TEXT
        {
            return Err("synthetic_marker");
        }
        ordinary_path(&root.join(DIRECTORY), true)?;
        ordinary_path(&root.join(FILE), false)?;
        let canonical = root.canonicalize().map_err(|_| "synthetic_root")?;
        let mut client = Client::connect(ConnectionSpec::Local {
            root: canonical.clone(),
            allow_run: false,
        })
        .map_err(|_| "connect")?;
        let verified = verify_connection(&mut client, &canonical);
        // Exercise the exact owning Client API, including failed verification.
        // A successful receipt requires the owned-child reaper's acknowledgement.
        client
            .close_and_wait(CLEANUP_BOUND)
            .map_err(|_| "client_reaped")?;
        verified
    }

    fn ordinary_path(path: &Path, directory: bool) -> ProbeResult<()> {
        let metadata = fs::symlink_metadata(path).map_err(|_| "synthetic_path")?;
        // Reject all Windows reparse points, including directory junctions.
        if metadata.file_attributes() & 0x400 != 0
            || metadata.is_dir() != directory
            || (!directory && !metadata.is_file())
        {
            return Err("synthetic_path");
        }
        Ok(())
    }

    fn verify_connection(client: &mut Client, root: &Path) -> ProbeResult<()> {
        let Payload::Hello {
            protocol,
            root: reported,
            agent: Some(info),
        } = client.handshake()
        else {
            return Err("metadata");
        };
        let mut expected_capabilities = vec![
            "git_changes",
            "git_diff",
            "list",
            "read",
            "write",
            "search",
            "run_start",
            "run_poll",
            "run_cancel",
            "language_start_java",
            "language_start_java_begin",
            "language_start_java_maven_begin",
            "language_maven_dependencies",
            "language_maven_model",
            "language_start_java_poll",
            "language_start_java_cancel",
            "java_diagnostics_refresh",
            "language_organize_java_imports",
            "language_java_implementations",
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
        ];
        expected_capabilities.sort_unstable();
        if *protocol != PROTOCOL_VERSION
            || reported != root.to_string_lossy().as_ref()
            || info.validate().is_err()
            || info.os != "windows"
            || info.arch != std::env::consts::ARCH
            || info.version != env!("CARGO_PKG_VERSION")
            || info.capabilities != expected_capabilities
            || !info.capability_groups.is_empty()
        {
            return Err("metadata");
        }
        let saved_info = info.clone();

        let Payload::Entries { entries } = client
            .request(Operation::List { path: ".".into() })
            .map_err(|_| "list_root")?
        else {
            return Err("list_root");
        };
        if !entries
            .iter()
            .any(|entry| entry.name == DIRECTORY && entry.path == DIRECTORY && entry.is_dir)
        {
            return Err("list_root");
        }
        let Payload::Entries { entries } = client
            .request(Operation::List {
                path: DIRECTORY.into(),
            })
            .map_err(|_| "list_file")?
        else {
            return Err("list_file");
        };
        if !entries
            .iter()
            .any(|entry| entry.name == FILE_NAME && entry.path == FILE && !entry.is_dir)
        {
            return Err("list_file");
        }
        let original_revision = read_expected(client, ORIGINAL, "read")?;
        let Payload::Written {
            revision: saved_revision,
        } = client
            .request(Operation::Write {
                path: FILE.into(),
                text: SAVED.into(),
                expected_revision: Some(original_revision.clone()),
            })
            .map_err(|_| "conditional_save")?
        else {
            return Err("conditional_save");
        };
        if saved_revision.is_empty() || saved_revision == original_revision {
            return Err("conditional_save");
        }
        if read_expected(client, SAVED, "readback")? != saved_revision {
            return Err("readback");
        }
        let Payload::Matches { matches, truncated } = client
            .request(Operation::Search {
                query: SAVED_QUERY.into(),
                limit: 10,
            })
            .map_err(|_| "search")?
        else {
            return Err("search");
        };
        if truncated
            || matches.len() != 1
            || matches[0].path != FILE
            || matches[0].line != 1
            || matches[0].text != SAVED_QUERY
        {
            return Err("search");
        }

        let stale = client.request(Operation::Write {
            path: FILE.into(),
            text: STALE.into(),
            expected_revision: Some(original_revision),
        });
        if !matches!(stale, Err(error) if error.starts_with("conflict:")) {
            return Err("stale_write_rejected");
        }
        if read_expected(client, SAVED, "stale_write_preserved")? != saved_revision {
            return Err("stale_write_preserved");
        }
        // These paths intentionally name no executable. A trust-off agent must
        // reject before inspecting a program, JDK, distribution, or data path.
        let execution = client.request(Operation::RunStart {
            program: String::new(),
            args: Vec::new(),
            timeout_secs: 1,
        });
        if !matches!(execution, Err(error) if error.starts_with("run_disabled:")) {
            return Err("execution_rejected");
        }
        let java = client.request(Operation::LanguageStartJava {
            java_executable: String::new(),
            distribution: String::new(),
            data_directory: String::new(),
        });
        if !matches!(java, Err(error) if error.starts_with("run_disabled:")) {
            return Err("java_rejected");
        }
        for operation in [
            Operation::LanguageStartJavaMavenBegin {
                java_executable: String::new(),
                distribution: String::new(),
                data_directory: String::new(),
                local_repository: String::new(),
            },
            Operation::LanguageMavenModel,
        ] {
            if !matches!(client.request(operation), Err(error) if error.starts_with("run_disabled:"))
            {
                return Err("maven_rejected");
            }
        }
        if !matches!(client.request(Operation::LanguageWorkspaceSymbols { query: "NeverLaunched".into() }), Err(error) if error.starts_with("run_disabled:"))
        {
            return Err("workspace_symbols_rejected");
        }
        if !matches!(client.request(Operation::LanguageJavaImplementations { path: "NeverLaunched.java".into(), version: 1, line: 0, character: 0 }), Err(error) if error.starts_with("run_disabled:"))
        {
            return Err("java_implementations_rejected");
        }
        if !matches!(client.request(Operation::LanguageMavenDependencies { startup_id: 1, pom_sha256: "a".repeat(64) }), Err(error) if error.starts_with("run_disabled:"))
        {
            return Err("maven_dependencies_rejected");
        }
        if !client.is_connected() {
            return Err("connected");
        }
        let Payload::Hello {
            protocol,
            root: reported,
            agent: Some(after),
        } = client
            .request(Operation::Hello)
            .map_err(|_| "hello_after")?
        else {
            return Err("hello_after");
        };
        if protocol != PROTOCOL_VERSION || reported != root.to_string_lossy() || after != saved_info
        {
            return Err("hello_after");
        }
        Ok(())
    }

    fn read_expected(
        client: &mut Client,
        expected: &str,
        stage: &'static str,
    ) -> ProbeResult<String> {
        let Payload::File {
            path,
            text,
            revision,
        } = client
            .request(Operation::Read { path: FILE.into() })
            .map_err(|_| stage)?
        else {
            return Err(stage);
        };
        if path != FILE || text != expected || revision.is_empty() {
            return Err(stage);
        }
        Ok(revision)
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn main() {
    eprintln!("cedar-client-bundle-probe requires real Windows");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    linux_desktop::main();
}

#[cfg(target_os = "linux")]
mod linux_desktop {
    use cedar_client::{Client, ConnectionCancellation, ConnectionOwnership, ConnectionSpec};
    use cedar_protocol::{
        AgentInfo, GitDiffKind, LanguageQueryKind, Operation, Payload, AGENT_INFO_SCHEMA,
        LANGUAGE_SESSION_CAPABILITIES, PROTOCOL_VERSION, RUN_TASK_CAPABILITIES,
    };
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        time::{Duration, Instant},
    };

    const ROOT: &str = "workspace café 雪";
    const MARKER: &str = ".cedar-linux-desktop-acceptance";
    const MARKER_BYTES: &[u8] = b"cedar-linux-desktop-acceptance-v1\n";
    const SOURCE: &str = "source preserved.txt";
    const SOURCE_BYTES: &[u8] = b"cedar-linux-source-preserved-v1\n";
    const OUTSIDE: &str = "outside preserved.txt";
    const OUTSIDE_BYTES: &[u8] = b"cedar-linux-outside-preserved-v1\n";
    const DIRECTORY: &str = "files café 雪";
    const FILE_NAME: &str = "saved file λ.txt";
    const FILE: &str = "files café 雪/saved file λ.txt";
    const ORIGINAL: &str = "cedar-linux-bundle-original-v1 café 雪\n";
    const SAVED: &str = "cedar-linux-bundle-saved-v1 café 雪\n";
    const SAVED_QUERY: &str = "cedar-linux-bundle-saved-v1";
    const STALE: &str = "cedar-linux-bundle-stale-v1\n";
    const CLOSE_BOUND: Duration = Duration::from_secs(5);
    const ELAPSED_BOUND: Duration = Duration::from_secs(30);
    const REQUEST_LIMIT: usize = 96;
    const AGENT_SPAWN_LIMIT: usize = 2;

    // Never retain raw environment values, errors, roots, revisions, or content in
    // test diagnostics. Every failure emitted by this test is a fixed stage name.
    type ProbeResult<T> = Result<T, &'static str>;

    struct Budget {
        started: Instant,
        requests: usize,
        spawns: usize,
        reaped: usize,
    }

    impl Budget {
        fn check(&self) -> ProbeResult<()> {
            if self.started.elapsed() > ELAPSED_BOUND {
                return Err("elapsed_bound");
            }
            Ok(())
        }

        fn request(
            &mut self,
            client: &mut Client,
            operation: Operation,
        ) -> ProbeResult<Result<Payload, String>> {
            self.check()?;
            if self.requests >= REQUEST_LIMIT {
                return Err("client_call_limit");
            }
            self.requests += 1;
            let result = client.request(operation);
            self.check()?;
            Ok(result)
        }

        fn successful(
            &mut self,
            client: &mut Client,
            operation: Operation,
            stage: &'static str,
        ) -> ProbeResult<Payload> {
            self.request(client, operation)?.map_err(|_| stage)
        }
    }

    // A panic during verification still has one owner that consumes the Client and
    // asks its existing owned-child reaper to finish. Error paths use explicit close
    // and propagate its acknowledgement failure. Drop is only a failure fallback;
    // it never sets a successful cleanup receipt. No new watchdog or process API.
    struct OwnedClient(Option<Client>);

    impl OwnedClient {
        fn close(mut self) -> ProbeResult<()> {
            self.0
                .take()
                .ok_or("client_owner")?
                .close_and_wait(CLOSE_BOUND)
                .map_err(|_| "client_reaped")
        }
    }

    impl Drop for OwnedClient {
        fn drop(&mut self) {
            if let Some(client) = self.0.take() {
                let _ = client.close_and_wait(CLOSE_BOUND);
            }
        }
    }

    fn with_client<T>(
        root: &Path,
        budget: &mut Budget,
        verify: impl FnOnce(&mut Client, &mut Budget) -> ProbeResult<T>,
    ) -> ProbeResult<T> {
        budget.check()?;
        if budget.spawns >= AGENT_SPAWN_LIMIT {
            return Err("agent_spawn_limit");
        }
        budget.spawns += 1;
        // The production GUI route derives only current_exe().parent()/cedar-agent.
        // The probe accepts no agent path and never calls spawn_agent directly.
        let client = Client::connect(ConnectionSpec::BundledLinux {
            root: root.to_path_buf(),
            allow_run: false,
        })
        .map_err(|_| "connect")?;
        let mut owned = OwnedClient(Some(client));
        let verified = verify(owned.0.as_mut().ok_or("client_owner")?, budget);
        owned.close()?;
        budget.reaped += 1;
        budget.check()?;
        verified
    }

    fn expected_capabilities() -> Vec<&'static str> {
        // Use the shared lifecycle groups plus the explicit Linux platform set.
        // Maven's two groups leave these 31 direct capability names unchanged.
        let mut expected = vec![
            "list",
            "read",
            "write",
            "search",
            "git_status",
            "git_changes",
            "git_diff",
            "run",
            "language_query",
            "language_format",
            "language_references",
            "language_document_symbols",
            "language_workspace_symbols",
            "language_resolve_uri",
            "language_resolve_completion",
            "language_start_java",
            "language_start_java_begin",
            "language_start_java_poll",
            "language_start_java_cancel",
            "java_diagnostics_refresh",
            "language_organize_java_imports",
            "language_java_implementations",
        ];
        expected.extend_from_slice(RUN_TASK_CAPABILITIES);
        expected.extend_from_slice(LANGUAGE_SESSION_CAPABILITIES);
        expected.sort_unstable();
        expected
    }

    fn metadata(client: &Client, root: &Path) -> ProbeResult<AgentInfo> {
        let Payload::Hello {
            protocol,
            root: reported,
            agent: Some(info),
        } = client.handshake()
        else {
            return Err("metadata");
        };
        let expected = expected_capabilities();
        if PROTOCOL_VERSION != 4
            || AGENT_INFO_SCHEMA != 1
            || *protocol != 4
            || info.schema != 1
            || reported != root.to_str().ok_or("root_utf8")?
            || info.validate().is_err()
            || info.version != env!("CARGO_PKG_VERSION")
            || info.os != "linux"
            || info.arch != "x86_64"
            || !expected.windows(2).all(|pair| pair[0] < pair[1])
            || info.capabilities != expected
            || info.capability_groups != ["java_maven_dependencies_v1", "java_maven_leaf_v1"]
        {
            return Err("metadata");
        }
        for capability in [
            "language_start_java_maven_begin",
            "language_maven_model",
            "language_maven_dependencies",
        ] {
            if info.capabilities.iter().any(|name| name == capability) || !info.supports(capability)
            {
                return Err("metadata");
            }
        }
        Ok(info.clone())
    }

    fn read_expected(
        client: &mut Client,
        budget: &mut Budget,
        expected: &str,
    ) -> ProbeResult<String> {
        let Payload::File {
            path,
            text,
            revision,
        } = budget.successful(client, Operation::Read { path: FILE.into() }, "read")?
        else {
            return Err("read");
        };
        if path != FILE || text != expected || revision.is_empty() {
            return Err("read");
        }
        Ok(revision)
    }

    fn refused_and_usable(
        client: &mut Client,
        budget: &mut Budget,
        operation: Operation,
        prefix: &'static str,
        revision: &str,
    ) -> ProbeResult<()> {
        if !matches!(budget.request(client, operation)?, Err(error) if error.starts_with(prefix)) {
            return Err("operation_refused");
        }
        // Hello is cached by Client; an actual Read proves the process still serves
        // requests and the last successful file revision survives every refusal.
        if !client.is_connected() || read_expected(client, budget, SAVED)? != revision {
            return Err("usable_after_refusal");
        }
        Ok(())
    }

    fn execution_operations() -> Vec<Operation> {
        // Empty program locations cannot identify an executable even if trust
        // checking regresses. Exact run_disabled proves refusal precedes validation.
        vec![
            Operation::Run {
                program: String::new(),
                args: Vec::new(),
                timeout_secs: 1,
            },
            Operation::RunStart {
                program: String::new(),
                args: Vec::new(),
                timeout_secs: 1,
            },
            Operation::RunPoll { task_id: 1 },
            Operation::RunCancel { task_id: 1 },
            Operation::GitStatus,
            Operation::GitChanges {
                git_executable: String::new(),
            },
            Operation::GitDiff {
                git_executable: String::new(),
                path: FILE.into(),
                kind: GitDiffKind::Unstaged,
            },
            Operation::LanguageStart {
                program: String::new(),
                args: Vec::new(),
            },
            Operation::LanguageOpen {
                path: FILE.into(),
                language_id: "plaintext".into(),
                version: 1,
                text: SAVED.into(),
            },
            Operation::LanguageChange {
                path: FILE.into(),
                version: 2,
                text: STALE.into(),
            },
            Operation::LanguageClose { path: FILE.into() },
            Operation::LanguageQuery {
                path: FILE.into(),
                line: 0,
                character: 0,
                kind: LanguageQueryKind::Hover,
            },
            Operation::LanguageFormat {
                path: FILE.into(),
                version: 1,
                tab_size: 4,
                insert_spaces: true,
            },
            Operation::LanguageReferences {
                path: FILE.into(),
                line: 0,
                character: 0,
                include_declaration: false,
            },
            Operation::LanguageDocumentSymbols { path: FILE.into() },
            Operation::LanguageWorkspaceSymbols {
                query: "NeverLaunched".into(),
            },
            Operation::LanguageResolveUri { uri: String::new() },
            Operation::LanguageResolveCompletion {
                item: Default::default(),
            },
            Operation::LanguageEvents,
            Operation::LanguageStop,
        ]
    }

    fn typed_java_operations() -> Vec<Operation> {
        // Empty paths cannot name a Java executable, distribution or data root.
        // Advertised requests must reach run_disabled before path validation.
        vec![
            Operation::LanguageStartJava {
                java_executable: String::new(),
                distribution: String::new(),
                data_directory: String::new(),
            },
            Operation::LanguageStartJavaBegin {
                java_executable: String::new(),
                distribution: String::new(),
                data_directory: String::new(),
            },
            Operation::LanguageStartJavaPoll { startup_id: 1 },
            Operation::LanguageStartJavaCancel { startup_id: 1 },
            Operation::LanguageRefreshJavaDiagnostics {
                path: FILE.into(),
                version: 1,
            },
            Operation::LanguageOrganizeJavaImports {
                path: FILE.into(),
                version: 1,
            },
            Operation::LanguageJavaImplementations {
                path: FILE.into(),
                version: 1,
                line: 0,
                character: 0,
            },
        ]
    }

    fn maven_operations() -> Vec<Operation> {
        // The normal Client recognizes both groups and sends all three routes to
        // the backend's trust gate. Empty paths cannot identify any executable;
        // exact run_disabled proves refusal precedes path or model validation.
        vec![
            Operation::LanguageStartJavaMavenBegin {
                java_executable: String::new(),
                distribution: String::new(),
                data_directory: String::new(),
                local_repository: String::new(),
            },
            Operation::LanguageMavenModel,
            Operation::LanguageMavenDependencies {
                startup_id: 1,
                pom_sha256: "a".repeat(64),
            },
        ]
    }

    fn verify_initial(
        client: &mut Client,
        root: &Path,
        budget: &mut Budget,
    ) -> ProbeResult<(AgentInfo, String)> {
        let info = metadata(client, root)?;
        for (path, name, relative, directory) in [
            (".", DIRECTORY, DIRECTORY, true),
            (DIRECTORY, FILE_NAME, FILE, false),
        ] {
            let Payload::Entries { entries } =
                budget.successful(client, Operation::List { path: path.into() }, "list")?
            else {
                return Err("list");
            };
            if !entries.iter().any(|entry| {
                entry.name == name && entry.path == relative && entry.is_dir == directory
            }) {
                return Err("list");
            }
        }
        let original_revision = read_expected(client, budget, ORIGINAL)?;
        let Payload::Written {
            revision: saved_revision,
        } = budget.successful(
            client,
            Operation::Write {
                path: FILE.into(),
                text: SAVED.into(),
                expected_revision: Some(original_revision.clone()),
            },
            "conditional_write",
        )?
        else {
            return Err("conditional_write");
        };
        if saved_revision.is_empty()
            || saved_revision == original_revision
            || read_expected(client, budget, SAVED)? != saved_revision
        {
            return Err("readback");
        }
        let Payload::Matches { matches, truncated } = budget.successful(
            client,
            Operation::Search {
                query: SAVED_QUERY.into(),
                limit: 10,
            },
            "search",
        )?
        else {
            return Err("search");
        };
        if truncated
            || matches.len() != 1
            || !matches.first().is_some_and(|found| {
                found.path == FILE && found.line == 1 && found.text == SAVED.trim_end()
            })
        {
            return Err("search");
        }
        refused_and_usable(
            client,
            budget,
            Operation::Write {
                path: FILE.into(),
                text: STALE.into(),
                expected_revision: Some(original_revision),
            },
            "conflict:",
            &saved_revision,
        )?;
        // The only attempted outside write names our own disposable sibling file.
        // No request references a user's workspace, home, checkout, or other data.
        let escape = format!("../{OUTSIDE}");
        for operation in [
            Operation::List { path: "..".into() },
            Operation::Read {
                path: escape.clone(),
            },
            Operation::Write {
                path: escape,
                text: STALE.into(),
                expected_revision: None,
            },
        ] {
            refused_and_usable(client, budget, operation, "invalid_path:", &saved_revision)?;
        }
        for operation in execution_operations() {
            refused_and_usable(client, budget, operation, "run_disabled:", &saved_revision)?;
        }
        for operation in typed_java_operations() {
            refused_and_usable(client, budget, operation, "run_disabled:", &saved_revision)?;
        }
        for operation in maven_operations() {
            refused_and_usable(client, budget, operation, "run_disabled:", &saved_revision)?;
        }
        let Payload::Hello {
            protocol,
            root: reported,
            agent: Some(after),
        } = budget.successful(client, Operation::Hello, "metadata_stable")?
        else {
            return Err("metadata_stable");
        };
        if protocol != 4 || reported != root.to_str().ok_or("root_utf8")? || after != info {
            return Err("metadata_stable");
        }
        Ok((info, saved_revision))
    }

    #[derive(PartialEq, Eq)]
    enum SnapshotEntry {
        Directory,
        File(Vec<u8>),
    }

    fn snapshot(base: &Path) -> ProbeResult<BTreeMap<PathBuf, SnapshotEntry>> {
        fn visit(
            base: &Path,
            relative: &Path,
            entries: &mut BTreeMap<PathBuf, SnapshotEntry>,
        ) -> ProbeResult<()> {
            if entries.len() >= 16 || relative.components().count() > 3 {
                return Err("fixture_snapshot_bound");
            }
            let path = base.join(relative);
            let metadata = fs::symlink_metadata(&path).map_err(|_| "fixture_snapshot")?;
            if metadata.file_type().is_symlink() {
                return Err("fixture_snapshot");
            }
            if metadata.is_file() && metadata.len() <= 4096 {
                entries.insert(
                    relative.into(),
                    SnapshotEntry::File(fs::read(&path).map_err(|_| "fixture_snapshot")?),
                );
            } else if metadata.is_dir() {
                entries.insert(relative.into(), SnapshotEntry::Directory);
                for entry in fs::read_dir(&path).map_err(|_| "fixture_snapshot")? {
                    let entry = entry.map_err(|_| "fixture_snapshot")?;
                    visit(base, &relative.join(entry.file_name()), entries)?;
                }
            } else {
                return Err("fixture_snapshot");
            }
            Ok(())
        }
        let mut entries = BTreeMap::new();
        visit(base, Path::new(""), &mut entries)?;
        Ok(entries)
    }

    fn marked_root(root: &Path) -> ProbeResult<()> {
        if !root.is_absolute() || root.file_name().and_then(|name| name.to_str()) != Some(ROOT) {
            return Err("synthetic_root");
        }
        for (path, directory) in [(root.to_path_buf(), true), (root.join(MARKER), false)] {
            let metadata = fs::symlink_metadata(path).map_err(|_| "synthetic_marker")?;
            if metadata.file_type().is_symlink()
                || metadata.is_dir() != directory
                || (!directory && (!metadata.is_file() || metadata.len() > 128))
            {
                return Err("synthetic_marker");
            }
        }
        if fs::read(root.join(MARKER)).map_err(|_| "synthetic_marker")? != MARKER_BYTES {
            return Err("synthetic_marker");
        }
        Ok(())
    }

    fn portable(root: &Path) -> ProbeResult<()> {
        marked_root(root)?;
        let mut budget = Budget {
            started: Instant::now(),
            requests: 0,
            spawns: 0,
            reaped: 0,
        };
        if std::env::consts::ARCH != "x86_64" {
            return Err("host_architecture");
        }
        let root = root.canonicalize().map_err(|_| "fixture_root")?;
        let fixture = root.parent().ok_or("fixture_root")?;
        let mut expected = snapshot(fixture)?;
        for (path, bytes) in [
            (Path::new(ROOT).join(MARKER), MARKER_BYTES),
            (Path::new(ROOT).join(SOURCE), SOURCE_BYTES),
            (PathBuf::from(OUTSIDE), OUTSIDE_BYTES),
            (Path::new(ROOT).join(FILE), ORIGINAL.as_bytes()),
        ] {
            if expected.get(&path) != Some(&SnapshotEntry::File(bytes.to_vec())) {
                return Err("fixture_original");
            }
        }
        expected.insert(
            Path::new(ROOT).join(FILE),
            SnapshotEntry::File(SAVED.as_bytes().to_vec()),
        );
        let (info, saved_revision) = with_client(&root, &mut budget, |client, budget| {
            verify_initial(client, &root, budget)
        })?;
        if snapshot(fixture)? != expected {
            return Err("fixture_preserved");
        }
        with_client(&root, &mut budget, |client, budget| {
            if metadata(client, &root)? != info
                || read_expected(client, budget, SAVED)? != saved_revision
            {
                return Err("reconnect");
            }
            Ok(())
        })?;
        if snapshot(fixture)? != expected {
            return Err("fixture_preserved");
        }
        budget.check()?;
        if budget.spawns != AGENT_SPAWN_LIMIT || budget.reaped != budget.spawns {
            return Err("client_reaped");
        }
        // Only fixed tags, boolean witnesses and bounded counts leave this
        // driver. No root, revision, contents, environment or raw diagnostics.
        println!(
            concat!(
                "{{\"kind\":\"cedar_linux_desktop_bundle_probe\",",
                "\"schema_version\":1,\"status\":\"success\",",
                "\"protocol_version\":4,\"agent_info_schema\":1,",
                "\"linux_x86_64\":true,\"bundled_linux_connection\":true,",
                "\"fixed_sibling_agent\":true,\"package_version_matches\":true,",
                "\"capabilities_exact\":true,\"capability_count\":{},",
                "\"capability_group_count\":{},\"trust_off\":true,",
                "\"list_verified\":true,\"read_verified\":true,",
                "\"conditional_write_verified\":true,\"readback_verified\":true,",
                "\"search_verified\":true,\"stale_write_rejected\":true,",
                "\"root_escape_rejected\":true,\"task_operations_rejected\":true,",
                "\"language_operations_rejected\":true,\"git_operations_rejected\":true,",
                "\"typed_java_advertised\":true,\"maven_groups_advertised\":true,",
                "\"maven_operations_rejected\":true,\"errors_leave_client_usable\":true,",
                "\"preserved_fixture_unchanged\":true,\"only_expected_file_changed\":true,",
                "\"reconnect_saved_bytes\":true,\"initial_client_reaped\":true,",
                "\"reconnect_client_reaped\":true,\"agent_processes_spawned\":{},",
                "\"explicit_client_calls\":{},\"elapsed_ms\":{},",
                "\"elapsed_bound_ms\":30000,\"client_call_limit\":96,",
                "\"agent_spawn_limit\":2,\"close_bound_ms\":5000}}"
            ),
            info.capabilities.len(),
            info.capability_groups.len(),
            budget.spawns,
            budget.requests,
            budget.started.elapsed().as_millis(),
        );
        Ok(())
    }

    fn rejection(root: &Path, case: &str) -> ProbeResult<()> {
        marked_root(root)?;
        let (prefix, spawned) = match case {
            "missing" => ("bundled_agent_missing:", false),
            "directory" | "symlink" | "dangling_symlink" | "nonexecutable" | "invalid_elf"
            | "wrong_elf_arch" => ("bundled_agent_invalid:", false),
            "missing_root" | "non_utf8_root" => ("invalid_root:", false),
            "protocol" => ("protocol_mismatch:", true),
            "missing_metadata" | "invalid_metadata" => ("invalid_agent_info:", true),
            "version" | "platform" | "architecture" => ("bundled_agent_mismatch:", true),
            "root_mismatch" => ("bundled_agent_root_mismatch:", true),
            "eof" => ("transport_eof:", true),
            "response_id" => ("protocol_error:", true),
            "cancel" => ("transport_cancelled:", true),
            "pre_cancel" => ("transport_cancelled:", false),
            _ => return Err("rejection_case"),
        };
        let requested_root = match case {
            "missing_root" => root.join("absent-generated-root"),
            "non_utf8_root" => {
                use std::os::unix::ffi::OsStringExt;
                root.parent()
                    .ok_or("fixture_root")?
                    .join(std::ffi::OsString::from_vec(b"nonutf8-\xff".to_vec()))
            }
            _ => root.to_path_buf(),
        };
        let cancellation = ConnectionCancellation::new();
        if case == "pre_cancel" {
            cancellation.cancel();
        }
        let canceller = if case == "cancel" {
            let started = root.join(".fault-started");
            let cancellation = cancellation.clone();
            Some(std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(3);
                while !started.is_file() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(5));
                }
                let observed = started.is_file();
                cancellation.cancel();
                observed
            }))
        } else {
            None
        };
        let started = Instant::now();
        let outcome = Client::connect_bundled_linux_with_cancellation_detailed(
            requested_root,
            false,
            cancellation,
        );
        if let Some(canceller) = canceller {
            if !canceller.join().map_err(|_| "cancel_thread")? {
                return Err("cancel_not_spawned");
            }
        }
        let failure = match outcome {
            Ok(client) => {
                client
                    .close_and_wait(CLOSE_BOUND)
                    .map_err(|_| "unexpected_client_cleanup")?;
                return Err("unexpected_connection");
            }
            Err(failure) => failure,
        };
        let expected_ownership = if spawned {
            ConnectionOwnership::CleanupVerified
        } else {
            ConnectionOwnership::NoChild
        };
        if !failure.message.starts_with(prefix)
            || failure.ownership != expected_ownership
            || failure.cleanup_error.is_some()
            || started.elapsed() > Duration::from_secs(10)
        {
            return Err("rejection_contract");
        }
        println!(
            concat!(
                "{{\"kind\":\"cedar_linux_desktop_rejection_probe\",",
                "\"schema_version\":1,\"status\":\"success\",\"case\":\"{}\",",
                "\"rejected\":true,\"trust_off\":true,\"child_spawned\":{},",
                "\"cleanup_verified\":{},\"no_child\":{},\"elapsed_ms\":{},",
                "\"elapsed_bound_ms\":10000}}"
            ),
            case,
            spawned,
            spawned,
            !spawned,
            started.elapsed().as_millis(),
        );
        Ok(())
    }

    // The same opt-in, nonshipping native ELF is the controlled fault peer when
    // its generated copy is named cedar-agent. It never launches another
    // process or opens a socket. Only a generated marked root selects a fixed
    // response; the production route gains no test controls or selectors.
    fn fault_peer(root: &Path) -> ProbeResult<()> {
        use cedar_protocol::{read_frame, write_frame, Request, Response};
        use std::io::{self, BufReader, Read};

        marked_root(root)?;
        let mode_path = root.join(".cedar-linux-desktop-fault");
        let metadata = fs::symlink_metadata(&mode_path).map_err(|_| "fault_mode")?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 {
            return Err("fault_mode");
        }
        let mode = fs::read_to_string(mode_path).map_err(|_| "fault_mode")?;
        fs::write(root.join(".fault-started"), b"started\n").map_err(|_| "fault_start")?;
        let mut input = BufReader::new(io::stdin());
        let request: Request = read_frame(&mut input)
            .map_err(|_| "fault_request")?
            .ok_or("fault_request")?;
        if !matches!(request.op, Operation::Hello) {
            return Err("fault_request");
        }
        if mode == "eof" {
            return Ok(());
        }
        if mode != "cancel" {
            let mut info = AgentInfo {
                schema: AGENT_INFO_SCHEMA,
                version: env!("CARGO_PKG_VERSION").into(),
                os: "linux".into(),
                arch: std::env::consts::ARCH.into(),
                capabilities: vec!["list".into(), "read".into()],
                capability_groups: Vec::new(),
            };
            match mode.as_str() {
                "version" => info.version = "0.0.0-incompatible".into(),
                "platform" => info.os = "windows".into(),
                "architecture" => info.arch = "incompatible_arch".into(),
                "invalid_metadata" => info.schema = AGENT_INFO_SCHEMA + 1,
                "protocol" | "missing_metadata" | "root_mismatch" | "response_id" => {}
                _ => return Err("fault_mode"),
            }
            let response = Response {
                id: if mode == "response_id" {
                    request.id + 1
                } else {
                    request.id
                },
                result: Ok(Payload::Hello {
                    protocol: if mode == "protocol" {
                        PROTOCOL_VERSION + 1
                    } else {
                        PROTOCOL_VERSION
                    },
                    root: if mode == "root_mismatch" {
                        root.parent()
                            .ok_or("fault_root")?
                            .to_str()
                            .ok_or("fault_root")?
                            .into()
                    } else {
                        root.to_str().ok_or("fault_root")?.into()
                    },
                    agent: if mode == "missing_metadata" {
                        None
                    } else {
                        Some(info)
                    },
                }),
            };
            write_frame(&mut io::stdout().lock(), &response).map_err(|_| "fault_response")?;
        }
        // Parent cleanup must close stdin and observe its own child reaper.
        // This EOF observation is test evidence only, never cleanup authority.
        let mut rest = Vec::new();
        input
            .take(4097)
            .read_to_end(&mut rest)
            .map_err(|_| "fault_drain")?;
        if !rest.is_empty() {
            return Err("fault_unexpected_request");
        }
        fs::write(root.join(".fault-eof"), b"eof\n").map_err(|_| "fault_eof")?;
        Ok(())
    }

    pub fn main() {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        let mode = args.first().and_then(|argument| argument.to_str());
        let result = match (mode, args.len()) {
            (Some("portable"), 2) => portable(Path::new(&args[1])),
            (Some("reject"), 3) => match args[2].to_str() {
                Some(case) => rejection(Path::new(&args[1]), case),
                None => Err("arguments"),
            },
            (Some("--root"), 2) => {
                let own_name = std::env::current_exe()
                    .ok()
                    .and_then(|path| path.file_name().map(|name| name.to_owned()));
                if own_name.as_deref() == Some(std::ffi::OsStr::new("cedar-agent")) {
                    fault_peer(Path::new(&args[1]))
                } else {
                    Err("fault_executable_name")
                }
            }
            _ => Err("arguments"),
        };
        if let Err(stage) = result {
            eprintln!("linux-desktop-probe-error:{stage}");
            std::process::exit(1);
        }
    }
}
