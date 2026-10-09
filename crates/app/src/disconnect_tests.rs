//! Explicit Disconnect admission, ownership receipts, and retained local drafts.
//! Recording workers never spawn a process; tests inject their terminal receipt.
use super::*;
use cedar_recovery::{record_id, Draft, Store};
use cedar_tasks::{TaskSnapshot, TaskState};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

const ROOT: &str = "/synthetic-explicit-disconnect";
const BASE: &str = "original disk text\n";
const DRAFT: &str = "retained draft 草稿 🐻\n";
const NEWER: &str = "newer retained draft 草稿 🐻\n";

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
    app.active_form = Some(form.clone());
    app.form = form;
    app.root = ROOT.into();
    app.state = ConnectionState::Ready;
    app.agent_info = Some(agent_support::full_test_agent());
    app.open_form = false;
    app.profiles.connected(app.recovery_workspace().unwrap());
    app.documents.push(Document::new(
        1,
        "draft.txt".into(),
        BASE.into(),
        "r0".into(),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    (app, commands)
}

#[derive(Debug, PartialEq, Eq)]
struct Buffer {
    id: u64,
    path: String,
    text: String,
    saved_text: String,
    revision: Option<String>,
    edit_version: u64,
    saving: bool,
    interrupted: Option<interrupted_save::InterruptedSave>,
    unverifiable: bool,
    undo_initialized: bool,
}

fn buffers(app: &CedarApp) -> Vec<Buffer> {
    app.documents
        .iter()
        .map(|doc| Buffer {
            id: doc.id,
            path: doc.path.clone(),
            text: doc.text.clone(),
            saved_text: doc.saved_text.clone(),
            revision: doc.revision.clone(),
            edit_version: doc.edit_version,
            saving: doc.saving,
            interrupted: doc.interrupted_save.clone(),
            unverifiable: doc.save_outcome_unverifiable,
            undo_initialized: doc.undo_initialized,
        })
        .collect()
}

fn frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) -> egui::FullOutput {
    frame_with(app, time, events, |_| ()).0
}

fn frame_with<T>(
    app: &mut CedarApp,
    time: f64,
    events: Vec<egui::Event>,
    mut inspect: impl FnMut(&CedarApp) -> T,
) -> (egui::FullOutput, T) {
    let ctx = app.editor_ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    let mut inspected = None;
    let output = ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 1000.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            eframe::App::update(app, ctx, &mut native);
            inspected = Some(inspect(app));
        },
    );
    (output, inspected.unwrap())
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers,
        })
        .collect()
}

fn click_at(app: &mut CedarApp, time: f64, at: egui::Pos2) {
    for (offset, pressed) in [true, false].into_iter().enumerate() {
        frame(
            app,
            time + offset as f64 * 0.01,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

fn rendered(output: &egui::FullOutput) -> String {
    fn collect(shape: &egui::epaint::Shape, text: &mut String) {
        match shape {
            egui::epaint::Shape::Text(value) => {
                text.push_str(&value.galley.job.text);
                text.push('\n');
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, text);
                }
            }
            _ => {}
        }
    }
    let mut text = String::new();
    for shape in &output.shapes {
        collect(&shape.shape, &mut text);
    }
    text
}

fn closed(app: &mut CedarApp, generation: u64, result: Result<(), String>) {
    app.apply_worker_event(WorkerEvent::Closed { generation, result });
}

fn reply(app: &mut CedarApp, id: u64, payload: Payload) {
    app.apply_event(Event {
        generation: app.generation,
        id,
        connected: true,
        result: Ok(payload),
    });
}

fn file(path: &str, text: &str) -> Payload {
    Payload::File {
        path: path.into(),
        text: text.into(),
        revision: format!("{:x}", Sha256::digest(text.as_bytes())),
    }
}

fn task(state: TaskState) -> TaskSnapshot {
    TaskSnapshot {
        id: 41,
        state,
        stdout: "prior output".into(),
        stderr: String::new(),
        exit_code: None,
        windows_exit_code: None,
        truncated: false,
        error: None,
    }
}

fn no_commands(commands: &Receiver<Command>) {
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected)
    ));
}

fn mark_interrupted(app: &mut CedarApp) {
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 5);
    let token = interrupted_save::InterruptedSave::capture(app, &app.documents[0]).unwrap();
    app.documents[0].interrupted_save = Some(token);
}

