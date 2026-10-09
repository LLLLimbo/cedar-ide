use super::*;
use crate::{
    agent_support::full_test_agent,
    java_language::{JavaConfiguration, ServerMode},
    model::Document,
    worker::{Command, Event, Worker},
    ConnectForm, ConnectionState,
};
use cedar_protocol::{
    LanguageQueryKind, JAVA_LANGUAGE_SESSION_CAPABILITIES, JAVA_STARTUP_CAPABILITIES,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::mpsc::Receiver;

fn app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    app.state = ConnectionState::Ready;
    app.generation = 9;
    app.root = "/workspace".into();
    let form = ConnectForm {
        local_root: app.root.clone(),
        allow_run: true,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    let mut info = full_test_agent();
    info.capabilities.push("language_start_java".into());
    info.capabilities
        .extend(JAVA_STARTUP_CAPABILITIES.iter().map(|name| (*name).into()));
    app.agent_info = Some(info);
    app.language.mode = ServerMode::Java;
    app.language.java = JavaConfiguration {
        executable: r"C:\Java\bin\java.exe".into(),
        distribution: r"D:\JDT 雪".into(),
        data_directory: r"D:\Java data 雪".into(),
    };
    let (worker, rx) = Worker::recording();
    app.worker = Some(worker);
    (app, rx)
}
fn reply(app: &mut CedarApp, id: u64, value: Value) {
    app.apply_event(Event {
        generation: app.generation,
        id,
        connected: true,
        result: Ok(Payload::Language { value }),
    });
}
fn starting(id: u64) -> Value {
    json!({"startup_id":id,"state":"starting","process_id":null})
}
fn ready(id: u64) -> Value {
    json!({"startup_id":id,"state":"ready","language":{"started":true,"initialize":{"capabilities":{"completionProvider":{}}},"root_uri":"file:///workspace"}})
}
fn cancelled(id: u64) -> Value {
    json!({"startup_id":id,"state":"cancelled","cleanup_verified":true})
}
fn begin(app: &mut CedarApp, rx: &Receiver<Command>, id: u64) -> u64 {
    app.start_language();
    let command = rx.try_recv().unwrap();
    assert!(matches!(
        command.op,
        Operation::LanguageStartJavaBegin { .. }
    ));
    reply(app, command.id, starting(id));
    assert!(app.language.startup_active());
    assert!(!app.language.running);
    command.id
}
fn tick(app: &mut CedarApp, now: f64) {
    let ctx = app.editor_ctx.clone();
    ctx.begin_pass(egui::RawInput {
        time: Some(now),
        ..Default::default()
    });
    app.language_tick(&ctx);
    let _ = ctx.end_pass();
}
fn draft(app: &mut CedarApp) {
    app.documents.push(Document::new(
        1,
        "Main.java".into(),
        "class Main {}".into(),
        "original-revision".into(),
    ));
    app.active_document = Some(1);
    crate::editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        "class Main { int draft; }".into(),
        10,
    );
}
fn assert_draft(app: &CedarApp) {
    let doc = &app.documents[0];
    assert_eq!(doc.text, "class Main { int draft; }");
    assert_eq!(doc.saved_text, "class Main {}");
    assert_eq!(doc.revision.as_deref(), Some("original-revision"));
    assert_eq!(doc.edit_version, 1);
    let state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64))).unwrap();
    let after = (state.cursor.char_range().unwrap(), doc.text.clone());
    assert_eq!(state.undoer().undo(&after).unwrap().1, "class Main {}");
}

