//! Recording-worker tests exercise the same Event dispatch as a Linux agent.
use super::*;
use crate::{
    worker::{Command, Event, Worker, WorkerEvent},
    ConnectForm, ConnectionState, Payload,
};
use cedar_recovery::{Draft, DraftMetadata, WorkspaceIdentity};
use std::sync::mpsc::Receiver;

const ROOT: &str = "/synthetic/linux-language-cleanup";
const BASE: &str = "fn main() { /* saved */ }\n";
const DRAFT: &str = "fn main() { /* unsaved 雪 🦀 */ }\n";
const REDO: &str = "fn main() { /* later unsaved 雪 🦀 */ }\n";
type Selection = (usize, bool, usize, bool);
type History = Vec<(Selection, String)>;

fn selection(value: egui::text::CCursorRange) -> Selection {
    (
        value.primary.index,
        value.primary.prefer_next_row,
        value.secondary.index,
        value.secondary.prefer_next_row,
    )
}

#[derive(Debug, PartialEq, Eq)]
struct DraftSnapshot {
    id: u64,
    path: String,
    text: String,
    saved_text: String,
    revision: Option<String>,
    cursor: (usize, usize),
    jump_to: Option<usize>,
    scroll_to: Option<usize>,
    edit_version: u64,
    saving: bool,
    dirty: bool,
    undo_initialized: bool,
    has_cjk: bool,
    active: Option<u64>,
    selection: Selection,
    undo: History,
    redo: History,
    recovery_workspace: Option<WorkspaceIdentity>,
    recovery_enabled: bool,
    recovery_initialized: bool,
    recovery_visible: bool,
    recovery_error: Option<String>,
    recovery_issues: Vec<(String, String)>,
    recovery_drafts: Vec<DraftMetadata>,
    recovery_pending: Option<Draft>,
}

fn snapshot(app: &CedarApp) -> DraftSnapshot {
    assert_eq!(app.documents.len(), 1);
    let doc = &app.documents[0];
    assert!(doc.interrupted_save.is_none());
    assert!(!doc.save_outcome_unverifiable);
    let state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", doc.id))).unwrap();
    let range = state.cursor.char_range().unwrap();
    let current = (range, doc.text.clone());
    let mut undo = state.undoer();
    let mut previous = current.clone();
    let mut undo_states = Vec::new();
    while let Some(value) = undo.undo(&previous).cloned() {
        undo_states.push((selection(value.0), value.1.clone()));
        previous = value;
    }
    let mut redo = state.undoer();
    let mut next = current;
    let mut redo_states = Vec::new();
    while let Some(value) = redo.redo(&next).cloned() {
        redo_states.push((selection(value.0), value.1.clone()));
        next = value;
    }
    DraftSnapshot {
        id: doc.id,
        path: doc.path.clone(),
        text: doc.text.clone(),
        saved_text: doc.saved_text.clone(),
        revision: doc.revision.clone(),
        cursor: doc.cursor,
        jump_to: doc.jump_to,
        scroll_to: doc.scroll_to,
        edit_version: doc.edit_version,
        saving: doc.saving,
        dirty: doc.dirty(),
        undo_initialized: doc.undo_initialized,
        has_cjk: doc.has_cjk,
        active: app.active_document,
        selection: selection(range),
        undo: undo_states,
        redo: redo_states,
        recovery_workspace: app.recovery_workspace(),
        recovery_enabled: app.recovery.enabled,
        recovery_initialized: app.recovery.initialized,
        recovery_visible: app.recovery.visible,
        recovery_error: app.recovery.error.clone(),
        recovery_issues: app.recovery.issues.clone(),
        recovery_drafts: app.recovery.drafts.clone(),
        recovery_pending: app.recovery.pending_restore.clone(),
    }
}

