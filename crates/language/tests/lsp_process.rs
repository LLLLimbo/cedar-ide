#![cfg(feature = "test-server")]

use cedar_language::{
    ClientOptions, Error, LspClient, LspEvent, Position, ProcessConfig, RpcEvent, StdioRpc,
};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{mpsc, Arc, Barrier};
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
    let (result, outcome) = client.shutdown_with_outcome();
    result.unwrap();
    assert!(outcome.shutdown_response_received && outcome.exit_frame_completed);
    #[cfg(target_os = "linux")]
    {
        assert_eq!(outcome.windows, None);
        assert!(outcome.linux.unwrap().worker_joined);
        assert!(outcome.is_graceful());
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        assert_eq!(outcome.windows, None);
        assert_eq!(outcome.linux, None);
        assert!(!outcome.is_graceful());
    }
    let (repeated, repeated_outcome) = client.shutdown_with_outcome();
    repeated.unwrap();
    assert_eq!(repeated_outcome, outcome);
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
fn shutdown_delivers_complete_exit_then_stdin_eof_before_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let audit_path = temp.path().join("exit-eof.jsonl");
    let client = ready("exit-waits-eof", Some(&audit_path));
    let started = Instant::now();
    client.shutdown().unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
    let audit: Vec<Value> = std::fs::read_to_string(audit_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(audit.len(), 5);
    let methods: Vec<_> = audit[..4]
        .iter()
        .map(|entry| entry["method"].as_str().unwrap())
        .collect();
    assert_eq!(methods, ["initialize", "initialized", "shutdown", "exit"]);
    // shutdown() can return Ok after forced cleanup. This server-written marker
    // exists only if it consumed a complete exit frame and then clean stdin EOF.
    assert_eq!(audit[4], json!({"fixture":"stdin-eof-after-exit"}));
}

#[test]
fn failed_shutdown_preserves_original_error_and_outcome_without_another_rpc() {
    let temp = tempfile::tempdir().unwrap();
    let audit_path = temp.path().join("failed-shutdown.jsonl");
    let client = ready("shutdown-error", Some(&audit_path));
    let (result, outcome) = client.shutdown_with_outcome();
    assert!(matches!(result, Err(Error::Remote { code: -32000, .. })));
    assert!(outcome.shutdown_response_received);
    assert!(!outcome.exit_frame_completed);
    assert!(!outcome.is_graceful());
    let (repeated, repeated_outcome) = client.shutdown_with_outcome();
    assert_eq!(
        repeated.unwrap_err().to_string(),
        result.clone().unwrap_err().to_string()
    );
    assert_eq!(repeated_outcome, outcome);
    assert_eq!(
        client.shutdown().unwrap_err().to_string(),
        result.unwrap_err().to_string()
    );
    let methods: Vec<String> = std::fs::read_to_string(audit_path)
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap()["method"]
                .as_str()
                .unwrap()
                .into()
        })
        .collect();
    assert_eq!(methods, ["initialize", "initialized", "shutdown"]);
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
fn startup_signal_interrupts_initialize_without_waiting_for_lifecycle_gate() {
    let client = Arc::new(LspClient::spawn(config("initialize-never", None), options()).unwrap());
    let abort = client.abort_handle();
    let initializer = Arc::clone(&client);
    let owner = thread::spawn(move || {
        initializer.initialize_with_deadline(
            None,
            json!({}),
            Duration::from_secs(60),
            Instant::now() + Duration::from_secs(75),
        )
    });
    assert!(matches!(
        client.next_event(Duration::from_secs(2)).unwrap(),
        Some(LspEvent::Notification { method, .. }) if method == "mock/initializePending"
    ));
    let started = Instant::now();
    abort.signal();
    assert!(matches!(owner.join().unwrap(), Err(Error::Closed(_))));
    assert!(started.elapsed() < Duration::from_secs(3));
    let outcome = Arc::try_unwrap(client).ok().unwrap().abort_and_join();
    assert!(!outcome.is_graceful());
    #[cfg(windows)]
    assert_eq!(
        outcome.windows.unwrap().cleanup,
        cedar_language::WindowsCleanupStatus::Joined
    );
    #[cfg(not(windows))]
    assert_eq!(outcome.windows, None);
    abort.signal(); // a surviving signal does not retain the process owner
}