#[test]
fn complete_optional_group_selects_async_and_partial_group_falls_back() {
    for mask in 0..8 {
        let (mut app, rx) = app();
        let capabilities = &mut app.agent_info.as_mut().unwrap().capabilities;
        for (index, name) in JAVA_STARTUP_CAPABILITIES.iter().enumerate() {
            if mask & (1 << index) == 0 {
                capabilities.retain(|value| value != name);
            }
        }
        assert_eq!(app.backend_java_startup_supported(), mask == 7);
        app.start_language();
        let command = rx.try_recv().unwrap();
        if mask == 7 {
            assert!(matches!(
                command.op,
                Operation::LanguageStartJavaBegin { .. }
            ));
            assert!(app.language.startup_active());
        } else {
            assert!(matches!(command.op, Operation::LanguageStartJava { .. }));
            assert!(!app.language.startup_active());
        }
    }
}
#[test]
fn async_claims_never_bypass_core_lifecycle_or_connection_trust() {
    for missing in JAVA_LANGUAGE_SESSION_CAPABILITIES {
        let (mut app, rx) = app();
        app.agent_info
            .as_mut()
            .unwrap()
            .capabilities
            .retain(|name| name != missing);
        app.start_language();
        assert!(rx.try_recv().is_err());
        assert!(!app.language.startup_active());
    }
    let (mut app, rx) = app();
    app.active_form.as_mut().unwrap().allow_run = false;
    app.form.allow_run = true;
    app.start_language();
    assert!(rx.try_recv().is_err());
    assert!(app
        .operation_problem(&Operation::LanguageStartJavaBegin {
            java_executable: "java".into(),
            distribution: "jdt".into(),
            data_directory: "data".into()
        })
        .is_some());
    for op in [
        Operation::LanguageStartJavaPoll { startup_id: 1 },
        Operation::LanguageStartJavaCancel { startup_id: 1 },
    ] {
        assert!(app.operation_problem(&op).is_none());
    }
}
#[test]
fn startup_survives_wire_gaps_and_allows_file_save_read_and_task_requests() {
    let (mut app, rx) = app();
    draft(&mut app);
    begin(&mut app, &rx, 42);
    assert!(app.pending.is_empty());
    assert!(app.mutation_pending());
    app.request_language_feature(LanguageQueryKind::Completion);
    assert!(app.language.intent.is_none());
    app.open("other.txt".into(), None);
    assert!(matches!(rx.try_recv().unwrap().op, Operation::Read { .. }));
    app.save_document(1);
    assert!(
        matches!(rx.try_recv().unwrap().op, Operation::Write { text, .. } if text == "class Main { int draft; }")
    );
    app.profiles.draft.program = "task-tool".into();
    app.run();
    assert!(matches!(
        rx.try_recv().unwrap().op,
        Operation::RunStart { .. }
    ));
    tick(&mut app, 1.0);
    let poll = rx.try_recv().unwrap();
    assert!(matches!(
        poll.op,
        Operation::LanguageStartJavaPoll { startup_id: 42 }
    ));
    assert_eq!(
        app.pending.len(),
        4,
        "one lifecycle poll follows existing file/task requests"
    );
    tick(&mut app, 2.0);
    assert!(
        rx.try_recv().is_err(),
        "ordinary backlog cannot cause duplicate polls"
    );
    assert_draft(&app);
}
#[test]
fn cancellation_before_begin_reply_remembers_intent_and_waits_for_verified_cleanup() {
    let (mut app, rx) = app();
    draft(&mut app);
    app.start_language();
    let begin = rx.try_recv().unwrap();
    app.cancel_java_startup();
    assert!(app.language.startup.as_ref().unwrap().cancel_intent);
    assert!(rx.try_recv().is_err());
    reply(&mut app, begin.id, starting(42));
    let cancel = rx.try_recv().unwrap();
    assert!(matches!(
        cancel.op,
        Operation::LanguageStartJavaCancel { startup_id: 42 }
    ));
    app.cancel_java_startup();
    assert!(rx.try_recv().is_err());
    reply(
        &mut app,
        cancel.id,
        json!({"startup_id":42,"state":"cancelling","process_id":123}),
    );
    assert!(app.language.startup_active());
    assert!(!app.language.running);
    tick(&mut app, 0.3);
    let poll = rx.try_recv().unwrap();
    assert!(matches!(
        poll.op,
        Operation::LanguageStartJavaPoll { startup_id: 42 }
    ));
    reply(&mut app, poll.id, cancelled(42));
    assert!(!app.language.startup_active());
    assert!(!app.language.running);
    assert!(!app.language.automatic);
    tick(&mut app, 1.0);
    assert!(rx.try_recv().is_err());
    assert_draft(&app);
}
#[test]
fn cancel_ready_race_never_activates_syncs_or_loses_original_owner() {
    let (mut app, rx) = app();
    draft(&mut app);
    begin(&mut app, &rx, 42);
    tick(&mut app, 0.3);
    let poll = rx.try_recv().unwrap();
    app.cancel_java_startup();
    assert!(rx.try_recv().is_err());
    reply(&mut app, poll.id, ready(42));
    assert!(!app.language.running);
    assert_eq!(app.language.startup.as_ref().unwrap().id, Some(42));
    assert!(app.language.sync.opened.is_empty());
    let cancel = rx.try_recv().unwrap();
    assert!(matches!(
        cancel.op,
        Operation::LanguageStartJavaCancel { startup_id: 42 }
    ));
    reply(&mut app, cancel.id, cancelled(42));
    tick(&mut app, 1.0);
    assert!(rx.try_recv().is_err());
    assert_draft(&app);
}
#[test]
fn ready_activates_current_draft_only_and_stop_remains_legacy() {
    let (mut app, rx) = app();
    draft(&mut app);
    begin(&mut app, &rx, 42);
    tick(&mut app, 0.3);
    let poll = rx.try_recv().unwrap();
    reply(&mut app, poll.id, ready(42));
    assert!(app.language.running);
    assert!(app.language.startup.is_none());
    app.sync_current_language();
    let sync = rx.try_recv().unwrap();
    assert!(
        matches!(sync.op, Operation::LanguageOpen { text, .. } if text == "class Main { int draft; }")
    );
    app.pending.remove(&sync.id);
    app.stop_language();
    assert!(matches!(rx.try_recv().unwrap().op, Operation::LanguageStop));
    assert_draft(&app);
}
#[test]
fn polling_is_rate_limited_single_flight_and_has_an_absolute_deadline() {
    let (mut app, rx) = app();
    begin(&mut app, &rx, 42);
    tick(&mut app, 0.24);
    assert!(rx.try_recv().is_err());
    tick(&mut app, 0.25);
    let poll = rx.try_recv().unwrap();
    for now in [0.3, 0.5, 1.0] {
        tick(&mut app, now);
        assert!(rx.try_recv().is_err());
    }
    reply(&mut app, poll.id, starting(42));
    tick(&mut app, 1.24);
    assert!(rx.try_recv().is_err());
    tick(&mut app, 74.9);
    let poll = rx.try_recv().unwrap();
    reply(&mut app, poll.id, starting(42));
    assert_eq!(
        app.language.startup.as_ref().unwrap().deadline,
        STARTUP_SECONDS
    );
    tick(&mut app, 75.0);
    assert!(app.language.startup_active());
    assert!(app.language.startup.as_ref().unwrap().cancel_intent);
    assert!(app.language.output.contains("timed out"));
    let cancel = rx.try_recv().unwrap();
    assert!(matches!(
        cancel.op,
        Operation::LanguageStartJavaCancel { startup_id: 42 }
    ));
    reply(
        &mut app,
        cancel.id,
        json!({"startup_id":42,"state":"cancelling","process_id":123}),
    );
    tick(&mut app, 80.0);
    let poll = rx.try_recv().unwrap();
    assert!(matches!(
        poll.op,
        Operation::LanguageStartJavaPoll { startup_id: 42 }
    ));
    assert!(
        app.language.startup_active(),
        "elapsed time cannot verify cleanup"
    );
    reply(&mut app, poll.id, cancelled(42));
    assert!(!app.language.startup_active());
    assert!(!app.language.restart_blocked);
    assert_eq!(
        app.notice,
        "Java startup timed out; process cleanup verified."
    );
}
#[test]
fn wrong_generation_stale_session_and_duplicate_events_cannot_change_owner() {
    let (mut app, rx) = app();
    app.start_language();
    let begin = rx.try_recv().unwrap();
    app.apply_event(Event {
        generation: app.generation - 1,
        id: begin.id,
        connected: false,
        result: Err("stale".into()),
    });
    assert!(app.pending.contains_key(&begin.id));
    assert!(app.ready());
    reply(&mut app, begin.id, starting(42));
    reply(&mut app, begin.id, ready(999));
    assert_eq!(app.language.startup.as_ref().unwrap().id, Some(42));
    for (session, startup_id) in [(app.language.session - 1, 42), (app.language.session, 999)] {
        app.apply_java_startup_action(
            Action {
                session,
                kind: ActionKind::JavaStartPoll { startup_id },
            },
            ready(startup_id),
        );
    }
    assert_eq!(app.language.startup.as_ref().unwrap().id, Some(42));
    assert!(!app.language.running);
    assert!(!app.language.restart_blocked);
    assert!(rx.try_recv().is_err());
}
#[test]
fn cancelled_owner_cannot_cancel_a_later_session() {
    let (mut app, rx) = app();
    begin(&mut app, &rx, 42);
    let old_session = app.language.session;
    app.cancel_java_startup();
    let cancel = rx.try_recv().unwrap();
    reply(&mut app, cancel.id, cancelled(42));
    begin(&mut app, &rx, 43);
    reply(&mut app, cancel.id, ready(42));
    app.apply_java_startup_action(
        Action {
            session: old_session,
            kind: ActionKind::JavaStartCancel { startup_id: 42 },
        },
        cancelled(42),
    );
    assert_eq!(app.language.startup.as_ref().unwrap().id, Some(43));
    assert!(!app.language.startup.as_ref().unwrap().cancel_intent);
    assert!(rx.try_recv().is_err());
}
#[test]
fn malformed_mismatched_and_unverified_snapshots_block_restart_without_raw_output() {
    for value in [
        Value::Null,
        json!({"state":"starting","startup_id":42}),
        json!({"state":"starting","startup_id":0,"process_id":null}),
        json!({"state":"starting","startup_id":42,"process_id":-1}),
        json!({"state":"ready","startup_id":42,"language":{"started":false}}),
        ready(99),
        json!({"state":"cancelled","startup_id":42,"cleanup_verified":false}),
        json!({"state":"failed","startup_id":42,"cleanup_verified":false,"error":{"code":"native_error","message":"private raw stderr"}}),
    ] {
        let (mut app, rx) = app();
        draft(&mut app);
        begin(&mut app, &rx, 42);
        tick(&mut app, 0.3);
        let poll = rx.try_recv().unwrap();
        reply(&mut app, poll.id, value);
        assert!(app.language.restart_blocked);
        assert!(!app.language.running);
        assert!(!app.language.output.contains("private"));
        app.start_language();
        assert!(rx.try_recv().is_err());
        assert_draft(&app);
    }
}
#[test]
fn unexpected_payload_and_transport_error_do_not_activate_or_publish_remote_details() {
    for connected in [true, false] {
        let (mut app, rx) = app();
        draft(&mut app);
        begin(&mut app, &rx, 42);
        tick(&mut app, 0.3);
        let poll = rx.try_recv().unwrap();
        app.apply_event(Event {
            generation: app.generation,
            id: poll.id,
            connected,
            result: Err("private stderr".into()),
        });
        assert!(!app.language.running);
        assert!(!app.error.as_ref().unwrap().contains("private"));
        assert_eq!(app.ready(), connected);
        assert_draft(&app);
    }
    let (mut app, rx) = app();
    app.start_language();
    let begin = rx.try_recv().unwrap();
    app.apply_event(Event {
        generation: app.generation,
        id: begin.id,
        connected: true,
        result: Ok(Payload::Entries { entries: vec![] }),
    });
    assert!(app.language.restart_blocked);
}
#[test]
fn failed_with_verified_cleanup_allows_only_explicit_restart() {
    let (mut app, rx) = app();
    begin(&mut app, &rx, 42);
    tick(&mut app, 0.3);
    let poll = rx.try_recv().unwrap();
    reply(
        &mut app,
        poll.id,
        json!({"startup_id":42,"state":"failed","cleanup_verified":true,"error":{"code":"launch_failed","message":"private backend details"}}),
    );
    assert!(!app.language.restart_blocked);
    assert!(!app.language.startup_active());
    tick(&mut app, 1.0);
    assert!(rx.try_recv().is_err());
    app.start_language();
    assert!(matches!(
        rx.try_recv().unwrap().op,
        Operation::LanguageStartJavaBegin { .. }
    ));
}
#[test]
fn reconnect_is_guarded_between_polls_and_disconnect_retains_draft_without_replay() {
    let (mut app, rx) = app();
    draft(&mut app);
    begin(&mut app, &rx, 42);
    let mut invalid = app.active_form.clone().unwrap();
    invalid.local_root.clear();
    app.connect(&egui::Context::default(), invalid);
    assert!(app.ready());
    assert_eq!(app.generation, 9);
    assert!(app.language.startup_active());
    assert!(rx.try_recv().is_err());
    app.disconnected("disconnected".into());
    assert!(!app.language.startup_active());
    assert!(!app.language.running);
    assert_draft(&app);
    // A replacement connection has no startup intent to replay.
    let (worker, replacement) = Worker::recording();
    app.worker = Some(worker);
    app.generation += 1;
    app.state = ConnectionState::Ready;
    app.agent_info = Some(full_test_agent());
    tick(&mut app, 1.0);
    assert!(replacement.try_recv().is_err());
}
#[test]
fn close_waits_for_cancel_cleanup_and_rechecks_newer_drafts() {
    let (mut app, rx) = app();
    draft(&mut app);
    begin(&mut app, &rx, 42);
    let ctx = app.editor_ctx.clone();
    app.begin_close(&ctx);
    assert!(app.close_after_language_stop);
    let cancel = rx.try_recv().unwrap();
    assert!(matches!(
        cancel.op,
        Operation::LanguageStartJavaCancel { startup_id: 42 }
    ));
    app.finish_pending_close(&ctx);
    assert!(!app.allow_close);
    assert!(app.recovery.closing.is_none());
    crate::editor_state::commit(
        &ctx,
        &mut app.documents[0],
        "class Main { int newer; }".into(),
        10,
    );
    reply(&mut app, cancel.id, cancelled(42));
    app.finish_pending_close(&ctx);
    assert!(matches!(app.confirm, Some(crate::Confirm::CloseWindow)));
    assert!(app.recovery.closing.is_none());
    assert_eq!(app.documents[0].text, "class Main { int newer; }");
}
#[test]
fn unverified_cancel_cannot_finish_a_pending_close() {
    let (mut app, rx) = app();
    begin(&mut app, &rx, 42);
    let ctx = app.editor_ctx.clone();
    app.begin_close(&ctx);
    let cancel = rx.try_recv().unwrap();
    reply(
        &mut app,
        cancel.id,
        json!({"startup_id":42,"state":"cancelled","cleanup_verified":false}),
    );
    app.finish_pending_close(&ctx);
    assert!(!app.close_after_language_stop);
    assert!(app.close_snapshot.is_none());
    assert!(app.recovery.closing.is_none());
    assert!(!app.allow_close);
    assert!(app.language.restart_blocked);
}
#[test]
fn idle_configuration_edits_never_start_java() {
    let (mut app, rx) = app();
    for now in [0.0, 1.0, 10.0] {
        tick(&mut app, now);
    }
    assert!(rx.try_recv().is_err());
    assert!(!app.language.startup_active());
}

