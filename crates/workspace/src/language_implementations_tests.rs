//! Real child-pipe checks of the typed Java implementation bridge. The private
//! fixture bypasses platform launch selection only inside these unit tests.
use super::java_refresh_tests::{audit, close, initialize, open, start};
use super::*;
use std::fs;

fn request(path: &str, version: i32, line: u32, character: u32) -> Operation {
    Operation::LanguageJavaImplementations {
        path: path.into(),
        version,
        line,
        character,
    }
}

fn supported_initialize() -> Value {
    let mut value = initialize();
    value["capabilities"]["implementationProvider"] = json!(true);
    value
}

fn location(uri: &str) -> Value {
    json!({"uri":uri,"range":{"start":{"line":0,"character":6},"end":{"line":0,"character":11}}})
}

#[test]
fn implementations_require_trust_and_existing_session_without_startup() {
    let root = tempfile::tempdir().unwrap();
    for backend in [
        cedar_tasks::BackendMode::InProcess,
        cedar_tasks::BackendMode::IsolatedAgent,
    ] {
        let mut workspace = Workspace::with_backend_mode(root.path(), backend).unwrap();
        assert_eq!(
            workspace
                .handle(request("Hello.java", 1, 0, 0))
                .unwrap_err()
                .code,
            "run_disabled"
        );
        workspace.set_allow_run(true);
        assert_eq!(
            workspace
                .handle(request("Hello.java", 1, 0, 0))
                .unwrap_err()
                .code,
            "language_not_running"
        );
        assert!(workspace.language.is_none());
        assert!(workspace.tasks.is_none());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[test]
fn implementations_require_typed_java_origin_and_actual_static_provider() {
    let (directory, mut workspace, _) = start(false, supported_initialize());
    open(&mut workspace, "Hello.java", 1);
    assert_eq!(
        workspace
            .handle(request("Hello.java", 1, 0, 0))
            .unwrap_err()
            .code,
        "language_implementations_unsupported"
    );
    assert!(!audit(directory.path(), &mut workspace)
        .iter()
        .any(|message| message["method"] == "textDocument/implementation"));
    close(workspace);
    for provider in [
        None,
        Some(Value::Null),
        Some(json!(false)),
        Some(json!("true")),
        Some(json!([])),
        Some(json!(1)),
    ] {
        let mut initialize = supported_initialize();
        initialize["capabilities"]
            .as_object_mut()
            .unwrap()
            .remove("implementationProvider");
        if let Some(provider) = provider {
            initialize["capabilities"]["implementationProvider"] = provider;
        }
        let (directory, mut workspace, _) = start(true, initialize);
        open(&mut workspace, "Hello.java", 1);
        assert_eq!(
            workspace
                .handle(request("Hello.java", 1, 0, 0))
                .unwrap_err()
                .code,
            "language_error"
        );
        assert!(!audit(directory.path(), &mut workspace)
            .iter()
            .any(|message| message["method"] == "textDocument/implementation"));
        close(workspace);
    }
}

#[test]
fn implementations_reject_bad_source_version_and_position_before_rpc() {
    let (directory, mut workspace, _) = start(true, supported_initialize());
    assert_eq!(
        workspace
            .handle(request("Hello.java", 1, 0, 0))
            .unwrap_err()
            .code,
        "language_document_closed"
    );
    for path in ["../outside.java", "/absolute.java", ""] {
        assert_eq!(
            workspace.handle(request(path, 1, 0, 0)).unwrap_err().code,
            "invalid_path"
        );
    }
    open(&mut workspace, "Hello.java", 7);
    for version in [-1, 0] {
        assert_eq!(
            workspace
                .handle(request("Hello.java", version, 0, 0))
                .unwrap_err()
                .code,
            "invalid_version"
        );
    }
    for version in [1, 6, 8, i32::MAX] {
        assert_eq!(
            workspace
                .handle(request("Hello.java", version, 0, 0))
                .unwrap_err()
                .code,
            "language_stale_version"
        );
    }
    for (path, language_id) in [("Hello.txt", "java"), ("Other.java", "plaintext")] {
        workspace
            .handle(Operation::LanguageOpen {
                path: path.into(),
                language_id: language_id.into(),
                version: 7,
                text: "class Hello {}".into(),
            })
            .unwrap();
        assert_eq!(
            workspace.handle(request(path, 7, 0, 0)).unwrap_err().code,
            "invalid_language"
        );
    }
    for (line, character) in [(i32::MAX as u32 + 1, 0), (0, u32::MAX)] {
        assert_eq!(
            workspace
                .handle(request("Hello.java", 7, line, character))
                .unwrap_err()
                .code,
            "language_error"
        );
    }
    workspace
        .handle(Operation::LanguageClose {
            path: "Hello.java".into(),
        })
        .unwrap();
    assert_eq!(
        workspace
            .handle(request("Hello.java", 7, 0, 0))
            .unwrap_err()
            .code,
        "language_document_closed"
    );
    assert!(!audit(directory.path(), &mut workspace)
        .iter()
        .any(|message| message["method"] == "textDocument/implementation"));
    close(workspace);
}

#[test]
fn implementations_send_only_one_standard_request_at_acknowledged_version() {
    for provider in [json!(true), json!({"workDoneProgress":false})] {
        let mut initialize = supported_initialize();
        initialize["capabilities"]["implementationProvider"] = provider;
        let (directory, mut workspace, _) = start(true, initialize);
        let path = "Hello #雪.java";
        let uri = workspace.language_uri(path).unwrap();
        workspace
            .handle(Operation::LanguageOpen {
                path: path.into(),
                language_id: "java".into(),
                version: 1,
                text: "// 🦀\r\ninterface Hello {}".into(),
            })
            .unwrap();
        workspace
            .handle(Operation::LanguageChange {
                path: path.into(),
                version: 7,
                text: "// 🦀雪\r\ninterface Hello {}".into(),
            })
            .unwrap();
        let expected = json!([location(&uri)]);
        fs::write(
            directory.path().join("implementations-result.json"),
            expected.to_string(),
        )
        .unwrap();
        let before = fs::read_dir(directory.path()).unwrap().count();
        assert!(
            matches!(workspace.handle(request(path, 7, 1, 10)).unwrap(), Payload::Language { value } if value == expected)
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), before);
        let messages = audit(directory.path(), &mut workspace);
        assert_eq!(
            messages[0]["params"]["capabilities"]["textDocument"]["implementation"],
            json!({"dynamicRegistration":false,"linkSupport":false})
        );
        let requests: Vec<_> = messages
            .iter()
            .filter(|message| message["method"] == "textDocument/implementation")
            .collect();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]["params"],
            json!({"textDocument":{"uri":uri},"position":{"line":1,"character":10}})
        );
        assert_eq!(workspace.language.as_ref().unwrap().opened.len(), 1);
        for message in messages {
            assert!(matches!(
                message["method"].as_str().unwrap(),
                "initialize"
                    | "initialized"
                    | "textDocument/didOpen"
                    | "textDocument/didChange"
                    | "textDocument/implementation"
                    | "fixture/barrier"
            ));
        }
        workspace.set_allow_run(false);
        assert_eq!(
            workspace.handle(request(path, 7, 1, 10)).unwrap_err().code,
            "run_disabled"
        );
        close(workspace);
    }
}

