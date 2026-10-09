//! Connection cancellation uses the real Worker request loop and a separately
//! built, marked synthetic stdio peer. No SSH, workspace trust or binary
//! resolution override is involved. Process cases are explicit acceptance tests.
use super::*;
use cedar_protocol::Request;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const ROOT: &str = "/synthetic-cancel-workspace";
const DRAFT: &str = "retained draft 草稿 🐻\n";
const WAIT: Duration = Duration::from_secs(5);
const CANCEL_BOUND: Duration = Duration::from_secs(3);

fn form() -> ConnectForm {
    ConnectForm {
        ssh: false,
        local_root: ROOT.into(),
        allow_run: false,
        ..Default::default()
    }
}

fn hello() -> Payload {
    Payload::Hello {
        protocol: cedar_protocol::PROTOCOL_VERSION,
        root: ROOT.into(),
        agent: None,
    }
}

fn retain_draft(app: &mut CedarApp) {
    let mut doc = Document::new(1, "draft.txt".into(), "original\n".into(), "r0".into());
    doc.text = DRAFT.into();
    doc.interrupted_save = Some(
        interrupted_save::InterruptedSave::capture(app, &doc)
            .expect("known workspace must capture the unknown save"),
    );
    app.documents.push(doc);
    app.active_document = Some(1);
    app.next_document = 2;
}

fn assert_draft(app: &CedarApp) {
    assert_eq!(app.documents.len(), 1);
    let doc = &app.documents[0];
    assert_eq!(doc.text, DRAFT);
    assert_eq!(doc.saved_text, "original\n");
    assert_eq!(doc.revision.as_deref(), Some("r0"));
    assert!(doc.dirty());
    assert!(!doc.saving);
    assert!(doc.interrupted_save.is_some());
    assert_eq!(app.active_document, Some(doc.id));
}

fn recorded_workspace() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.generation = 7;
    app.workspace_key = Some(form().key());
    app.active_form = Some(form());
    app.root = ROOT.into();
    app.state = ConnectionState::Ready;
    retain_draft(&mut app);
    (app, commands)
}