#[test]
fn explicit_disconnect_refuses_every_pending_mutation_without_dropping_ownership() {
    for kind in [
        "save", "git", "language", "start", "poll", "cancel", "saving",
    ] {
        let (mut app, commands) = app();
        editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 5);
        let job = match kind {
            "save" => Some(Job::Save {
                document: 1,
                snapshot: DRAFT.into(),
                submission: interrupted_save::InterruptedSave::capture(&app, &app.documents[0]),
            }),
            "git" => Some(Job::Git),
            "language" => Some(Job::Language(language_ui::Action {
                session: app.language.session,
                kind: language_ui::ActionKind::Events,
            })),
            "start" | "poll" | "cancel" => Some(Job::Run(run_ui::Action {
                epoch: 0,
                kind: match kind {
                    "start" => run_ui::Kind::Start,
                    "poll" => run_ui::Kind::Poll(41),
                    _ => run_ui::Kind::Cancel(41),
                },
            })),
            "saving" => {
                app.documents[0].saving = true;
                None
            }
            _ => unreachable!(),
        };
        if let Some(job) = job {
            app.pending.insert(19, job);
        }
        let before = buffers(&app);
        let count = app.pending.len();
        assert!(app.disconnect_problem().is_some(), "{kind}");
        app.disconnect_idle();
        assert!(app.ready(), "{kind}");
        assert_eq!(app.generation, 7, "{kind}");
        assert!(app.worker.is_some(), "{kind}");
        assert_eq!(app.pending.len(), count, "{kind}");
        assert_eq!(buffers(&app), before, "{kind}");
        no_commands(&commands);
    }
}

#[test]
fn explicit_disconnect_refuses_active_unknown_tasks_language_and_close_transitions() {
    for state in [
        TaskState::Starting,
        TaskState::Running,
        TaskState::Cancelling,
    ] {
        let (mut app, commands) = app();
        app.run_state.snapshot = Some(task(state));
        app.disconnect_idle();
        assert!(app.ready());
        assert!(app.worker.is_some());
        assert_eq!(app.run_state.snapshot.as_ref().unwrap().state, state);
        no_commands(&commands);
    }
    {
        let (mut app, commands) = app();
        app.run_error(
            &run_ui::Action {
                epoch: 0,
                kind: run_ui::Kind::Poll(41),
            },
            true,
            "synthetic task status failure",
        );
        assert!(app
            .disconnect_problem()
            .unwrap()
            .contains("Unknown command outcomes"));
        app.disconnect_idle();
        assert!(app.ready());
        no_commands(&commands);
    }

    for reason in [
        "language",
        "confirm",
        "language_close",
        "snapshot",
        "recovery_close",
        "restore",
    ] {
        let (mut app, commands) = app();
        match reason {
            "language" => app.language.running = true,
            "confirm" => app.confirm = Some(Confirm::CloseWindow),
            "language_close" => app.close_after_language_stop = true,
            "snapshot" => app.close_snapshot = Some(app.draft_versions()),
            "recovery_close" => app.recovery.closing = Some(app.draft_versions()),
            "restore" => app.recovery.restoring_generation = Some(app.generation),
            _ => unreachable!(),
        }
        app.disconnect_idle();
        assert!(app.ready(), "{reason}");
        assert!(app.worker.is_some(), "{reason}");
        no_commands(&commands);
    }
}

