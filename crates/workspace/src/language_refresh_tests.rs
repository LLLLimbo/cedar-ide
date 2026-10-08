//! Real child pipes test the Java-only guard and exact extension wire payload.
//! The private setup bypasses platform launch selection only in this test module;
//! normal workspace constructors retain their Windows-isolated startup policy.
use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};
use tempfile::TempDir;

struct PeerBinary {
    _directory: TempDir,
    path: PathBuf,
}

fn peer_binary() -> &'static Path {
    static PEER: OnceLock<PeerBinary> = OnceLock::new();
    &PEER
        .get_or_init(|| {
            let directory = tempfile::tempdir().unwrap();
            let path = directory
                .path()
                .join(format!("java-refresh-peer{}", std::env::consts::EXE_SUFFIX));
            let output = Command::new("rustc")
                .args(["--edition=2021", "--crate-name", "java_refresh_peer"])
                .arg(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/fixtures/java_refresh_peer.rs"),
                )
                .arg("-o")
                .arg(&path)
                .output()
                .expect("Rust compiler required for synthetic real-child tests");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            PeerBinary {
                _directory: directory,
                path,
            }
        })
        .path
}

fn initialize() -> Value {
    json!({
        "capabilities":{"textDocumentSync":{"openClose":true,"change":1},"hoverProvider":true},
        "serverInfo":{"name":"JDT Language Server (Standard)","version":"1.61.0-SNAPSHOT"},
        "cedar_java_diagnostics_refresh":true
    })
}

fn start(production_java: bool, initialize: Value) -> (TempDir, Workspace, Value) {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("initialize.json"),
        initialize.to_string(),
    )
    .unwrap();
    let mut workspace =
        Workspace::with_backend_mode(directory.path(), cedar_tasks::BackendMode::IsolatedAgent)
            .unwrap();
    workspace.set_allow_run(true);
    let mut config = ProcessConfig::new(peer_binary());
    config.args.push(directory.path().into());
    let options = ClientOptions {
        request_timeout: Duration::from_secs(2),
        shutdown_timeout: Duration::from_millis(200),
        ..ClientOptions::default()
    };
    let Payload::Language { value } = workspace
        .start_language_session(config, options, Value::Null, production_java)
        .unwrap()
    else {
        panic!("expected start response")
    };
    (directory, workspace, value)
}

fn refresh(path: &str, version: i32) -> Operation {
    Operation::LanguageRefreshJavaDiagnostics {
        path: path.into(),
        version,
    }
}

fn open(workspace: &mut Workspace, path: &str, version: i32) {
    workspace
        .handle(Operation::LanguageOpen {
            path: path.into(),
            language_id: "java".into(),
            version,
            text: "class Hello { /* draft is sent only by didOpen */ }".into(),
        })
        .unwrap();
}

