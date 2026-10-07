#![cfg(feature = "test-server")]
use cedar_debugger::{DapClient, Error, Event, Options, ProcessConfig};
use serde_json::json;
use std::time::{Duration, Instant};
fn spawn(scenario: &str, options: Options) -> DapClient {
    let mut config = ProcessConfig::new(env!("CARGO_BIN_EXE_cedar-mock-dap"));
    config.args.push(scenario.into());
    DapClient::spawn(config, options).unwrap()
}
fn options() -> Options {
    Options {
        request_timeout: Duration::from_secs(2),
        ..Options::default()
    }
}
fn event(client: &DapClient, expected: &str) -> serde_json::Value {
    match client.next_event(Duration::from_secs(2)).expect("event") {
        Event::Adapter { event, body, .. } => {
            assert_eq!(event, expected);
            body.unwrap()
        }
        other => panic!("unexpected {other:?}"),
    }
}
#[test]
fn deferred_launch_configuration_and_debugger_queries() {
    let mut client = spawn("", options());
    assert_eq!(
        client
            .initialize("mock")
            .unwrap()
            .wait()
            .unwrap()
            .body
            .unwrap()["supportsConfigurationDoneRequest"],
        true
    );
    let launch = client
        .request("launch", Some(json!({"program":"owned fixture"})))
        .unwrap();
    event(&client, "initialized");
    assert!(launch.try_result().is_none());
    assert_eq!(
        client
            .request("setBreakpoints", Some(json!({"breakpoints":[{"line":10}]})))
            .unwrap()
            .wait()
            .unwrap()
            .body
            .unwrap()["breakpoints"][0]["verified"],
        true
    );
    client
        .request("configurationDone", None)
        .unwrap()
        .wait()
        .unwrap();
    launch.wait().unwrap();
    assert_eq!(event(&client, "stopped")["threadId"], 1);
    for (command, field) in [
        ("threads", "threads"),
        ("stackTrace", "stackFrames"),
        ("scopes", "scopes"),
        ("variables", "variables"),
    ] {
        assert!(!client
            .request(command, None)
            .unwrap()
            .wait()
            .unwrap()
            .body
            .unwrap()[field]
            .as_array()
            .unwrap()
            .is_empty());
    }
    client
        .request("continue", Some(json!({"threadId":1})))
        .unwrap()
        .wait()
        .unwrap();
    event(&client, "continued");
    event(&client, "terminated");
    client.disconnect(true).unwrap();
    client.stop();
    client.stop();
    assert_eq!(client.stats().pending_requests, 0);
}
#[test]
fn out_of_order_correlation_uses_request_seq() {
    let client = spawn("", options());
    let first = client
        .request("outOfOrder", Some(json!({"value":1})))
        .unwrap();
    let second = client
        .request("outOfOrder", Some(json!({"value":2})))
        .unwrap();
    assert!(second.request_seq() > first.request_seq());
    assert_eq!(second.wait().unwrap().body.unwrap()["value"], 2);
    assert_eq!(first.wait().unwrap().body.unwrap()["value"], 1);
}
#[test]
fn reverse_requests_are_explicitly_refused() {
    let client = spawn("", options());
    client.request("reverse", None).unwrap().wait().unwrap();
    let mut refused = 0;
    let mut observed = 0;
    while refused + observed < 4 {
        match client.next_event(Duration::from_secs(2)).unwrap() {
            Event::ReverseRequestRejected { command, .. } => {
                assert!(command == "runInTerminal" || command == "startDebugging");
                refused += 1;
            }
            Event::Adapter { event, .. } => {
                assert_eq!(event, "refusalObserved");
                observed += 1;
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(client.stats().rejected_reverse_requests, 2);
}
#[test]
fn output_flood_has_bounded_utf8_tail_and_does_not_starve_responses() {
    let client = spawn(
        "",
        Options {
            event_capacity: 1,
            output_capacity_bytes: 63,
            ..options()
        },
    );
    client.request("outputFlood", None).unwrap().wait().unwrap();
    let output = client.output_tail();
    assert!(output.text.len() <= 63);
    assert!(output.text.ends_with("THE-END"));
    assert!(output.discarded_bytes > 3000);
    assert!(client.terminal_error().is_none());
}
#[test]
fn event_overflow_is_terminal_and_never_hidden_by_full_queue() {
    let client = spawn(
        "overflow",
        Options {
            event_capacity: 1,
            ..options()
        },
    );
    let response = client.request("trigger", None).unwrap().wait();
    assert_eq!(response, Err(Error::EventOverflow));
    assert_eq!(
        client.next_event(Duration::ZERO),
        Some(Event::Closed(Error::EventOverflow))
    );
    assert_eq!(client.next_event(Duration::ZERO), None);
    assert_eq!(client.terminal_error(), Some(Error::EventOverflow));
}
#[test]
fn bad_frames_eof_and_mismatched_commands_fail_all_requests() {
    for scenario in ["malformed", "oversize", "partial", "eof", "mismatch"] {
        let client = spawn(scenario, options());
        let result = client.request("trigger", None).unwrap().wait();
        assert!(
            matches!(result, Err(Error::Protocol(_) | Error::Closed(_))),
            "{scenario}: {result:?}"
        );
        assert!(client.terminal_error().is_some());
        assert_eq!(client.stats().pending_requests, 0);
    }
}
#[test]
fn deadlines_late_replies_and_local_cancellation() {
    let client = spawn("", options());
    let pending = client
        .request_with_timeout("late", None, Duration::from_millis(40))
        .unwrap();
    assert!(matches!(pending.wait(), Err(Error::Timeout { .. })));
    let canceled = client.request("echo", None).unwrap();
    drop(canceled);
    assert_eq!(
        client
            .request("echo", Some(json!({"alive":true})))
            .unwrap()
            .wait()
            .unwrap()
            .body
            .unwrap()["alive"],
        true
    );
    assert!(client.stats().ignored_responses >= 1);
    assert_eq!(client.stats().pending_requests, 0);
}
#[test]
fn pending_caps_and_validation_do_not_corrupt_session() {
    let client = spawn(
        "never",
        Options {
            max_pending_requests: 1,
            ..options()
        },
    );
    assert!(matches!(
        client.request("bad\ncommand", None),
        Err(Error::Invalid(_))
    ));
    let pending = client.request("never", None).unwrap();
    assert!(matches!(
        client.request("another", None),
        Err(Error::Capacity)
    ));
    drop(pending);
    assert_eq!(client.stats().pending_requests, 0);
    assert!(client.request("another", None).is_ok());
}
#[test]
fn outgoing_limits_and_adapter_errors() {
    let client = spawn(
        "",
        Options {
            frame_limits: cedar_debugger::FrameLimits {
                max_content_bytes: 512,
                ..Default::default()
            },
            ..options()
        },
    );
    assert!(matches!(
        client.request("echo", Some(json!({"large":"x".repeat(1000)}))),
        Err(Error::Protocol(_))
    ));
    assert_eq!(client.stats().pending_requests, 0);
    assert!(matches!(
        client.request("fail", None).unwrap().wait(),
        Err(Error::Remote { .. })
    ));
    client.request("echo", None).unwrap().wait().unwrap();
}
#[test]
fn stalled_stdin_does_not_block_deadlines_or_repeated_stop() {
    let mut client = spawn(
        "stalled-stdin",
        Options {
            request_timeout: Duration::from_millis(100),
            ..options()
        },
    );
    let start = Instant::now();
    let handle = client
        .request("large", Some(json!({"data":"x".repeat(1_000_000)})))
        .unwrap();
    assert!(matches!(handle.wait(), Err(Error::Timeout { .. })));
    assert!(start.elapsed() < Duration::from_secs(2));
    for _ in 0..3 {
        client.stop();
    }
    assert!(start.elapsed() < Duration::from_secs(2));
    #[cfg(target_os = "linux")]
    assert!(!std::path::Path::new(&format!("/proc/{}", client.process_id())).exists());
}
#[test]
fn repeated_drop_reaps_owned_adapter_children() {
    for _ in 0..10 {
        let client = spawn("never", options());
        #[cfg(target_os = "linux")]
        let pid = client.process_id();
        drop(client);
        #[cfg(target_os = "linux")]
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    }
}

#[test]
fn immediate_adapter_exit_preserves_the_final_response() {
    for _ in 0..30 {
        let mut client = spawn("", options());
        client.disconnect(true).unwrap();
    }
}

#[test]
fn abandoned_handles_still_expire_without_waiting() {
    let client = spawn(
        "never",
        Options {
            request_timeout: Duration::from_millis(30),
            max_pending_requests: 1,
            ..options()
        },
    );
    let handle = client.request("never", None).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(client.stats().pending_requests, 0);
    assert!(matches!(
        handle.try_result(),
        Some(Err(Error::Timeout { .. }))
    ));
    assert!(client.request("next", None).is_ok());
}

#[test]
fn stalled_writer_queue_is_bounded_and_submission_is_nonblocking() {
    let mut client = spawn(
        "stalled-stdin",
        Options {
            outbound_capacity: 1,
            max_pending_requests: 32,
            ..options()
        },
    );
    let first = client
        .request("huge", Some(json!({"data":"x".repeat(1_000_000)})))
        .unwrap();
    std::thread::sleep(Duration::from_millis(30));
    let second = client.request("queued", None).unwrap();
    let start = Instant::now();
    assert!(matches!(
        client.request("overflow", None),
        Err(Error::Capacity)
    ));
    assert!(start.elapsed() < Duration::from_millis(250));
    client.stop();
    assert!(matches!(first.wait(), Err(Error::Closed(_))));
    assert!(matches!(second.wait(), Err(Error::Closed(_))));
}