#[test]
fn explicit_disconnect_accepts_every_terminal_task_without_issuing_cancel_or_stop() {
    for state in [
        TaskState::Succeeded,
        TaskState::Failed,
        TaskState::Cancelled,
        TaskState::TimedOut,
        TaskState::OutputLimit,
        TaskState::SpawnFailed,
    ] {
        let (mut app, commands) = app();
        app.run_state.snapshot = Some(task(state));
        assert!(app.disconnect_problem().is_none());
        app.disconnect_idle();
        assert!(app.state == ConnectionState::Disconnecting);
        assert_eq!(app.generation, 7);
        assert!(app.worker.is_none());
        assert!(matches!(
            commands.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
}

#[test]
fn explicit_disconnect_requires_matching_receipt_and_ignores_late_responses_eof_and_duplicates() {
    let (mut app, commands) = app();
    frame(&mut app, 0.0, vec![]);
    app.open("late.txt".into(), Some(2));
    let read = commands.try_recv().unwrap();
    assert!(matches!(read.op, Operation::Read { .. }));
    let before = buffers(&app);
    app.disconnect_idle();
    assert!(app.state == ConnectionState::Disconnecting);
    assert!(app.pending.is_empty());
    assert!(app.agent_info.is_none());
    assert!(app.worker.is_none());
    let generation = app.generation;
    reply(&mut app, read.id, file("late.txt", "must not open\n"));
    app.apply_worker_event(WorkerEvent::TransportLost {
        generation,
        message: "EOF is not a cleanup receipt".into(),
    });
    app.apply_event(Event {
        generation,
        id: read.id,
        connected: false,
        result: Err("late failed read".into()),
    });
    closed(&mut app, generation - 1, Ok(()));
    closed(&mut app, generation + 1, Err("unrelated cleanup".into()));
    app.disconnect_idle();
    for time in [1.0, 2.0, 3600.0] {
        frame(&mut app, time, vec![]);
        assert!(app.state == ConnectionState::Disconnecting);
    }
    assert_eq!(buffers(&app), before);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.generation, generation);
    no_commands(&commands);
    closed(&mut app, generation, Ok(()));
    assert!(app.state == ConnectionState::Disconnected);
    assert!(app.notice.contains("local connection cleanup confirmed"));
    let notice = app.notice.clone();
    closed(&mut app, generation, Err("duplicate failure".into()));
    app.apply_worker_event(WorkerEvent::TransportLost {
        generation,
        message: "duplicate EOF".into(),
    });
    reply(&mut app, read.id, file("late.txt", "still must not open\n"));
    assert_eq!(app.notice, notice);
    assert!(!app.unverified_local_close);
    assert_eq!(buffers(&app), before);
}

#[test]
fn explicit_disconnect_blocks_connect_and_window_close_until_terminal_receipt() {
    let (mut app, commands) = app();
    app.disconnect_idle();
    let ctx = app.editor_ctx.clone();
    let form = app.active_form.clone().unwrap();
    app.connect(&ctx, form);
    assert_eq!(app.error.as_deref(), Some(disconnect::WAITING));
    assert!(app.state == ConnectionState::Disconnecting);
    assert!(app.worker.is_none());
    app.cancel_connection();
    app.request_window_close(&ctx);
    app.begin_close(&ctx);
    assert!(!app.allow_close);
    assert!(app.confirm.is_none());
    assert!(app.recovery.closing.is_none());
    assert_eq!(app.generation, 7);
    no_commands(&commands);
}

#[test]
fn current_connecting_worker_terminal_error_cannot_leave_an_attempt_stuck() {
    let (mut app, commands) = app();
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 5);
    app.state = ConnectionState::Connecting;
    app.connecting_form = app.active_form.clone();
    app.agent_info = None;
    let before = buffers(&app);
    let generation = app.generation;

    closed(
        &mut app,
        generation - 1,
        Err("old private cleanup details".into()),
    );
    assert!(app.state == ConnectionState::Connecting);
    assert!(app.connecting_form.is_some());
    assert!(app.worker.is_some());
    assert!(!app.unverified_local_close);
    no_commands(&commands);

    closed(
        &mut app,
        generation,
        Err("current private cleanup details".into()),
    );
    assert!(app.state == ConnectionState::Disconnected);
    assert!(app.connecting_form.is_none());
    assert!(app.worker.is_none());
    assert!(app.agent_info.is_none());
    assert!(app.pending.is_empty());
    assert!(app.unverified_local_close);
    assert_eq!(app.generation, generation);
    assert_eq!(buffers(&app), before);
    assert_eq!(
        app.error.as_deref(),
        Some("The connection worker exited. Your drafts are retained; no operation was retried.")
    );
    assert!(!app.notice.contains("private cleanup details"));
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));

    // A late handshake and duplicate receipt cannot resurrect this attempt or
    // replace its fixed explanation with unsanitized worker error text.
    reply(
        &mut app,
        0,
        Payload::Hello {
            protocol: cedar_protocol::PROTOCOL_VERSION,
            root: ROOT.into(),
            agent: Some(agent_support::full_test_agent()),
        },
    );
    closed(&mut app, generation, Ok(()));
    assert!(app.state == ConnectionState::Disconnected);
    assert!(app.unverified_local_close);
    assert_eq!(buffers(&app), before);
    assert_eq!(
        app.error.as_deref(),
        Some("The connection worker exited. Your drafts are retained; no operation was retried.")
    );
}

