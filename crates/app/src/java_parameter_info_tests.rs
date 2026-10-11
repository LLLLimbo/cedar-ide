//! Production-frame activation, exact admission and inert/stale-result coverage.
use super::*;
use crate::worker::{Command, Event, Worker, WorkerEvent};
use crate::Payload;
use serde_json::json;
use std::sync::mpsc::Receiver;

const SOURCE: &str = "/*😀*/ class Main { void run() { call(1, 2); } }\n";
const FIRST: &str = "call(int first, int second)";
const SECOND: &str = "call(String value)";

fn signatures() -> Value {
    json!({"signatures":[
        {"label": FIRST, "parameters":[{"label":[5,14]},{"label":"int second"}]},
        {"label": SECOND, "parameters":[{"label":"String value"}]}
    ],"activeSignature":0,"activeParameter":1})
}

fn app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = crate::ConnectForm {
        local_root: "/workspace".into(),
        allow_run: true,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/workspace".into();
    app.state = crate::ConnectionState::Ready;
    app.agent_info = Some(crate::agent_support::full_test_agent());
    app.language.mode = ServerMode::Java;
    app.language.running = true;
    app.language.session = 7;
    app.language.automatic = false;
    app.language.capabilities = json!({"signatureHelpProvider":{"triggerCharacters":["(",","]}});
    app.tool = crate::Tool::Language;
    app.tools_open = true;
    app.tools_height = Some(570.0);
    app.open_form = false;
    app.documents.push(Document::new(
        1,
        "Main.java".into(),
        SOURCE.into(),
        "saved-revision".into(),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    set_cursor(
        &mut app,
        SOURCE[..SOURCE.find("2);").unwrap()].chars().count(),
    );
    acknowledge(&mut app, 1);
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn set_cursor(app: &mut CedarApp, chars: usize) {
    let doc = app
        .documents
        .iter_mut()
        .find(|doc| Some(doc.id) == app.active_document)
        .unwrap();
    let mut state = crate::editor_state::load(&app.editor_ctx, doc);
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(chars),
        )));
    state.store(&app.editor_ctx, egui::Id::new(("editor", doc.id)));
    doc.cursor = crate::model::cursor_location(&doc.text, chars);
}

fn acknowledge(app: &mut CedarApp, version: i32) {
    let doc = app.active().unwrap();
    let (id, edit_version, path) = (doc.id, doc.edit_version, doc.path.clone());
    app.language.sync.acknowledge(
        id,
        Acknowledged {
            version,
            edit_version,
            uri: format!("file:///workspace/{path}"),
        },
    );
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

fn frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut input = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1320.0, 1080.0),
        )),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native))
}

fn settle(app: &mut CedarApp) {
    for time in [0.0, 0.1, 0.2] {
        frame(app, time, vec![]);
    }
}

fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

fn click(app: &mut CedarApp, name: &str, time: f64) {
    let pos = crate::workspace_access_tests::recorded_rect(app, name).center();
    frame(app, time, pointer(pos, true));
    frame(app, time + 0.01, pointer(pos, false));
}

fn event(app: &CedarApp, command: &Command, result: Result<Payload, String>) -> Event {
    Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result,
    }
}

fn queued(app: &CedarApp, command: &Command, result: Result<Payload, String>) {
    app.result_tx
        .send(WorkerEvent::Response(event(app, command, result)))
        .unwrap();
}

fn dispatch(app: &mut CedarApp, commands: &Receiver<Command>) -> Command {
    app.request_java_parameter_info();
    assert!(
        commands.try_recv().is_err(),
        "intent capture must not dispatch"
    );
    app.finish_java_parameter_info_frame();
    let command = commands.try_recv().unwrap();
    assert!(matches!(
        command.op,
        Operation::LanguageJavaSignatureHelp { .. }
    ));
    command
}

fn result(app: &mut CedarApp, command: &Command, value: Value) {
    app.apply_event(event(app, command, Ok(Payload::Language { value })));
    assert!(
        app.language.parameter_info.result.is_none(),
        "reply waits for final admission"
    );
    app.finish_java_parameter_info_frame();
}

