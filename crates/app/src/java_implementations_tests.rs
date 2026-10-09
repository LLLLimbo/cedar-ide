//! Independent parsing, exact synchronization, retained navigation and native-frame tests.
use super::*;
use crate::{
    worker::{Command, Event, Worker, WorkerEvent},
    Payload,
};
use serde_json::json;
use std::sync::mpsc::Receiver;

const SOURCE: &str = "/*😀*/ interface Source { void run(); }\n";
const TARGET: &str = "/*😀*/ class Target implements Source { public void run() {} }\n";
const URI: &str = "file:///workspace/Target.java";

fn marker(text: &str, name: &str) -> Range {
    let offset = text[..text.find(name).unwrap()].chars().count();
    Range {
        start: completion::chars_to_position(text, offset).unwrap(),
        end: completion::chars_to_position(text, offset + name.chars().count()).unwrap(),
    }
}
fn location(uri: &str) -> Value {
    let range = marker(TARGET, "Target");
    json!({"uri":uri,"range":{"start":{"line":range.start.line,"character":range.start.character},"end":{"line":range.end.line,"character":range.end.character}}})
}
fn rows() -> Value {
    json!([location(URI)])
}
fn acknowledge(app: &mut CedarApp, id: u64, version: i32) {
    let doc = app.documents.iter().find(|doc| doc.id == id).unwrap();
    app.language.sync.acknowledge(
        id,
        Acknowledged {
            version,
            edit_version: doc.edit_version,
            uri: format!("file:///workspace/{}", doc.path),
        },
    );
}
fn set_cursor(app: &mut CedarApp, position: Position) {
    let doc = app
        .documents
        .iter_mut()
        .find(|doc| Some(doc.id) == app.active_document)
        .unwrap();
    let chars = completion::position_to_offsets(&doc.text, position)
        .unwrap()
        .1;
    doc.cursor = crate::model::cursor_location(&doc.text, chars);
    let mut state = crate::editor_state::load(&app.editor_ctx, doc);
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(chars),
        )));
    state.store(&app.editor_ctx, egui::Id::new(("editor", doc.id)));
}
fn app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    app.state = crate::ConnectionState::Ready;
    app.agent_info = Some(crate::agent_support::full_test_agent());
    app.active_form = Some(crate::ConnectForm {
        allow_run: true,
        ..Default::default()
    });
    app.language.mode = ServerMode::Java;
    app.language.running = true;
    app.language.session = 3;
    app.language.automatic = false;
    app.language.capabilities = json!({"implementationProvider":true});
    app.language.view = View::JavaImplementations;
    app.tools_open = true;
    app.tool = crate::Tool::Language;
    app.open_form = false;
    app.documents.push(Document::new(
        1,
        "Source.java".into(),
        SOURCE.into(),
        "s0".into(),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    acknowledge(&mut app, 1, 1);
    set_cursor(&mut app, marker(SOURCE, "Source").start);
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}
fn event(app: &CedarApp, command: &Command, result: Result<Payload, String>) -> Event {
    Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result,
    }
}
fn reply(app: &mut CedarApp, command: &Command, result: Result<Payload, String>) {
    app.apply_event(event(app, command, result));
    app.finish_java_implementation_replies();
    app.invalidate_java_implementations();
}
fn query(app: &mut CedarApp, commands: &Receiver<Command>, value: Value) {
    app.request_java_implementations();
    assert!(app.language_feature_tick());
    let command = commands.try_recv().unwrap();
    assert!(matches!(
        command.op,
        Operation::LanguageJavaImplementations { .. }
    ));
    reply(app, &command, Ok(Payload::Language { value }));
}
fn select(app: &mut CedarApp, commands: &Receiver<Command>) -> Command {
    app.select_java_implementation(0);
    let command = commands.try_recv().unwrap();
    assert!(matches!(command.op, Operation::LanguageResolveUri { .. }));
    command
}
fn selection(app: &CedarApp, expected: Range) -> Result<(), String> {
    let doc = app.active().ok_or("No active target")?;
    let start = completion::position_to_offsets(&doc.text, expected.start)?.1;
    let end = completion::position_to_offsets(&doc.text, expected.end)?.1;
    let state = egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", doc.id)))
        .ok_or("No selection state")?;
    let actual = state.cursor.char_range().ok_or("No selection")?;
    if actual.primary.index.min(actual.secondary.index) != start
        || actual.primary.index.max(actual.secondary.index) != end
    {
        return Err("Full UTF-16 implementation range was not selected".into());
    }
    Ok(())
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
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1320.0, 1080.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    )
}
fn label(output: &egui::FullOutput, wanted: &str) -> Option<egui::Pos2> {
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

#[test]
fn implementation_parser_is_plain_bounded_atomic_and_inert() {
    let parse = crate::language_navigation_results::parse_java_implementations;
    assert!(parse(&json!([])).unwrap().is_empty());
    assert_eq!(parse(&rows()).unwrap()[0].range, marker(TARGET, "Target"));
    for uri in [
        URI,
        "jdt://contents/dependency.jar/A.class",
        "https://example.com/A.java",
        "file:///outside/A.java",
        "file:///with%20space/雪.java",
    ] {
        assert_eq!(parse(&json!([location(uri)])).unwrap().len(), 1);
    }
    for malformed in [
        Value::Null,
        json!({}),
        json!(false),
        location(URI),
        json!([null]),
        json!([location(URI), null]),
    ] {
        assert!(parse(&malformed).is_err());
    }
    for key in ["uri", "range"] {
        let mut bad = location(URI);
        bad.as_object_mut().unwrap().remove(key);
        assert!(parse(&json!([bad])).is_err());
    }
    for key in [
        "targetUri",
        "targetRange",
        "targetSelectionRange",
        "originSelectionRange",
        "command",
        "data",
        "newText",
        "version",
        "resourceOperations",
    ] {
        let mut bad = location(URI);
        bad[key] = json!({"deep":[1, 2]});
        assert!(parse(&json!([location(URI), bad])).is_err());
    }
    for pointer in ["/range", "/range/start", "/range/end"] {
        let mut bad = location(URI);
        bad.pointer_mut(pointer).unwrap()["extra"] = json!(true);
        assert!(parse(&json!([bad])).is_err());
    }
    for value in [
        json!(-1),
        json!(1.0),
        json!(u32::MAX),
        json!("1"),
        Value::Null,
    ] {
        let mut bad = location(URI);
        bad["range"]["start"]["line"] = value;
        assert!(parse(&json!([bad])).is_err());
    }
    let mut reversed = location(URI);
    reversed["range"]["end"]["character"] = json!(0);
    assert!(parse(&json!([reversed])).is_err());
    for uri in [
        "",
        "relative.java",
        "1bad:/A",
        "file:",
        "file:///a b",
        "file:///a\\b",
        "file:///a\nb",
        "file:///a%",
        "file:///a%GG",
        "file:///a%00",
        "file:///a%0a",
        "file:///a%C2%85",
        "file:///a%FF",
        "file:///a\"b",
        "file:///a<b",
        "file:///a{b",
        "file:///a`b",
    ] {
        assert!(parse(&json!([location(uri)])).is_err(), "{uri}");
    }
    assert_eq!(
        parse(&json!(vec![location(URI); 1024])).unwrap().len(),
        1024
    );
    assert!(parse(&json!(vec![location(URI); 1025])).is_err());
    let uri = format!("file:///{}", "u".repeat(16 * 1024 - 8));
    assert!(parse(&json!(vec![location(&uri); 32])).is_ok());
    assert!(parse(&json!(vec![location(&uri); 33])).is_err());
    assert!(parse(&json!([location(&(uri + "x"))])).is_err());
}

#[test]
fn typed_trust_provider_and_capability_gates_emit_no_request() {
    for gate in 0..12 {
        let (mut app, commands) = app();
        match gate {
            0 => app.state = crate::ConnectionState::Disconnected,
            1 => app.language.running = false,
            2 => app.language.mode = ServerMode::Generic,
            3 => app.active_form.as_mut().unwrap().allow_run = false,
            4 => app.language.diagnostics_exited = true,
            5 => app.language.capabilities = json!({}),
            6 => app.language.capabilities = json!({"implementationProvider":false}),
            7 => app.language.capabilities = json!({"implementationProvider":"true"}),
            8 => app
                .agent_info
                .as_mut()
                .unwrap()
                .capabilities
                .retain(|name| name != "language_java_implementations"),
            9 => app.active_document = None,
            10 => app.documents[0].path = "Source.kt".into(),
            11 => app.close_after_language_stop = true,
            _ => unreachable!(),
        }
        app.request_java_implementations();
        assert!(!app.language_feature_tick(), "gate {gate}");
        assert!(commands.try_recv().is_err(), "gate {gate}");
        assert!(app
            .java_implementations_operation_problem("Source.java", 1, 0, 17)
            .is_some());
    }
    for provider in [json!(true), json!({})] {
        let (mut app, commands) = app();
        app.language.capabilities = json!({"implementationProvider":provider});
        query(&mut app, &commands, json!([]));
        assert!(app
            .language
            .implementations
            .rows
            .as_ref()
            .unwrap()
            .is_empty());
    }
}

#[test]
fn query_syncs_all_java_drafts_and_uses_acknowledged_version_and_utf16_cursor() {
    let (mut app, commands) = app();
    app.language.sync.clear();
    app.documents.push(Document::new(
        2,
        "Other.java".into(),
        "class Other {}".into(),
        "o0".into(),
    ));
    app.next_document = 3;
    app.request_java_implementations();
    for expected in ["Source.java", "Other.java"] {
        assert!(app.language_feature_tick());
        let command = commands.try_recv().unwrap();
        let version = match &command.op {
            Operation::LanguageOpen { path, version, .. } => {
                assert_eq!(path, expected);
                *version
            }
            _ => panic!("expected explicit document sync"),
        };
        reply(
            &mut app,
            &command,
            Ok(Payload::Language {
                value: json!({"opened":format!("file:///workspace/{expected}"),"version":version}),
            }),
        );
    }
    assert!(app.language_feature_tick());
    let command = commands.try_recv().unwrap();
    let position = marker(SOURCE, "Source").start;
    let version = app.language.sync.opened[&1].version;
    assert!(
        matches!(command.op, Operation::LanguageJavaImplementations { ref path, version: actual, line, character } if path == "Source.java" && actual == version && line == position.line && character == position.character)
    );
    assert!(!app.language.automatic);
    assert!(commands.try_recv().is_err());
    for (path, version, line, character) in [
        ("Other.java", version, position.line, position.character),
        (
            "Source.java",
            version + 1,
            position.line,
            position.character,
        ),
        (
            "Source.java",
            version,
            position.line,
            position.character + 1,
        ),
    ] {
        assert!(app
            .java_implementations_operation_problem(path, version, line, character)
            .is_some());
    }
}

fn change(app: &mut CedarApp, which: usize) {
    match which {
        0 => {
            app.documents[0].text.push('x');
            app.documents[0].edit_version += 1;
        }
        1 => app.documents[0].cursor.1 += 1,
        2 => app.cancel_java_implementations(),
        3 => app.generation += 1,
        4 => app.language.session += 1,
        5 => app.navigation_changed(),
        6 => app.language.view = View::Problems,
        7 => app.tools_open = false,
        8 => app.tool = crate::Tool::Search,
        9 => app.documents[0].path = "Renamed.java".into(),
        10 => app.documents[0].id = 3,
        11 => {
            app.documents[1].text.push('x');
            app.documents[1].edit_version += 1;
        }
        12 => {
            app.documents.pop();
        }
        13 => app.documents.push(Document::new(
            3,
            "New.java".into(),
            "class New {}".into(),
            "n0".into(),
        )),
        14 => acknowledge(app, 1, 10),
        15 => acknowledge(app, 2, 10),
        16 => app.request_java_implementations(),
        17 => app.language.capabilities = json!({"implementationProvider":false}),
        18 => app.active_form.as_mut().unwrap().allow_run = false,
        19 => app.confirm = Some(crate::Confirm::CloseTab(1)),
        _ => unreachable!(),
    }
}
#[test]
fn query_resolve_and_read_retain_every_context_guard_and_suppress_stale_errors() {
    for phase in 0..3 {
        for which in 0..20 {
            for error in [false, true] {
                let (mut app, commands) = app();
                app.documents.push(Document::new(
                    2,
                    "Other.java".into(),
                    "class Other {}".into(),
                    "o0".into(),
                ));
                app.next_document = 3;
                acknowledge(&mut app, 2, 2);
                app.request_java_implementations();
                app.language_feature_tick();
                let mut command = commands.try_recv().unwrap();
                let mut value = Payload::Language { value: rows() };
                if phase > 0 {
                    reply(&mut app, &command, Ok(value));
                    command = select(&mut app, &commands);
                    value = Payload::Language {
                        value: json!({"path":"Target.java"}),
                    };
                }
                if phase > 1 {
                    reply(&mut app, &command, Ok(value));
                    command = commands.try_recv().unwrap();
                    assert!(matches!(command.op, Operation::Read { .. }));
                    value = Payload::File {
                        path: "Target.java".into(),
                        text: TARGET.into(),
                        revision: "t0".into(),
                    };
                }
                change(&mut app, which);
                app.error = None;
                // Keep the response's original generation for reconnect races.
                let result = if error {
                    Err("obsolete implementation failure".into())
                } else {
                    Ok(value)
                };
                app.apply_event(Event {
                    generation: 0,
                    id: command.id,
                    connected: true,
                    result,
                });
                app.finish_java_implementation_replies();
                app.invalidate_java_implementations();
                assert!(
                    !app.documents.iter().any(|doc| doc.path == "Target.java"),
                    "phase {phase}, change {which}"
                );
                assert!(
                    commands.try_recv().is_err(),
                    "phase {phase}, change {which}"
                );
                assert!(
                    app.error.is_none(),
                    "stale error phase {phase}, change {which}"
                );
            }
        }
    }
}

#[test]
fn external_archives_root_escape_and_bad_read_are_never_opened() {
    for uri in [
        "jdt://contents/library/A.class",
        "https://example.com/A.java",
    ] {
        let (mut app, commands) = app();
        query(&mut app, &commands, json!([location(uri)]));
        app.select_java_implementation(0);
        assert!(commands.try_recv().is_err());
        assert_eq!(app.documents.len(), 1);
    }
    for path in [
        "/outside/A.java",
        "../A.java",
        "C:/A.java",
        "a\\A.java",
        "a//A.java",
    ] {
        let (mut app, commands) = app();
        query(&mut app, &commands, rows());
        let command = select(&mut app, &commands);
        reply(
            &mut app,
            &command,
            Ok(Payload::Language {
                value: json!({"path":path}),
            }),
        );
        assert!(commands.try_recv().is_err());
        assert_eq!(app.documents.len(), 1);
    }
    for capability in ["language_resolve_uri", "read"] {
        let (mut app, commands) = app();
        query(&mut app, &commands, rows());
        app.agent_info
            .as_mut()
            .unwrap()
            .capabilities
            .retain(|name| name != capability);
        app.select_java_implementation(0);
        assert!(commands.try_recv().is_err());
    }
    for phase in 0..2 {
        let (mut app, commands) = app();
        app.request_java_implementations();
        app.language_feature_tick();
        let mut command = commands.try_recv().unwrap();
        if phase == 1 {
            reply(&mut app, &command, Ok(Payload::Language { value: rows() }));
            command = select(&mut app, &commands);
        }
        reply(
            &mut app,
            &command,
            Ok(Payload::File {
                path: "unexpected".into(),
                text: "".into(),
                revision: "".into(),
            }),
        );
        assert!(!app.language.implementations.pending && !app.language.implementations.navigating);
        assert!(app.language.implementations.error.is_some());
    }
}

#[test]
fn cursor_and_visibility_roundtrips_never_resurrect_query_resolve_or_read() {
    for phase in 0..3 {
        for hide in [false, true] {
            let (mut app, commands) = app();
            app.request_java_implementations();
            app.language_feature_tick();
            let mut command = commands.try_recv().unwrap();
            let mut value = Payload::Language { value: rows() };
            if phase > 0 {
                reply(&mut app, &command, Ok(value));
                command = select(&mut app, &commands);
                value = Payload::Language {
                    value: json!({"path":"Target.java"}),
                };
            }
            if phase > 1 {
                reply(&mut app, &command, Ok(value));
                command = commands.try_recv().unwrap();
                value = Payload::File {
                    path: "Target.java".into(),
                    text: TARGET.into(),
                    revision: "t0".into(),
                };
            }
            let cursor = app.documents[0].cursor;
            if hide {
                app.tools_open = false;
            } else {
                app.documents[0].cursor.1 += 1;
            }
            app.invalidate_java_implementations();
            app.tools_open = true;
            app.documents[0].cursor = cursor;
            reply(&mut app, &command, Ok(value));
            assert_eq!(app.documents.len(), 1, "phase {phase}, hide {hide}");
            assert!(app.language.implementations.context.is_none());
            assert!(commands.try_recv().is_err());
            // A genuinely new request remains available after dismissal.
            query(&mut app, &commands, rows());
            assert_eq!(app.language.implementations.rows.as_ref().unwrap().len(), 1);
        }
    }
}

#[test]
fn actual_frame_ready_query_resolve_read_yield_to_text_paste_cursor_and_escape() {
    for phase in 0..3 {
        for input in 0..4 {
            let (mut app, commands) = app();
            // Existing dirty targets make the resolve race capable of changing focus
            // immediately; ordinary Read races exercise the unopened-target branch.
            if phase == 1 {
                app.documents.push(Document::new(
                    2,
                    "Target.java".into(),
                    TARGET.into(),
                    "t0".into(),
                ));
                app.next_document = 3;
                acknowledge(&mut app, 2, 2);
            }
            frame(&mut app, 0.0, vec![]);
            app.editor_ctx
                .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
            set_cursor(&mut app, marker(SOURCE, "Source").start);
            frame(&mut app, 0.1, vec![]);
            app.request_java_implementations();
            app.language_feature_tick();
            let mut command = commands.try_recv().unwrap();
            let mut value = Payload::Language { value: rows() };
            if phase > 0 {
                reply(&mut app, &command, Ok(value));
                command = select(&mut app, &commands);
                value = Payload::Language {
                    value: json!({"path":"Target.java"}),
                };
            }
            if phase > 1 {
                reply(&mut app, &command, Ok(value));
                command = commands.try_recv().unwrap();
                value = Payload::File {
                    path: "Target.java".into(),
                    text: TARGET.into(),
                    revision: "t0".into(),
                };
            }
            app.result_tx
                .send(WorkerEvent::Response(event(&app, &command, Ok(value))))
                .unwrap();
            let events = match input {
                0 => vec![egui::Event::Text("typed".into())],
                1 => vec![egui::Event::Paste("pasted".into())],
                2 => key(egui::Key::ArrowRight, egui::Modifiers::NONE),
                3 => key(egui::Key::Escape, egui::Modifiers::NONE),
                _ => unreachable!(),
            };
            frame(&mut app, 0.2, events);
            assert_eq!(app.active_document, Some(1), "phase {phase}, input {input}");
            assert_eq!(app.documents.len(), if phase == 1 { 2 } else { 1 });
            if phase == 1 {
                assert_eq!(app.documents[1].text, TARGET);
            }
            if input < 2 {
                assert_ne!(app.documents[0].text, SOURCE);
            }
            assert!(
                app.language.implementations.context.is_none(),
                "phase {phase}, input {input}"
            );
            assert!(commands.try_recv().is_err(), "phase {phase}, input {input}");
        }
    }
}

#[test]
fn actual_frame_ready_navigation_yields_to_cancel_tools_shortcuts_and_modal() {
    for phase in [1, 2] {
        for input in 0..5 {
            let (mut app, commands) = app();
            if phase == 1 {
                app.documents.push(Document::new(
                    2,
                    "Target.java".into(),
                    TARGET.into(),
                    "t0".into(),
                ));
                app.next_document = 3;
                acknowledge(&mut app, 2, 2);
            }
            if input == 4 {
                app.documents[0].saved_text = "old source".into();
            }
            frame(&mut app, 0.0, vec![]);
            set_cursor(&mut app, marker(SOURCE, "Source").start);
            query(&mut app, &commands, rows());
            let mut command = select(&mut app, &commands);
            let mut value = Payload::Language {
                value: json!({"path":"Target.java"}),
            };
            if phase == 2 {
                reply(&mut app, &command, Ok(value));
                command = commands.try_recv().unwrap();
                value = Payload::File {
                    path: "Target.java".into(),
                    text: TARGET.into(),
                    revision: "t0".into(),
                };
            }
            let output = frame(&mut app, 0.1, vec![]);
            let events = match input {
                0 => key(egui::Key::J, egui::Modifiers::COMMAND),
                1 => key(
                    egui::Key::E,
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                ),
                2 | 3 => {
                    let at = if input == 2 {
                        label(&output, "Cancel").expect("visible Cancel")
                    } else {
                        crate::workspace_access_tests::recorded_rect(&app, "close_tools").center()
                    };
                    frame(
                        &mut app,
                        0.11,
                        vec![
                            egui::Event::PointerMoved(at),
                            egui::Event::PointerButton {
                                pos: at,
                                button: egui::PointerButton::Primary,
                                pressed: true,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    );
                    vec![
                        egui::Event::PointerMoved(at),
                        egui::Event::PointerButton {
                            pos: at,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                }
                4 => key(egui::Key::W, egui::Modifiers::COMMAND),
                _ => unreachable!(),
            };
            app.result_tx
                .send(WorkerEvent::Response(event(&app, &command, Ok(value))))
                .unwrap();
            frame(&mut app, 0.2, events);
            if input == 3 {
                let close = crate::workspace_access_tests::recorded_response(&app, "close_tools");
                assert!(
                    close.clicked_by(egui::PointerButton::Primary),
                    "the intended close activation must be recognized"
                );
                assert!(
                    !app.tools_open,
                    "a captured moving-panel close must actually hide tools"
                );
                assert_eq!(
                    app.editor_ctx.memory(|memory| memory.focused()),
                    Some(egui::Id::new(("editor", 1u64)))
                );
            }
            assert_eq!(app.active_document, Some(1), "phase {phase}, input {input}");
            assert_eq!(app.documents.len(), if phase == 1 { 2 } else { 1 });
            assert!(commands.try_recv().is_err(), "phase {phase}, input {input}");
            assert!(
                app.language.implementations.context.is_none(),
                "phase {phase}, input {input}"
            );
            app.tools_open = true;
            app.language.view = View::JavaImplementations;
            frame(&mut app, 0.3, vec![]);
            assert_eq!(app.active_document, Some(1));
            assert!(commands.try_recv().is_err());
        }
    }
}

impl CedarApp {
    /// Real native-agent response data fed through actual UI request/reply and
    /// explicit row selection handlers. No server/workspace write is simulated.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn java_implementations_navigation_acceptance(
        query_path: &str,
        query_text: &str,
        cursor: Position,
        implementations: Value,
        target_uri: &str,
        path: &str,
        disk_text: &str,
        revision: &str,
    ) -> Result<(), String> {
        let parsed =
            crate::language_navigation_results::parse_java_implementations(&implementations)?;
        let index = parsed
            .iter()
            .position(|row| row.uri == target_uri)
            .ok_or("Native target missing from implementation rows")?;
        let range = parsed[index].range;
        completion::position_to_offsets(disk_text, range.start)?;
        completion::position_to_offsets(disk_text, range.end)?;
        let (mut app, commands) = app();
        app.documents[0] =
            Document::new(1, query_path.into(), query_text.into(), "source-r0".into());
        acknowledge(&mut app, 1, 1);
        completion::position_to_offsets(query_text, cursor)?;
        set_cursor(&mut app, cursor);
        app.request_java_implementations();
        app.language_feature_tick();
        let query = commands.try_recv().map_err(|e| e.to_string())?;
        if !matches!(&query.op, Operation::LanguageJavaImplementations { path,version:1,line,character } if path == query_path && *line == cursor.line && *character == cursor.character)
        {
            return Err("Frontend implementation query lost exact version/cursor".into());
        }
        reply(
            &mut app,
            &query,
            Ok(Payload::Language {
                value: implementations.clone(),
            }),
        );
        app.select_java_implementation(index);
        let resolve = commands.try_recv().map_err(|e| e.to_string())?;
        if !matches!(&resolve.op, Operation::LanguageResolveUri { uri } if uri == target_uri) {
            return Err("Frontend bypassed authoritative URI resolution".into());
        }
        reply(
            &mut app,
            &resolve,
            Ok(Payload::Language {
                value: json!({"path":path}),
            }),
        );
        let read = commands.try_recv().map_err(|e| e.to_string())?;
        if !matches!(&read.op, Operation::Read { path:requested } if requested == path) {
            return Err("Frontend bypassed ordinary Read".into());
        }
        if !matches!(app.pending.get(&read.id), Some(Job::JavaImplementationOpen { context, .. }) if app.java_implementation_context_current(context))
        {
            return Err("Read lost the retained implementation request context".into());
        }
        reply(
            &mut app,
            &read,
            Ok(Payload::File {
                path: path.into(),
                text: disk_text.into(),
                revision: revision.into(),
            }),
        );
        frame(&mut app, 0.0, vec![]);
        frame(&mut app, 0.1, vec![]);
        let doc = app.active().ok_or("Implementation target did not open")?;
        if doc.path != path || doc.text != disk_text || doc.revision.as_deref() != Some(revision) {
            return Err("Implementation opened data differs from native Read".into());
        }
        selection(&app, range)?;
        let id = doc.id;
        let dirty = format!("{disk_text}\n// retained unsaved implementation witness\n");
        let target = app
            .documents
            .iter_mut()
            .find(|doc| doc.id == id)
            .ok_or("Missing target")?;
        crate::editor_state::commit(
            &app.editor_ctx,
            target,
            dirty.clone(),
            dirty.chars().count(),
        );
        let edit_version = target.edit_version;
        target.jump_to = Some(0);
        app.navigation_changed();
        app.active_document = Some(1);
        set_cursor(&mut app, cursor);
        acknowledge(&mut app, id, 2);
        // A fresh query captures the existing dirty target as a synchronized
        // participant. The original result cannot be reused after an edit.
        app.request_java_implementations();
        app.language_feature_tick();
        let query = commands.try_recv().map_err(|e| e.to_string())?;
        if !matches!(&query.op, Operation::LanguageJavaImplementations { path,version:1,line,character } if path == query_path && *line == cursor.line && *character == cursor.character)
        {
            return Err("Dirty target query skipped exact snapshot preparation".into());
        }
        reply(
            &mut app,
            &query,
            Ok(Payload::Language {
                value: implementations.clone(),
            }),
        );
        app.select_java_implementation(index);
        let resolve = commands.try_recv().map_err(|e| e.to_string())?;
        if !matches!(&resolve.op, Operation::LanguageResolveUri { uri } if uri == target_uri) {
            return Err("Dirty implementation target bypassed URI resolution".into());
        }
        reply(
            &mut app,
            &resolve,
            Ok(Payload::Language {
                value: json!({"path":path}),
            }),
        );
        if commands.try_recv().is_ok() {
            return Err("Dirty implementation target issued a Read or mutation".into());
        }
        frame(&mut app, 0.2, vec![]);
        frame(&mut app, 0.3, vec![]);
        let doc = app.active().ok_or("Dirty implementation not active")?;
        if app.documents.len() != 2
            || doc.id != id
            || doc.text != dirty
            || doc.saved_text != disk_text
            || doc.revision.as_deref() != Some(revision)
            || doc.edit_version != edit_version
            || !doc.dirty()
        {
            return Err("Implementation navigation altered the dirty target".into());
        }
        selection(&app, range)?;
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", id))));
        frame(&mut app, 1.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
        let doc = app.active().ok_or("Undo lost implementation document")?;
        if doc.text != disk_text
            || doc.saved_text != disk_text
            || doc.dirty()
            || doc.revision.as_deref() != Some(revision)
        {
            return Err("Undo did not retain implementation navigation history".into());
        }
        frame(&mut app, 2.0, key(egui::Key::Y, egui::Modifiers::COMMAND));
        let doc = app.active().ok_or("Redo lost implementation document")?;
        if doc.text != dirty
            || doc.saved_text != disk_text
            || !doc.dirty()
            || doc.revision.as_deref() != Some(revision)
        {
            return Err("Redo did not restore implementation dirty target".into());
        }
        if commands.try_recv().is_ok() {
            return Err("Implementation Undo/Redo sent an unexpected operation".into());
        }
        // The same externally verified result cannot revive a cancelled query.
        app.navigation_changed();
        app.active_document = Some(1);
        set_cursor(&mut app, cursor);
        acknowledge(&mut app, id, 3);
        app.request_java_implementations();
        app.language_feature_tick();
        let cancelled = commands.try_recv().map_err(|e| e.to_string())?;
        app.cancel_java_implementations();
        reply(
            &mut app,
            &cancelled,
            Ok(Payload::Language {
                value: implementations,
            }),
        );
        if app.language.implementations.context.is_some()
            || commands.try_recv().is_ok()
            || app.active_document != Some(1)
        {
            return Err("Cancelled implementation context was resurrected".into());
        }
        Ok(())
    }
}

#[test]
fn native_acceptance_helper_earns_navigation_selection_dirty_undo_and_lifetime_flags() {
    CedarApp::java_implementations_navigation_acceptance(
        "Source.java",
        SOURCE,
        marker(SOURCE, "Source").start,
        rows(),
        URI,
        "Target.java",
        TARGET,
        "t0",
    )
    .unwrap();
}

#[test]
fn target_appearing_dirty_while_read_is_pending_preserves_text_history_and_source_focus() {
    let (mut app, commands) = app();
    query(&mut app, &commands, rows());
    let resolve = select(&mut app, &commands);
    reply(
        &mut app,
        &resolve,
        Ok(Payload::Language {
            value: json!({"path":"Target.java"}),
        }),
    );
    let read = commands.try_recv().unwrap();
    assert!(matches!(read.op, Operation::Read { .. }));
    let mut target = Document::new(2, "Target.java".into(), TARGET.into(), "t0".into());
    let dirty = format!("{TARGET}// recovered draft\n");
    crate::editor_state::commit(
        &app.editor_ctx,
        &mut target,
        dirty.clone(),
        dirty.chars().count(),
    );
    let version = target.edit_version;
    app.documents.push(target);
    app.next_document = 3;
    reply(
        &mut app,
        &read,
        Ok(Payload::File {
            path: "Target.java".into(),
            text: TARGET.into(),
            revision: "obsolete".into(),
        }),
    );
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.documents[1].text, dirty);
    assert_eq!(app.documents[1].saved_text, TARGET);
    assert_eq!(app.documents[1].revision.as_deref(), Some("t0"));
    assert_eq!(app.documents[1].edit_version, version);
    let state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 2u64))).unwrap();
    let after = (state.cursor.char_range().unwrap(), dirty);
    assert_eq!(state.undoer().undo(&after).unwrap().1, TARGET);
    assert!(commands.try_recv().is_err());
}

#[test]
fn invalid_target_ranges_never_focus_existing_drafts_or_open_new_tabs() {
    for existing in [false, true] {
        for malformed in 0..3 {
            let (mut app, commands) = app();
            let mut row = location(URI);
            match malformed {
                0 => {
                    row["range"] =
                        json!({"start":{"line":0,"character":3},"end":{"line":0,"character":4}})
                } // middle of emoji's surrogate pair
                1 => {
                    row["range"] =
                        json!({"start":{"line":99,"character":0},"end":{"line":99,"character":1}})
                }
                2 => {
                    row["range"] = json!({"start":{"line":0,"character":999},"end":{"line":0,"character":1000}})
                }
                _ => unreachable!(),
            }
            if existing {
                app.documents.push(Document::new(
                    2,
                    "Target.java".into(),
                    TARGET.into(),
                    "t0".into(),
                ));
                acknowledge(&mut app, 2, 2);
                app.next_document = 3;
            }
            query(&mut app, &commands, json!([row]));
            let resolve = select(&mut app, &commands);
            reply(
                &mut app,
                &resolve,
                Ok(Payload::Language {
                    value: json!({"path":"Target.java"}),
                }),
            );
            if !existing {
                let read = commands.try_recv().unwrap();
                reply(
                    &mut app,
                    &read,
                    Ok(Payload::File {
                        path: "Target.java".into(),
                        text: TARGET.into(),
                        revision: "t0".into(),
                    }),
                );
            }
            assert_eq!(app.active_document, Some(1));
            assert_eq!(app.documents.len(), if existing { 2 } else { 1 });
            assert!(app
                .error
                .as_ref()
                .is_some_and(|error| error.contains("range")));
            assert!(!app.language.implementations.navigating);
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn wrong_read_path_and_resolver_failure_cannot_open_or_replace_documents() {
    for read_wrong_path in [false, true] {
        let (mut app, commands) = app();
        query(&mut app, &commands, rows());
        let resolve = select(&mut app, &commands);
        if read_wrong_path {
            reply(
                &mut app,
                &resolve,
                Ok(Payload::Language {
                    value: json!({"path":"Target.java"}),
                }),
            );
            let read = commands.try_recv().unwrap();
            reply(
                &mut app,
                &read,
                Ok(Payload::File {
                    path: "Different.java".into(),
                    text: TARGET.into(),
                    revision: "bad".into(),
                }),
            );
        } else {
            reply(
                &mut app,
                &resolve,
                Err("confined resolver rejected an outside-root target".into()),
            );
        }
        assert_eq!(app.active_document, Some(1));
        assert_eq!(app.documents.len(), 1);
        assert!(!app.language.implementations.pending && !app.language.implementations.navigating);
        assert!(app.error.is_some());
        assert!(commands.try_recv().is_err());
    }
}

fn source_cursor_point(output: &egui::FullOutput, chars: usize) -> egui::Pos2 {
    fn find(shape: &egui::epaint::Shape, chars: usize) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text == SOURCE => {
                let cursor = text.galley.from_ccursor(egui::text::CCursor::new(chars));
                Some(text.pos + text.galley.pos_from_cursor(&cursor).center().to_vec2())
            }
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, chars)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, chars))
        .expect("rendered source cursor")
}

#[test]
fn newer_focus_without_source_changes_irrevocably_cancels_selected_navigation() {
    for phase in [1, 2] {
        for reply_later in [false, true] {
            for pointer in [false, true] {
                let (mut app, commands) = app();
                if phase == 1 {
                    app.documents.push(Document::new(
                        2,
                        "Target.java".into(),
                        TARGET.into(),
                        "t0".into(),
                    ));
                    acknowledge(&mut app, 2, 2);
                    app.next_document = 3;
                }
                frame(&mut app, 0.0, vec![]);
                set_cursor(&mut app, marker(SOURCE, "Source").start);
                app.editor_ctx
                    .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
                query(&mut app, &commands, rows());
                let mut command = select(&mut app, &commands);
                let mut payload = Payload::Language {
                    value: json!({"path":"Target.java"}),
                };
                if phase == 2 {
                    reply(&mut app, &command, Ok(payload));
                    command = commands.try_recv().unwrap();
                    payload = Payload::File {
                        path: "Target.java".into(),
                        text: TARGET.into(),
                        revision: "t0".into(),
                    };
                }
                for time in [0.1, 0.2, 0.3] {
                    frame(&mut app, time, vec![]);
                }
                let output = frame(&mut app, 0.4, vec![]);
                let cursor = app.documents[0].cursor;
                let edit_version = app.documents[0].edit_version;
                let events = if pointer {
                    let chars =
                        completion::position_to_offsets(SOURCE, marker(SOURCE, "Source").start)
                            .unwrap()
                            .1;
                    let at = source_cursor_point(&output, chars);
                    vec![
                        egui::Event::PointerMoved(at),
                        egui::Event::PointerButton {
                            pos: at,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                        egui::Event::PointerButton {
                            pos: at,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                } else {
                    key(egui::Key::F, egui::Modifiers::COMMAND)
                };
                if !reply_later {
                    app.result_tx
                        .send(WorkerEvent::Response(event(
                            &app,
                            &command,
                            Ok(payload.clone()),
                        )))
                        .unwrap();
                }
                frame(&mut app, 0.5, events);
                assert_eq!(app.documents[0].text, SOURCE);
                assert_eq!(
                    app.documents[0].cursor, cursor,
                    "focus alone must not change source cursor"
                );
                assert_eq!(app.documents[0].edit_version, edit_version);
                assert_eq!(app.active_document, Some(1));
                assert!(app.language.implementations.context.is_none());
                if !pointer {
                    assert!(app.find_open);
                    assert_ne!(
                        app.editor_ctx.memory(|memory| memory.focused()),
                        Some(egui::Id::new(("editor", 1u64)))
                    );
                }
                if reply_later {
                    app.result_tx
                        .send(WorkerEvent::Response(event(&app, &command, Ok(payload))))
                        .unwrap();
                    frame(&mut app, 0.6, vec![]);
                }
                assert_eq!(app.active_document, Some(1));
                assert_eq!(app.documents.len(), if phase == 1 { 2 } else { 1 });
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn actual_control_dispatch_and_disabled_hover_guidance_are_explicit() {
    for gate in 0..4 {
        let (mut app, commands) = app();
        let reason = match gate {
            0 => None,
            1 => {
                app.active_form.as_mut().unwrap().allow_run = false;
                Some("Go to Implementations requires trusted tool permission for this connection")
            }
            2 => {
                app.language.capabilities = json!({"implementationProvider":false});
                Some("The running Java server does not advertise implementation locations")
            }
            3 => {
                app.agent_info
                    .as_mut()
                    .unwrap()
                    .capabilities
                    .retain(|name| name != "language_java_implementations");
                Some("The workspace agent does not advertise language_java_implementations. Drafts remain editable; upgrade or use an agent with this capability")
            }
            _ => unreachable!(),
        };
        app.language.view = View::Problems;
        for time in [0.0, 0.1, 0.2] {
            frame(&mut app, time, vec![]);
        }
        let output = frame(&mut app, 0.3, vec![]);
        let at = label(&output, "Go to Implementations").expect("implementation control visible");
        frame(&mut app, 0.4, vec![egui::Event::PointerMoved(at)]);
        frame(&mut app, 1.5, vec![]);
        let hover = frame(&mut app, 2.0, vec![]);
        if let Some(reason) = reason {
            assert!(label(&hover, reason).is_some(), "gate {gate}");
        }
        assert!(commands.try_recv().is_err());
        frame(
            &mut app,
            2.1,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        if gate == 0 {
            assert!(matches!(
                commands.try_recv().unwrap().op,
                Operation::LanguageJavaImplementations { .. }
            ));
        } else {
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn actual_row_activation_survives_its_own_click_but_not_mixed_newer_input() {
    for mixed in [false, true] {
        let (mut app, commands) = app();
        query(&mut app, &commands, rows());
        for time in [0.0, 0.1, 0.2] {
            frame(&mut app, time, vec![]);
        }
        let output = frame(&mut app, 0.3, vec![]);
        let range = marker(TARGET, "Target");
        let at = label(
            &output,
            &format!(
                "{}:{}:{}",
                URI,
                range.start.line + 1,
                range.start.character + 1
            ),
        )
        .expect("visible implementation row");
        let mut events = vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        if mixed {
            events.push(egui::Event::Text("newer field input".into()));
        }
        frame(&mut app, 0.4, events);
        let resolve = commands
            .try_recv()
            .expect("the native row click must activate the resolver");
        assert!(matches!(&resolve.op,Operation::LanguageResolveUri {uri} if uri==URI));
        assert_eq!(app.language.implementations.navigating, !mixed);
        assert_eq!(app.active_document, Some(1));
        // The next idle reply is accepted only for the isolated activation.
        app.result_tx
            .send(WorkerEvent::Response(event(
                &app,
                &resolve,
                Ok(Payload::Language {
                    value: json!({"path":"Target.java"}),
                }),
            )))
            .unwrap();
        frame(&mut app, 0.5, vec![]);
        if mixed {
            assert!(commands.try_recv().is_err());
            assert!(app.language.implementations.context.is_none());
        } else {
            let read = commands
                .try_recv()
                .expect("isolated row activation must complete ordinary Read");
            assert!(matches!(&read.op,Operation::Read{path} if path=="Target.java"));
            app.result_tx
                .send(WorkerEvent::Response(event(
                    &app,
                    &read,
                    Ok(Payload::File {
                        path: "Target.java".into(),
                        text: TARGET.into(),
                        revision: "t0".into(),
                    }),
                )))
                .unwrap();
            frame(&mut app, 0.6, vec![]);
            assert_eq!(app.active_document, Some(2));
            selection(&app, range).unwrap();
        }
    }
}

#[test]
fn implementation_location_history_admits_once_after_guarded_final_selection() {
    let (mut app, commands) = app();
    query(&mut app, &commands, rows());
    assert!(app.location_history.back.is_empty());
    let resolve = select(&mut app, &commands);
    assert!(app.location_history.back.is_empty());
    reply(
        &mut app,
        &resolve,
        Ok(Payload::Language {
            value: json!({"path":"Target.java"}),
        }),
    );
    assert!(app.location_history.back.is_empty());
    let read = commands.try_recv().unwrap();
    reply(
        &mut app,
        &read,
        Ok(Payload::File {
            path: "Target.java".into(),
            text: TARGET.into(),
            revision: "r".into(),
        }),
    );
    assert_eq!(app.location_history.back.len(), 1);
    assert_eq!(app.location_history.back[0].document, 1);
    assert!(app.location_history.pending.is_none());
    selection(&app, marker(TARGET, "Target")).unwrap();
}