#[test]
fn save_ack_during_startup_preserves_newer_draft_and_undo() {
    let (mut app, rx) = app();
    let revision = format!("{:x}", Sha256::digest(b"class Main { int draft; }"));
    draft(&mut app);
    begin(&mut app, &rx, 42);
    app.save_document(1);
    let save = rx.try_recv().unwrap();
    assert!(matches!(save.op, Operation::Write { .. }));
    crate::editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        "class Main { int newer; }".into(),
        10,
    );
    app.apply_event(Event {
        generation: app.generation,
        id: save.id,
        connected: true,
        result: Ok(Payload::Written {
            revision: revision.clone(),
        }),
    });
    assert!(app.language.startup_active());
    let doc = &app.documents[0];
    assert_eq!(doc.text, "class Main { int newer; }");
    assert_eq!(doc.saved_text, "class Main { int draft; }");
    assert_eq!(doc.revision.as_deref(), Some(revision.as_str()));
    assert!(doc.dirty());
    let state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64))).unwrap();
    let after = (state.cursor.char_range().unwrap(), doc.text.clone());
    assert_eq!(
        state.undoer().undo(&after).unwrap().1,
        "class Main { int draft; }"
    );
    // A successful save always refreshes the current directory. It must not
    // replay the write, synchronize a language document, or start another owner.
    let refresh = rx.try_recv().unwrap();
    assert!(
        matches!(&refresh.op, Operation::List { path } if path == &app.directory),
        "Unexpected post-save operation: {:?}",
        refresh.op
    );
    assert!(
        matches!(app.pending.get(&refresh.id), Some(Job::List { path }) if path == &app.directory)
    );
    if let Ok(extra) = rx.try_recv() {
        panic!("Unexpected extra post-save operation: {:?}", extra.op);
    }
}