#[test]
fn cancel_connecting_invalidates_generation_and_preserves_unknown_save() {
    let (mut app, commands) = recorded_workspace();
    app.state = ConnectionState::Connecting;
    app.connecting_form = Some(form());
    app.agent_info = Some(agent_support::full_test_agent());
    app.recovery.restoring_generation = Some(app.generation);
    let cancelled_generation = app.generation;
    let interrupted = app.documents[0].interrupted_save.clone();

    app.cancel_connection();
    assert_eq!(app.generation, cancelled_generation + 1);
    assert!(app.state == ConnectionState::Disconnected);
    assert!(app.worker.is_none());
    assert!(app.connecting_form.is_none());
    assert!(app.agent_info.is_none());
    assert!(app.recovery.restoring_generation.is_none());
    assert_eq!(app.workspace_key, Some(form().key()));
    assert_eq!(app.root, ROOT);
    assert_eq!(app.documents[0].interrupted_save, interrupted);
    assert_draft(&app);
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));

    // Repeated Cancel and either kind of stale completion are harmless.
    app.cancel_connection();
    for result in [Ok(hello()), Err("transport_cancelled: old attempt".into())] {
        app.apply_event(Event {
            generation: cancelled_generation,
            id: 0,
            connected: result.is_ok(),
            result,
        });
    }
    assert_eq!(app.generation, cancelled_generation + 1);
    assert!(app.state == ConnectionState::Disconnected);
    assert_draft(&app);

    // A fresh accepted connection keeps the existing interrupted-save guard.
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.state = ConnectionState::Connecting;
    app.connecting_form = Some(form());
    app.apply_event(Event {
        generation: app.generation,
        id: 0,
        connected: true,
        result: Ok(hello()),
    });
    assert!(matches!(
        commands.try_recv().unwrap().op,
        Operation::List { .. }
    ));
    app.save();
    assert!(app.ready());
    assert_draft(&app);
    assert!(app.error.as_deref().unwrap().contains("interrupted save"));
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn first_connection_cancel_returns_to_idle() {
    let mut app = CedarApp::empty();
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.state = ConnectionState::Connecting;
    app.connecting_form = Some(form());
    app.cancel_connection();
    assert!(app.state == ConnectionState::Idle);
    assert_eq!(app.generation, 1);
    assert!(app.documents.is_empty());
    assert!(app.workspace_key.is_none());
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
fn reconnect_preserves_pending_save_git_language_and_task_guards() {
    for kind in ["save", "git", "language", "task"] {
        let (mut app, commands) = recorded_workspace();
        let job = match kind {
            "save" => Job::Save {
                document: 1,
                snapshot: DRAFT.into(),
                submission: app.documents[0].interrupted_save.clone(),
            },
            "git" => Job::Git,
            "language" => Job::Language(language_ui::Action {
                session: 0,
                kind: language_ui::ActionKind::Events,
            }),
            "task" => {
                app.run_state.snapshot = Some(cedar_tasks::TaskSnapshot {
                    id: 1,
                    state: cedar_tasks::TaskState::Running,
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: None,
                    windows_exit_code: None,
                    truncated: false,
                    error: None,
                });
                Job::Run(run_ui::Action {
                    epoch: 0,
                    kind: run_ui::Kind::Poll(1),
                })
            }
            _ => unreachable!(),
        };
        app.pending.insert(19, job);
        // Invalid input is an additional failsafe against starting a process
        // if a guard regresses. A correct guard retains the ready generation.
        let invalid = ConnectForm {
            local_root: String::new(),
            ..form()
        };
        app.connect(&egui::Context::default(), invalid);
        assert!(app.ready(), "{kind}");
        assert_eq!(app.generation, 7, "{kind}");
        assert!(app.worker.is_some(), "{kind}");
        assert!(app.pending.contains_key(&19), "{kind}");
        assert_draft(&app);
        if kind == "task" {
            assert!(app.tools_open);
            assert!(app.tool == Tool::Run);
        } else {
            assert!(app
                .error
                .as_deref()
                .unwrap()
                .contains("current save, Git, command, or language"));
        }
        assert!(matches!(
            commands.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }
}

fn peer_binary() -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os("CEDAR_CONNECTION_CANCEL_PEER_BIN")
            .expect("set CEDAR_CONNECTION_CANCEL_PEER_BIN to cedar-client-transport-peer"),
    );
    assert!(path.is_absolute(), "fixture path must be absolute");
    assert!(path.is_file(), "fixture binary must exist");
    path
}

fn fixture(mode: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join(".cedar-transport-fixture"),
        b"cedar-transport-fixture-v1\n",
    )
    .unwrap();
    fs::write(root.path().join("fixture-mode"), mode).unwrap();
    fs::write(
        root.path().join("hello.json"),
        serde_json::to_vec(&hello()).unwrap(),
    )
    .unwrap();
    root
}

fn wait_marker(root: &Path, name: &str) {
    let deadline = Instant::now() + WAIT;
    while !root.join(name).is_file() {
        assert!(Instant::now() < deadline, "fixture did not publish {name}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn ready_request(root: &Path, wire_id: u64) -> Request {
    let name = format!("ready-{wire_id}");
    wait_marker(root, &name);
    serde_json::from_slice(&fs::read(root.join(name)).unwrap()).unwrap()
}

fn assert_operations(root: &Path, expected: &[&str]) {
    let requests = fs::read_to_string(root.join("requests")).unwrap();
    let operations: Vec<String> = requests
        .lines()
        .map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            value["op"]["type"].as_str().unwrap().into()
        })
        .collect();
    assert_eq!(operations, expected);
}

fn next_event(app: &CedarApp) -> Event {
    response(
        app.result_rx
            .recv_timeout(WAIT)
            .expect("worker did not return an event"),
    )
}

