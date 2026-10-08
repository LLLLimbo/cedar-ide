//! Public API acceptance with production transport deadlines and exact owned
//! child cleanup. The peers are synthetic and never execute workspace commands.
#[allow(dead_code)]
#[path = "support/cancellation_harness.rs"]
mod harness;

use cedar_client::{Client, ConnectionCancellation, ConnectionSpec};
use cedar_protocol::{LanguageQueryKind, Operation, Payload};
use harness::*;
use std::{
    fs,
    process::Command,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[test]
fn fixture_root_mode_requires_the_exact_marker_and_rejects_execution_trust() {
    let fixture = Fixture::new("capability_peer");
    for marker in [
        None,
        Some(b"incorrect\n".as_slice()),
        Some(b"cedar-transport-fixture-v1\n ".as_slice()),
    ] {
        let path = fixture.root().join(".cedar-transport-fixture");
        if let Some(contents) = marker {
            fs::write(path, contents).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
        let output = Command::new(binary())
            .arg("--root")
            .arg(fixture.root())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(!fixture.root().join("started").exists());
    }
    fs::write(
        fixture.root().join(".cedar-transport-fixture"),
        b"cedar-transport-fixture-v1\n",
    )
    .unwrap();
    let output = Command::new(binary())
        .arg("--root")
        .arg(fixture.root())
        .arg("--allow-run")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!fixture.root().join("started").exists());
}

#[test]
fn precancelled_public_constructors_do_not_spawn_or_touch_a_workspace() {
    let fixture = Fixture::new("stalled_hello");
    let cancellation = ConnectionCancellation::new();
    cancellation.cancel();
    let Err(error) = Client::spawn_agent_with_cancellation(
        &binary(),
        fixture.root(),
        false,
        cancellation.clone(),
    ) else {
        panic!("precancelled separate-agent startup succeeded");
    };
    assert_cancelled(&error);
    let Err(error) = Client::connect_with_cancellation(
        ConnectionSpec::Local {
            root: fixture.root().join("nonexistent workspace"),
            allow_run: false,
        },
        cancellation.clone(),
    ) else {
        panic!("precancelled Local startup succeeded");
    };
    assert_cancelled(&error);
    // Cancellation precedes even SSH argument validation; no SSH is invoked.
    let Err(error) = Client::connect_with_cancellation(
        ConnectionSpec::Ssh {
            host: "-invalid-host".into(),
            port: 0,
            root: String::new(),
            agent_path: String::new(),
            allow_run: false,
        },
        cancellation,
    ) else {
        panic!("precancelled SSH startup succeeded");
    };
    assert_cancelled(&error);
    assert!(!fixture.root().join("started").exists());
    assert!(!fixture.root().join("requests").exists());
}

#[test]
fn stalled_read_cancels_before_its_deadline_and_rejects_later_wire_work() {
    let _watchdog = Watchdog::start(Duration::from_secs(12));
    cancelled_read_cycle();
}

#[test]
fn cancellation_is_permanent_but_a_fresh_connection_has_independent_ids_and_state() {
    let _watchdog = Watchdog::start(Duration::from_secs(12));
    let old = Fixture::new("capability_peer");
    let token = ConnectionCancellation::new();
    let mut client = old.connect(token.clone());
    let clone = token.clone();
    clone.cancel();
    assert!(token.is_cancelled());
    assert_cancelled(&client.request(Operation::Hello).unwrap_err());
    assert_cancelled(&client.request(list()).unwrap_err());
    client.close_and_wait(CLEANUP_BOUND).unwrap();
    old.assert_closed(1);

    let fresh = Fixture::new("capability_peer");
    let fresh_token = ConnectionCancellation::new();
    let mut client = fresh.connect(fresh_token.clone());
    token.cancel();
    assert!(!fresh_token.is_cancelled());
    assert!(matches!(
        client.request(read()).unwrap(),
        Payload::File { .. }
    ));
    assert!(matches!(
        client.request(list()).unwrap(),
        Payload::Entries { .. }
    ));
    client.close_and_wait(CLEANUP_BOUND).unwrap();
    fresh.assert_closed(3);
    assert_eq!(
        fresh
            .requests()
            .iter()
            .map(|request| request.id)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
}

#[test]
fn cancellation_interrupts_each_eligible_file_operation() {
    let _watchdog = Watchdog::start(Duration::from_secs(12));
    for operation in [
        list(),
        read(),
        Operation::Search {
            query: "fixture".into(),
            limit: 5,
        },
    ] {
        let fixture = Fixture::new("held_reply");
        let token = ConnectionCancellation::new();
        let mut client = fixture.connect(token.clone());
        let (finished, result) = mpsc::channel();
        let caller = thread::spawn(move || {
            let outcome = client.request(operation);
            finished.send((client, outcome)).unwrap();
        });
        fixture.ready(2);
        token.cancel();
        let (client, outcome) = result.recv_timeout(CALLER_BOUND).unwrap();
        assert_cancelled(&outcome.unwrap_err());
        // Release the synthetic peer so normal EOF cleanup, rather than the
        // production grace kill, is what this particular test exercises.
        fixture.release(2);
        caller.join().unwrap();
        client.close_and_wait(CLEANUP_BOUND).unwrap();
        fixture.assert_closed(2);
    }
}

#[test]
fn response_cancel_race_never_allows_another_request_or_replays_the_first() {
    let _watchdog = Watchdog::start(Duration::from_secs(12));
    for _ in 0..8 {
        let fixture = Fixture::new("held_reply");
        let token = ConnectionCancellation::new();
        let mut client = fixture.connect(token.clone());
        let (finished, result) = mpsc::channel();
        let caller = thread::spawn(move || {
            let outcome = client.request(read());
            finished.send((client, outcome)).unwrap();
        });
        fixture.ready(2);
        // Release and cancel without asserting a scheduler order. Either
        // accepted response or cancellation is valid before observation.
        fixture.release(2);
        token.cancel();
        let (mut client, outcome) = result.recv_timeout(CALLER_BOUND).unwrap();
        match outcome {
            Ok(Payload::File { .. }) => {}
            Err(error) => assert_cancelled(&error),
            other => panic!("unexpected response/cancel result: {other:?}"),
        }
        assert_cancelled(&client.request(list()).unwrap_err());
        caller.join().unwrap();
        client.close_and_wait(CLEANUP_BOUND).unwrap();
        fixture.assert_closed(2);
    }
}

#[test]
fn already_received_write_git_language_and_task_requests_keep_their_replies() {
    let _watchdog = Watchdog::start(Duration::from_secs(15));
    for operation in [
        Operation::Write {
            path: "fixture.txt".into(),
            text: "saved".into(),
            expected_revision: None,
        },
        Operation::GitStatus,
        Operation::LanguageOpen {
            path: "Main.java".into(),
            language_id: "java".into(),
            version: 1,
            text: "class Main {}".into(),
        },
        Operation::LanguageQuery {
            path: "Main.java".into(),
            line: 0,
            character: 0,
            kind: LanguageQueryKind::Hover,
        },
        Operation::RunPoll { task_id: 7 },
    ] {
        let fixture = Fixture::new("held_reply");
        let token = ConnectionCancellation::new();
        let mut client = fixture.connect(token.clone());
        let (finished, result) = mpsc::channel();
        let caller = thread::spawn(move || {
            let outcome = client.request(operation);
            finished.send((client, outcome)).unwrap();
        });
        fixture.ready(2);
        token.cancel();
        // Negative assertion is marker-gated: the full mutation is already on
        // the wire, and the controller has not permitted its reply yet.
        assert!(matches!(
            result.recv_timeout(Duration::from_millis(150)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(!fixture.root().join("eof").exists());
        fixture.release(2);
        let (mut client, outcome) = result.recv_timeout(CALLER_BOUND).unwrap();
        outcome.expect("cancellation must preserve the mutation's normal reply");
        assert_cancelled(&client.request(list()).unwrap_err());
        caller.join().unwrap();
        client.close_and_wait(CLEANUP_BOUND).unwrap();
        fixture.assert_closed(2);
    }
}

#[test]
fn cancellation_observed_before_reply_release_wins_for_reads() {
    let _watchdog = Watchdog::start(Duration::from_secs(12));
    let fixture = Fixture::new("held_reply");
    let token = ConnectionCancellation::new();
    let mut client = fixture.connect(token.clone());
    let (finished, result) = mpsc::channel();
    let caller = thread::spawn(move || {
        let outcome = client.request(read());
        finished.send((client, outcome)).unwrap();
    });
    fixture.ready(2);
    let cancelled = Instant::now();
    token.cancel();
    fixture.release(2);
    let (client, outcome) = result.recv_timeout(CALLER_BOUND).unwrap();
    assert_cancelled(&outcome.unwrap_err());
    assert!(cancelled.elapsed() < CALLER_BOUND);
    caller.join().unwrap();
    client.close_and_wait(CLEANUP_BOUND).unwrap();
    fixture.assert_closed(2);
}