#[test]
fn ready_received_after_deadline_requests_cancel_without_activation() {
    let (mut app, rx) = app();
    begin(&mut app, &rx, 42);
    tick(&mut app, 0.3);
    let poll = rx.try_recv().unwrap();
    let ctx = app.editor_ctx.clone();
    ctx.begin_pass(egui::RawInput {
        time: Some(76.0),
        ..Default::default()
    });
    reply(&mut app, poll.id, ready(42));
    let _ = ctx.end_pass();
    assert!(!app.language.running);
    let cancel = rx.try_recv().unwrap();
    assert!(matches!(
        cancel.op,
        Operation::LanguageStartJavaCancel { startup_id: 42 }
    ));
    assert!(app.language.startup_active());
}

#[test]
fn real_close_dispatch_reaches_cancellation_before_and_after_begin_ack() {
    for acknowledged in [false, true] {
        let (mut app, rx) = app();
        app.start_language();
        let begin = rx.try_recv().unwrap();
        if acknowledged {
            reply(&mut app, begin.id, starting(42));
        }
        let ctx = app.editor_ctx.clone();
        app.request_window_close(&ctx);
        assert!(app.close_after_language_stop);
        assert!(app.language.startup.as_ref().unwrap().cancel_intent);
        assert!(!app.allow_close);
        assert!(app.recovery.closing.is_none());
        if !acknowledged {
            assert!(rx.try_recv().is_err());
            reply(&mut app, begin.id, starting(42));
        }
        let cancel = rx.try_recv().unwrap();
        assert!(matches!(
            cancel.op,
            Operation::LanguageStartJavaCancel { startup_id: 42 }
        ));
        reply(&mut app, cancel.id, cancelled(42));
        app.finish_pending_close(&ctx);
        assert!(app.recovery.closing.is_some());
    }
}

