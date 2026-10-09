//! Explicit trust-off acceptance through a normal agent and a controlled pipe.
//! The relay closes only the owned agent's stdin; no SSH or network is used.
use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(5);
const SOURCE: &str = "saved source 草稿 🐻\n";
const DRAFT: &str = "retained unsaved draft 草稿 🐻\n";

fn binary(name: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(name).expect("explicit acceptance binary required"));
    assert!(path.is_absolute() && path.is_file());
    path
}

fn wait_file(root: &Path, name: &str) -> Vec<u8> {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Ok(bytes) = fs::read(root.join(name)) {
            return bytes;
        }
        assert!(
            Instant::now() < deadline,
            "missing controlled relay marker {name}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn requests(root: &Path) -> Vec<String> {
    fs::read_to_string(root.join("requests"))
        .unwrap()
        .lines()
        .map(|line| {
            let request: cedar_protocol::Request = serde_json::from_str(line).unwrap();
            match request.op {
                Operation::Hello => "hello",
                Operation::List { .. } => "list",
                Operation::Read { .. } => "read",
                _ => panic!("idle acceptance sent an unexpected operation"),
            }
            .into()
        })
        .collect()
}

fn response(app: &mut CedarApp) {
    let event = app
        .result_rx
        .recv_timeout(WAIT)
        .expect("normal agent response timed out");
    let WorkerEvent::Response(ref reply) = event else {
        panic!("unexpected idle loss")
    };
    assert!(reply.connected && reply.result.is_ok());
    app.apply_worker_event(event);
}

fn start(app: &mut CedarApp, root: &Path, peer: &Path) {
    assert!(app.worker.is_none());
    app.generation += 1;
    app.state = ConnectionState::Connecting;
    app.connecting_form = Some(ConnectForm {
        ssh: false,
        local_root: root.to_str().unwrap().into(),
        allow_run: false,
        ..Default::default()
    });
    app.worker = Some(Worker::spawn_agent(
        peer.into(),
        root.into(),
        app.generation,
        app.result_tx.clone(),
        app.editor_ctx.clone(),
    ));
    response(app); // Hello schedules the ordinary initial List.
    response(app);
    assert!(app.ready() && app.pending.is_empty());
    assert!(!app.execution_trusted());
    assert!(app.agent_info.is_some());
}

fn close_peer(app: &mut CedarApp, root: &Path) {
    let owned_pid = String::from_utf8(wait_file(root, "relay-agent-started"))
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    fs::write(
        root.join("relay-disconnect.pending"),
        b"close-agent-stdin\n",
    )
    .unwrap();
    fs::rename(
        root.join("relay-disconnect.pending"),
        root.join("relay-disconnect"),
    )
    .unwrap();
    let event = app
        .result_rx
        .recv_timeout(WAIT)
        .expect("idle EOF needed another user operation");
    assert!(
        matches!(&event, WorkerEvent::TransportLost { generation, .. } if *generation == app.generation)
    );
    app.apply_worker_event(event);
    assert!(app.state == ConnectionState::Disconnected);
    assert!(app.worker.is_none() && app.agent_info.is_none() && app.pending.is_empty());
    let receipt: serde_json::Value =
        serde_json::from_slice(&wait_file(root, "relay-agent-reaped")).unwrap();
    assert_eq!(receipt["process_id"], owned_pid);
    assert_eq!(receipt["cleanup_verified"], true);
    assert_eq!(receipt["exit_success"], true);
    let closed = app
        .result_rx
        .recv_timeout(WAIT)
        .expect("local relay cleanup receipt missing");
    assert!(
        matches!(&closed, WorkerEvent::Closed { generation, result: Ok(()) } if *generation == app.generation)
    );
    app.apply_worker_event(closed);
}

fn frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) {
    let ctx = app.editor_ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(780.0, 540.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    );
}

fn undo_events(redo: bool) -> Vec<egui::Event> {
    let modifiers = if redo {
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
    } else {
        egui::Modifiers::COMMAND
    };
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key: egui::Key::Z,
            physical_key: Some(egui::Key::Z),
            pressed,
            repeat: false,
            modifiers,
        })
        .collect()
}