fn label_at(output: &egui::FullOutput, wanted: &str) -> Option<egui::Pos2> {
    fn find(shape: &egui::epaint::Shape, wanted: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text == wanted => {
                Some(text.visual_bounding_rect().center())
            }
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, wanted)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, wanted))
}

fn has_label(output: &egui::FullOutput, wanted: &str) -> bool {
    label_at(output, wanted).is_some()
}

#[test]
fn explicit_native_button_synchronizes_exact_draft_then_queries_once() {
    for opened in [false, true] {
        let (mut app, commands) = app();
        settle(&mut app);
        if opened {
            crate::editor_state::commit(
                &app.editor_ctx,
                &mut app.documents[0],
                SOURCE.replace("2);", "3);"),
                39,
            );
        } else {
            app.language.sync.clear();
        }
        let expected = app.documents[0].text.clone();
        let cursor = utf16_position(&expected, app.documents[0].cursor).unwrap();
        let version = if opened { 2 } else { 1 };
        click(&mut app, "java_parameter_info", 0.3);
        let sync = commands
            .try_recv()
            .expect("explicit button sends ordinary sync");
        match &sync.op {
            Operation::LanguageOpen {
                path,
                language_id,
                version: actual,
                text,
            } if !opened => {
                assert_eq!(
                    (path.as_str(), language_id.as_str(), *actual, text.as_str()),
                    ("Main.java", "java", version, expected.as_str())
                );
            }
            Operation::LanguageChange {
                path,
                version: actual,
                text,
            } if opened => {
                assert_eq!(
                    (path.as_str(), *actual, text.as_str()),
                    ("Main.java", version, expected.as_str())
                );
            }
            other => panic!("unexpected sync {other:?}"),
        }
        assert!(commands.try_recv().is_err());
        queued(
            &app,
            &sync,
            Ok(Payload::Language {
                value: if opened {
                    json!({"changed":"file:///workspace/Main.java"})
                } else {
                    json!({"opened":"file:///workspace/Main.java"})
                },
            }),
        );
        frame(&mut app, 0.5, vec![]);
        let query = commands.try_recv().unwrap();
        assert!(
            matches!(query.op, Operation::LanguageJavaSignatureHelp { ref path, version: actual, line, character }
            if path == "Main.java" && actual == version && line == cursor.line && character == cursor.character)
        );
        queued(
            &app,
            &query,
            Ok(Payload::Language {
                value: signatures(),
            }),
        );
        frame(&mut app, 0.6, vec![]);
        let output = frame(&mut app, 0.7, vec![]);
        assert!(has_label(&output, FIRST));
        assert!(has_label(&output, "Server-selected overload"));
        assert!(!app.language.parameter_info.pending);
        assert_eq!(app.documents[0].text, expected);
        assert_eq!(app.documents[0].saved_text, SOURCE);
        assert_eq!(app.documents[0].revision.as_deref(), Some("saved-revision"));
        assert!(!app.language.automatic);
        for time in [0.8, 1.0, 2.0] {
            frame(&mut app, time, vec![]);
        }
        assert!(
            commands.try_recv().is_err(),
            "one explicit query; no save or follow-on call"
        );
    }
}

#[test]
fn native_tab_focus_and_enter_or_space_activate_parameter_info_without_a_shortcut() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (mut app, commands) = app();
        settle(&mut app);
        // Start from the real tools tab, then use native Tab traversal. Do not
        // request focus for the new control or inject a synthetic clicked response.
        click(&mut app, "LANGUAGE", 0.3);
        let wanted =
            crate::workspace_access_tests::recorded_response(&app, "java_parameter_info").id;
        let mut reached = false;
        for index in 0..80 {
            frame(
                &mut app,
                0.5 + index as f64 * 0.02,
                key(egui::Key::Tab, egui::Modifiers::NONE),
            );
            assert!(
                commands.try_recv().is_err(),
                "Tab cannot request parameter info"
            );
            if app.editor_ctx.memory(|memory| memory.focused()) == Some(wanted) {
                reached = true;
                break;
            }
        }
        assert!(reached, "native Tab traversal reaches Parameter info");
        let selection =
            egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64)))
                .unwrap()
                .cursor
                .char_range();
        let mut events = key(activation, egui::Modifiers::NONE);
        if activation == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        frame(&mut app, 3.0, events);
        assert!(matches!(
            commands.try_recv().unwrap().op,
            Operation::LanguageJavaSignatureHelp { .. }
        ));
        assert!(commands.try_recv().is_err());
        assert_eq!(app.documents[0].text, SOURCE);
        assert_eq!(
            egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64)))
                .unwrap()
                .cursor
                .char_range(),
            selection
        );
        assert_eq!(
            app.editor_ctx.memory(|memory| memory.focused()),
            Some(wanted)
        );
    }
}