fn app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        allow_run: true,
        ..Default::default()
    };
    app.state = ConnectionState::Ready;
    app.root = ROOT.into();
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.agent_info = Some(crate::agent_support::full_test_agent());
    app.open_form = false;
    app.language.program = "must-not-execute".into();
    app.language.running = true;
    app.language.session = 7;
    app.language.capabilities = serde_json::json!({"completionProvider":{},"hoverProvider":true});
    app.language.hover = "stale hover".into();
    app.language.sync.acknowledge(
        1,
        crate::language_sync::Acknowledged {
            version: 2,
            edit_version: 3,
            uri: "file:///synthetic/linux-language-cleanup/main.rs".into(),
        },
    );
    let mut doc = Document::new(1, "main.rs".into(), BASE.into(), "saved-revision".into());
    crate::editor_state::commit(&app.editor_ctx, &mut doc, DRAFT.into(), 14);
    let mut state = crate::editor_state::load(&app.editor_ctx, &mut doc);
    let selected = egui::text::CCursorRange {
        primary: egui::text::CCursor {
            index: 19,
            prefer_next_row: true,
        },
        secondary: egui::text::CCursor {
            index: 3,
            prefer_next_row: false,
        },
    };
    let base_range = egui::text::CCursorRange::one(egui::text::CCursor::new(0));
    let redo_range = egui::text::CCursorRange::one(egui::text::CCursor::new(22));
    let mut undoer = egui::util::undoer::Undoer::default();
    undoer.add_undo(&(base_range, BASE.to_owned()));
    undoer.add_undo(&(selected, DRAFT.to_owned()));
    let later = (redo_range, REDO.to_owned());
    undoer.add_undo(&later);
    assert_eq!(undoer.undo(&later).unwrap().1, DRAFT);
    state.set_undoer(undoer);
    state.cursor.set_char_range(Some(selected));
    state.store(&app.editor_ctx, egui::Id::new(("editor", doc.id)));
    doc.edit_version = 3;
    doc.cursor = crate::model::cursor_location(DRAFT, 19);
    doc.jump_to = Some(1);
    doc.scroll_to = Some(19);
    app.documents.push(doc);
    app.active_document = Some(1);
    let workspace = app.recovery_workspace().unwrap();
    app.recovery.enabled = true;
    app.recovery.initialized = true;
    app.recovery.visible = true;
    app.recovery.pending_restore = Some(Draft {
        workspace: workspace.clone(),
        path: "main.rs".into(),
        text: DRAFT.into(),
        base_text: BASE.into(),
        base_revision: Some("saved-revision".into()),
        modified_ms: 17,
    });
    app.recovery.drafts.push(DraftMetadata {
        id: cedar_recovery::record_id(&workspace, "main.rs").unwrap(),
        workspace,
        path: "main.rs".into(),
        base_revision: Some("saved-revision".into()),
        modified_ms: 17,
        text_bytes: DRAFT.len(),
        base_text_bytes: BASE.len(),
    });
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    let before = snapshot(&app);
    assert_eq!(before.undo.last().unwrap().1, BASE);
    assert_eq!(before.redo.last().unwrap().1, REDO);
    (app, commands)
}

fn stop(app: &mut CedarApp, commands: &Receiver<Command>) -> Command {
    app.stop_language();
    let command = commands.try_recv().expect("one explicit Stop command");
    assert!(matches!(command.op, Operation::LanguageStop));
    assert!(commands.try_recv().is_err());
    command
}

fn reply(app: &mut CedarApp, command: Command, result: Result<Payload, String>) {
    app.apply_worker_event(WorkerEvent::Response(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result,
    }));
}

fn malformed_replies() -> Vec<Result<Payload, String>> {
    [
        serde_json::json!({}),
        serde_json::json!({"stopped":false}),
        serde_json::json!({"stopped":null}),
        serde_json::json!({"stopped":"true"}),
        serde_json::json!([{"stopped":true}]),
        serde_json::json!(true),
        serde_json::Value::Null,
    ]
    .into_iter()
    .map(|value| Ok(Payload::Language { value }))
    .chain([
        Ok(Payload::Written {
            revision: "private wrong payload".into(),
        }),
        Err("private server failure /secret/path".into()),
    ])
    .collect()
}

#[test]
fn malformed_generic_stop_events_reset_block_and_preserve_complete_dirty_editor() {
    for result in malformed_replies() {
        let (mut app, commands) = app();
        let before = snapshot(&app);
        let session = app.language.session;
        let command = stop(&mut app, &commands);
        app.close_after_language_stop = true;
        app.close_snapshot = Some(app.draft_versions());
        reply(&mut app, command, result);
        assert!(app.state == ConnectionState::Ready);
        assert_eq!(app.language.session, session + 1);
        assert!(!app.language.running);
        assert!(app.language.restart_blocked);
        assert!(app.language.capabilities.is_null());
        assert!(app.language.sync.opened.is_empty());
        assert!(app.language.hover.is_empty());
        assert!(!app.close_after_language_stop);
        assert!(app.close_snapshot.is_none());
        assert!(!app.allow_close);
        let message = app.error.as_deref().unwrap();
        assert!(message.contains("cleanup") && message.contains("drafts"));
        assert!(message.contains("Inspect the previous server cleanup"));
        assert!(!message.contains("Java") && !message.contains("private"));
        assert!(!app.language.output.contains("session stopped"));
        assert_eq!(snapshot(&app), before);
        app.finish_pending_close(&app.editor_ctx.clone());
        app.start_language();
        app.language_tick(&app.editor_ctx.clone());
        app.sync_document(1);
        app.request_language_feature(LanguageQueryKind::Completion);
        app.disconnect_idle();
        assert!(app.state == ConnectionState::Ready);
        assert!(app.language.restart_blocked);
        assert!(!app.allow_close);
        assert!(commands.try_recv().is_err());
        assert_eq!(snapshot(&app), before);
    }
}