fn response(event: WorkerEvent) -> Event {
    match event {
        WorkerEvent::Response(event) => event,
        WorkerEvent::TransportLost { message, .. } => panic!("unexpected idle loss: {message}"),
        WorkerEvent::Closed { .. } => panic!("unexpected cleanup receipt before response"),
    }
}

fn start_fixture(app: &mut CedarApp, root: &Path) {
    assert!(app.worker.is_none());
    app.generation += 1;
    app.pending.clear();
    app.agent_info = None;
    app.state = ConnectionState::Connecting;
    app.connecting_form = Some(form());
    app.worker = Some(Worker::spawn_agent(
        peer_binary(),
        root.into(),
        app.generation,
        app.result_tx.clone(),
        app.editor_ctx.clone(),
    ));
}

fn accept_fixture(app: &mut CedarApp) {
    let event = next_event(app);
    assert_eq!(event.generation, app.generation);
    assert_eq!(event.id, 0);
    app.apply_event(event);
    assert!(app.ready());
    let event = next_event(app);
    assert!(matches!(&event.result, Ok(Payload::Entries { .. })));
    app.apply_event(event);
    assert!(app.pending.is_empty());
    assert!(!app.execution_trusted());
}

#[test]
#[ignore = "requires CEDAR_CONNECTION_CANCEL_PEER_BIN; explicit process acceptance"]
fn stalled_hello_cancel_is_bounded_and_retains_drafts() {
    let (mut app, _) = recorded_workspace();
    app.worker = None;
    // Each retry gets its own token. Cancellation cannot leak to the next one.
    for _ in 0..2 {
        let root = fixture("stalled_hello");
        start_fixture(&mut app, root.path());
        let old_generation = app.generation;
        assert!(matches!(ready_request(root.path(), 1).op, Operation::Hello));
        let start = Instant::now();
        app.cancel_connection();
        let event = response(
            app.result_rx
                .recv_timeout(CANCEL_BOUND)
                .expect("cancelled Hello exceeded the cancellation bound"),
        );
        assert!(start.elapsed() < CANCEL_BOUND);
        assert_eq!(event.generation, old_generation);
        assert_eq!(event.id, 0);
        assert!(!event.connected);
        assert!(event
            .result
            .as_ref()
            .unwrap_err()
            .starts_with("transport_cancelled:"));
        app.apply_event(event);
        assert_eq!(app.generation, old_generation + 1);
        assert!(app.state == ConnectionState::Disconnected);
        assert_draft(&app);
        wait_marker(root.path(), "eof");
        assert_operations(root.path(), &["hello"]);
    }
}

#[test]
#[ignore = "requires CEDAR_CONNECTION_CANCEL_PEER_BIN; explicit process acceptance"]
fn stalled_read_cancels_queued_list_and_replacement_stays_connected() {
    let old_root = fixture("stalled_read");
    let new_root = fixture("capability_peer");
    let mut app = CedarApp::empty();
    start_fixture(&mut app, old_root.path());
    accept_fixture(&mut app);
    retain_draft(&mut app);
    let old_generation = app.generation;
    app.open("fixture.txt".into(), None);
    assert!(matches!(
        ready_request(old_root.path(), 3).op,
        Operation::Read { .. }
    ));
    let read_id = *app.pending.keys().next().unwrap();
    app.list("queued-only".into());
    assert_eq!(app.pending.len(), 2);

    let start = Instant::now();
    app.worker = None;
    let cancelled = response(
        app.result_rx
            .recv_timeout(CANCEL_BOUND)
            .expect("cancelled Read exceeded the cancellation bound"),
    );
    assert!(start.elapsed() < CANCEL_BOUND);
    assert_eq!(cancelled.generation, old_generation);
    assert_eq!(cancelled.id, read_id);
    assert!(!cancelled.connected);
    assert!(cancelled
        .result
        .as_ref()
        .unwrap_err()
        .starts_with("transport_cancelled:"));
    wait_marker(old_root.path(), "eof");
    assert_operations(old_root.path(), &["hello", "list", "read"]);
    assert!(matches!(app.result_rx.recv_timeout(WAIT).unwrap(),
        WorkerEvent::Closed { generation, result: Ok(()) } if generation == old_generation));

    start_fixture(&mut app, new_root.path());
    accept_fixture(&mut app);
    app.apply_event(cancelled);
    assert!(app.ready());
    assert_draft(&app);
    app.list("replacement-works".into());
    let event = next_event(&app);
    assert_eq!(event.generation, app.generation);
    assert!(event.connected);
    assert!(matches!(&event.result, Ok(Payload::Entries { .. })));
    app.apply_event(event);
    assert!(app.ready());
    assert!(app.pending.is_empty());
    app.save();
    assert!(app.error.as_deref().unwrap().contains("interrupted save"));
    assert_draft(&app);
    app.worker = None;
    wait_marker(new_root.path(), "eof");
    assert_operations(new_root.path(), &["hello", "list", "list"]);
}

