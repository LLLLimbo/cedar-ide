use super::super::java_refresh_tests::{audit, close, initialize, open, start};
use super::*;
use cedar_protocol::Operation;
use std::fs;

fn organize(path: &str, version: i32) -> Operation {
    Operation::LanguageOrganizeJavaImports {
        path: path.into(),
        version,
    }
}

fn edit() -> Value {
    json!({"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},
        "newText":"import java.util.List;\r\n"})
}

#[test]
fn imports_require_exact_advertised_command_and_audited_typed_java_identity() {
    let supported_initialize = initialize();
    assert!(supported(true, &supported_initialize));
    assert!(!supported(false, &supported_initialize));
    for server in [
        Value::Null,
        json!({}),
        json!({"name":"JDT Language Server (Syntax)","version":"1.61.0-SNAPSHOT"}),
        json!({"name":"JDT Language Server (Standard)","version":"1.61.0"}),
        json!({"name":"JDT Language Server (Standard)","version":"1.62.0-SNAPSHOT"}),
    ] {
        let mut value = supported_initialize.clone();
        value["serverInfo"] = server;
        assert!(!supported(true, &value));
    }
    for commands in [
        Value::Null,
        json!([]),
        json!(COMMAND),
        json!([true, COMMAND]),
        json!(["java/organizeImports"]),
        json!(["java.edit.organizeImports "]),
        json!(["Java.edit.organizeImports"]),
        json!(["java.edit.organizeImports.other"]),
    ] {
        let mut value = supported_initialize.clone();
        value["capabilities"]["executeCommandProvider"]["commands"] = commands;
        assert!(!supported(true, &value));
    }
    let mut value = supported_initialize;
    value["capabilities"]["executeCommandProvider"] = json!({});
    value["capabilities"]["experimental"] = json!({"java.edit.organizeImports":true});
    assert!(!supported(true, &value));
}

#[test]
fn imports_require_trust_and_running_session_without_starting_anything() {
    let directory = tempfile::tempdir().unwrap();
    for backend in [
        cedar_tasks::BackendMode::InProcess,
        cedar_tasks::BackendMode::IsolatedAgent,
    ] {
        let mut workspace = Workspace::with_backend_mode(directory.path(), backend).unwrap();
        assert_eq!(
            workspace
                .handle(organize("Hello.java", 1))
                .unwrap_err()
                .code,
            "run_disabled"
        );
        workspace.set_allow_run(true);
        assert_eq!(
            workspace
                .handle(organize("Hello.java", 1))
                .unwrap_err()
                .code,
            "language_not_running"
        );
        assert!(workspace.language.is_none());
    }
}

#[test]
fn imports_initialization_flag_is_agent_owned_and_unsupported_sessions_never_execute() {
    let mut unknown = initialize();
    unknown["serverInfo"]["version"] = json!("1.62.0-SNAPSHOT");
    let mut unadvertised = initialize();
    unadvertised["capabilities"]["executeCommandProvider"] = Value::Null;
    let mut syntax = initialize();
    syntax["serverInfo"]["name"] = json!("JDT Language Server (Syntax)");
    for (production, result) in [
        (false, initialize()),
        (true, unknown),
        (true, unadvertised),
        (true, syntax),
    ] {
        let (directory, mut workspace, started) = start(production, result);
        assert_eq!(started["initialize"]["cedar_java_organize_imports"], false);
        assert!(!workspace.language.as_ref().unwrap().java_organize_imports);
        open(&mut workspace, "Hello.java", 1);
        assert_eq!(
            workspace
                .handle(organize("Hello.java", 1))
                .unwrap_err()
                .code,
            "language_organize_imports_unsupported"
        );
        assert!(!audit(directory.path(), &mut workspace)
            .iter()
            .any(|message| message["method"] == "workspace/executeCommand"));
        close(workspace);
    }
}