#[test]
fn cancelled_connecting_attempt_terminal_error_is_inert_for_offline_and_replacement_states() {
    let (mut app, commands) = app();
    app.state = ConnectionState::Connecting;
    app.connecting_form = app.active_form.clone();
    let cancelled = app.generation;
    let before = buffers(&app);
    app.cancel_connection();
    assert_eq!(app.generation, cancelled + 1);
    let notice = app.notice.clone();
    app.error = Some("newer explanation".into());
    closed(
        &mut app,
        cancelled,
        Err("cancelled private cleanup details".into()),
    );
    assert!(app.state == ConnectionState::Disconnected);
    assert_eq!(app.notice, notice);
    assert_eq!(app.error.as_deref(), Some("newer explanation"));
    assert!(!app.unverified_local_close);
    assert_eq!(buffers(&app), before);
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));

    let (worker, replacement) = Worker::recording();
    app.worker = Some(worker);
    app.state = ConnectionState::Connecting;
    app.connecting_form = app.active_form.clone();
    closed(
        &mut app,
        cancelled,
        Err("old attempt failed after replacement".into()),
    );
    closed(&mut app, cancelled, Ok(()));
    assert!(app.state == ConnectionState::Connecting);
    assert!(app.connecting_form.is_some());
    assert!(app.worker.is_some());
    assert!(!app.unverified_local_close);
    assert_eq!(app.error.as_deref(), Some("newer explanation"));
    assert_eq!(buffers(&app), before);
    no_commands(&replacement);
}

#[test]
fn cleanup_unverified_is_sticky_across_terminal_duplicates_and_replacement_hello() {
    let (mut app, commands) = app();
    app.disconnect_idle();
    let old = app.generation;
    closed(&mut app, old, Err("private raw cleanup details".into()));
    assert!(app.state == ConnectionState::CleanupUnverified);
    assert!(app.unverified_local_close);
    assert_eq!(app.error.as_deref(), Some(disconnect::UNVERIFIED));
    closed(&mut app, old, Ok(()));
    assert!(app.state == ConnectionState::CleanupUnverified);
    assert!(app.unverified_local_close);
    no_commands(&commands);

    // A terminal failure no longer blocks the explicit Connect entry point.
    // Invalid form validation proves admission without spawning any process.
    let invalid = ConnectForm {
        local_root: String::new(),
        ..app.form.clone()
    };
    app.connect(&egui::Context::default(), invalid);
    assert!(app
        .error
        .as_deref()
        .unwrap()
        .contains("Choose a local workspace"));
    assert!(app.unverified_local_close);
    assert_eq!(app.generation, old);

    // Replace the transport with a recording worker and deliver a normal Hello.
    // This is a frontend receipt test, not a production connection acceptance.
    let (worker, replacement) = Worker::recording();
    app.worker = Some(worker);
    app.generation += 1;
    app.state = ConnectionState::Connecting;
    app.connecting_form = app.active_form.clone();
    reply(
        &mut app,
        0,
        Payload::Hello {
            protocol: cedar_protocol::PROTOCOL_VERSION,
            root: ROOT.into(),
            agent: Some(agent_support::full_test_agent()),
        },
    );
    assert!(app.ready());
    assert!(app.unverified_local_close);
    assert!(app.error.is_none());
    assert!(matches!(
        replacement.try_recv().unwrap().op,
        Operation::List { .. }
    ));
    closed(&mut app, old, Ok(()));
    closed(&mut app, old, Err("old failure".into()));
    assert!(app.ready());
    assert!(app.unverified_local_close);
    let output = frame(&mut app, 1.0, vec![]);
    assert!(rendered(&output).contains("Prior cleanup unverified"));
    no_commands(&replacement);
}

#[test]
fn successful_ssh_disconnect_reports_only_local_transport_cleanup() {
    let (mut app, commands) = app();
    let form = app.active_form.as_mut().unwrap();
    form.ssh = true;
    form.host = "synthetic-host".into();
    form.remote_root = ROOT.into();
    app.workspace_key = Some(form.key());
    app.disconnect_idle();
    let generation = app.generation;
    closed(&mut app, generation, Ok(()));
    assert!(app.state == ConnectionState::Disconnected);
    assert!(app.notice.contains("local SSH client closed"));
    assert!(app.notice.contains("remote cleanup is not verified"));
    assert!(!app.unverified_local_close);
    no_commands(&commands);
}