#[test]
fn unsupported_native_controls_and_typed_operation_guards_emit_zero_operations() {
    for gate in 0..8 {
        let (mut app, commands) = app();
        match gate {
            0 => app
                .agent_info
                .as_mut()
                .unwrap()
                .capabilities
                .retain(|name| name != "language_java_signature_help"),
            1 => app.language.capabilities = json!({}),
            2 => app.language.capabilities = json!({"signatureHelpProvider":false}),
            3 => app.active_form.as_mut().unwrap().allow_run = false,
            4 => app.documents[0].path = "Main.kt".into(),
            5 => app.language.running = false,
            6 => app.language.diagnostics_exited = true,
            7 => app.close_after_language_stop = true,
            _ => unreachable!(),
        }
        settle(&mut app);
        assert!(
            !crate::workspace_access_tests::recorded_response(&app, "java_parameter_info")
                .enabled(),
            "gate {gate}"
        );
        click(&mut app, "java_parameter_info", 0.3);
        assert!(commands.try_recv().is_err(), "gate {gate}");
        app.request_java_parameter_info();
        app.finish_java_parameter_info_frame();
        assert!(commands.try_recv().is_err(), "programmatic gate {gate}");
    }
    for invalid in 0..6 {
        let (mut app, commands) = app();
        let cursor = utf16_position(SOURCE, app.documents[0].cursor).unwrap();
        let (mut path, mut version, mut line, mut character) =
            ("Main.java".to_owned(), 1, cursor.line, cursor.character);
        match invalid {
            0 => path = "Other.java".into(),
            1 => version = 0,
            2 => version = 2,
            3 => line += 1,
            4 => character += 1,
            5 => app.language.mode = ServerMode::Generic,
            _ => unreachable!(),
        }
        assert_eq!(
            app.request(
                Operation::LanguageJavaSignatureHelp {
                    path,
                    version,
                    line,
                    character
                },
                Job::Git
            ),
            0
        );
        assert!(commands.try_recv().is_err());
    }
}

fn change(app: &mut CedarApp, which: usize) {
    match which {
        0 => {
            app.documents[0].text.push(' ');
            app.documents[0].edit_version += 1;
        }
        1 => app.documents[0].cursor.1 += 1,
        2 => app.active_document = None,
        3 => app.language.parameter_info.reset(),
        4 => app.language.session += 1,
        5 => app.generation += 1,
        6 => app.root = "/another".into(),
        7 => app.workspace_key = None,
        8 => app.navigation_changed(),
        9 => app.language.view = View::Problems,
        10 => app.tools_open = false,
        11 => app.documents[0].path = "Renamed.java".into(),
        12 => app.documents[0].id = 42,
        13 => acknowledge(app, 2),
        14 => app.language.running = false,
        15 => app.language.capabilities = json!({}),
        16 => app.active_form.as_mut().unwrap().allow_run = false,
        17 => app.confirm = Some(crate::Confirm::CloseTab(1)),
        18 => app.language.features.cancel_pending(),
        _ => unreachable!(),
    }
}