#[test]
fn imports_reject_closed_stale_nonpositive_and_outside_documents_before_execute() {
    let (directory, mut workspace, _) = start(true, initialize());
    assert_eq!(
        workspace
            .handle(organize("Hello.java", 1))
            .unwrap_err()
            .code,
        "language_document_closed"
    );
    open(&mut workspace, "Hello.java", 2);
    fs::create_dir(directory.path().join("directory")).unwrap();
    for version in [1, 3, i32::MAX] {
        assert_eq!(
            workspace
                .handle(organize("Hello.java", version))
                .unwrap_err()
                .code,
            "language_stale_version"
        );
    }
    for version in [i32::MIN, -1, 0] {
        assert_eq!(
            workspace
                .handle(organize("Hello.java", version))
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
            workspace.handle(organize(path, 2)).unwrap_err().code,
            "invalid_path",
            "{path:?}"
        );
    }
    #[cfg(unix)]
    {
        fs::write(directory.path().join("Hello.java"), "class Hello {}").unwrap();
        std::os::unix::fs::symlink("Hello.java", directory.path().join("alias.java")).unwrap();
        assert_eq!(
            workspace
                .handle(organize("alias.java", 2))
                .unwrap_err()
                .code,
            "invalid_path"
        );
    }
    assert_eq!(
        workspace
            .handle(organize("Other.java", 2))
            .unwrap_err()
            .code,
        "language_document_closed"
    );
    workspace.set_allow_run(false);
    assert_eq!(
        workspace
            .handle(organize("Hello.java", 2))
            .unwrap_err()
            .code,
        "run_disabled"
    );
    workspace.set_allow_run(true);
    workspace
        .handle(Operation::LanguageClose {
            path: "Hello.java".into(),
        })
        .unwrap();
    assert_eq!(
        workspace
            .handle(organize("Hello.java", 2))
            .unwrap_err()
            .code,
        "language_document_closed"
    );
    assert!(!audit(directory.path(), &mut workspace)
        .iter()
        .any(|message| message["method"] == "workspace/executeCommand"));
    close(workspace);
}

#[test]
fn imports_reject_non_java_open_documents_even_after_synchronized_changes() {
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
            workspace.handle(organize(path, 2)).unwrap_err().code,
            "invalid_language"
        );
    }
    assert!(!audit(directory.path(), &mut workspace)
        .iter()
        .any(|message| message["method"] == "workspace/executeCommand"));
    close(workspace);
}

#[test]
fn imports_audit_fixed_command_single_generated_uri_and_reject_server_application() {
    let mut initialization = initialize();
    initialization["cedar_java_organize_imports"] = json!(false);
    let (directory, mut workspace, started) = start(true, initialization);
    assert_eq!(started["initialize"]["cedar_java_organize_imports"], true);
    let path = "Hello 你好 #.java";
    let disk = "class Hello { /* unchanged disk */ }";
    fs::write(directory.path().join(path), disk).unwrap();
    open(&mut workspace, path, 7);
    workspace
        .handle(Operation::LanguageChange {
            path: path.into(),
            version: 8,
            text: "class Hello { List<String> values; }".into(),
        })
        .unwrap();
    let uri = workspace.language_uri(path).unwrap();
    let jdt_uri = uri.replacen("file:///", "file:/", 1);
    let edits = json!([edit()]);
    fs::write(
        directory.path().join("organize-result.json"),
        json!({"changes":{jdt_uri:edits}}).to_string(),
    )
    .unwrap();
    fs::write(directory.path().join("server-apply-edit.json"), json!({"jsonrpc":"2.0","id":9001,"method":"workspace/applyEdit","params":{"edit":{"changes":{uri.clone():[edit()]}}}}).to_string()).unwrap();
    let Payload::Language { value } = workspace.handle(organize(path, 8)).unwrap() else {
        panic!("language payload required")
    };
    assert_eq!(value, edits);
    let messages = audit(directory.path(), &mut workspace);
    assert_eq!(
        messages[0]["params"]["capabilities"]["workspace"]["applyEdit"],
        false
    );
    let commands: Vec<_> = messages
        .iter()
        .filter(|message| message["method"] == "workspace/executeCommand")
        .collect();
    assert_eq!(commands.len(), 1);
    let command = commands[0];
    assert_eq!(
        command["params"],
        json!({"command":COMMAND,"arguments":[uri]})
    );
    assert!(command["id"].is_u64());
    assert!(messages
        .iter()
        .any(|message| message["id"] == 9001 && message["error"]["code"] == -32601));
    assert!(!messages.iter().any(|message| matches!(
        message["method"].as_str(),
        Some("textDocument/didSave" | "workspace/applyEdit")
    )));
    assert_eq!(
        messages
            .iter()
            .filter(|message| message["method"] == "textDocument/didChange")
            .count(),
        1
    );
    assert_eq!(workspace.language.as_ref().unwrap().opened[&uri].version, 8);
    assert_eq!(
        fs::read_to_string(directory.path().join(path)).unwrap(),
        disk
    );
    let Payload::Language { value } = workspace.handle(Operation::LanguageEvents).unwrap() else {
        panic!("events required")
    };
    assert!(value["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["type"] == "unsupported_server_request"
            && event["method"] == "workspace/applyEdit"));
    workspace.handle(Operation::LanguageEvents).unwrap();
    assert_eq!(
        audit(directory.path(), &mut workspace)
            .iter()
            .filter(|message| message["method"] == "workspace/executeCommand")
            .count(),
        1
    );
    close(workspace);
}