#[test]
fn disconnect_preserves_native_selection_affinity_undo_and_offline_back_forward() {
    let (mut app, commands) = app();
    frame(&mut app, 0.0, vec![]);
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 5);
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], NEWER.into(), 9);
    frame(&mut app, 1.0, vec![]);
    let id = egui::Id::new(("editor", 1u64));
    let mut selection =
        egui::text::CCursorRange::two(egui::text::CCursor::new(3), egui::text::CCursor::new(9));
    selection.primary.prefer_next_row = true;
    selection.secondary.prefer_next_row = false;
    let mut state = egui::TextEdit::load_state(&app.editor_ctx, id).unwrap();
    state.cursor.set_char_range(Some(selection));
    state.store(&app.editor_ctx, id);
    app.editor_ctx.memory_mut(|memory| memory.request_focus(id));
    app.documents.push(Document::new(
        2,
        "other.txt".into(),
        "other buffer\n".into(),
        "r2".into(),
    ));
    app.next_document = 3;
    app.location_history.back.push(location_history::Location {
        generation: app.generation,
        document: 2,
        edit_version: 0,
        selection: egui::text::CCursorRange::two(
            egui::text::CCursor::new(1),
            egui::text::CCursor::new(4),
        ),
    });
    let before = buffers(&app);
    let workspace = app.recovery_workspace();
    app.disconnect_idle();
    assert_eq!(app.generation, 7);
    assert_eq!(app.recovery_workspace(), workspace);
    assert_eq!(buffers(&app), before);
    assert_eq!(app.location_history.back.len(), 1);
    let retained = egui::TextEdit::load_state(&app.editor_ctx, id)
        .unwrap()
        .cursor
        .char_range()
        .unwrap();
    assert!(location_history::same_selection(selection, retained));
    closed(&mut app, 7, Ok(()));
    frame(
        &mut app,
        2.0,
        key(egui::Key::OpenBracket, egui::Modifiers::COMMAND),
    );
    assert_eq!(app.active_document, Some(2));
    frame(
        &mut app,
        3.0,
        key(egui::Key::CloseBracket, egui::Modifiers::COMMAND),
    );
    assert_eq!(app.active_document, Some(1));
    let returned = egui::TextEdit::load_state(&app.editor_ctx, id)
        .unwrap()
        .cursor
        .char_range()
        .unwrap();
    assert!(location_history::same_selection(selection, returned));
    frame(&mut app, 4.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, DRAFT);
    frame(
        &mut app,
        5.0,
        key(
            egui::Key::Z,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        ),
    );
    assert_eq!(app.documents[0].text, NEWER);
    assert_eq!(app.documents[0].saved_text, BASE);
    assert_eq!(app.documents[0].revision.as_deref(), Some("r0"));
    assert_eq!(app.generation, 7);
    no_commands(&commands);
}

#[test]
fn disconnect_preserves_unknown_save_identity_and_unverifiable_new_buffers() {
    for success in [false, true] {
        let (mut app, commands) = app();
        mark_interrupted(&mut app);
        let mut new = Document::new(2, "new.txt".into(), String::new(), String::new());
        new.revision = None;
        new.text = "unsaved new file".into();
        new.save_outcome_unverifiable = true;
        app.documents.push(new);
        app.next_document = 3;
        let before = buffers(&app);
        let workspace = app.recovery_workspace();
        assert!(app.disconnect_problem().is_none());
        app.disconnect_idle();
        assert_eq!(buffers(&app), before);
        closed(
            &mut app,
            7,
            if success {
                Ok(())
            } else {
                Err("unverified".into())
            },
        );
        assert_eq!(buffers(&app), before);
        assert_eq!(app.recovery_workspace(), workspace);
        assert!(app.documents.iter().all(Document::save_outcome_unknown));
        no_commands(&commands);
    }
}

#[test]
fn disconnect_cancels_interrupted_save_reads_and_staged_baseline_adoption() {
    for phase in [0, 1, 2] {
        let (mut app, commands) = app();
        mark_interrupted(&mut app);
        let before = buffers(&app);
        app.check_interrupted_save();
        let first = commands.try_recv().unwrap();
        assert!(matches!(first.op, Operation::Read { .. }));
        let mut late = first.id;
        if phase > 0 {
            reply(&mut app, first.id, file("draft.txt", DRAFT));
            let second = commands.try_recv().unwrap();
            assert!(matches!(second.op, Operation::Read { .. }));
            late = second.id;
            if phase == 2 {
                reply(&mut app, second.id, file("draft.txt", DRAFT));
            }
        }
        assert!(app.interrupted_save_check.busy());
        app.disconnect_idle();
        assert!(!app.interrupted_save_check.busy());
        reply(&mut app, late, file("draft.txt", DRAFT));
        app.finish_interrupted_save_check();
        assert_eq!(buffers(&app), before, "phase {phase}");
        closed(&mut app, 7, Ok(()));
        reply(&mut app, late, file("draft.txt", DRAFT));
        app.finish_interrupted_save_check();
        assert_eq!(buffers(&app), before, "phase {phase}");
        no_commands(&commands);
    }
}