#[test]
fn true_generic_stop_acknowledgement_accepts_harmless_extra_fields() {
    let (mut app, commands) = app();
    let before = snapshot(&app);
    let command = stop(&mut app, &commands);
    reply(
        &mut app,
        command,
        Ok(Payload::Language {
            value: serde_json::json!({"stopped":true,"extra":{"ignored":true},"private":"server text"}),
        }),
    );
    assert!(!app.language.running);
    assert!(!app.language.restart_blocked);
    assert!(app.language.idle_for_disconnect());
    assert!(app.error.is_none());
    assert!(app.language.output.contains("session stopped"));
    assert!(!app.language.output.contains("private"));
    assert_eq!(snapshot(&app), before);
    app.language_tick(&app.editor_ctx.clone());
    assert!(commands.try_recv().is_err());
}

#[test]
fn stale_stop_success_malformed_payload_and_error_do_not_touch_new_session() {
    let mut results = malformed_replies();
    results.push(Ok(Payload::Language {
        value: serde_json::json!({"stopped":true}),
    }));
    for result in results {
        let (mut app, commands) = app();
        let before = snapshot(&app);
        let command = stop(&mut app, &commands);
        app.language.session += 1;
        app.language.running = true;
        app.language.output = "new session output".into();
        app.language.capabilities = serde_json::json!({"newSession":true});
        app.close_after_language_stop = true;
        let close = app.draft_versions();
        app.close_snapshot = Some(close.clone());
        app.error = Some("new session error".into());
        let session = app.language.session;
        reply(&mut app, command, result);
        assert_eq!(app.language.session, session);
        assert!(app.language.running);
        assert!(!app.language.restart_blocked);
        assert_eq!(app.language.output, "new session output");
        assert_eq!(
            app.language.capabilities,
            serde_json::json!({"newSession":true})
        );
        assert_eq!(app.error.as_deref(), Some("new session error"));
        assert!(app.close_after_language_stop);
        assert_eq!(app.close_snapshot.as_ref(), Some(&close));
        assert!(app.pending.is_empty());
        assert_eq!(snapshot(&app), before);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn stop_response_before_eof_cannot_close_or_restart_automatically() {
    for result in malformed_replies() {
        let (mut app, commands) = app();
        let before = snapshot(&app);
        let command = stop(&mut app, &commands);
        app.close_after_language_stop = true;
        app.close_snapshot = Some(app.draft_versions());
        app.result_tx
            .send(WorkerEvent::Response(Event {
                generation: app.generation,
                id: command.id,
                connected: true,
                result,
            }))
            .unwrap();
        app.poll();
        assert!(app.language.restart_blocked);
        assert!(!app.close_after_language_stop);
        assert_eq!(snapshot(&app), before);
        app.result_tx
            .send(WorkerEvent::TransportLost {
                generation: app.generation,
                message: "transport_closed: idle EOF".into(),
            })
            .unwrap();
        app.poll();
        assert!(app.state == ConnectionState::Disconnected);
        assert!(app.worker.is_none());
        assert!(!app.language.running);
        assert!(app
            .language
            .output
            .contains("cleanup could not be confirmed"));
        assert!(!app.close_after_language_stop);
        assert!(app.close_snapshot.is_none());
        app.finish_pending_close(&app.editor_ctx.clone());
        app.language_tick(&app.editor_ctx.clone());
        assert!(!app.allow_close);
        assert!(commands.try_recv().is_err());
        assert_eq!(snapshot(&app), before);
    }
}

#[test]
fn successful_stop_then_eof_in_one_event_drain_retains_the_dirty_editor() {
    let (mut app, commands) = app();
    let before = snapshot(&app);
    let command = stop(&mut app, &commands);
    app.close_after_language_stop = true;
    app.close_snapshot = Some(app.draft_versions());
    app.result_tx
        .send(WorkerEvent::Response(Event {
            generation: app.generation,
            id: command.id,
            connected: true,
            result: Ok(Payload::Language {
                value: serde_json::json!({"stopped":true}),
            }),
        }))
        .unwrap();
    app.result_tx
        .send(WorkerEvent::TransportLost {
            generation: app.generation,
            message: "transport_closed: EOF after Stop".into(),
        })
        .unwrap();
    app.poll();
    app.finish_pending_close(&app.editor_ctx.clone());
    assert!(app.state == ConnectionState::Disconnected);
    assert!(!app.allow_close);
    assert!(!app.close_after_language_stop);
    assert!(app.close_snapshot.is_none());
    assert!(app.worker.is_none());
    assert!(app
        .language
        .output
        .contains("cleanup could not be confirmed"));
    assert_eq!(snapshot(&app), before);
    assert!(commands.try_recv().is_err());
}

#[test]
fn stale_stop_transport_loss_still_disconnects_the_current_connection() {
    let (mut app, commands) = app();
    let before = snapshot(&app);
    let command = stop(&mut app, &commands);
    app.language.session += 1;
    app.language.running = true;
    app.close_after_language_stop = true;
    app.close_snapshot = Some(app.draft_versions());
    app.apply_worker_event(WorkerEvent::Response(Event {
        generation: app.generation,
        id: command.id,
        connected: false,
        result: Err("transport_closed: stale Stop".into()),
    }));
    assert!(app.state == ConnectionState::Disconnected);
    assert!(!app.language.running);
    assert!(!app.allow_close);
    assert!(!app.close_after_language_stop);
    assert!(app.close_snapshot.is_none());
    assert!(app.worker.is_none());
    assert_eq!(snapshot(&app), before);
    assert!(commands.try_recv().is_err());
}

#[test]
fn explicit_reconnect_preserves_draft_and_does_not_start_language_automatically() {
    let (mut app, commands) = app();
    let before = snapshot(&app);
    let command = stop(&mut app, &commands);
    reply(&mut app, command, Err("cleanup unknown".into()));
    app.apply_worker_event(WorkerEvent::TransportLost {
        generation: app.generation,
        message: "transport_closed".into(),
    });
    app.language_tick(&app.editor_ctx.clone());
    assert!(app.worker.is_none());
    assert!(commands.try_recv().is_err());
    let (worker, reconnected_commands) = Worker::recording();
    app.worker = Some(worker);
    app.generation += 1;
    app.state = ConnectionState::Connecting;
    app.connecting_form = app.active_form.clone();
    app.explorer.mode = crate::explorer_tree::Mode::Tree;
    app.apply_worker_event(WorkerEvent::Response(Event {
        generation: app.generation,
        id: 0,
        connected: true,
        result: Ok(Payload::Hello {
            protocol: cedar_protocol::PROTOCOL_VERSION,
            agent: Some(crate::agent_support::full_test_agent()),
            root: ROOT.into(),
        }),
    }));
    assert!(app.state == ConnectionState::Ready);
    assert!(!app.language.running);
    assert_eq!(snapshot(&app), before);
    app.language_tick(&app.editor_ctx.clone());
    assert!(reconnected_commands.try_recv().is_err());
    assert!(app.pending.is_empty());
}

#[test]
fn typed_java_stop_still_requires_its_rich_cleanup_outcome() {
    for (value, blocked) in [
        (serde_json::json!({"stopped":true}), true),
        (
            serde_json::json!({"stopped":true,"shutdown":{"platform":"linux","status":"forced","reason":"grace_expired","root_exit":{"kind":"signal","signal":9},"cleanup_joined":true,"shutdown_response_received":true,"exit_frame_completed":true}}),
            false,
        ),
        (
            serde_json::json!({"stopped":true,"shutdown":{"platform":"linux","status":"error","reason":"transport_failure","root_exit":{"kind":"code","code":7},"cleanup_joined":true,"shutdown_response_received":false,"exit_frame_completed":false}}),
            false,
        ),
        (
            serde_json::json!({"stopped":true,"shutdown":{"platform":"linux","status":"forced","reason":"grace_expired","root_exit":{"kind":"signal","signal":9},"root_exit_code":137,"cleanup_joined":true,"shutdown_response_received":true,"exit_frame_completed":true}}),
            true,
        ),
        (
            serde_json::json!({"stopped":true,"shutdown":{
                "status":"forced","reason":"grace_expired","root_exit_code":1,
                "cleanup_joined":true,"shutdown_response_received":true,"exit_frame_completed":true
            }}),
            false,
        ),
    ] {
        let (mut app, commands) = app();
        app.language.mode = ServerMode::Java;
        let before = snapshot(&app);
        let command = stop(&mut app, &commands);
        reply(&mut app, command, Ok(Payload::Language { value }));
        assert_eq!(app.language.restart_blocked, blocked);
        assert!(!app.language.running);
        assert_eq!(snapshot(&app), before);
        assert!(commands.try_recv().is_err());
    }
}
