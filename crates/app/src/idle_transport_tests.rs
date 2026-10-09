//! Generation-bound passive loss uses the same frontend disconnect transition
//! as a failed request, without requiring another user operation.
use super::*;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

const ROOT: &str = "/synthetic-idle-workspace";
const ORIGINAL: &str = "saved text\n";
const DRAFT: &str = "retained draft 草稿 🐻\n";
const NEWER: &str = "newer unsaved draft 草稿 🐻\n";

fn app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        local_root: ROOT.into(),
        allow_run: false,
        ..Default::default()
    };
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.generation = 7;
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = ROOT.into();
    app.state = ConnectionState::Ready;
    app.open_form = false;
    app.documents.push(Document::new(
        1,
        "draft.txt".into(),
        ORIGINAL.into(),
        "r0".into(),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    (app, commands)
}

fn frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) {
    let ctx = app.editor_ctx.clone();
    let mut native_frame = eframe::Frame::_new_kittest();
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
        |ctx| eframe::App::update(app, ctx, &mut native_frame),
    );
}

fn history_key(redo: bool) -> Vec<egui::Event> {
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

fn lose(app: &CedarApp, generation: u64) {
    app.result_tx
        .send(WorkerEvent::TransportLost {
            generation,
            message: "transport_closed: peer exited while idle".into(),
        })
        .unwrap();
}

#[test]
fn idle_loss_preserves_native_selection_undo_and_unknown_task_safeguards() {
    let (mut app, commands) = app();
    frame(&mut app, 0.0, vec![]);
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 6);
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], NEWER.into(), 9);
    frame(&mut app, 1.0, vec![]);
    let id = egui::Id::new(("editor", 1u64));
    let selection =
        egui::text::CCursorRange::two(egui::text::CCursor::new(3), egui::text::CCursor::new(9));
    let mut state = egui::TextEdit::load_state(&app.editor_ctx, id).unwrap();
    state.cursor.set_char_range(Some(selection));
    state.store(&app.editor_ctx, id);
    app.editor_ctx.memory_mut(|memory| memory.request_focus(id));
    let edits = app.documents[0].edit_version;
    app.agent_info = Some(agent_support::full_test_agent());
    app.language.running = true;
    app.close_after_language_stop = true;
    app.close_snapshot = Some(app.draft_versions());
    app.run_state.snapshot = Some(cedar_tasks::TaskSnapshot {
        id: 41,
        state: cedar_tasks::TaskState::Running,
        stdout: "prior output".into(),
        stderr: String::new(),
        exit_code: None,
        windows_exit_code: None,
        truncated: false,
        error: None,
    });

    lose(&app, app.generation);
    frame(&mut app, 2.0, vec![]);
    assert!(app.state == ConnectionState::Disconnected);
    assert!(app.worker.is_none());
    assert!(app.agent_info.is_none());
    assert!(app.pending.is_empty());
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents[0].text, NEWER);
    assert_eq!(app.documents[0].saved_text, ORIGINAL);
    assert_eq!(app.documents[0].revision.as_deref(), Some("r0"));
    assert_eq!(app.documents[0].edit_version, edits);
    assert!(app.documents[0].dirty());
    assert!(app.documents[0].interrupted_save.is_none());
    assert_eq!(
        egui::TextEdit::load_state(&app.editor_ctx, id)
            .unwrap()
            .cursor
            .char_range(),
        Some(selection)
    );
    assert!(!app.language.running);
    assert!(!app.close_after_language_stop);
    assert!(app.close_snapshot.is_none());
    assert!(!app.allow_close);
    assert!(app.run_state.snapshot.is_none());
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));

    frame(&mut app, 3.0, history_key(false));
    assert_eq!(app.documents[0].text, DRAFT);
    frame(&mut app, 4.0, history_key(true));
    assert_eq!(app.documents[0].text, NEWER);
    assert_eq!(app.documents[0].saved_text, ORIGINAL);
    assert!(!app.guard_run_transition(run_ui::Transition::Reconnect));
    assert!(app.run_state.transition_pending());
}

#[test]
fn healthy_idle_frames_send_no_requests_and_stale_loss_cannot_disconnect_replacement() {
    let (mut app, commands) = app();
    for time in [0.0, 1.0, 60.0, 3600.0] {
        frame(&mut app, time, vec![]);
    }
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    app.list("requested".into());
    let command = commands.try_recv().unwrap();
    lose(&app, app.generation - 1);
    app.poll();
    assert!(app.ready());
    assert!(app.worker.is_some());
    assert!(app.pending.contains_key(&command.id));

    // A queued old reader notification also cannot revoke a newer connect.
    app.state = ConnectionState::Connecting;
    app.connecting_form = app.active_form.clone();
    lose(&app, app.generation - 1);
    app.poll();
    assert!(app.state == ConnectionState::Connecting);
    assert!(app.worker.is_some());
    assert!(app.connecting_form.is_some());

    lose(&app, app.generation);
    app.poll();
    assert!(app.state == ConnectionState::Disconnected);
    app.error = Some("a newer explanation".into());
    lose(&app, app.generation);
    app.poll();
    assert_eq!(app.error.as_deref(), Some("a newer explanation"));
}