#[test]
fn disconnect_cancels_profile_report_disk_and_search_reads_without_adoption() {
    for kind in ["profiles", "report", "disk", "search", "list"] {
        let (mut app, commands) = app();
        match kind {
            "profiles" => app.load_profiles(),
            "report" => {
                app.test_report.path = "TEST-late.xml".into();
                app.load_test_report();
            }
            "disk" => app.compare_with_disk(),
            "search" => {
                app.search_query = "late".into();
                app.search();
            }
            "list" => app.list("late-dir".into()),
            _ => unreachable!(),
        }
        let read = commands.try_recv().unwrap();
        let before = buffers(&app);
        assert!(app.disconnect_problem().is_none(), "{kind}");
        app.disconnect_idle();
        let payload = match kind {
            "profiles" => file(profile_ui::PATH, r#"{"version":1,"profiles":[]}"#),
            "report" => file(
                "TEST-late.xml",
                r#"<testsuite name="late"><testcase name="late"/></testsuite>"#,
            ),
            "disk" => file("draft.txt", "late replacement"),
            "search" => Payload::Matches {
                matches: vec![SearchMatch {
                    path: "late.txt".into(),
                    line: 1,
                    text: "late result".into(),
                }],
                truncated: false,
            },
            "list" => Payload::Entries {
                entries: vec![Entry {
                    path: "late-dir/late.txt".into(),
                    name: "late.txt".into(),
                    is_dir: false,
                }],
            },
            _ => unreachable!(),
        };
        reply(&mut app, read.id, payload);
        app.finish_disk_reload(&egui::Context::default());
        app.finish_disk_merge(&egui::Context::default());
        app.finish_profile_actions();
        assert_eq!(buffers(&app), before, "{kind}");
        assert!(app.pending.is_empty());
        assert!(!app.disk_review.busy());
        assert!(app.test_report.loading.is_none());
        assert!(app.test_report.snapshot.is_none());
        assert!(app.search_results.is_empty());
        assert!(app.entries.is_empty());
        no_commands(&commands);
    }
}

#[test]
fn disconnect_retains_profile_form_raw_draft_and_requires_connection_review() {
    let (mut app, commands) = app();
    let raw = r#"{"version":1,"profiles":[{"name":"Build","program":"cargo","args":["check"],"timeout_secs":30}]}"#;
    app.load_profiles();
    let read = commands.try_recv().unwrap();
    reply(&mut app, read.id, file(profile_ui::PATH, raw));
    app.select_profile(Some(0));
    app.profiles.draft.args.push("retained form edit".into());
    app.profiles.changed();
    let raw_id = app
        .documents
        .iter()
        .position(|doc| doc.path == profile_ui::PATH)
        .unwrap();
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[raw_id],
        format!("{raw}\n"),
        0,
    );
    let form = app.profiles.draft.clone();
    let before = buffers(&app);
    assert!(app.profiles.dirty());
    app.disconnect_idle();
    closed(&mut app, 7, Ok(()));
    assert_eq!(app.profiles.draft, form);
    assert!(app.profiles.dirty());
    assert_eq!(buffers(&app), before);
    assert!(app
        .profile_run_problem()
        .unwrap()
        .contains("Connection changed"));
    no_commands(&commands);
}

