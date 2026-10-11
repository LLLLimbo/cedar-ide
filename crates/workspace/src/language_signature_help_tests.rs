//! Real child-pipe checks of the typed Java signature-help bridge. The private
//! fixture bypasses platform launch selection only inside these unit tests.
use super::java_refresh_tests::{audit, close, initialize, open, start};
use super::*;
use std::fs;

fn request(path: &str, version: i32, line: u32, character: u32) -> Operation {
    Operation::LanguageJavaSignatureHelp {
        path: path.into(),
        version,
        line,
        character,
    }
}

fn supported_initialize() -> Value {
    let mut value = initialize();
    value["capabilities"]["signatureHelpProvider"] = json!(true);
    value
}

#[test]
fn signature_help_require_trust_and_existing_session_without_startup() {
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
fn signature_help_require_typed_java_origin_and_actual_static_provider() {
    let (directory, mut workspace, _) = start(false, supported_initialize());
    open(&mut workspace, "Hello.java", 1);
    assert_eq!(
        workspace
            .handle(request("Hello.java", 1, 0, 0))
            .unwrap_err()
            .code,
        "language_signature_help_unsupported"
    );
    assert!(!audit(directory.path(), &mut workspace)
        .iter()
        .any(|message| message["method"] == "textDocument/signatureHelp"));
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
            .remove("signatureHelpProvider");
        if let Some(provider) = provider {
            initialize["capabilities"]["signatureHelpProvider"] = provider;
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
            .any(|message| message["method"] == "textDocument/signatureHelp"));
        close(workspace);
    }
}

#[test]
fn signature_help_reject_bad_source_version_and_position_before_rpc() {
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
        .any(|message| message["method"] == "textDocument/signatureHelp"));
    close(workspace);
}

#[test]
fn signature_help_sends_one_standard_request_and_normalizes_only_inert_subset() {
    for provider in [
        json!(true),
        json!({}),
        json!({"triggerCharacters":["(",","]}),
    ] {
        let mut initialize = supported_initialize();
        initialize["capabilities"]["signatureHelpProvider"] = provider;
        let (directory, mut workspace, _) = start(true, initialize);
        let path = "Hello #雪.java";
        let uri = workspace.language_uri(path).unwrap();
        open(&mut workspace, path, 1);
        workspace
            .handle(Operation::LanguageChange {
                path: path.into(),
                version: 7,
                text: "// 🦀雪\r\nclass Hello {}".into(),
            })
            .unwrap();
        let result = json!({"signatures":[{"label":"f(雪, 🦀)","parameters":[{"label":"雪","documentation":"ignored"},{"label":[5,7]}],"documentation":{"value":"ignored"}}],"activeSignature":0,"activeParameter":1,"command":"ignored"});
        let expected = json!({"signatures":[{"label":"f(雪, 🦀)","parameters":[{"label":"雪"},{"label":[5,7]}]}],"activeSignature":0,"activeParameter":1});
        fs::write(
            directory.path().join("signature-help-result.json"),
            result.to_string(),
        )
        .unwrap();
        let before = fs::read_dir(directory.path()).unwrap().count();
        assert!(
            matches!(workspace.handle(request(path, 7, 1, 10)).unwrap(), Payload::Language { value } if value == expected)
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), before);
        let messages = audit(directory.path(), &mut workspace);
        assert_eq!(
            messages[0]["params"]["capabilities"]["textDocument"]["signatureHelp"],
            json!({"dynamicRegistration":false,"signatureInformation":{"parameterInformation":{"labelOffsetSupport":true},"activeParameterSupport":true}})
        );
        let requests: Vec<_> = messages
            .iter()
            .filter(|message| message["method"] == "textDocument/signatureHelp")
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
                    | "textDocument/signatureHelp"
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
fn signature_help_rejects_whole_malformed_result_without_retry_or_side_effects() {
    let (directory, mut workspace, _) = start(true, supported_initialize());
    open(&mut workspace, "Hello.java", 1);
    let good = json!({"label":"f(x)","parameters":[{"label":[2,3]}]});
    for invalid in [
        json!({"signatures":[good.clone(),null]}),
        json!({"signatures":[{"label":"f(🦀)","parameters":[{"label":[2,3]}]}]}),
        json!({"signatures":[good.clone()],"activeParameter":-1}),
        json!({"signatures":vec![good.clone();33]}),
    ] {
        fs::write(
            directory.path().join("signature-help-result.json"),
            invalid.to_string(),
        )
        .unwrap();
        let error = workspace
            .handle(request("Hello.java", 1, 0, 0))
            .unwrap_err();
        assert_eq!(error.code, "language_error");
        assert!(error.message.contains("signature help result"));
    }
    for (result, expected) in [
        (Value::Null, json!({"signatures":[]})),
        (json!({"signatures":[]}), json!({"signatures":[]})),
        (
            json!({"signatures":[good.clone()],"activeParameter":99}),
            json!({"signatures":[good],"activeParameter":99}),
        ),
    ] {
        fs::write(
            directory.path().join("signature-help-result.json"),
            result.to_string(),
        )
        .unwrap();
        assert!(
            matches!(workspace.handle(request("Hello.java", 1, 0, 0)).unwrap(), Payload::Language { value } if value == expected)
        );
    }
    fs::write(
        directory.path().join("signature-help-error.json"),
        json!({"code":-32001,"message":"index unavailable"}).to_string(),
    )
    .unwrap();
    let error = workspace
        .handle(request("Hello.java", 1, 0, 0))
        .unwrap_err();
    assert_eq!(error.code, "language_error");
    assert!(error.message.contains("index unavailable"));
    let messages = audit(directory.path(), &mut workspace);
    assert_eq!(
        messages
            .iter()
            .filter(|message| message["method"] == "textDocument/signatureHelp")
            .count(),
        8
    );
    for message in messages {
        assert!(matches!(
            message["method"].as_str().unwrap(),
            "initialize"
                | "initialized"
                | "textDocument/didOpen"
                | "textDocument/signatureHelp"
                | "fixture/barrier"
        ));
    }
    assert_eq!(workspace.language.as_ref().unwrap().opened.len(), 1);
    close(workspace);
}
