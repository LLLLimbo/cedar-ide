//! Workspace routing uses the real stdio peer; no document is opened by search.
use super::java_refresh_tests::{audit, close, initialize, start};
use super::*;
use std::fs;

fn symbols(query: &str) -> Operation {
    Operation::LanguageWorkspaceSymbols {
        query: query.into(),
    }
}

fn supported_initialize() -> Value {
    let mut value = initialize();
    value["capabilities"]["workspaceSymbolProvider"] = json!(true);
    value
}

fn symbol(uri: &str) -> Value {
    json!({"name":"Type","kind":5,"containerName":"demo","location":{"uri":uri,
        "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":4}}}})
}

#[test]
fn workspace_symbols_require_trust_and_session_without_autostart_in_both_host_modes() {
    let root = tempfile::tempdir().unwrap();
    for backend in [
        cedar_tasks::BackendMode::InProcess,
        cedar_tasks::BackendMode::IsolatedAgent,
    ] {
        let mut workspace = Workspace::with_backend_mode(root.path(), backend).unwrap();
        assert_eq!(
            workspace.handle(symbols("Type")).unwrap_err().code,
            "run_disabled"
        );
        workspace.set_allow_run(true);
        assert_eq!(
            workspace.handle(symbols("Type")).unwrap_err().code,
            "language_not_running"
        );
        assert!(workspace.language.is_none());
        assert!(workspace.tasks.is_none());
    }
}

#[test]
fn workspace_symbols_route_literal_query_in_generic_and_typed_java_sessions() {
    for production_java in [false, true] {
        let (directory, mut workspace, _) = start(production_java, supported_initialize());
        let uri = workspace.language_uri("Type.java").unwrap();
        let result = json!([symbol(&uri)]);
        fs::write(
            directory.path().join("symbols-result.json"),
            result.to_string(),
        )
        .unwrap();
        let before = fs::read_dir(directory.path()).unwrap().count();
        let Payload::Language { value } = workspace.handle(symbols(" 你好.Type* ")).unwrap()
        else {
            panic!("expected language result");
        };
        assert_eq!(value, result);
        assert!(workspace.language.as_ref().unwrap().opened.is_empty());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), before);
        let requests = audit(directory.path(), &mut workspace);
        assert_eq!(
            requests
                .iter()
                .filter(|message| message["method"] == "workspace/symbol")
                .count(),
            1
        );
        let request = requests
            .iter()
            .find(|message| message["method"] == "workspace/symbol")
            .unwrap();
        assert_eq!(request["params"], json!({"query":" 你好.Type* "}));
        for message in requests {
            assert!(matches!(
                message["method"].as_str().unwrap(),
                "initialize" | "initialized" | "workspace/symbol" | "fixture/barrier"
            ));
        }
        workspace.set_allow_run(false);
        assert_eq!(
            workspace.handle(symbols("Type")).unwrap_err().code,
            "run_disabled"
        );
        close(workspace);
    }
}

#[test]
fn workspace_symbols_require_actual_provider_and_validate_before_and_after_rpc() {
    for provider in [Value::Null, json!(false), json!("true"), json!([])] {
        let mut initialize = supported_initialize();
        initialize["capabilities"]["workspaceSymbolProvider"] = provider;
        let (directory, mut workspace, _) = start(true, initialize);
        assert_eq!(
            workspace.handle(symbols("Type")).unwrap_err().code,
            "language_error"
        );
        assert!(!audit(directory.path(), &mut workspace)
            .iter()
            .any(|message| message["method"] == "workspace/symbol"));
        close(workspace);
    }
    let (directory, mut workspace, _) = start(true, supported_initialize());
    for query in [
        String::new(),
        "   ".into(),
        "\u{2003}".into(),
        "x".repeat(257),
        "🦀".repeat(65),
        "Type\n".into(),
    ] {
        assert_eq!(
            workspace.handle(symbols(&query)).unwrap_err().code,
            "language_error"
        );
    }
    assert!(!audit(directory.path(), &mut workspace)
        .iter()
        .any(|message| message["method"] == "workspace/symbol"));
    for result in [
        json!([symbol("file:///a.java"), {"name":"Lazy","kind":5,"location":{"uri":"file:///b.java"}}]),
        json!(vec![symbol("file:///a.java"); 257]),
        json!([{"name":"Type","kind":5,"location":{"uri":"file:///a.java","range":null}}]),
    ] {
        fs::write(
            directory.path().join("symbols-result.json"),
            result.to_string(),
        )
        .unwrap();
        let error = workspace.handle(symbols("Type")).unwrap_err();
        assert_eq!(error.code, "language_error");
        assert!(error.message.contains("workspace symbol result"));
    }
    fs::write(directory.path().join("symbols-result.json"), "null").unwrap();
    assert!(
        matches!(workspace.handle(symbols("Type")).unwrap(), Payload::Language { value } if value.is_null())
    );
    close(workspace);
}

#[test]
fn symbol_uris_require_separate_authoritative_resolution_and_read() {
    let (directory, mut workspace, _) = start(true, supported_initialize());
    fs::write(directory.path().join("Type.java"), "class Type {}\n").unwrap();
    let local_uri = workspace.language_uri("Type.java").unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("Other.java"), "private outside content").unwrap();
    let outside_uri = url::Url::from_file_path(outside.path().join("Other.java"))
        .unwrap()
        .to_string();
    let result = json!([
        symbol(&local_uri),
        symbol(&outside_uri),
        symbol("jdt://contents/lib/Type.class")
    ]);
    fs::write(
        directory.path().join("symbols-result.json"),
        result.to_string(),
    )
    .unwrap();
    assert!(
        matches!(workspace.handle(symbols("Type")).unwrap(), Payload::Language { value } if value == result)
    );
    assert!(workspace.language.as_ref().unwrap().opened.is_empty());
    assert_eq!(
        workspace
            .handle(Operation::LanguageResolveUri { uri: outside_uri })
            .unwrap_err()
            .code,
        "invalid_path"
    );
    assert_eq!(
        workspace
            .handle(Operation::LanguageResolveUri {
                uri: "jdt://contents/lib/Type.class".into()
            })
            .unwrap_err()
            .code,
        "unsupported_uri"
    );
    assert!(
        matches!(workspace.handle(Operation::LanguageResolveUri { uri: local_uri }).unwrap(), Payload::Language { value } if value["path"] == "Type.java")
    );
    assert!(
        matches!(workspace.handle(Operation::Read { path: "Type.java".into() }).unwrap(), Payload::File { text, .. } if text == "class Type {}\n")
    );
    let requests = audit(directory.path(), &mut workspace);
    assert!(!requests.iter().any(|message| matches!(
        message["method"].as_str(),
        Some(
            "java/searchSymbols"
                | "workspaceSymbol/resolve"
                | "workspace/executeCommand"
                | "java/classFileContents"
                | "textDocument/didOpen"
        )
    )));
    close(workspace);
}