fn wait_recovery(app: &mut CedarApp, done: impl Fn(&CedarApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.recovery_tick(&egui::Context::default());
        if done(app) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "recovery timed out: {:?}",
            app.recovery.error
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn disconnect_retains_owned_recovery_and_offline_edits_keep_same_record() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let (mut app, commands) = app();
    app.recovery.start(Ok(path.clone()), &app.editor_ctx);
    wait_recovery(&mut app, |app| {
        app.recovery.initialized || app.recovery.error.is_some()
    });
    assert!(app.recovery.error.is_none());
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 5);
    let workspace = app.recovery_workspace().unwrap();
    wait_recovery(&mut app, |app| {
        app.recovery.protected(&workspace, &app.documents[0])
    });
    app.disconnect_idle();
    assert!(app.recovery.protected(&workspace, &app.documents[0]));
    assert!(app.recovery.closing.is_none());
    closed(&mut app, 7, Err("synthetic unverified close".into()));
    assert!(app.recovery.protected(&workspace, &app.documents[0]));
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], NEWER.into(), 9);
    wait_recovery(&mut app, |app| {
        app.recovery.protected(&workspace, &app.documents[0])
    });
    assert_eq!(app.recovery_workspace(), Some(workspace.clone()));
    no_commands(&commands);
    drop(app);
    let store = Store::open(path).unwrap();
    let saved = store
        .read(&record_id(&workspace, "draft.txt").unwrap())
        .unwrap();
    assert_eq!(saved.text, NEWER);
    assert_eq!(saved.base_text, BASE);
    assert_eq!(saved.base_revision.as_deref(), Some("r0"));
}