fn audit(directory: &Path, workspace: &mut Workspace) -> Vec<Value> {
    // A private fixture barrier proves all preceding notification writes were
    // consumed before inspecting the audit. The shipping refresh never queries.
    workspace
        .language
        .as_ref()
        .unwrap()
        .client
        .request("fixture/barrier", Value::Null)
        .unwrap();
    fs::read_to_string(directory.join("audit.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn close(mut workspace: Workspace) {
    workspace
        .language
        .take()
        .unwrap()
        .client
        .shutdown()
        .unwrap();
}

#[test]
fn refresh_guard_requires_exact_standard_identity_and_verified_version() {
    let supported = initialize();
    assert!(java_diagnostics_refresh_supported(true, &supported));
    assert!(!java_diagnostics_refresh_supported(false, &supported));
    for metadata in [
        Value::Null,
        json!({}),
        json!({"name":"JDT Language Server (Syntax)","version":"1.61.0-SNAPSHOT"}),
        json!({"name":"JDT Language Server","version":"1.61.0-SNAPSHOT"}),
        json!({"name":"JDT Language Server (Standard)"}),
        json!({"version":"1.61.0-SNAPSHOT"}),
        json!({"name":"JDT Language Server (Standard)","version":1.61}),
    ] {
        let mut result = supported.clone();
        result["serverInfo"] = metadata;
        assert!(!java_diagnostics_refresh_supported(true, &result));
    }
    for version in [
        "",
        "1.61.0",
        "1.61.0-SNAPSHOT-extra",
        "1.61.0.202609031315",
        "1.60.0-SNAPSHOT",
        "1.62.0-SNAPSHOT",
    ] {
        let mut result = supported.clone();
        result["serverInfo"]["version"] = json!(version);
        assert!(
            !java_diagnostics_refresh_supported(true, &result),
            "{version}"
        );
    }
}

#[test]
fn refresh_requires_trust_and_a_running_session_without_starting_anything() {
    let directory = tempfile::tempdir().unwrap();
    for backend in [
        cedar_tasks::BackendMode::InProcess,
        cedar_tasks::BackendMode::IsolatedAgent,
    ] {
        let mut workspace = Workspace::with_backend_mode(directory.path(), backend).unwrap();
        assert_eq!(
            workspace.handle(refresh("Hello.java", 1)).unwrap_err().code,
            "run_disabled"
        );
        workspace.set_allow_run(true);
        assert_eq!(
            workspace.handle(refresh("Hello.java", 1)).unwrap_err().code,
            "language_not_running"
        );
        assert!(workspace.language.is_none());
    }
}

#[test]
fn refresh_initialization_flag_is_agent_owned_and_unsupported_sessions_never_notify() {
    let mut unknown = initialize();
    unknown["serverInfo"]["version"] = json!("1.62.0-SNAPSHOT");
    let mut syntax = initialize();
    syntax["serverInfo"]["name"] = json!("JDT Language Server (Syntax)");
    let mut absent = initialize();
    absent.as_object_mut().unwrap().remove("serverInfo");
    for (production, result) in [
        (false, initialize()),
        (true, unknown),
        (true, syntax),
        (true, absent),
    ] {
        let (directory, mut workspace, started) = start(production, result);
        assert_eq!(
            started["initialize"]["cedar_java_diagnostics_refresh"],
            false
        );
        assert!(
            !workspace
                .language
                .as_ref()
                .unwrap()
                .java_diagnostics_refresh
        );
        open(&mut workspace, "Hello.java", 1);
        assert_eq!(
            workspace.handle(refresh("Hello.java", 1)).unwrap_err().code,
            "language_refresh_unsupported"
        );
        assert!(!audit(directory.path(), &mut workspace)
            .iter()
            .any(|message| message["method"] == "java/validateDocument"));
        close(workspace);
    }
}

#[test]
fn refresh_rejects_closed_stale_nonpositive_and_outside_documents_before_any_notification() {
    let (directory, mut workspace, _) = start(true, initialize());
    assert_eq!(
        workspace.handle(refresh("Hello.java", 1)).unwrap_err().code,
        "language_document_closed"
    );
    open(&mut workspace, "Hello.java", 2);
    fs::create_dir(directory.path().join("directory")).unwrap();
    for version in [1, 3, i32::MAX] {
        assert_eq!(
            workspace
                .handle(refresh("Hello.java", version))
                .unwrap_err()
                .code,
            "language_stale_version"
        );
    }
    for version in [i32::MIN, -1, 0] {
        assert_eq!(
            workspace
                .handle(refresh("Hello.java", version))
                .unwrap_err()
                .code,
            "invalid_version"
        );
    }
    for path in [
        "../Hello.java",
        "/Hello.java",
        "",
        ".",
        "a\\b.java",
        "nul\0.java",
        "directory",
        "file:///Hello.java",
    ] {
        assert_eq!(
            workspace.handle(refresh(path, 2)).unwrap_err().code,
            "invalid_path",
            "{path:?}"
        );
    }
    #[cfg(unix)]
    {
        fs::write(directory.path().join("Hello.java"), "class Hello {}").unwrap();
        std::os::unix::fs::symlink("Hello.java", directory.path().join("alias.java")).unwrap();
        assert_eq!(
            workspace.handle(refresh("alias.java", 2)).unwrap_err().code,
            "invalid_path"
        );
    }
    assert_eq!(
        workspace.handle(refresh("Other.java", 2)).unwrap_err().code,
        "language_document_closed"
    );
    workspace.set_allow_run(false);
    assert_eq!(
        workspace.handle(refresh("Hello.java", 2)).unwrap_err().code,
        "run_disabled"
    );
    workspace.set_allow_run(true);
    workspace
        .handle(Operation::LanguageClose {
            path: "Hello.java".into(),
        })
        .unwrap();
    assert_eq!(
        workspace.handle(refresh("Hello.java", 2)).unwrap_err().code,
        "language_document_closed"
    );
    assert!(!audit(directory.path(), &mut workspace)
        .iter()
        .any(|message| message["method"] == "java/validateDocument"));
    close(workspace);
}

#[test]
fn refresh_rejects_non_java_paths_and_non_java_open_language_after_changes() {
    let (directory, mut workspace, _) = start(true, initialize());
    for (path, language_id) in [
        ("Hello.txt", "java"),
        ("Hello.JAVA", "java"),
        ("Hello.java", "plaintext"),
        ("Other.java", "Java"),
    ] {
        workspace
            .handle(Operation::LanguageOpen {
                path: path.into(),
                language_id: language_id.into(),
                version: 1,
                text: "class Hello {}".into(),
            })
            .unwrap();
        workspace
            .handle(Operation::LanguageChange {
                path: path.into(),
                version: 2,
                text: "class Hello { int count; }".into(),
            })
            .unwrap();
        assert_eq!(
            workspace.handle(refresh(path, 2)).unwrap_err().code,
            "invalid_language"
        );
    }
    assert!(!audit(directory.path(), &mut workspace)
        .iter()
        .any(|message| message["method"] == "java/validateDocument"));
    close(workspace);
}

#[test]
fn refresh_writes_exact_uri_only_notification_and_acknowledges_no_diagnostics() {
    let mut result = initialize();
    result["cedar_java_diagnostics_refresh"] = json!(false);
    let (directory, mut workspace, started) = start(true, result);
    assert_eq!(
        started["initialize"]["cedar_java_diagnostics_refresh"],
        true
    );
    let path = "Hello 你好 #.java";
    open(&mut workspace, path, 7);
    workspace
        .handle(Operation::LanguageChange {
            path: path.into(),
            version: 8,
            text: "class Hello { int count; }".into(),
        })
        .unwrap();
    let uri = workspace.language_uri(path).unwrap();
    let Payload::Language { value } = workspace.handle(refresh(path, 8)).unwrap() else {
        panic!("expected refresh response")
    };
    assert_eq!(
        value,
        json!({"diagnostics_refresh_requested":uri,"version":8,"notification_only":true})
    );
    let messages = audit(directory.path(), &mut workspace);
    assert_eq!(
        messages
            .iter()
            .map(|message| message["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "initialize",
            "initialized",
            "textDocument/didOpen",
            "textDocument/didChange",
            "java/validateDocument",
            "fixture/barrier",
        ]
    );
    assert_eq!(
        messages[4],
        json!({"jsonrpc":"2.0","method":"java/validateDocument","params":{"textDocument":{"uri":uri}}})
    );
    assert_eq!(workspace.language.as_ref().unwrap().opened[&uri].version, 8);
    let Payload::Language { value } = workspace.handle(Operation::LanguageEvents).unwrap() else {
        panic!("expected events response")
    };
    assert_eq!(value, json!({"events":[],"truncated":false}));
    // Repeated event reads must not replay validation or fabricate completion.
    workspace.handle(Operation::LanguageEvents).unwrap();
    assert_eq!(
        audit(directory.path(), &mut workspace)
            .iter()
            .filter(|message| message["method"] == "java/validateDocument")
            .count(),
        1
    );
    close(workspace);
}