#[test]
fn imports_invalid_results_and_server_errors_are_visible_without_partial_edits_or_retry() {
    let (directory, mut workspace, _) = start(true, initialize());
    open(&mut workspace, "Hello.java", 1);
    let uri = workspace.language_uri("Hello.java").unwrap();
    // A valid first edit cannot escape a malformed later edit or extra document.
    for value in [
        json!({"changes":{uri.clone():[edit(), {"newText":"missing range"}]}}),
        json!({"changes":{uri.clone():[edit()], "file:///elsewhere/Other.java":[]}}),
        Value::Null,
    ] {
        fs::write(
            directory.path().join("organize-result.json"),
            value.to_string(),
        )
        .unwrap();
        assert_eq!(
            workspace
                .handle(organize("Hello.java", 1))
                .unwrap_err()
                .code,
            "language_imports_invalid_edit"
        );
    }
    fs::write(
        directory.path().join("organize-error.json"),
        json!({"code":-32603,"message":"fixture command refused"}).to_string(),
    )
    .unwrap();
    let error = workspace.handle(organize("Hello.java", 1)).unwrap_err();
    assert_eq!(error.code, "language_error");
    assert!(error.message.contains("fixture command refused"));
    assert_eq!(
        audit(directory.path(), &mut workspace)
            .iter()
            .filter(|message| message["method"] == "workspace/executeCommand")
            .count(),
        4
    );
    assert!(!directory.path().join("Hello.java").exists());
    close(workspace);
}

#[test]
fn imports_normalize_only_audited_empty_or_current_document_plain_edits() {
    let uri = "file:///C:/work%20%E9%9B%AA/Hello%20%23.java";
    for value in [
        json!({}),
        json!({"changes":{}}),
        json!({"changes":{uri:[]}}),
    ] {
        assert_eq!(
            normalize_workspace_edit(value, uri, true).unwrap(),
            json!([])
        );
    }
    assert_eq!(
        normalize_workspace_edit(
            json!({"changes":{"file:/c:/work%20雪/Hello%20%23.java":[edit()]}}),
            uri,
            true
        )
        .unwrap(),
        json!([edit()])
    );
    for value in [
        Value::Null,
        json!([]),
        json!(true),
        json!({"changes":null}),
        json!({"changes":[]}),
        json!({"documentChanges":[]}),
        json!({"changes":{},"documentChanges":null}),
        json!({"changes":{},"changeAnnotations":{}}),
        json!({"changes":{},"commands":[]}),
        json!({"changes":{},"command":"anything"}),
        json!({"changes":{},"unexpected":true}),
        json!({"changes":{uri:null}}),
        json!({"changes":{uri:{}}}),
        json!({"changes":{uri:[],"file:///C:/work/Other.java":[]}}),
        json!({"changes":{"file:///C:/elsewhere/Other.java":[]}}),
    ] {
        assert!(
            normalize_workspace_edit(value.clone(), uri, true).is_err(),
            "{value}"
        );
    }
}