#[test]
fn real_close_dispatch_keeps_dirty_confirmation_and_pending_save_guards() {
    let (mut app, rx) = app();
    draft(&mut app);
    begin(&mut app, &rx, 42);
    let ctx = app.editor_ctx.clone();
    app.request_window_close(&ctx);
    assert!(matches!(app.confirm, Some(crate::Confirm::CloseWindow)));
    assert!(!app.language.startup.as_ref().unwrap().cancel_intent);
    app.confirm = None;
    app.save_document(1);
    let save = rx.try_recv().unwrap();
    app.request_window_close(&ctx);
    assert!(app.confirm.is_none());
    assert!(app.pending.contains_key(&save.id));
    assert!(!app.close_after_language_stop);
    assert!(!app.language.startup.as_ref().unwrap().cancel_intent);
    assert_draft(&app);
}

#[test]
fn ordinary_request_disconnect_cannot_complete_a_close_awaiting_cleanup() {
    let (mut app, rx) = app();
    draft(&mut app);
    begin(&mut app, &rx, 42);
    app.open("other.txt".into(), None);
    let read = rx.try_recv().unwrap();
    let ctx = app.editor_ctx.clone();
    app.begin_close(&ctx);
    let cancel = rx.try_recv().unwrap();
    assert!(matches!(
        cancel.op,
        Operation::LanguageStartJavaCancel { .. }
    ));
    app.apply_event(Event {
        generation: app.generation,
        id: read.id,
        connected: false,
        result: Err("transport lost".into()),
    });
    app.finish_pending_close(&ctx);
    assert!(!app.close_after_language_stop);
    assert!(app.close_snapshot.is_none());
    assert!(app.recovery.closing.is_none());
    assert!(!app.allow_close);
    assert_draft(&app);
}

