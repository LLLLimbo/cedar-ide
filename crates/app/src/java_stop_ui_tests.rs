use super::*;
use crate::{
    editor_state,
    worker::{Command, Worker},
    ConnectForm, ConnectionState,
};
use serde_json::json;
use std::sync::mpsc::Receiver;

fn app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    app.state = ConnectionState::Ready;
    app.agent_info = Some(crate::agent_support::full_test_agent());
    app.agent_info
        .as_mut()
        .unwrap()
        .capabilities
        .push("language_start_java".into());
    app.active_form = Some(ConnectForm {
        allow_run: true,
        ..Default::default()
    });
    app.language.mode = ServerMode::Java;
    app.language.java = JavaConfiguration {
        executable: "/opt/java/bin/java".into(),
        distribution: "/opt/JDT 雪".into(),
        data_directory: "/opt/Java data 雪".into(),
    };
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn linux_stop(status: &str, joined: bool) -> Value {
    json!({"stopped":true,"shutdown":{
        "platform":"linux","status":status,"reason":"transport_failure",
        "root_exit":{"kind":"signal","signal":9},"cleanup_joined":joined,
        "shutdown_response_received":false,"exit_frame_completed":false
    }})
}

fn selection() -> egui::text::CCursorRange {
    egui::text::CCursorRange {
        primary: egui::text::CCursor {
            index: 2,
            prefer_next_row: false,
        },
        secondary: egui::text::CCursor {
            index: 10,
            prefer_next_row: true,
        },
    }
}

fn seed_draft(app: &mut CedarApp) {
    let mut doc = Document::new(
        1,
        "Main.java".into(),
        "class Main {}".into(),
        "original-revision".into(),
    );
    editor_state::commit(
        &app.editor_ctx,
        &mut doc,
        "class Main { int draft; }".into(),
        4,
    );
    let mut state = editor_state::load(&app.editor_ctx, &mut doc);
    state.cursor.set_char_range(Some(selection()));
    state.store(&app.editor_ctx, egui::Id::new(("editor", 1u64)));
    app.active_document = Some(1);
    app.documents.push(doc);
}

fn assert_draft_unchanged(app: &CedarApp) {
    let doc = &app.documents[0];
    assert_eq!(doc.text, "class Main { int draft; }");
    assert_eq!(doc.saved_text, "class Main {}");
    assert_eq!(doc.revision.as_deref(), Some("original-revision"));
    assert_eq!(doc.edit_version, 1);
    let state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64))).unwrap();
    assert_eq!(state.cursor.char_range(), Some(selection()));
    // Check both affinity bits as well as the complete directional selection.
    let actual = state.cursor.char_range().unwrap();
    assert!(!actual.primary.prefer_next_row);
    assert!(actual.secondary.prefer_next_row);
    let after = (actual, doc.text.clone());
    let mut history = state.undoer();
    let cursor_checkpoint = history.undo(&after).unwrap().clone();
    let before = if cursor_checkpoint.1 == doc.text {
        history.undo(&cursor_checkpoint).unwrap().clone()
    } else {
        cursor_checkpoint
    };
    assert_eq!(before.1, "class Main {}");
    assert_eq!(history.redo(&before).unwrap().1, doc.text);
}

#[test]
fn linux_stop_preserves_draft_selection_undo_and_error_close_veto() {
    for status in ["forced", "error"] {
        let (mut app, commands) = app();
        seed_draft(&mut app);
        app.language.running = true;
        app.close_after_language_stop = true;
        app.close_snapshot = Some(vec![]);
        let mut stopped = linux_stop(status, true);
        stopped["private"] = json!("private stderr");
        app.apply_language_action(
            Action {
                session: app.language.session,
                kind: ActionKind::Stop,
            },
            stopped,
        );
        assert!(!app.language.running);
        assert!(!app.language.restart_blocked);
        assert!(app.language.idle_for_disconnect());
        assert!(app.language.output.contains("signal 9"));
        assert!(!app.language.output.contains("137"));
        assert!(!app.language.output.contains("private"));
        assert_eq!(app.error.is_some(), status == "error");
        assert_eq!(app.close_after_language_stop, status != "error");
        assert_eq!(app.close_snapshot.is_some(), status != "error");
        assert!(commands.try_recv().is_err());
        assert_draft_unchanged(&app);
    }
}

#[test]
fn invalid_or_uncertain_linux_stop_blocks_restart_and_preserves_draft_history() {
    for stopped in [
        Some(linux_stop("forced", false)),
        Some(linux_stop("graceful", true)),
        Some(json!({"stopped":true})),
        None,
    ] {
        let (mut app, commands) = app();
        seed_draft(&mut app);
        app.language.running = true;
        app.close_after_language_stop = true;
        app.close_snapshot = Some(vec![]);
        let action = Action {
            session: app.language.session,
            kind: ActionKind::Stop,
        };
        if let Some(stopped) = stopped {
            app.apply_language_action(action, stopped);
        } else {
            let message = app.language_public_error(&action, "private transport error");
            app.language_error(&action, &message);
        }
        assert!(!app.language.running);
        assert!(app.language.restart_blocked);
        assert!(!app.language.idle_for_disconnect());
        assert!(!app.close_after_language_stop);
        assert!(app.close_snapshot.is_none());
        assert!(app
            .language
            .output
            .contains("Inspect the previous server cleanup"));
        assert!(app.language.output.contains("explicitly reconnecting"));
        assert!(app
            .language
            .output
            .contains("reconnecting does not verify cleanup"));
        assert!(!app.language.output.contains("private"));
        app.start_language();
        app.finish_pending_close(&egui::Context::default());
        assert!(!app.allow_close);
        assert!(commands.try_recv().is_err());
        assert_draft_unchanged(&app);
    }
}

#[test]
fn backend_os_changes_only_java_executable_hint_not_authorization() {
    for os in ["windows", "linux", "unknown"] {
        let (mut app, commands) = app();
        app.agent_info.as_mut().unwrap().os = os.into();
        assert_eq!(
            app.java_executable_hint(),
            match os {
                "windows" => "Absolute ASCII path to java.exe",
                "linux" => "Absolute path to java",
                _ => "Absolute path to Java on the workspace host",
            }
        );
        app.active_form.as_mut().unwrap().allow_run = false;
        app.start_language();
        assert!(commands.try_recv().is_err());
        app.active_form.as_mut().unwrap().allow_run = true;
        app.agent_info
            .as_mut()
            .unwrap()
            .capabilities
            .retain(|name| name != "language_start_java");
        assert!(app.backend_generic_language_supported());
        assert!(!app.backend_java_language_supported());
        app.start_language();
        assert!(commands.try_recv().is_err());
        assert_eq!(app.language.session, 0);
    }
    let (mut app, commands) = app();
    app.start_language();
    assert!(matches!(commands.try_recv().unwrap().op,
        Operation::LanguageStartJava { java_executable, distribution, data_directory }
            if java_executable == "/opt/java/bin/java"
                && distribution == "/opt/JDT 雪"
                && data_directory == "/opt/Java data 雪"));
    assert!(commands.try_recv().is_err());
}