#[test]
fn disconnect_button_is_enabled_only_when_idle_and_shows_disabled_reason() {
    fn button(app: &mut CedarApp, time: f64) -> (bool, egui::Rect) {
        frame_with(app, time, vec![], |app| {
            // egui 0.31 swaps its pass stores in end_pass; read_response after
            // run can return the previous pass, including Window's disabled
            // sizing pass. Inspect inside the production pass and retain only
            // detached values, never a Response owning this same Context.
            let response = workspace_access_tests::recorded_response(app, "Disconnect");
            (response.enabled(), response.rect)
        })
        .1
    }

    let (mut app, commands) = app();
    app.open_form = true;
    frame(&mut app, 0.0, vec![]);
    assert!(button(&mut app, 0.01).0);
    app.documents[0].saving = true;
    let (enabled, rect) = button(&mut app, 0.1);
    assert!(!enabled);
    let reason = app.disconnect_problem().unwrap();
    let at = rect.center();
    let delay = f64::from(app.editor_ctx.style().interaction.tooltip_delay);
    frame(&mut app, 0.2, vec![egui::Event::PointerMoved(at)]);
    frame(&mut app, 1.2 + delay, vec![]);
    let hovered = frame(&mut app, 1.3 + delay, vec![]);
    assert!(rendered(&hovered).contains(reason));
    click_at(&mut app, 2.0 + delay, at);
    assert!(app.ready());
    assert!(app.worker.is_some());
    no_commands(&commands);
    app.documents[0].saving = false;
    let (enabled, rect) = button(&mut app, 3.0 + delay);
    assert!(enabled);
    let at = rect.center();
    click_at(&mut app, 4.0 + delay, at);
    assert!(app.state == ConnectionState::Disconnecting);
    assert!(!button(&mut app, 5.0 + delay).0);
    assert!(app.worker.is_none());
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
fn queued_save_ack_is_validated_before_same_frame_disconnect_click() {
    for valid in [false, true] {
        let (mut app, commands) = app();
        // Acknowledging the save should only mark Tree mode stale; an automatic
        // Flat-mode directory read would obscure unexpected worker commands.
        app.explorer.mode = explorer_tree::Mode::Tree;
        app.open_form = true;
        frame(&mut app, 0.0, vec![]);
        let (_, (enabled, rect)) = frame_with(&mut app, 0.01, vec![], |app| {
            let response = workspace_access_tests::recorded_response(app, "Disconnect");
            (response.enabled(), response.rect)
        });
        assert!(enabled);
        let at = rect.center();
        let pointer = |pressed| {
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 5);
        frame(&mut app, 0.1, pointer(true));
        assert!(app.ready());

        // Queue the real save and its receipt between pointer-down and release.
        // The release frame must drain and validate it before the actual button
        // handler decides admission, rather than cancelling an unfinished save.
        app.save_document(1);
        let command = commands.try_recv().expect("save sends one Write");
        assert!(matches!(
            &command.op,
            Operation::Write { path, text, expected_revision }
                if path == "draft.txt" && text == DRAFT
                    && expected_revision.as_deref() == Some("r0")
        ));
        let Some(Job::Save {
            document,
            snapshot,
            submission: Some(submission),
        }) = app.pending.get(&command.id)
        else {
            panic!("save must retain its submitted snapshot and identity");
        };
        assert_eq!(*document, 1);
        assert_eq!(snapshot, DRAFT);
        let submission = submission.clone();
        assert!(app.documents[0].saving);
        assert!(app.disconnect_problem().is_some());
        no_commands(&commands);

        editor_state::commit(&app.editor_ctx, &mut app.documents[0], NEWER.into(), 9);
        let editor = egui::Id::new(("editor", 1u64));
        let mut selection =
            egui::text::CCursorRange::two(egui::text::CCursor::new(3), egui::text::CCursor::new(9));
        selection.primary.prefer_next_row = true;
        selection.secondary.prefer_next_row = false;
        let mut state = egui::TextEdit::load_state(&app.editor_ctx, editor).unwrap();
        state.cursor.set_char_range(Some(selection));
        state.store(&app.editor_ctx, editor);
        let version = app.documents[0].edit_version;
        let revision = format!("{:x}", Sha256::digest(DRAFT.as_bytes()));
        app.result_tx
            .send(WorkerEvent::Response(Event {
                generation: app.generation,
                id: command.id,
                connected: true,
                result: Ok(Payload::Written {
                    revision: if valid {
                        revision.clone()
                    } else {
                        "g".repeat(64)
                    },
                }),
            }))
            .unwrap();
        let (_, (enabled, clicked)) = frame_with(&mut app, 0.2, pointer(false), |app| {
            let response = workspace_access_tests::recorded_response(app, "Disconnect");
            (response.enabled(), response.clicked())
        });
        assert!(enabled && clicked);
        assert!(app.state == ConnectionState::Disconnecting);
        assert!(app.worker.is_none());
        assert!(app.pending.is_empty());
        assert_eq!(app.generation, 7);
        assert_eq!(app.documents[0].text, NEWER);
        assert_eq!(app.documents[0].edit_version, version);
        assert!(!app.documents[0].saving);
        assert!(app.documents[0].dirty());
        let retained = egui::TextEdit::load_state(&app.editor_ctx, editor)
            .unwrap()
            .cursor
            .char_range()
            .unwrap();
        assert!(location_history::same_selection(selection, retained));
        let assert_baseline = |app: &CedarApp| {
            let doc = &app.documents[0];
            if valid {
                assert_eq!(doc.saved_text, DRAFT);
                assert_eq!(doc.revision.as_deref(), Some(revision.as_str()));
                assert!(doc.interrupted_save.is_none());
                assert!(!doc.save_outcome_unknown());
            } else {
                assert_eq!(doc.saved_text, BASE);
                assert_eq!(doc.revision.as_deref(), Some("r0"));
                assert_eq!(doc.interrupted_save.as_ref(), Some(&submission));
                assert!(doc.save_outcome_unknown());
            }
            assert!(!doc.save_outcome_unverifiable);
        };
        assert_baseline(&app);

        app.open_form = false;
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(editor));
        app.result_tx
            .send(WorkerEvent::Closed {
                generation: 7,
                result: Ok(()),
            })
            .unwrap();
        frame(&mut app, 1.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
        assert!(app.state == ConnectionState::Disconnected);
        assert_eq!(app.documents[0].text, DRAFT);
        assert_baseline(&app);
        frame(
            &mut app,
            2.0,
            key(
                egui::Key::Z,
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            ),
        );
        assert_eq!(app.documents[0].text, NEWER);
        assert!(app.documents[0].dirty());
        assert_baseline(&app);
        assert!(matches!(
            commands.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
}

#[test]
fn waiting_disconnect_rejects_recovery_install_without_touching_saved_copy() {
    let (mut app, commands) = app();
    let draft = Draft {
        workspace: app.recovery_workspace().unwrap(),
        path: "recovered.txt".into(),
        text: "retained recovery text".into(),
        base_text: "recovery base".into(),
        base_revision: Some("recovery-revision".into()),
        modified_ms: 1,
    };
    app.recovery.pending_restore = Some(draft.clone());
    let before = buffers(&app);
    app.disconnect_idle();
    assert!(app.install_recovered(draft).is_err());
    assert!(app.state == ConnectionState::Disconnecting);
    assert_eq!(buffers(&app), before);
    assert_eq!(
        app.recovery.pending_restore.as_ref().unwrap().text,
        "retained recovery text"
    );
    assert!(app.recovery.restoring_generation.is_none());
    assert!(app.recovery.closing.is_none());
    no_commands(&commands);
}