#[test]
fn repeated_close_keeps_original_draft_snapshot_until_cleanup_is_verified() {
    let (mut app, rx) = app();
    draft(&mut app);
    begin(&mut app, &rx, 42);
    let ctx = app.editor_ctx.clone();
    app.begin_close(&ctx);
    let original_snapshot = app.close_snapshot.clone();
    let cancel = rx.try_recv().unwrap();
    app.request_window_close(&ctx);
    assert!(
        app.confirm.is_none(),
        "no second discard dialog while the original close waits"
    );
    assert_eq!(app.close_snapshot, original_snapshot);
    crate::editor_state::commit(
        &ctx,
        &mut app.documents[0],
        "class Main { int newer; }".into(),
        10,
    );
    app.request_window_close(&ctx);
    assert!(app.confirm.is_none());
    assert_eq!(app.close_snapshot, original_snapshot);
    reply(&mut app, cancel.id, cancelled(42));
    app.finish_pending_close(&ctx);
    assert!(matches!(app.confirm, Some(crate::Confirm::CloseWindow)));
    assert!(!app.close_after_language_stop);
    assert!(app.recovery.closing.is_none());
    app.confirm = None; // Keep editing after the newer draft confirmation.
    app.finish_pending_close(&ctx);
    assert!(!app.allow_close);
    assert!(app.recovery.closing.is_none());
}