#[test]
fn acknowledged_write_before_terminal_event_keeps_saved_revision_and_later_edits() {
    let (mut app, commands) = app();
    let revision = format!("{:x}", Sha256::digest(DRAFT.as_bytes()));
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 6);
    app.save();
    let write = commands.try_recv().unwrap();
    assert!(matches!(write.op, Operation::Write { .. }));
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], NEWER.into(), 9);
    app.result_tx
        .send(WorkerEvent::Response(Event {
            generation: app.generation,
            id: write.id,
            connected: true,
            result: Ok(Payload::Written {
                revision: revision.clone(),
            }),
        }))
        .unwrap();
    lose(&app, app.generation);
    app.poll();
    assert!(app.state == ConnectionState::Disconnected);
    assert_eq!(app.documents[0].text, NEWER);
    assert_eq!(app.documents[0].saved_text, DRAFT);
    assert_eq!(
        app.documents[0].revision.as_deref(),
        Some(revision.as_str())
    );
    assert!(app.documents[0].dirty());
    assert!(!app.documents[0].saving);
    assert!(app.documents[0].interrupted_save.is_none());
    assert!(app.pending.is_empty());
    // The ordinary acknowledged-save refresh is the only follow-up; loss does
    // not replay the Write or trigger a reconnect.
    assert!(matches!(
        commands.try_recv().unwrap().op,
        Operation::List { .. }
    ));
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
fn cancelled_attempt_ignores_its_late_loss_and_hello() {
    let (mut app, commands) = app();
    app.state = ConnectionState::Connecting;
    app.connecting_form = app.active_form.clone();
    let cancelled = app.generation;
    app.cancel_connection();
    assert_eq!(app.generation, cancelled + 1);
    lose(&app, cancelled);
    app.result_tx
        .send(WorkerEvent::Response(Event {
            generation: cancelled,
            id: 0,
            connected: true,
            result: Ok(Payload::Hello {
                protocol: cedar_protocol::PROTOCOL_VERSION,
                root: "/wrong-root".into(),
                agent: None,
            }),
        }))
        .unwrap();
    app.poll();
    assert!(app.state == ConnectionState::Disconnected);
    assert!(app.worker.is_none());
    assert_eq!(app.root, ROOT);
    assert_eq!(app.documents[0].text, ORIGINAL);
    assert_eq!(app.notice, "Connection cancelled");
    assert!(app.error.is_none());
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
fn unacknowledged_write_loss_retains_unknown_save_and_does_not_retry() {
    let (mut app, commands) = app();
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 6);
    app.save();
    let write = commands.try_recv().unwrap();
    assert!(matches!(write.op, Operation::Write { .. }));
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], NEWER.into(), 9);
    lose(&app, app.generation);
    app.poll();
    assert!(app.state == ConnectionState::Disconnected);
    assert_eq!(app.documents[0].text, NEWER);
    assert_eq!(app.documents[0].saved_text, ORIGINAL);
    assert_eq!(app.documents[0].revision.as_deref(), Some("r0"));
    assert!(!app.documents[0].saving);
    assert!(app.documents[0].interrupted_save.is_some());
    let interrupted = app.documents[0].interrupted_save.clone();
    app.save();
    assert_eq!(app.documents[0].interrupted_save, interrupted);
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

fn wait_recovery(app: &mut CedarApp, done: impl Fn(&CedarApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        app.recovery_tick(&app.editor_ctx.clone());
        assert!(
            Instant::now() < deadline,
            "recovery acknowledgement timed out"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn idle_loss_retains_owned_recovery_and_original_workspace_identity() {
    let (mut app, commands) = app();
    let temp = tempfile::tempdir().unwrap();
    let recovery = temp.path().join("recovery");
    app.recovery.start(Ok(recovery.clone()), &app.editor_ctx);
    wait_recovery(&mut app, |app| app.recovery.initialized);
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 6);
    app.recovery_tick(&app.editor_ctx.clone());
    app.recovery.flush();
    let workspace = app.recovery_workspace().unwrap();
    wait_recovery(&mut app, |app| {
        app.recovery.protected(&workspace, &app.documents[0])
    });

    lose(&app, app.generation);
    app.poll();
    app.recovery_tick(&app.editor_ctx.clone());
    assert_eq!(app.recovery_workspace(), Some(workspace.clone()));
    assert!(app.recovery.protected(&workspace, &app.documents[0]));
    assert_eq!(app.root, ROOT);
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    drop(app);
    let store = cedar_recovery::Store::open(recovery).unwrap();
    let draft = store
        .read(&cedar_recovery::record_id(&workspace, "draft.txt").unwrap())
        .unwrap();
    assert_eq!(draft.text, DRAFT);
    assert_eq!(draft.base_text, ORIGINAL);
    assert_eq!(draft.base_revision.as_deref(), Some("r0"));
}