#[test]
fn cloned_abort_signal_bypasses_full_transport_queue() {
    let mut opts = options();
    opts.request_timeout = Duration::from_secs(5);
    opts.outbound_capacity = 2;
    opts.max_pending_requests = 8;
    let client = Arc::new(LspClient::spawn(config("ready-blocked-stdin", None), opts).unwrap());
    client.initialize(None, json!({})).unwrap();
    assert!(matches!(
        client.next_event(Duration::from_secs(2)).unwrap(),
        Some(LspEvent::Notification { method, .. }) if method == "mock/readyBlocked"
    ));
    let abort = client.abort_handle();
    let start = Arc::new(Barrier::new(7));
    let (finished, replies) = mpsc::channel();
    let callers: Vec<_> = (0..6)
        .map(|_| {
            let client = Arc::clone(&client);
            let start = Arc::clone(&start);
            let finished = finished.clone();
            thread::spawn(move || {
                let params = json!({"data":"x".repeat(900_000)});
                start.wait();
                finished.send(client.request("mock/large", params)).unwrap();
            })
        })
        .collect();
    start.wait();
    // Six callers cannot reach the eight-pending limit. This proves the
    // outbound queue is full while another frame is partly transported.
    assert!(matches!(
        replies.recv_timeout(Duration::from_secs(2)).unwrap(),
        Err(Error::QueueFull)
    ));
    abort.clone().signal();
    let mut canceled = 0;
    for _ in 1..6 {
        match replies.recv_timeout(Duration::from_secs(2)).unwrap() {
            Err(Error::Closed(_)) => canceled += 1,
            Err(Error::QueueFull) => {}
            result => panic!("unexpected canceled request: {result:?}"),
        }
    }
    for caller in callers {
        caller.join().unwrap();
    }
    assert!(canceled > 0);
    let outcome = Arc::try_unwrap(client).ok().unwrap().abort_and_join();
    #[cfg(windows)]
    assert_eq!(
        outcome.windows.unwrap().cleanup,
        cedar_language::WindowsCleanupStatus::Joined
    );
    #[cfg(not(windows))]
    assert_eq!(outcome.windows, None);
}

#[test]
fn accepted_startup_deadline_clips_the_initialize_response_budget() {
    let client = LspClient::spawn(config("delayed-initialize", None), options()).unwrap();
    let started = Instant::now();
    assert!(matches!(
        client.initialize_with_deadline(
            None,
            json!({}),
            Duration::from_secs(60),
            started + Duration::from_millis(50),
        ),
        Err(Error::Timeout(_))
    ));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(!client.abort_and_join().is_graceful());
}