#[test]
fn every_snapshot_guard_retires_success_failure_and_unexpected_payloads() {
    for which in 0..19 {
        for response in 0..3 {
            let (mut app, commands) = app();
            let command = dispatch(&mut app, &commands);
            let result = match response {
                0 => Ok(Payload::Language {
                    value: signatures(),
                }),
                1 => Err("late private Java failure".into()),
                2 => Ok(Payload::File {
                    path: "Main.java".into(),
                    text: "unexpected".into(),
                    revision: "bad".into(),
                }),
                _ => unreachable!(),
            };
            let reply = event(&app, &command, result);
            change(&mut app, which);
            app.error = None;
            app.apply_event(reply);
            app.finish_java_parameter_info_frame();
            assert!(
                app.language.parameter_info.context.is_none(),
                "guard {which}, response {response}"
            );
            assert!(app.language.parameter_info.result.is_none());
            assert!(app.language.parameter_info.error.is_none());
            assert!(app.error.is_none(), "late error escaped guard {which}");
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn final_frame_admission_rechecks_button_snapshot_after_editor_cursor_update() {
    for synced in [false, true] {
        let (mut app, commands) = app();
        settle(&mut app);
        if !synced {
            app.language.sync.clear();
        }
        let pos =
            crate::workspace_access_tests::recorded_rect(&app, "java_parameter_info").center();
        frame(&mut app, 0.3, pointer(pos, true));
        // The real editor consumes its pending selection after the tools render
        // captures the button intent, before the final admission hook.
        app.documents[0].jump_to = Some(0);
        frame(&mut app, 0.31, pointer(pos, false));
        assert_eq!(app.documents[0].cursor, (1, 1));
        assert!(app.language.parameter_info.context.is_none());
        assert!(
            commands.try_recv().is_err(),
            "stale click must not even synchronize"
        );
    }
}

#[test]
fn actual_frame_ready_success_and_error_yield_to_edit_cursor_dismiss_and_tab() {
    for input in 0..8 {
        for error in [false, true] {
            let (mut app, commands) = app();
            app.documents.push(Document::new(
                2,
                "Other.java".into(),
                "class Other {}".into(),
                "other".into(),
            ));
            app.next_document = 3;
            settle(&mut app);
            app.editor_ctx
                .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
            frame(&mut app, 0.3, vec![]);
            let command = dispatch(&mut app, &commands);
            let output = frame(&mut app, 0.4, vec![]);
            let events = match input {
                0 => vec![egui::Event::Text("x".into())],
                1 => vec![egui::Event::Paste("paste".into())],
                2 => key(egui::Key::ArrowRight, egui::Modifiers::NONE),
                3 => key(egui::Key::Escape, egui::Modifiers::NONE),
                4..=7 => {
                    let pos = match input {
                        4 => label_at(&output, "Other.java").expect("real document tab"),
                        5 => crate::workspace_access_tests::recorded_rect(
                            &app,
                            "dismiss_parameter_info",
                        )
                        .center(),
                        6 => label_at(&output, "Problems (0)").expect("real Problems view"),
                        7 => crate::workspace_access_tests::recorded_rect(&app, "close_tools")
                            .center(),
                        _ => unreachable!(),
                    };
                    frame(&mut app, 0.45, pointer(pos, true));
                    pointer(pos, false)
                }
                _ => unreachable!(),
            };
            queued(
                &app,
                &command,
                if error {
                    Err("obsolete server failure".into())
                } else {
                    Ok(Payload::Language {
                        value: signatures(),
                    })
                },
            );
            frame(&mut app, 0.5, events);
            if input < 2 {
                assert_ne!(app.documents[0].text, SOURCE);
            }
            if input == 4 {
                assert_eq!(app.active_document, Some(2));
            }
            assert!(
                app.language.parameter_info.context.is_none(),
                "input {input}, error {error}"
            );
            assert!(app.language.parameter_info.result.is_none());
            assert!(app.error.is_none());
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn newer_explicit_query_wins_over_queued_old_success_or_error() {
    for error in [false, true] {
        let (mut app, commands) = app();
        settle(&mut app);
        click(&mut app, "java_parameter_info", 0.3);
        let first = commands.try_recv().unwrap();
        // The existing busy notice changes the tools layout after dispatch.
        // Capture the real control only after that pending layout is rendered.
        frame(&mut app, 0.35, vec![]);
        frame(&mut app, 0.36, vec![]);
        let pos =
            crate::workspace_access_tests::recorded_rect(&app, "java_parameter_info").center();
        frame(&mut app, 0.4, pointer(pos, true));
        queued(
            &app,
            &first,
            if error {
                Err("old failure".into())
            } else {
                Ok(Payload::Language {
                    value: signatures(),
                })
            },
        );
        frame(&mut app, 0.41, pointer(pos, false));
        assert!(app.language.parameter_info.result.is_none());
        assert!(app.language.parameter_info.error.is_none());
        let second = commands.try_recv().unwrap();
        assert_ne!(first.id, second.id);
        assert!(matches!(
            second.op,
            Operation::LanguageJavaSignatureHelp { .. }
        ));
        queued(
            &app,
            &second,
            Ok(Payload::Language {
                value: json!({"signatures":[]}),
            }),
        );
        frame(&mut app, 0.5, vec![]);
        let output = frame(&mut app, 0.6, vec![]);
        assert!(has_label(
            &output,
            "No signatures returned. Move the cursor inside a call or retry after indexing."
        ));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn cursor_and_visibility_roundtrips_do_not_resurrect_a_retired_request() {
    for hidden in [false, true] {
        let (mut app, commands) = app();
        let command = dispatch(&mut app, &commands);
        let old = app.documents[0].cursor;
        if hidden {
            app.tools_open = false;
        } else {
            app.documents[0].cursor.1 += 1;
        }
        app.finish_java_parameter_info_frame();
        app.tools_open = true;
        app.documents[0].cursor = old;
        result(&mut app, &command, signatures());
        assert!(app.language.parameter_info.context.is_none());
        assert!(app.language.parameter_info.result.is_none());
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn stop_and_connection_loss_retire_parameter_info_without_hiding_transport_failure() {
    {
        let (mut app, commands) = app();
        let command = dispatch(&mut app, &commands);
        app.stop_language();
        assert!(matches!(
            commands.try_recv().unwrap().op,
            Operation::LanguageStop
        ));
        result(&mut app, &command, signatures());
        assert!(app.language.parameter_info.result.is_none());
    }
    let (mut app, commands) = app();
    let command = dispatch(&mut app, &commands);
    app.language.parameter_info.reset();
    let mut lost = event(&app, &command, Err("closed".into()));
    lost.connected = false;
    app.apply_event(lost);
    assert!(!app.ready());
    assert_eq!(app.documents[0].text, SOURCE);
    assert!(app.language.parameter_info.result.is_none());
}

#[test]
fn overload_browsing_keeps_selection_history_draft_disk_and_undo_unchanged() {
    let (mut app, commands) = app();
    settle(&mut app);
    let cursor =
        egui::text::CCursorRange::two(egui::text::CCursor::new(38), egui::text::CCursor::new(40));
    let draft = SOURCE.replace("2);", "4);");
    crate::editor_state::commit_selection(
        &app.editor_ctx,
        &mut app.documents[0],
        draft.clone(),
        cursor,
    );
    acknowledge(&mut app, 2);
    let doc = &app.documents[0];
    let checkpoint = (
        doc.edit_version,
        doc.cursor,
        doc.saved_text.clone(),
        doc.revision.clone(),
    );
    let history = (
        app.location_history.back.len(),
        app.location_history.forward.len(),
        app.location_history.pending.is_some(),
        app.navigation_epoch,
    );
    click(&mut app, "java_parameter_info", 0.3);
    let command = commands.try_recv().unwrap();
    queued(
        &app,
        &command,
        Ok(Payload::Language {
            value: signatures(),
        }),
    );
    frame(&mut app, 0.4, vec![]);
    frame(&mut app, 0.5, vec![]);
    assert_eq!(
        app.language
            .parameter_info
            .result
            .as_ref()
            .unwrap()
            .active_parameter(0),
        Some(1)
    );
    click(&mut app, "java_parameter_overload_1", 0.6);
    assert_eq!(app.language.parameter_info.selected, Some(1));
    assert_eq!(
        app.language
            .parameter_info
            .result
            .as_ref()
            .unwrap()
            .active_parameter(1),
        None,
        "top-level hint must not migrate to a manually selected row"
    );
    let output = frame(&mut app, 0.7, vec![]);
    assert!(has_label(&output, SECOND));
    assert!(!has_label(&output, "Server-selected overload"));
    let doc = &app.documents[0];
    assert_eq!(doc.text, draft);
    assert_eq!(
        (
            doc.edit_version,
            doc.cursor,
            doc.saved_text.clone(),
            doc.revision.clone()
        ),
        checkpoint
    );
    assert_eq!(
        (
            app.location_history.back.len(),
            app.location_history.forward.len(),
            app.location_history.pending.is_some(),
            app.navigation_epoch
        ),
        history
    );
    let state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64))).unwrap();
    assert!(crate::location_history::same_selection(
        state.cursor.char_range().unwrap(),
        cursor
    ));
    let mut undo = state.undoer();
    let before = undo.undo(&(cursor, draft.clone())).unwrap().clone();
    assert_eq!(before.1, SOURCE);
    assert_eq!(undo.redo(&before).unwrap().1, draft);
    assert!(
        commands.try_recv().is_err(),
        "viewing cannot save, resolve, navigate or launch work"
    );
}

#[test]
fn inert_bounded_results_expose_hint_policies_without_inventing_defaults() {
    let (mut app, commands) = app();
    let command = dispatch(&mut app, &commands);
    result(
        &mut app,
        &command,
        json!({"signatures":[{"label":"f(😀, x)","parameters":[{"label":[2,4]},{"label":"x"}],"activeParameter":99}],"activeSignature":99,"activeParameter":1}),
    );
    let parsed = app.language.parameter_info.result.as_ref().unwrap();
    assert_eq!(parsed.initial_signature(), Some(0));
    assert_eq!(parsed.server_selected_signature(), None);
    assert_eq!(parsed.active_parameter(0), None);
    assert_eq!(parsed.highlight(0), None);
    let output = frame(&mut app, 0.0, vec![]);
    assert!(has_label(
        &output,
        "First overload shown; the server did not select a valid overload"
    ));
    assert!(!has_label(&output, "Server-selected overload"));
    for value in [json!(null), json!({"signatures":[]})] {
        let command = dispatch(&mut app, &commands);
        result(&mut app, &command, value);
        assert!(app
            .language
            .parameter_info
            .result
            .as_ref()
            .unwrap()
            .signatures
            .is_empty());
    }
    for value in [
        json!({"signatures":[{"label":"x".repeat(8193)}]}),
        json!({"signatures":[{"label":"f(😀)","parameters":[{"label":[2,3]}]}]}),
        json!({"signatures":[{"label":"bad","parameters":[{"label":false}]}]}),
    ] {
        let command = dispatch(&mut app, &commands);
        result(&mut app, &command, value);
        assert!(app.language.parameter_info.result.is_none());
        assert!(app
            .language
            .parameter_info
            .error
            .as_ref()
            .unwrap()
            .contains("invalid or over-limit"));
    }
}

#[test]
fn failed_or_malformed_sync_is_not_retried_without_another_explicit_request() {
    for malformed in [false, true] {
        let (mut app, commands) = app();
        settle(&mut app);
        app.language.sync.clear();
        click(&mut app, "java_parameter_info", 0.3);
        let sync = commands.try_recv().unwrap();
        assert!(matches!(sync.op, Operation::LanguageOpen { .. }));
        queued(
            &app,
            &sync,
            if malformed {
                Ok(Payload::Language {
                    value: json!({"unexpected":true}),
                })
            } else {
                Err("sync failed".into())
            },
        );
        frame(&mut app, 0.4, vec![]);
        for time in [0.5, 1.0, 2.0] {
            frame(&mut app, time, vec![]);
        }
        assert!(!app.language.parameter_info.waiting);
        assert!(app.language.parameter_info.result.is_none());
        assert!(
            commands.try_recv().is_err(),
            "failed sync cannot retry or query automatically"
        );
        click(&mut app, "java_parameter_info", 2.1);
        assert!(matches!(
            commands.try_recv().unwrap().op,
            Operation::LanguageOpen { .. }
        ));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn dismissal_retires_parameter_info_but_keeps_existing_ordinary_sync_error_status() {
    let (mut app, commands) = app();
    settle(&mut app);
    app.language.sync.clear();
    click(&mut app, "java_parameter_info", 0.3);
    let sync = commands.try_recv().unwrap();
    assert!(matches!(sync.op, Operation::LanguageOpen { .. }));
    frame(&mut app, 0.4, vec![]);
    click(&mut app, "dismiss_parameter_info", 0.5);
    assert!(app.language.parameter_info.context.is_none());
    assert!(app.language.view == View::Problems);
    // Dismissal cannot cancel an already admitted ordinary LanguageOpen. Its
    // established global status/error behavior is deliberately preserved.
    queued(&app, &sync, Err("ordinary synchronization failed".into()));
    frame(&mut app, 0.6, vec![]);
    assert!(app.error.is_some());
    assert!(app.language.paused_reason.is_some());
    assert!(app.language.parameter_info.context.is_none());
    assert!(app.language.parameter_info.result.is_none());
    assert!(!app.language.parameter_info.waiting);
    assert!(app.language.view == View::Problems);
    assert_eq!(app.documents[0].text, SOURCE);
    assert!(commands.try_recv().is_err());
}

#[test]
fn old_generation_transport_loss_cannot_disconnect_the_new_connection() {
    let (mut app, commands) = app();
    let command = dispatch(&mut app, &commands);
    let mut obsolete = event(&app, &command, Err("old transport closed".into()));
    obsolete.connected = false;
    app.generation += 1;
    app.language.session += 1;
    app.pending.clear();
    app.language.parameter_info.reset();
    app.result_tx.send(WorkerEvent::Response(obsolete)).unwrap();
    frame(&mut app, 0.0, vec![]);
    assert!(app.ready());
    assert!(app.language.running);
    assert!(app.error.is_none());
    assert!(app.language.parameter_info.result.is_none());
    assert!(commands.try_recv().is_err());
}

#[test]
fn one_deferred_reply_owns_the_request_and_duplicate_delivery_is_bounded() {
    let (mut app, commands) = app();
    let command = dispatch(&mut app, &commands);
    let first = event(
        &app,
        &command,
        Ok(Payload::Language {
            value: signatures(),
        }),
    );
    app.apply_event(first);
    for _ in 0..64 {
        app.apply_event(event(
            &app,
            &command,
            Err("duplicate must not replace first response".into()),
        ));
    }
    assert!(app.language.parameter_info.reply.is_some());
    assert_eq!(app.pending.len(), 1);
    app.finish_java_parameter_info_frame();
    assert!(app.language.parameter_info.reply.is_none());
    assert!(app.pending.is_empty());
    assert_eq!(
        app.language
            .parameter_info
            .result
            .as_ref()
            .unwrap()
            .signatures
            .len(),
        2
    );
    assert!(app.language.parameter_info.error.is_none());
    assert!(commands.try_recv().is_err());
}

#[test]
fn typing_trigger_characters_and_completion_results_do_not_request_parameter_info() {
    let (mut app, commands) = app();
    settle(&mut app);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
    for (index, text) in ["(", ",", ")"].iter().enumerate() {
        frame(
            &mut app,
            0.3 + index as f64 * 0.1,
            vec![egui::Event::Text((*text).into())],
        );
        assert!(commands.try_recv().is_err());
        assert!(app.language.parameter_info.context.is_none());
    }
    app.language.capabilities = json!({"signatureHelpProvider":{},"completionProvider":{}});
    acknowledge(&mut app, 2);
    app.request_language_feature(cedar_protocol::LanguageQueryKind::Completion);
    frame(&mut app, 0.8, vec![]);
    let completion = commands.try_recv().unwrap();
    assert!(matches!(
        completion.op,
        Operation::LanguageQuery {
            kind: cedar_protocol::LanguageQueryKind::Completion,
            ..
        }
    ));
    queued(
        &app,
        &completion,
        Ok(Payload::Language {
            value: json!([{"label":"call"}]),
        }),
    );
    frame(&mut app, 0.9, vec![]);
    frame(&mut app, 1.0, vec![]);
    assert!(app.language.parameter_info.context.is_none());
    assert!(commands.try_recv().is_err());
}