#[test]
#[ignore = "requires CEDAR_CONNECTION_CANCEL_PEER_BIN; explicit process acceptance"]
fn dropped_worker_drains_sent_write_reply_and_suppresses_queued_list() {
    let root = fixture("held_reply");
    let (tx, rx) = mpsc::channel();
    let worker = Worker::spawn_agent(
        peer_binary(),
        root.path().into(),
        17,
        tx,
        egui::Context::default(),
    );
    let hello = response(rx.recv_timeout(WAIT).unwrap());
    assert!(hello.connected);
    worker
        .tx
        .send(Command {
            id: 41,
            op: Operation::Write {
                path: "fixture.txt".into(),
                text: "generated submission".into(),
                expected_revision: Some("fixture-revision".into()),
            },
        })
        .unwrap();
    assert!(matches!(
        ready_request(root.path(), 2).op,
        Operation::Write { .. }
    ));
    worker
        .tx
        .send(Command {
            id: 42,
            op: Operation::List {
                path: "queued-only".into(),
            },
        })
        .unwrap();
    drop(worker);
    assert!(matches!(
        rx.recv_timeout(Duration::from_millis(150)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    fs::write(root.path().join("release-2"), b"release generated reply\n").unwrap();
    let event = response(rx.recv_timeout(WAIT).unwrap());
    assert_eq!(event.generation, 17);
    assert_eq!(event.id, 41);
    assert!(event.connected);
    assert!(
        matches!(event.result, Ok(Payload::Written { revision }) if revision == "written-revision")
    );
    wait_marker(root.path(), "eof");
    assert_operations(root.path(), &["hello", "write"]);
    assert!(matches!(
        rx.recv_timeout(WAIT).unwrap(),
        WorkerEvent::Closed {
            generation: 17,
            result: Ok(())
        }
    ));
    assert!(matches!(
        rx.recv_timeout(WAIT),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[test]
#[ignore = "requires CEDAR_CONNECTION_CANCEL_PEER_BIN; explicit process acceptance"]
fn idle_worker_observes_terminal_reader_events_without_sending_requests() {
    for mode in ["idle_eof", "idle_bad_frame", "idle_unsolicited"] {
        let root = fixture(mode);
        let (tx, rx) = mpsc::channel();
        let ctx = egui::Context::default();
        let (repaint_tx, repaint_rx) = mpsc::channel();
        ctx.set_request_repaint_callback(move |_| {
            let _ = repaint_tx.send(());
        });
        let worker = Worker::spawn_agent(peer_binary(), root.path().into(), 23, tx, ctx.clone());
        let hello = response(rx.recv_timeout(WAIT).unwrap());
        assert!(hello.connected);
        repaint_rx
            .recv_timeout(WAIT)
            .expect("Hello did not request repaint");
        // Settle Hello's requested frames before requiring a new idle-loss
        // repaint. No wall-clock polling or user action wakes the worker.
        for _ in 0..4 {
            let _ = ctx.run(egui::RawInput::default(), |_| {});
            if !ctx.has_requested_repaint() {
                break;
            }
        }
        assert!(!ctx.has_requested_repaint());
        while repaint_rx.try_recv().is_ok() {}
        wait_marker(root.path(), "ready-1");
        assert_operations(root.path(), &["hello"]);
        fs::write(root.path().join("release-1"), b"release idle loss\n").unwrap();
        let terminal = rx
            .recv_timeout(WAIT)
            .expect("idle reader did not wake worker");
        let WorkerEvent::TransportLost {
            generation,
            message,
        } = terminal
        else {
            panic!("passive loss must have a dedicated event");
        };
        assert_eq!(generation, 23);
        repaint_rx
            .recv_timeout(WAIT)
            .expect("idle loss did not request repaint");
        let prefix = if mode == "idle_unsolicited" {
            "protocol_error:"
        } else {
            "transport_"
        };
        assert!(message.starts_with(prefix), "{message}");
        assert_operations(root.path(), &["hello"]);
        assert!(matches!(
            rx.recv_timeout(WAIT).unwrap(),
            WorkerEvent::Closed {
                generation: 23,
                result: Ok(())
            }
        ));
        assert!(matches!(
            rx.recv_timeout(WAIT),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        drop(worker);
    }
}

#[test]
#[ignore = "requires CEDAR_CONNECTION_CANCEL_PEER_BIN; explicit process acceptance"]
fn worker_publishes_completed_write_ack_before_immediate_eof() {
    let root = fixture("held_reply_eof");
    let (tx, rx) = mpsc::channel();
    let worker = Worker::spawn_agent(
        peer_binary(),
        root.path().into(),
        29,
        tx,
        egui::Context::default(),
    );
    assert!(response(rx.recv_timeout(WAIT).unwrap()).connected);
    worker
        .tx
        .send(Command {
            id: 61,
            op: Operation::Write {
                path: "fixture.txt".into(),
                text: "acknowledged submission".into(),
                expected_revision: Some("fixture-revision".into()),
            },
        })
        .unwrap();
    assert!(matches!(
        ready_request(root.path(), 2).op,
        Operation::Write { .. }
    ));
    fs::write(root.path().join("release-2"), b"acknowledge then close\n").unwrap();
    let acknowledgement = response(rx.recv_timeout(WAIT).unwrap());
    assert_eq!(acknowledgement.generation, 29);
    assert_eq!(acknowledgement.id, 61);
    assert!(acknowledgement.connected);
    assert!(matches!(
        acknowledgement.result,
        Ok(Payload::Written { revision }) if revision == "written-revision"
    ));
    assert!(matches!(
        rx.recv_timeout(WAIT).unwrap(),
        WorkerEvent::TransportLost { generation: 29, .. }
    ));
    assert_operations(root.path(), &["hello", "write"]);
    assert!(matches!(
        rx.recv_timeout(WAIT).unwrap(),
        WorkerEvent::Closed {
            generation: 29,
            result: Ok(())
        }
    ));
    assert!(matches!(
        rx.recv_timeout(WAIT),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[test]
#[ignore = "requires CEDAR_CONNECTION_CANCEL_PEER_BIN; explicit process acceptance"]
fn dropping_healthy_idle_worker_closes_mailbox_and_process() {
    let root = fixture("capability_peer");
    let (tx, rx) = mpsc::channel();
    let worker = Worker::spawn_agent(
        peer_binary(),
        root.path().into(),
        31,
        tx,
        egui::Context::default(),
    );
    assert!(response(rx.recv_timeout(WAIT).unwrap()).connected);
    drop(worker);
    assert!(matches!(
        rx.recv_timeout(CANCEL_BOUND).unwrap(),
        WorkerEvent::Closed {
            generation: 31,
            result: Ok(())
        }
    ));
    assert!(matches!(
        rx.recv_timeout(CANCEL_BOUND),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    wait_marker(root.path(), "eof");
    assert_operations(root.path(), &["hello"]);
}