#[test]
fn initialized_write_uses_remaining_startup_budget_after_a_successful_response() {
    let mut opts = options();
    opts.request_timeout = Duration::from_secs(5);
    let client = LspClient::spawn(config("initialize-blocked-notification", None), opts).unwrap();
    let started = Instant::now();
    let result = client.initialize_with_deadline(
        None,
        json!({}),
        Duration::from_secs(60),
        started + Duration::from_secs(1),
    );
    assert!(
        matches!(result, Err(Error::Timeout(ref reason)) if reason == "write notification initialized"),
        "{result:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    let outcome = client.abort_and_join();
    assert!(!outcome.is_graceful());
    #[cfg(windows)]
    assert_eq!(
        outcome.windows.unwrap().cleanup,
        cedar_language::WindowsCleanupStatus::Joined
    );
}

#[test]
fn already_expired_startup_does_not_send_initialize() {
    let temp = tempfile::tempdir().unwrap();
    let audit = temp.path().join("expired.jsonl");
    let client = LspClient::spawn(config("normal", Some(&audit)), options()).unwrap();
    assert!(matches!(
        client.initialize_with_deadline(None, json!({}), Duration::from_secs(60), Instant::now()),
        Err(Error::Timeout(_))
    ));
    client.abort_and_join();
    assert!(std::fs::read_to_string(audit)
        .unwrap_or_default()
        .is_empty());
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

#[test]
fn formatting_references_and_symbols_use_exact_static_lsp_contracts() {
    let temp = tempfile::tempdir().unwrap();
    let audit_path = temp.path().join("navigation.jsonl");
    let client = ready("navigation-object-provider", Some(&audit_path));
    let uri = "file:///mock/space%20%23%20%E4%BD%A0.java";
    client.did_open(uri, "java", 7, "class Hello {}").unwrap();
    let formatted = client.formatting(uri, 1, true).unwrap();
    assert_eq!(formatted[0]["newText"], "// mock formatted\n");
    client.formatting(uri, 16, false).unwrap();
    let position = Position {
        line: 2,
        character: 3,
    };
    let references = client.references(uri, position, false).unwrap();
    assert_eq!(references[0]["uri"], uri);
    client
        .references(
            uri,
            Position {
                line: i32::MAX as u32,
                character: i32::MAX as u32,
            },
            true,
        )
        .unwrap();
    let symbols = client.document_symbols(uri).unwrap();
    assert_eq!(symbols[0]["name"], "Hello");
    assert_eq!(symbols[0]["children"][0]["name"], "count");
    client.shutdown().unwrap();
    let audit: Vec<Value> = std::fs::read_to_string(audit_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let capabilities = &audit[0]["params"]["capabilities"];
    assert_eq!(capabilities["workspace"]["applyEdit"], false);
    assert!(capabilities["workspace"].get("workspaceEdit").is_none());
    assert_eq!(
        capabilities["textDocument"]["formatting"],
        json!({"dynamicRegistration":false})
    );
    assert_eq!(
        capabilities["textDocument"]["references"],
        json!({"dynamicRegistration":false})
    );
    assert_eq!(
        capabilities["textDocument"]["documentSymbol"],
        json!({"dynamicRegistration":false,"hierarchicalDocumentSymbolSupport":true})
    );
    let formatting: Vec<_> = audit
        .iter()
        .filter(|v| v["method"] == "textDocument/formatting")
        .collect();
    assert_eq!(
        formatting[0]["params"],
        json!({"textDocument":{"uri":uri},"options":{"tabSize":1,"insertSpaces":true}})
    );
    assert_eq!(
        formatting[1]["params"],
        json!({"textDocument":{"uri":uri},"options":{"tabSize":16,"insertSpaces":false}})
    );
    let references: Vec<_> = audit
        .iter()
        .filter(|v| v["method"] == "textDocument/references")
        .collect();
    assert_eq!(
        references[0]["params"],
        json!({"textDocument":{"uri":uri},"position":position,"context":{"includeDeclaration":false}})
    );
    assert_eq!(
        references[1]["params"]["context"],
        json!({"includeDeclaration":true})
    );
    let symbols = audit
        .iter()
        .find(|v| v["method"] == "textDocument/documentSymbol")
        .unwrap();
    assert_eq!(symbols["params"], json!({"textDocument":{"uri":uri}}));
    assert!(!audit
        .iter()
        .any(|v| v["method"] == "workspace/executeCommand"
            || v["method"] == "workspace/applyEdit"
            || v["method"] == "textDocument/rename"));
}

#[test]
fn new_document_features_require_initialization_open_document_and_live_session() {
    let temp = tempfile::tempdir().unwrap();
    let audit_path = temp.path().join("lifecycle.jsonl");
    let client = LspClient::spawn(config("normal", Some(&audit_path)), options()).unwrap();
    let uri = "file:///mock/Hello.java";
    let require_invalid_state = || {
        assert!(matches!(
            client.formatting(uri, 4, true),
            Err(Error::InvalidState(_))
        ));
        assert!(matches!(
            client.references(uri, Position::default(), true),
            Err(Error::InvalidState(_))
        ));
        assert!(matches!(
            client.document_symbols(uri),
            Err(Error::InvalidState(_))
        ));
    };
    require_invalid_state();
    client.initialize(None, Value::Null).unwrap();
    require_invalid_state();
    client.did_open(uri, "java", 1, "class Hello {}").unwrap();
    client.did_close(uri).unwrap();
    require_invalid_state();
    client.shutdown().unwrap();
    require_invalid_state();
    let audit = std::fs::read_to_string(audit_path).unwrap();
    for method in [
        "textDocument/formatting",
        "textDocument/references",
        "textDocument/documentSymbol",
    ] {
        assert!(
            !audit.contains(method),
            "rejected {method} must never reach the server"
        );
    }
}

#[test]
fn new_document_features_reject_absent_false_and_invalid_static_capabilities() {
    for mode in [
        "no-capabilities",
        "navigation-no-provider",
        "navigation-false-provider",
        "navigation-invalid-provider",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let audit_path = temp.path().join("unsupported.jsonl");
        let client = ready(mode, Some(&audit_path));
        let uri = "file:///mock/Hello.java";
        if mode != "no-capabilities" {
            client.did_open(uri, "java", 1, "class Hello {}").unwrap();
        }
        assert!(matches!(
            client.formatting(uri, 4, true),
            Err(Error::Unsupported(_))
        ));
        assert!(matches!(
            client.references(uri, Position::default(), true),
            Err(Error::Unsupported(_))
        ));
        assert!(matches!(
            client.document_symbols(uri),
            Err(Error::Unsupported(_))
        ));
        client.shutdown().unwrap();
        let audit = std::fs::read_to_string(audit_path).unwrap();
        for method in [
            "textDocument/formatting",
            "textDocument/references",
            "textDocument/documentSymbol",
        ] {
            assert!(!audit.contains(method));
        }
    }
}

#[test]
fn invalid_formatting_options_and_unsigned31_positions_never_reach_server() {
    let temp = tempfile::tempdir().unwrap();
    let audit_path = temp.path().join("bounds.jsonl");
    let client = ready("normal", Some(&audit_path));
    let uri = "file:///mock/Hello.java";
    client.did_open(uri, "java", 1, "class Hello {}").unwrap();
    for tab_size in [0, 17, i32::MAX as u32, u32::MAX] {
        assert!(matches!(
            client.formatting(uri, tab_size, true),
            Err(Error::InvalidState(_))
        ));
    }
    for position in [
        Position {
            line: i32::MAX as u32 + 1,
            character: 0,
        },
        Position {
            line: 0,
            character: i32::MAX as u32 + 1,
        },
        Position {
            line: u32::MAX,
            character: u32::MAX,
        },
    ] {
        assert!(matches!(
            client.references(uri, position, true),
            Err(Error::InvalidState(_))
        ));
    }
    client.shutdown().unwrap();
    let audit = std::fs::read_to_string(audit_path).unwrap();
    assert!(!audit.contains("textDocument/formatting"));
    assert!(!audit.contains("textDocument/references"));
}

#[test]
fn new_features_preserve_null_empty_flat_and_server_error_results() {
    let uri = "file:///mock/Hello.java";
    for (mode, expected) in [
        ("navigation-null", Value::Null),
        ("navigation-empty", json!([])),
    ] {
        let client = ready(mode, None);
        client.did_open(uri, "java", 1, "class Hello {}").unwrap();
        assert_eq!(client.formatting(uri, 4, true).unwrap(), expected);
        assert_eq!(
            client.references(uri, Position::default(), true).unwrap(),
            expected
        );
        assert_eq!(client.document_symbols(uri).unwrap(), expected);
        client.shutdown().unwrap();
    }
    let client = ready("symbols-flat", None);
    client.did_open(uri, "java", 1, "class Hello {}").unwrap();
    let symbols = client.document_symbols(uri).unwrap();
    assert_eq!(symbols[0]["location"]["uri"], uri);
    assert_eq!(symbols[0]["containerName"], "demo");
    assert!(symbols[0].get("children").is_none());
    client.shutdown().unwrap();
    let client = ready("navigation-error", None);
    client.did_open(uri, "java", 1, "class Hello {}").unwrap();
    for (result, method) in [
        (client.formatting(uri, 4, true), "textDocument/formatting"),
        (
            client.references(uri, Position::default(), true),
            "textDocument/references",
        ),
        (client.document_symbols(uri), "textDocument/documentSymbol"),
    ] {
        assert!(
            matches!(result, Err(Error::Remote { code: -32602, data: Some(data), .. }) if data["method"] == method)
        );
    }
    assert_eq!(
        client.request("mock/echo", json!({"alive":true})).unwrap()["alive"],
        true
    );
    client.shutdown().unwrap();
}

#[test]
fn workspace_symbols_require_live_static_provider_but_no_open_document() {
    for mode in ["normal", "navigation-object-provider"] {
        let directory = tempfile::tempdir().unwrap();
        let audit_path = directory.path().join("workspace-symbols.jsonl");
        let client = LspClient::spawn(config(mode, Some(&audit_path)), options()).unwrap();
        assert!(matches!(
            client.workspace_symbols("Type"),
            Err(Error::InvalidState(_))
        ));
        client.initialize(None, Value::Null).unwrap();
        for query in [" 你好.Type* ".to_owned(), "🦀".repeat(64), "x".repeat(256)] {
            let result = client.workspace_symbols(&query).unwrap();
            assert_eq!(result[0]["name"], "Hello");
        }
        client.shutdown().unwrap();
        assert!(matches!(
            client.workspace_symbols("Type"),
            Err(Error::InvalidState(_))
        ));
        let audit: Vec<Value> = std::fs::read_to_string(audit_path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            audit[0]["params"]["capabilities"]["workspace"]["symbol"],
            json!({"dynamicRegistration":false})
        );
        let methods: Vec<_> = audit
            .iter()
            .map(|entry| entry["method"].as_str().unwrap())
            .collect();
        assert_eq!(
            methods,
            [
                "initialize",
                "initialized",
                "workspace/symbol",
                "workspace/symbol",
                "workspace/symbol",
                "shutdown",
                "exit"
            ]
        );
        assert_eq!(audit[2]["params"], json!({"query":" 你好.Type* "}));
        assert_eq!(audit[3]["params"], json!({"query":"🦀".repeat(64)}));
        assert_eq!(audit[4]["params"], json!({"query":"x".repeat(256)}));
    }
}

#[test]
fn workspace_symbols_reject_missing_false_invalid_providers_and_bad_queries_before_rpc() {
    for mode in [
        "no-capabilities",
        "navigation-no-provider",
        "navigation-false-provider",
        "navigation-invalid-provider",
        "normal",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let audit_path = directory.path().join("unsupported.jsonl");
        let client = ready(mode, Some(&audit_path));
        if mode == "normal" {
            for query in [
                String::new(),
                "   ".into(),
                "\u{2003}".into(),
                "x".repeat(257),
                "🦀".repeat(65),
                "a\nb".into(),
                "a\tb".into(),
                "a\0b".into(),
                "a\u{7f}b".into(),
                "a\u{85}b".into(),
            ] {
                assert!(
                    matches!(
                        client.workspace_symbols(&query),
                        Err(Error::InvalidState(_))
                    ),
                    "{query:?}"
                );
            }
        } else {
            assert!(
                matches!(client.workspace_symbols("Type"), Err(Error::Unsupported(method)) if method == "workspace/symbol")
            );
        }
        client.shutdown().unwrap();
        assert!(!std::fs::read_to_string(audit_path)
            .unwrap()
            .contains("\"method\":\"workspace/symbol\""));
    }
}

#[test]
fn workspace_symbols_validate_complete_bounded_results_without_partial_success() {
    let directory = tempfile::tempdir().unwrap();
    let audit_path = directory.path().join("custom.jsonl");
    let result_path = audit_path.with_extension("result.json");
    let client = ready("workspace-symbol-custom", Some(&audit_path));
    let symbol = json!({"name":"Type","kind":5,"containerName":"demo","tags":[1],"deprecated":true,
        "location":{"uri":"file:///mock/%E4%BD%A0%20Type.java","range":{"start":{"line":0,"character":0},"end":{"line":1,"character":3}}}});
    for result in [
        Value::Null,
        json!([]),
        json!([symbol.clone()]),
        json!(vec![symbol.clone(); 256]),
    ] {
        std::fs::write(&result_path, result.to_string()).unwrap();
        assert_eq!(client.workspace_symbols("Type").unwrap(), result);
    }
    let mut extended = symbol.clone();
    extended["command"] = json!({"command":"mock.mustNotRun"});
    extended["textEdit"] = json!({"newText":"never apply"});
    extended["data"] = json!({"resolve":"never"});
    std::fs::write(&result_path, json!([extended]).to_string()).unwrap();
    assert_eq!(
        client.workspace_symbols("Type").unwrap(),
        json!([symbol.clone()])
    );
    let mut bad_symbols = Vec::new();
    for (field, value) in [
        ("name", Value::Null),
        ("name", json!("")),
        ("name", json!("x".repeat(4097))),
        ("name", json!("a\nb")),
        ("kind", json!(0)),
        ("kind", json!(27)),
        ("kind", json!(1.5)),
        ("containerName", json!("x".repeat(4097))),
        ("containerName", json!(false)),
        ("tags", json!([1, 1])),
        ("tags", json!([2])),
        ("tags", json!("1")),
        ("deprecated", json!(1)),
        ("children", json!([])),
    ] {
        let mut bad = symbol.clone();
        bad[field] = value;
        bad_symbols.push(bad);
    }
    for uri in [
        "relative.java".to_owned(),
        "file:///a\nb".into(),
        "file:///a b".into(),
        format!("file:///{}", "x".repeat(16 * 1024)),
    ] {
        let mut bad = symbol.clone();
        bad["location"]["uri"] = json!(uri);
        bad_symbols.push(bad);
    }
    for range in [
        Value::Null,
        json!({"start":{"line":0,"character":0}}),
        json!({"start":{"line":0,"character":4},"end":{"line":0,"character":3}}),
        json!({"start":{"line":-1,"character":0},"end":{"line":1,"character":3}}),
        json!({"start":{"line":0,"character":0},"end":{"line":2147483648_u64,"character":3}}),
    ] {
        let mut bad = symbol.clone();
        bad["location"]["range"] = range;
        bad_symbols.push(bad);
    }
    let mut malformed = vec![
        json!({}),
        json!("invalid"),
        json!(vec![symbol.clone(); 257]),
    ];
    malformed.extend(
        bad_symbols
            .into_iter()
            .map(|bad| json!([symbol.clone(), bad])),
    );
    let mut lazy = symbol.clone();
    lazy["location"].as_object_mut().unwrap().remove("range");
    malformed.push(json!([lazy]));
    let mut oversized = symbol.clone();
    oversized["data"] = json!("x".repeat(1024 * 1024));
    malformed.push(json!([oversized]));
    let mut deep = symbol.clone();
    let mut data = Value::Null;
    for _ in 0..9 {
        data = json!([data]);
    }
    deep["data"] = data;
    malformed.push(json!([deep]));
    let mut wide = symbol.clone();
    wide["data"] = json!(vec![Value::Null; 16 * 1024]);
    malformed.push(json!([wide]));
    let mut large_text = symbol.clone();
    large_text["name"] = json!("x".repeat(4096));
    malformed.push(json!(vec![large_text; 128]));
    for result in malformed {
        std::fs::write(&result_path, result.to_string()).unwrap();
        assert!(matches!(
            client.workspace_symbols("Type"),
            Err(Error::Protocol(_))
        ));
    }
    // Rejection does not poison the running server or execute a fallback.
    std::fs::write(&result_path, json!([symbol.clone()]).to_string()).unwrap();
    assert_eq!(client.workspace_symbols("Type").unwrap(), json!([symbol]));
    client.shutdown().unwrap();
    for line in std::fs::read_to_string(audit_path).unwrap().lines() {
        let request: Value = serde_json::from_str(line).unwrap();
        assert!(matches!(
            request["method"].as_str().unwrap(),
            "initialize" | "initialized" | "workspace/symbol" | "shutdown" | "exit"
        ));
    }
}

#[test]
fn workspace_symbols_report_server_errors_without_fallback_requests() {
    let directory = tempfile::tempdir().unwrap();
    let audit_path = directory.path().join("error.jsonl");
    let client = ready("workspace-symbol-error", Some(&audit_path));
    assert!(matches!(
        client.workspace_symbols("Type"),
        Err(Error::Remote { code: -32602, .. })
    ));
    client.shutdown().unwrap();
    let methods: Vec<String> = std::fs::read_to_string(audit_path)
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap()["method"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        methods,
        [
            "initialize",
            "initialized",
            "workspace/symbol",
            "shutdown",
            "exit"
        ]
    );
}

#[test]
fn implementations_require_live_open_document_and_static_provider_before_rpc() {
    let uri = "file:///mock/Hello%20雪.java";
    for mode in [
        "normal",
        "navigation-object-provider",
        "navigation-no-provider",
        "navigation-false-provider",
        "navigation-invalid-provider",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let audit_path = directory.path().join("implementations.jsonl");
        let client = LspClient::spawn(config(mode, Some(&audit_path)), options()).unwrap();
        assert!(matches!(
            client.implementations(uri, Position::default()),
            Err(Error::InvalidState(_))
        ));
        client.initialize(None, Value::Null).unwrap();
        let supported = matches!(mode, "normal" | "navigation-object-provider");
        if supported {
            assert!(matches!(
                client.implementations(uri, Position::default()),
                Err(Error::InvalidState(_))
            ));
        }
        client
            .did_open(uri, "java", 7, "// 🦀\r\ninterface Hello {}")
            .unwrap();
        if supported {
            for position in [
                Position {
                    line: u32::MAX,
                    character: 0,
                },
                Position {
                    line: 0,
                    character: u32::MAX,
                },
            ] {
                assert!(matches!(
                    client.implementations(uri, position),
                    Err(Error::InvalidState(_))
                ));
            }
            assert_eq!(
                client
                    .implementations(
                        uri,
                        Position {
                            line: 1,
                            character: 10
                        }
                    )
                    .unwrap()[0]["uri"],
                uri
            );
            client.did_close(uri).unwrap();
            assert!(matches!(
                client.implementations(uri, Position::default()),
                Err(Error::InvalidState(_))
            ));
        } else {
            assert!(
                matches!(client.implementations(uri, Position::default()), Err(Error::Unsupported(method)) if method == "textDocument/implementation")
            );
        }
        client.shutdown().unwrap();
        assert!(matches!(
            client.implementations(uri, Position::default()),
            Err(Error::InvalidState(_))
        ));
        let audit: Vec<Value> = std::fs::read_to_string(audit_path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            audit[0]["params"]["capabilities"]["textDocument"]["implementation"],
            json!({"dynamicRegistration":false,"linkSupport":false})
        );
        let requests: Vec<_> = audit
            .iter()
            .filter(|message| message["method"] == "textDocument/implementation")
            .collect();
        assert_eq!(requests.len(), usize::from(supported));
        if supported {
            assert_eq!(
                requests[0]["params"],
                json!({"textDocument":{"uri":uri},"position":{"line":1,"character":10}})
            );
        }
    }
}

#[test]
fn implementations_validate_results_and_report_errors_without_retry_or_commands() {
    let uri = "file:///mock/Hello.java";
    let directory = tempfile::tempdir().unwrap();
    let audit_path = directory.path().join("implementations.jsonl");
    let result_path = audit_path.with_extension("result.json");
    let client = ready("implementation-custom", Some(&audit_path));
    client.did_open(uri, "java", 1, "class Hello {}").unwrap();
    let location = json!({"uri":uri,"range":{"start":{"line":0,"character":6},"end":{"line":0,"character":11}}});
    for (result, expected) in [
        (Value::Null, json!([])),
        (location.clone(), json!([location.clone()])),
        (json!([location.clone()]), json!([location.clone()])),
    ] {
        std::fs::write(&result_path, result.to_string()).unwrap();
        assert_eq!(
            client.implementations(uri, Position::default()).unwrap(),
            expected
        );
    }
    for result in [
        json!([location.clone(), {"uri":uri}]),
        json!({"targetUri":uri,"targetRange":location["range"],"targetSelectionRange":location["range"]}),
        json!(vec![location; 1025]),
    ] {
        std::fs::write(&result_path, result.to_string()).unwrap();
        assert!(matches!(
            client.implementations(uri, Position::default()),
            Err(Error::Protocol(_))
        ));
    }
    client.shutdown().unwrap();
    let audit: Vec<Value> = std::fs::read_to_string(&audit_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        audit
            .iter()
            .filter(|message| message["method"] == "textDocument/implementation")
            .count(),
        6
    );
    for message in audit {
        assert!(matches!(
            message["method"].as_str().unwrap(),
            "initialize"
                | "initialized"
                | "textDocument/didOpen"
                | "textDocument/implementation"
                | "shutdown"
                | "exit"
        ));
    }
    let error_audit_path = directory.path().join("error.jsonl");
    let client = ready("implementation-error", Some(&error_audit_path));
    client.did_open(uri, "java", 1, "class Hello {}").unwrap();
    assert!(matches!(
        client.implementations(uri, Position::default()),
        Err(Error::Remote { code: -32602, .. })
    ));
    client.shutdown().unwrap();
    let methods: Vec<String> = std::fs::read_to_string(error_audit_path)
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap()["method"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        methods,
        [
            "initialize",
            "initialized",
            "textDocument/didOpen",
            "textDocument/implementation",
            "shutdown",
            "exit"
        ]
    );
}
