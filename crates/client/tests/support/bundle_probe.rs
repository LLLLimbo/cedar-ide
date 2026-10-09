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
                "\"execution_rejected\":true,\"java_rejected\":true,\"maven_rejected\":true,",
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
            "language_maven_model",
            "language_start_java_poll",
            "language_start_java_cancel",
            "java_diagnostics_refresh",
            "language_organize_java_imports",
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
        ];
        expected_capabilities.sort_unstable();
        if *protocol != PROTOCOL_VERSION
            || reported != root.to_string_lossy().as_ref()
            || info.validate().is_err()
            || info.os != "windows"
            || info.arch != std::env::consts::ARCH
            || info.version != env!("CARGO_PKG_VERSION")
            || info.capabilities != expected_capabilities
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

#[cfg(not(windows))]
fn main() {
    eprintln!("cedar-client-bundle-probe requires real Windows");
    std::process::exit(1);
}