#[test]
fn verified_failure_messages_distinguish_timeout_configuration_and_initialization() {
    for (code, expected) in [
        ("language_startup_timeout", "timed out"),
        ("invalid_java_launch", "configuration was rejected"),
        ("invalid_path", "workspace path"),
        ("language_startup_failed", "could not launch the server"),
        ("language_error", "language-server initialization"),
        (
            "future_private_error",
            "Java startup failed; process cleanup verified.",
        ),
    ] {
        let (mut app, rx) = app();
        begin(&mut app, &rx, 42);
        tick(&mut app, 0.3);
        let poll = rx.try_recv().unwrap();
        reply(
            &mut app,
            poll.id,
            json!({"startup_id":42,"state":"failed","cleanup_verified":true,"error":{"code":code,"message":"private host details"}}),
        );
        assert!(
            app.language.output.contains(expected),
            "{code}: {}",
            app.language.output
        );
        assert!(!app.language.output.contains("private"));
        assert!(app.language.output.len() < 256);
        if code != "invalid_java_launch" {
            assert!(!app.language.output.contains("Check the Java"));
        }
        assert!(!app.language.startup_active());
        assert!(!app.language.restart_blocked);
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn user_cancel_before_deadline_is_not_relabelled_when_cleanup_finishes_later() {
    let (mut app, rx) = app();
    begin(&mut app, &rx, 42);
    app.cancel_java_startup();
    let cancel = rx.try_recv().unwrap();
    let ctx = app.editor_ctx.clone();
    ctx.begin_pass(egui::RawInput {
        time: Some(80.0),
        ..Default::default()
    });
    reply(&mut app, cancel.id, cancelled(42));
    let _ = ctx.end_pass();
    assert_eq!(
        app.notice,
        "Java startup cancelled; process cleanup verified."
    );
    assert!(!app.language.startup_active());
    assert!(!app.language.restart_blocked);
    assert!(rx.try_recv().is_err());
}

#[test]
fn explicit_disconnect_refuses_startup_and_unverified_startup_without_dispatch() {
    let (mut app, commands) = app();
    app.start_language();
    let _begin = commands.try_recv().unwrap();
    assert!(app.language.startup_active());
    app.disconnect_idle();
    assert!(app.state == ConnectionState::Ready);
    assert!(commands.try_recv().is_err());
    app.java_startup_unknown();
    assert!(!app.language.startup_active());
    assert!(!app.language.idle_for_disconnect());
    app.disconnect_idle();
    assert!(app.state == ConnectionState::Ready);
    assert!(commands.try_recv().is_err());
}