#[test]
#[ignore = "requires explicit normal agent and relay binaries; actual process acceptance"]
fn normal_agent_idle_loss_preserves_draft_and_requires_explicit_reconnect() {
    let agent = binary("CEDAR_IDLE_DISCONNECT_AGENT_BIN");
    let peer = binary("CEDAR_IDLE_DISCONNECT_PEER_BIN");
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("idle workspace 草稿");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join(".cedar-transport-fixture"),
        b"cedar-transport-fixture-v1\n",
    )
    .unwrap();
    fs::write(root.join("fixture-mode"), "normal_agent_idle_relay").unwrap();
    fs::write(root.join("relay-agent-path"), agent.to_str().unwrap()).unwrap();
    fs::write(root.join("draft.txt"), SOURCE).unwrap();
    let mut app = CedarApp::empty();
    app.open_form = false;
    start(&mut app, &root, &peer);
    app.open("draft.txt".into(), None);
    response(&mut app);
    assert_eq!(app.documents.len(), 1);
    frame(&mut app, 0.0, vec![]);
    let revision = app.documents[0].revision.clone();
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 6);
    frame(&mut app, 1.0, vec![]);
    let id = egui::Id::new(("editor", app.documents[0].id));
    let selection =
        egui::text::CCursorRange::two(egui::text::CCursor::new(3), egui::text::CCursor::new(6));
    let mut state = egui::TextEdit::load_state(&app.editor_ctx, id).unwrap();
    state.cursor.set_char_range(Some(selection));
    state.store(&app.editor_ctx, id);
    app.editor_ctx.memory_mut(|m| m.request_focus(id));
    let before = requests(&root);
    assert_eq!(before, ["hello", "list", "read"]);
    // A bounded observation window demonstrates no idle wire traffic. This is
    // a test wait, not a production heartbeat or a silent-stall guarantee.
    assert!(matches!(
        app.result_rx.recv_timeout(Duration::from_millis(250)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert_eq!(requests(&root), before);
    close_peer(&mut app, &root);
    frame(&mut app, 2.0, vec![]);
    assert_eq!(app.documents[0].text, DRAFT);
    assert_eq!(app.documents[0].saved_text, SOURCE);
    assert_eq!(app.documents[0].revision, revision);
    assert!(app.documents[0].dirty() && app.documents[0].interrupted_save.is_none());
    assert_eq!(
        egui::TextEdit::load_state(&app.editor_ctx, id)
            .unwrap()
            .cursor
            .char_range(),
        Some(selection)
    );
    frame(&mut app, 3.0, undo_events(false));
    assert_eq!(app.documents[0].text, SOURCE);
    frame(&mut app, 4.0, undo_events(true));
    assert_eq!(app.documents[0].text, DRAFT);
    assert_eq!(requests(&root), before);
    assert_eq!(fs::read_to_string(root.join("draft.txt")).unwrap(), SOURCE);

    // The test explicitly starts a new connection. No reconnect or Write was
    // emitted by the loss path. The original request audit is already checked.
    for name in [
        "relay-disconnect",
        "relay-agent-reaped",
        "relay-agent-started",
        "started",
        "requests",
    ] {
        fs::remove_file(root.join(name)).unwrap();
    }
    let old_generation = app.generation;
    start(&mut app, &root, &peer);
    assert!(app.generation > old_generation);
    app.apply_worker_event(WorkerEvent::TransportLost {
        generation: old_generation,
        message: "old relay closed".into(),
    });
    assert!(app.ready());
    app.list(String::new());
    response(&mut app);
    assert_eq!(app.documents[0].text, DRAFT);
    assert_eq!(app.documents[0].saved_text, SOURCE);
    assert_eq!(app.documents[0].revision, revision);
    assert_eq!(requests(&root), ["hello", "list", "list"]);
    close_peer(&mut app, &root);
    assert_eq!(fs::read_to_string(root.join("draft.txt")).unwrap(), SOURCE);
    drop(app);
    temp.close().unwrap();
    println!("idle_disconnect_acceptance healthy_idle_no_traffic=true idle_loss_observed=true trust_off=true normal_agent_reaped=true draft_selection_undo_retained=true explicit_reconnect=true stale_generation_ignored=true no_write_or_run=true source_unchanged=true fixture_removed=true");
}