#[test]
fn implementations_reject_whole_invalid_result_and_never_retry_or_open_targets() {
    let (directory, mut workspace, _) = start(true, supported_initialize());
    open(&mut workspace, "Hello.java", 1);
    let good = location("file:///outside/Target.java");
    for invalid in [
        json!([good.clone(), {"uri":"file:///outside/Other.java"}]),
        json!({"targetUri":"file:///outside/Target.java","targetRange":good["range"],"targetSelectionRange":good["range"]}),
        json!([good.clone(), {"kind":"create","uri":"file:///outside/Target.java"}]),
        json!(vec![good.clone(); 1025]),
    ] {
        fs::write(
            directory.path().join("implementations-result.json"),
            invalid.to_string(),
        )
        .unwrap();
        let error = workspace
            .handle(request("Hello.java", 1, 0, 0))
            .unwrap_err();
        assert_eq!(error.code, "language_error");
        assert!(error.message.contains("implementation result"));
    }
    for (result, expected) in [
        (Value::Null, json!([])),
        (good.clone(), json!([good.clone()])),
        (
            json!([good.clone(), location("jdt://contents/lib/Target.class")]),
            json!([good, location("jdt://contents/lib/Target.class")]),
        ),
    ] {
        fs::write(
            directory.path().join("implementations-result.json"),
            result.to_string(),
        )
        .unwrap();
        assert!(
            matches!(workspace.handle(request("Hello.java", 1, 0, 0)).unwrap(), Payload::Language { value } if value == expected)
        );
    }
    fs::write(
        directory.path().join("implementations-error.json"),
        json!({"code":-32001,"message":"index unavailable"}).to_string(),
    )
    .unwrap();
    let error = workspace
        .handle(request("Hello.java", 1, 0, 0))
        .unwrap_err();
    assert_eq!(error.code, "language_error");
    assert!(error.message.contains("index unavailable"));
    assert_eq!(workspace.language.as_ref().unwrap().opened.len(), 1);
    let messages = audit(directory.path(), &mut workspace);
    assert_eq!(
        messages
            .iter()
            .filter(|message| message["method"] == "textDocument/implementation")
            .count(),
        8
    );
    for message in messages {
        assert!(matches!(
            message["method"].as_str().unwrap(),
            "initialize"
                | "initialized"
                | "textDocument/didOpen"
                | "textDocument/implementation"
                | "fixture/barrier"
        ));
    }
    close(workspace);
}

#[cfg(windows)]
#[test]
fn normal_windows_generic_start_cannot_create_an_implementation_session() {
    let root = tempfile::tempdir().unwrap();
    let mut workspace =
        Workspace::with_backend_mode(root.path(), cedar_tasks::BackendMode::IsolatedAgent).unwrap();
    workspace.set_allow_run(true);
    assert_eq!(
        workspace
            .handle(Operation::LanguageStart {
                program: "never-start".into(),
                args: vec![]
            })
            .unwrap_err()
            .code,
        "unsupported_platform"
    );
    assert_eq!(
        workspace
            .handle(request("Hello.java", 1, 0, 0))
            .unwrap_err()
            .code,
        "language_not_running"
    );
    assert!(workspace.language.is_none());
}
