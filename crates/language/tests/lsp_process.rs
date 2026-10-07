#![cfg(feature = "test-server")]

use cedar_language::{
    ClientOptions, Error, LspClient, LspEvent, Position, ProcessConfig, RpcEvent, StdioRpc,
};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn config(mode: &str, audit: Option<&Path>) -> ProcessConfig {
    let mut config = ProcessConfig::new(env!("CARGO_BIN_EXE_cedar-mock-lsp"));
    config.args.push(mode.into());
    if let Some(path) = audit {
        config.args.push(path.into());
    }
    config
}

fn options() -> ClientOptions {
    ClientOptions {
        request_timeout: Duration::from_secs(2),
        shutdown_timeout: Duration::from_millis(200),
        ..ClientOptions::default()
    }
}

fn ready(mode: &str, audit: Option<&Path>) -> LspClient {
    let client = LspClient::spawn(config(mode, audit), options()).unwrap();
    client
        .initialize(Some("file:///mock/workspace"), json!({}))
        .unwrap();
    client
}

#[test]
fn complete_lifecycle_features_and_full_document_sync() {
    let temp = tempfile::tempdir().unwrap();
    let audit_path = temp.path().join("audit.jsonl");
    let client = LspClient::spawn(config("normal", Some(&audit_path)), options()).unwrap();
    assert!(matches!(
        client.hover("file:///a", Position::default()),
        Err(Error::InvalidState(_))
    ));
    client
        .initialize(Some("file:///mock/workspace"), json!({"custom":true}))
        .unwrap();
    assert!(client.initialize(None, Value::Null).is_err());
    let uri = "file:///mock/workspace/Main.java";
    client
        .did_open(uri, "java", 1, "class Main { 🦀 }")
        .unwrap();
    assert!(client.did_open(uri, "java", 1, "").is_err());
    match client.next_event(Duration::from_secs(1)).unwrap().unwrap() {
        LspEvent::Diagnostics(diagnostics) => {
            assert_eq!(diagnostics.uri, uri);
            assert_eq!(diagnostics.version, Some(1));
            assert_eq!(diagnostics.diagnostics[0].message, "mock diagnostic");
        }
        event => panic!("unexpected event {event:?}"),
    }
    client.did_change(uri, 2, "class Main {}").unwrap();
    assert!(client.did_change(uri, 2, "stale").is_err());
    assert_eq!(
        client.completion(uri, Position::default()).unwrap()["items"][0]["label"],
        "hello"
    );
    assert_eq!(
        client.definition(uri, Position::default()).unwrap()["uri"],
        uri
    );
    assert_eq!(
        client.hover(uri, Position::default()).unwrap()["contents"]["value"],
        "mock hover"
    );
    client.did_close(uri).unwrap();
    assert!(client.did_change(uri, 3, "closed").is_err());
    client.shutdown().unwrap();
    client.shutdown().unwrap();
    assert!(client.request("mock/echo", json!({})).is_err());
    let audit: Vec<Value> = std::fs::read_to_string(audit_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let methods: Vec<&str> = audit
        .iter()
        .map(|v| v["method"].as_str().unwrap())
        .collect();
    assert_eq!(
        methods,
        [
            "initialize",
            "initialized",
            "textDocument/didOpen",
            "textDocument/didChange",
            "textDocument/completion",
            "textDocument/definition",
            "textDocument/hover",
            "textDocument/didClose",
            "shutdown",
            "exit"
        ]
    );
    assert_eq!(audit[0]["params"]["initializationOptions"]["custom"], true);
    assert_eq!(
        audit[0]["params"]["capabilities"]["general"]["positionEncodings"],
        json!(["utf-16"])
    );
    assert!(audit[8].get("params").is_none());
    assert!(audit[9].get("params").is_none());
    let change = &audit[3]["params"]["contentChanges"][0];
    assert!(change.get("range").is_none());
    assert_eq!(change["text"], "class Main {}");
}

#[test]
fn incremental_servers_receive_a_utf16_whole_range_edit() {
    let temp = tempfile::tempdir().unwrap();
    let audit_path = temp.path().join("audit.jsonl");
    let client = ready("incremental", Some(&audit_path));
    client
        .did_open("file:///a.kt", "kotlin", 7, "a\r\nλ🦀")
        .unwrap();
    client.did_change("file:///a.kt", 8, "x🦀").unwrap();
    client.did_change("file:///a.kt", 9, "last").unwrap();
    client.shutdown().unwrap();
    let audit: Vec<Value> = std::fs::read_to_string(audit_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let changes: Vec<_> = audit
        .iter()
        .filter(|v| v["method"] == "textDocument/didChange")
        .collect();
    assert_eq!(
        changes[0]["params"]["contentChanges"][0]["range"]["end"],
        json!({"line":1,"character":3})
    );
    assert_eq!(
        changes[1]["params"]["contentChanges"][0]["range"]["end"],
        json!({"line":0,"character":3})
    );
}

#[test]
fn response_ids_multiplex_out_of_order() {
    let client = Arc::new(ready("normal", None));
    thread::scope(|scope| {
        let a = scope.spawn(|| {
            client
                .request("mock/reverse", json!({"caller":"a"}))
                .unwrap()
        });
        let b = scope.spawn(|| {
            client
                .request("mock/reverse", json!({"caller":"b"}))
                .unwrap()
        });
        assert_eq!(a.join().unwrap()["caller"], "a");
        assert_eq!(b.join().unwrap()["caller"], "b");
    });
    client.shutdown().unwrap();
}

#[test]
fn notifications_arrive_while_another_request_is_pending_and_timeout_recovers() {
    let client = Arc::new(ready("normal", None));
    thread::scope(|scope| {
        let pending = scope.spawn(|| {
            client.request_with_timeout("mock/never", json!({}), Duration::from_millis(350))
        });
        let event = client.next_event(Duration::from_secs(1)).unwrap().unwrap();
        assert!(matches!(event, LspEvent::Notification { method, .. } if method == "mock/pending"));
        assert_eq!(
            client.request("mock/echo", json!({"works":true})).unwrap()["works"],
            true
        );
        assert!(matches!(pending.join().unwrap(), Err(Error::Timeout(_))));
    });
    assert_eq!(
        client.request("mock/echo", json!({"value":123})).unwrap()["value"],
        123
    );
    client.shutdown().unwrap();
}

#[test]
fn event_backpressure_does_not_block_request_responses() {
    let mut opts = options();
    opts.event_capacity = 2;
    let client = StdioRpc::spawn(config("normal", None), opts).unwrap();
    assert_eq!(
        client.request("mock/flood", json!({})).unwrap(),
        "flood finished"
    );
    assert!(
        matches!(client.next_event(Duration::ZERO).unwrap(), Some(RpcEvent::Lagged { dropped }) if dropped >= 998)
    );
    assert!(client.next_event(Duration::ZERO).unwrap().is_some());
}

#[test]
fn unsupported_server_requests_are_rejected_instead_of_hanging() {
    let client = ready("normal", None);
    assert_eq!(
        client.request("mock/requestClient", json!({})).unwrap(),
        true
    );
    assert!(
        matches!(client.next_event(Duration::from_secs(1)).unwrap(), Some(LspEvent::UnsupportedServerRequest { method, .. }) if method == "workspace/applyEdit")
    );
    assert!(
        matches!(client.next_event(Duration::from_secs(1)).unwrap(), Some(LspEvent::Notification { method, .. }) if method == "mock/requestRejected")
    );
    client.shutdown().unwrap();
}

#[test]
fn remote_errors_preserve_code_message_and_data() {
    let client = ready("normal", None);
    assert!(
        matches!(client.request("mock/error", json!({})), Err(Error::Remote { code: -32602, data: Some(data), .. }) if data["detail"] == 1)
    );
    assert_eq!(
        client.request("mock/echo", json!({"value":true})).unwrap()["value"],
        true
    );
    client.shutdown().unwrap();
}

#[test]
fn server_eof_bad_frames_and_invalid_json_fail_promptly() {
    for mode in ["eof", "bad-frame", "invalid-json"] {
        let client = StdioRpc::spawn(config(mode, None), options()).unwrap();
        let start = Instant::now();
        assert!(matches!(
            client.request("initialize", json!({})),
            Err(Error::Closed(_) | Error::Protocol(_) | Error::Io(_))
        ));
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(client.request("again", json!({})).is_err());
    }
}

#[test]
fn malformed_response_is_terminal_and_pending_calls_wake() {
    let client = ready("normal", None);
    assert!(matches!(
        client.request("mock/invalidResponse", json!({})),
        Err(Error::Protocol(_))
    ));
    assert!(client.request("mock/echo", json!({})).is_err());
}

#[test]
fn absent_features_and_unadvertised_position_encoding_are_not_claimed() {
    let client = ready("no-capabilities", None);
    assert!(matches!(
        client.did_open("file:///a", "java", 1, ""),
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        client.hover("file:///a", Position::default()),
        Err(Error::Unsupported(_))
    ));
    client.shutdown().unwrap();
    let client = LspClient::spawn(config("bad-encoding", None), options()).unwrap();
    assert!(matches!(
        client.initialize(None, Value::Null),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn blocked_server_stdin_cannot_block_the_caller_past_request_deadline() {
    let client = StdioRpc::spawn(config("blocked-stdin", None), options()).unwrap();
    let start = Instant::now();
    assert!(matches!(
        client.request_with_timeout(
            "mock/large",
            json!({"text":"x".repeat(1024 * 1024)}),
            Duration::from_millis(100)
        ),
        Err(Error::Timeout(_))
    ));
    assert!(start.elapsed() < Duration::from_secs(1));
    // Drop must also release a writer blocked on the child pipe.
    drop(client);
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn pending_requests_are_bounded() {
    let mut opts = options();
    opts.max_pending_requests = 1;
    let client = StdioRpc::spawn(config("normal", None), opts).unwrap();
    thread::scope(|scope| {
        let pending = scope.spawn(|| {
            client.request_with_timeout("mock/never", json!({}), Duration::from_millis(250))
        });
        assert!(client.next_event(Duration::from_secs(1)).unwrap().is_some());
        assert!(matches!(
            client.request("mock/echo", json!({})),
            Err(Error::QueueFull)
        ));
        assert!(matches!(pending.join().unwrap(), Err(Error::Timeout(_))));
    });
    assert_eq!(
        client.request("mock/echo", json!({"value":true})).unwrap()["value"],
        true
    );
}

#[cfg(target_os = "linux")]
#[test]
fn dropping_client_reaps_its_direct_child() {
    let client = StdioRpc::spawn(config("blocked-stdin", None), options()).unwrap();
    let process_path = format!("/proc/{}", client.process_id());
    assert!(Path::new(&process_path).exists());
    drop(client);
    assert!(!Path::new(&process_path).exists());
}

#[test]
fn shutdown_waits_for_inflight_operations_and_forces_an_uncooperative_exit() {
    let client = ready("ignore-exit", None);
    let process_path = format!("/proc/{}", client.process_id());
    thread::scope(|scope| {
        let pending = scope.spawn(|| {
            client.request_with_timeout("mock/never", json!({}), Duration::from_millis(300))
        });
        assert!(client.next_event(Duration::from_secs(1)).unwrap().is_some());
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let client_ref = &client;
        scope.spawn(move || {
            done_tx.send(client_ref.shutdown()).unwrap();
        });
        assert!(matches!(
            done_rx.recv_timeout(Duration::from_millis(50)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(matches!(pending.join().unwrap(), Err(Error::Timeout(_))));
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
    });
    #[cfg(target_os = "linux")]
    assert!(!Path::new(&process_path).exists());
    #[cfg(not(target_os = "linux"))]
    let _ = process_path;
}

#[test]
fn uncertain_notification_timeout_aborts_the_connection() {
    let mut opts = options();
    opts.request_timeout = Duration::from_millis(100);
    let client = StdioRpc::spawn(config("blocked-stdin", None), opts).unwrap();
    let start = Instant::now();
    assert!(matches!(
        client.notify("mock/large", json!({"text":"x".repeat(1024 * 1024)})),
        Err(Error::Timeout(_))
    ));
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(client.request("mock/echo", json!({})).is_err());
}

#[test]
fn outgoing_limit_rejects_message_without_poisoning_connection() {
    let mut opts = options();
    opts.frame_limits.max_content_bytes = 256;
    let client = StdioRpc::spawn(config("normal", None), opts).unwrap();
    assert!(matches!(
        client.request("mock/echo", json!({"text":"x".repeat(1024)})),
        Err(Error::Protocol(_))
    ));
    assert_eq!(
        client.request("mock/echo", json!({"ok":true})).unwrap()["ok"],
        true
    );
    assert!(matches!(
        client.request("mock/echo", json!(true)),
        Err(Error::InvalidState(_))
    ));
}

#[test]
fn cold_initialize_timeout_can_be_extended_without_changing_feature_deadlines() {
    let mut opts = options();
    opts.request_timeout = Duration::from_millis(100);
    let ordinary = LspClient::spawn(config("delayed-initialize", None), opts.clone()).unwrap();
    assert!(matches!(
        ordinary.initialize(None, json!({})),
        Err(Error::Timeout(_))
    ));
    let extended = LspClient::spawn(config("delayed-initialize", None), opts).unwrap();
    extended
        .initialize_with_timeout(None, json!({}), Duration::from_secs(2))
        .unwrap();
    let start = Instant::now();
    assert!(matches!(
        extended.request("mock/never", json!({})),
        Err(Error::Timeout(_))
    ));
    assert!(start.elapsed() < Duration::from_millis(600));
    extended.shutdown().unwrap();
}

#[test]
fn completion_resolve_negotiates_lazy_fields_and_preserves_opaque_item_data() {
    let temp = tempfile::tempdir().unwrap();
    let audit_path = temp.path().join("resolve-audit.jsonl");
    let client = ready("normal", Some(&audit_path));
    let uri = "file:///mock/Main.java";
    client.did_open(uri, "java", 1, "hel").unwrap();
    let completion = client
        .completion(
            uri,
            Position {
                line: 0,
                character: 3,
            },
        )
        .unwrap();
    let original = completion["items"][0].clone();
    assert!(original.get("additionalTextEdits").is_none());
    let resolved = client.resolve_completion(original.clone()).unwrap();
    assert_eq!(resolved["data"], original["data"]);
    assert_eq!(resolved["extension"], original["extension"]);
    assert_eq!(resolved["command"], original["command"]);
    assert_eq!(resolved["textEdit"], original["textEdit"]);
    assert_eq!(resolved["detail"], "demo.Hello");
    assert_eq!(
        resolved["documentation"]["value"],
        "Resolved mock documentation"
    );
    assert_eq!(
        resolved["additionalTextEdits"][0]["newText"],
        "import demo.Hello;\n"
    );
    client.shutdown().unwrap();
    let audit: Vec<Value> = std::fs::read_to_string(audit_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(audit[0].pointer("/params/capabilities/textDocument/completion/completionItem/resolveSupport/properties").unwrap(), &json!(["documentation","detail","additionalTextEdits"]));
    let resolve = audit
        .iter()
        .find(|v| v["method"] == "completionItem/resolve")
        .unwrap();
    assert_eq!(resolve["params"], original);
    assert!(!audit
        .iter()
        .any(|v| v["method"] == "workspace/executeCommand"));
}

#[test]
fn completion_resolve_requires_ready_state_and_advertised_provider() {
    let item = json!({"label":"hello", "data":{"opaque":1}});
    let client = LspClient::spawn(config("normal", None), options()).unwrap();
    assert!(matches!(
        client.resolve_completion(item.clone()),
        Err(Error::InvalidState(_))
    ));
    client.initialize(None, json!({})).unwrap();
    for malformed in [Value::Null, json!([]), json!({}), json!({"label":99})] {
        assert!(matches!(
            client.resolve_completion(malformed),
            Err(Error::InvalidState(_))
        ));
    }
    client.shutdown().unwrap();
    assert!(matches!(
        client.resolve_completion(item.clone()),
        Err(Error::InvalidState(_))
    ));
    for mode in [
        "resolve-no-provider",
        "resolve-false-provider",
        "no-capabilities",
    ] {
        let client = ready(mode, None);
        assert!(matches!(
            client.resolve_completion(item.clone()),
            Err(Error::Unsupported(_))
        ));
        client.shutdown().unwrap();
    }
}

#[test]
fn completion_resolve_propagates_server_error_and_rejects_malformed_result() {
    let item = json!({"label":"hello"});
    let client = ready("resolve-error", None);
    assert!(
        matches!(client.resolve_completion(item.clone()), Err(Error::Remote { code: -32602, data: Some(data), .. }) if data["reason"] == "expired")
    );
    assert_eq!(
        client.request("mock/echo", json!({"ok":true})).unwrap()["ok"],
        true
    );
    client.shutdown().unwrap();
    let client = ready("resolve-invalid", None);
    assert!(matches!(
        client.resolve_completion(item),
        Err(Error::Protocol(_))
    ));
    client.shutdown().unwrap();
}

#[test]
fn completion_resolve_keeps_normal_deadline_and_notifications_flow() {
    let mut opts = options();
    opts.request_timeout = Duration::from_millis(250);
    let client = LspClient::spawn(config("resolve-never", None), opts).unwrap();
    client.initialize(None, json!({})).unwrap();
    thread::scope(|scope| {
        let pending = scope.spawn(|| client.resolve_completion(json!({"label":"hello"})));
        assert!(
            matches!(client.next_event(Duration::from_secs(1)).unwrap(), Some(LspEvent::Notification { method, .. }) if method == "mock/resolvePending")
        );
        assert_eq!(
            client.request("mock/echo", json!({"ok":true})).unwrap()["ok"],
            true
        );
        assert!(
            matches!(pending.join().unwrap(), Err(Error::Timeout(method)) if method == "completionItem/resolve")
        );
    });
    client.shutdown().unwrap();
}

#[test]
fn completion_resolve_enforces_frame_limit_without_poisoning_connection() {
    let mut opts = options();
    opts.frame_limits.max_content_bytes = 2048;
    let client = LspClient::spawn(config("normal", None), opts).unwrap();
    client.initialize(None, json!({})).unwrap();
    assert!(matches!(
        client.resolve_completion(json!({"label":"hello","data":"x".repeat(4096)})),
        Err(Error::Protocol(_))
    ));
    let resolved = client
        .resolve_completion(json!({"label":"hello","data":{"small":true}}))
        .unwrap();
    assert_eq!(resolved["data"]["small"], true);
    assert_eq!(resolved["detail"], "demo.Hello");
    client.shutdown().unwrap();
}