#[test]
fn imports_uri_authorization_is_current_only_and_never_normalizes_traversal() {
    let expected = "file:///C:/work%20%E9%9B%AA/Hello%20%23.java";
    for actual in [
        expected,
        "file:/C:/work%20雪/Hello%20%23.java",
        "file:///c:/work%20%e9%9b%aa/Hello%20%23.java",
    ] {
        assert!(current_document_uri(actual, expected, true), "{actual}");
    }
    for actual in [
        "file:///C:/work%20%E9%9B%AA/hello%20%23.java", // No path case folding.
        "file:/C:/work 雪/Hello%20%23.java",            // ASCII spaces must be escaped.
        "file:///D:/work%20%E9%9B%AA/Hello%20%23.java",
        "file://host/C:/work%20%E9%9B%AA/Hello%20%23.java",
        "file://localhost/C:/work%20%E9%9B%AA/Hello%20%23.java",
        "file:////C:/work%20%E9%9B%AA/Hello%20%23.java",
        "file:C:/work%20%E9%9B%AA/Hello%20%23.java",
        "FILE:///C:/work%20%E9%9B%AA/Hello%20%23.java",
        "file:///C:/work%20%E9%9B%AA/Hello%20%23.java?x",
        "file:///C:/work%20%E9%9B%AA/Hello%20%23.java#x",
        "file:///C:/work%20%E9%9B%AA/Hello%20%23.java%00",
        "file:///C:/work%20%E9%9B%AA/Hello%20%23.java%0a",
        "file:///C:/work%20%E9%9B%AA/Hello%20%23.java%C2%85",
        "file:///C:/work%20%E9%9B%AA/Hello%20%23.java\0",
        "file:///C:/work%20%E9%9B%AA/Hello%20%23.java%ZZ",
        "file:///C:/work%20%E9%9B%AA/Hello%20%23.java%",
        "file:///C:/work%20%E9%9B%AA/Hello%20%23.java%FF",
        "file:///C:/work%20%E9%9B%AA%2fHello%20%23.java",
        "file:///C:/work%20%E9%9B%AA%5cHello%20%23.java",
        "file:///C:/work%20%E9%9B%AA\\Hello%20%23.java",
        "file:///C:/work%20%E9%9B%AA/other/../Hello%20%23.java",
        "file:///C:/work%20%E9%9B%AA/other/%2e%2e/Hello%20%23.java",
        "file:///C:/work%20%E9%9B%AA/./Hello%20%23.java",
        "file:///C:/work%20%E9%9B%AA//Hello%20%23.java",
        "https:///C:/work%20%E9%9B%AA/Hello%20%23.java",
        "file:///C:/work%20%E9%9B%AA/Hello%20%23.java:stream",
        "file:///C:/work%20%E9%9B%AA/Hello%20%23.java.",
        "file:///C:/work%20%E9%9B%AA/Hello%20%23.java%20",
    ] {
        assert!(!current_document_uri(actual, expected, true), "{actual}");
    }
    assert!(!current_document_uri(
        "file:///c:/work%20雪/Hello%20%23.java",
        expected,
        false
    ));
    assert!(current_document_uri(
        "file:/work%20雪/Hello.java",
        "file:///work%20%E9%9B%AA/Hello.java",
        false
    ));
    assert!(!current_document_uri(
        &format!("file:///{}", "x".repeat(MAX_URI_BYTES)),
        expected,
        true
    ));
    for invalid in [
        "file:///C:/NUL.java",
        "file:///C:/COM1.java",
        "file:///C|/Hello.java",
        "file:///Device/Hello.java",
    ] {
        assert!(!current_document_uri(invalid, invalid, true));
    }
}

#[test]
fn imports_plain_edit_schema_and_aggregate_limits_are_strict() {
    let uri = "file:///work/Hello.java";
    let mut annotated = edit();
    annotated["annotationId"] = json!("a");
    let mut command = edit();
    command["command"] = json!({});
    let mut extra_range = edit();
    extra_range["range"]["unknown"] = json!(true);
    let mut extra_position = edit();
    extra_position["range"]["start"]["unknown"] = json!(true);
    let mut reversed = edit();
    reversed["range"]["start"]["character"] = json!(1);
    let mut absent = edit();
    absent.as_object_mut().unwrap().remove("newText");
    let mut non_text = edit();
    non_text["newText"] = json!(1);
    let mut nul = edit();
    nul["newText"] = json!("\0");
    let mut invalid = vec![
        annotated,
        command,
        extra_range,
        extra_position,
        reversed,
        absent,
        non_text,
        nul,
        Value::Null,
    ];
    for number in [
        json!(-1),
        json!(2147483648_u64),
        json!(0.5),
        json!("0"),
        Value::Null,
    ] {
        let mut value = edit();
        value["range"]["start"]["line"] = number;
        invalid.push(value);
    }
    for value in invalid {
        assert!(
            normalize_workspace_edit(json!({"changes":{uri:[edit(),value]}}), uri, false).is_err()
        );
    }
    assert!(normalize_workspace_edit(
        json!({"changes":{uri:vec![edit();MAX_EDITS+1]}}),
        uri,
        false
    )
    .is_err());
    let mut large = edit();
    large["newText"] = json!("a".repeat(MAX_FILE_BYTES));
    assert!(normalize_workspace_edit(json!({"changes":{uri:[large.clone()]}}), uri, false).is_ok());
    assert!(normalize_workspace_edit(json!({"changes":{uri:[large,edit()]}}), uri, false).is_err());
}
