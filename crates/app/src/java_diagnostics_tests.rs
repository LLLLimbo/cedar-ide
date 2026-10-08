//! Deterministic refresh dispatch, snapshot lifetime, and rendered freshness checks.
use super::*;
use crate::worker::{Command, Worker};
use serde_json::json;
use std::sync::mpsc::Receiver;

const URI: &str = "file:///workspace/Main.java";
const SOURCE: &str = "class Main {}\n";

fn app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    app.state = crate::ConnectionState::Ready;
    app.active_form = Some(crate::ConnectForm {
        allow_run: true,
        ..Default::default()
    });
    let mut info = crate::agent_support::full_test_agent();
    info.capabilities.extend([
        "language_start_java".into(),
        "java_diagnostics_refresh".into(),
    ]);
    app.agent_info = Some(info);
    app.language.mode = ServerMode::Java;
    app.apply_language_action(
        Action {
            session: 0,
            kind: ActionKind::Start,
        },
        json!({"initialize":{"capabilities":{},"cedar_java_diagnostics_refresh":true}}),
    );
    app.language.automatic = false;
    app.documents.push(Document::new(
        1,
        "Main.java".into(),
        SOURCE.into(),
        "disk-revision".into(),
    ));
    app.active_document = Some(1);
    acknowledge(&mut app, 7);
    let (worker, rx) = Worker::recording();
    app.worker = Some(worker);
    (app, rx)
}

fn acknowledge(app: &mut CedarApp, version: i32) {
    app.language.sync.acknowledge(
        1,
        Acknowledged {
            version,
            edit_version: app.documents[0].edit_version,
            uri: URI.into(),
        },
    );
}

fn dispatch(app: &mut CedarApp, rx: &Receiver<Command>) -> Action {
    app.request_java_diagnostics_refresh();
    let command = rx.try_recv().expect("refresh was sent");
    assert!(
        matches!(command.op, Operation::LanguageRefreshJavaDiagnostics { ref path, version: 7 } if path == "Main.java")
    );
    let Job::Language(action) = app.pending.remove(&command.id).unwrap() else {
        panic!("not a language job");
    };
    action
}

fn response() -> Value {
    json!({"diagnostics_refresh_requested":URI,"version":7,"notification_only":true})
}

fn batch(app: &mut CedarApp, version: Option<i32>, count: usize) {
    app.apply_language_events(&json!({"events":[{"type":"diagnostics","value":{"uri":URI,"version":version,"diagnostics":(0..count).map(|_| json!({"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"message":"problem","severity":1})).collect::<Vec<_>>()}}],"truncated":false}));
}

#[test]
fn java_refresh_ack_and_silent_events_never_establish_freshness_or_resend() {
    let (mut app, rx) = app();
    let before = app.active_diagnostics_status().message;
    let action = dispatch(&mut app, &rx);
    assert!(!app.active_diagnostics_status().current);
    app.apply_language_action(action, response());
    assert_eq!(app.notice, "Java diagnostic refresh request sent");
    assert_eq!(app.active_diagnostics_status().message, before);
    assert_eq!(app.language.diagnostics.files.len(), 0);
    for time in [1.0, 100.0, 10_000.0] {
        app.apply_language_events(&json!({"events":[],"truncated":false}));
        let ctx = egui::Context::default();
        let _ = ctx.run(
            egui::RawInput {
                time: Some(time),
                ..Default::default()
            },
            |ctx| app.language_tick(ctx),
        );
        assert!(
            rx.try_recv().is_err(),
            "an explicit refresh must not poll, sync, or retry automatically"
        );
        assert!(!app.active_diagnostics_status().current);
    }
    assert!(
        app.language
            .diagnostic_refresh
            .as_ref()
            .unwrap()
            .acknowledged
    );
    assert_eq!(app.documents[0].text, SOURCE);
    assert_eq!(app.documents[0].saved_text, SOURCE);
    assert_eq!(app.documents[0].revision.as_deref(), Some("disk-revision"));
    assert!(!app.documents[0].undo_initialized);
}

#[test]
fn java_refresh_unversioned_empty_and_missing_batches_stay_unverified() {
    let (mut app, rx) = app();
    let action = dispatch(&mut app, &rx);
    app.apply_language_action(action, response());
    batch(&mut app, Some(6), 0);
    assert!(app.active_diagnostics_status().message.contains("pending"));
    batch(&mut app, None, 1);
    assert!(!app.active_diagnostics_status().current);
    assert!(app
        .active_diagnostics_status()
        .message
        .contains("unversioned"));
    assert!(
        app.language
            .diagnostic_refresh
            .as_ref()
            .unwrap()
            .observation
            == Observation::Unversioned
    );
    batch(&mut app, None, 0);
    assert_eq!(app.language.diagnostics.len(), 0);
    assert!(!app.active_diagnostics_status().current);
    assert!(app
        .active_diagnostics_status()
        .message
        .contains("0 reported problems"));
    app.apply_language_events(&json!({"events":[],"truncated":false}));
    assert!(!app.active_diagnostics_status().current);
}

#[test]
fn versioned_empty_batch_is_current_only_for_its_exact_synchronized_draft() {
    let (mut app, _) = app();
    batch(&mut app, Some(7), 2);
    assert!(app.active_diagnostics_status().current);
    batch(&mut app, Some(7), 0);
    assert!(app.active_diagnostics_status().current);
    assert!(app
        .active_diagnostics_status()
        .message
        .contains("0 reported problems"));
    crate::editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        "class Newer {}\n".into(),
        0,
    );
    assert!(!app.active_diagnostics_status().current);
    assert!(app.active_diagnostics_status().message.contains("stale"));
    assert!(app.active_diagnostics_status().message.contains("Sync now"));
    acknowledge(&mut app, 8);
    assert!(!app.active_diagnostics_status().current);
    batch(&mut app, Some(7), 0);
    assert!(!app.active_diagnostics_status().current);
    batch(&mut app, Some(8), 0);
    assert!(app.active_diagnostics_status().current);
    batch(&mut app, None, 0);
    assert!(
        !app.active_diagnostics_status().current,
        "a new unversioned batch cannot inherit an old batch's version"
    );
}

#[test]
fn java_refresh_rejects_unsynced_unsupported_generic_and_untrusted_without_syncing() {
    for blocker in [
        "unsynced",
        "unopened",
        "capability",
        "initialization",
        "generic",
        "trust",
        "wrong-language",
        "closed",
        "busy",
        "path",
        "version",
    ] {
        let (mut app, rx) = app();
        match blocker {
            "unsynced" => app.documents[0].edit_version += 1,
            "unopened" => app.language.sync.clear(),
            "capability" => app
                .agent_info
                .as_mut()
                .unwrap()
                .capabilities
                .retain(|name| name != "java_diagnostics_refresh"),
            "initialization" => app.language.java_diagnostics_refresh_supported = false,
            "generic" => app.language.mode = ServerMode::Generic,
            "trust" => app.active_form.as_mut().unwrap().allow_run = false,
            "wrong-language" => app.documents[0].path = "Other.rs".into(),
            "closed" => app.language.diagnostics_exited = true,
            "busy" => {
                app.pending.insert(
                    100,
                    Job::Language(Action {
                        session: app.language.session,
                        kind: ActionKind::Events,
                    }),
                );
            }
            "path" | "version" => {}
            _ => unreachable!(),
        }
        let operation = Operation::LanguageRefreshJavaDiagnostics {
            path: if blocker == "path" {
                "Other.java"
            } else {
                "Main.java"
            }
            .into(),
            version: if blocker == "version" { 8 } else { 7 },
        };
        assert!(
            app.operation_problem(&operation).is_some(),
            "central dispatch must enforce {blocker}"
        );
        if !matches!(blocker, "path" | "version") {
            app.request_java_diagnostics_refresh();
            assert!(app.error.is_some(), "{blocker}");
        }
        assert!(rx.try_recv().is_err(), "{blocker}");
        assert!(app.language.diagnostic_refresh.is_none());
        assert_eq!(app.documents[0].text, SOURCE);
    }
}

#[test]
fn java_refresh_requires_agent_owned_initialization_boolean() {
    for marker in [
        Value::Null,
        json!(false),
        json!("true"),
        json!({}),
        json!(true),
    ] {
        for mode in [ServerMode::Java, ServerMode::Generic] {
            let (mut app, _) = app();
            app.language.mode = mode;
            app.apply_language_action(Action { session: app.language.session, kind: ActionKind::Start }, json!({"initialize":{"capabilities":{"cedar_java_diagnostics_refresh":true},"cedar_java_diagnostics_refresh":marker}}));
            assert_eq!(
                app.language.java_diagnostics_refresh_supported,
                mode == ServerMode::Java && marker == json!(true)
            );
        }
    }
}

#[test]
fn java_refresh_late_ack_is_inert_after_edit_path_resync_close_or_reconnect() {
    for change in [
        "typing",
        "path",
        "resync",
        "close-reopen",
        "session",
        "generation",
        "disconnect",
        "stop",
    ] {
        let (mut app, rx) = app();
        let action = dispatch(&mut app, &rx);
        match change {
            "typing" => crate::editor_state::commit(
                &app.editor_ctx,
                &mut app.documents[0],
                "new draft".into(),
                0,
            ),
            "path" => app.documents[0].path = "Other.java".into(),
            "resync" => acknowledge(&mut app, 8),
            "close-reopen" => {
                app.close_language_document(1);
                app.documents.clear();
                app.documents.push(Document::new(
                    2,
                    "Main.java".into(),
                    SOURCE.into(),
                    "disk-revision".into(),
                ));
                app.active_document = Some(2);
            }
            "session" => {
                app.language.reset();
                app.language.running = true;
            }
            "generation" => app.generation += 1,
            "disconnect" => app.disconnected("test disconnect".into()),
            "stop" => app.stop_language(),
            _ => unreachable!(),
        }
        let text = app.documents[0].text.clone();
        let version = app.documents[0].edit_version;
        app.notice = "newer status".into();
        app.language.output = "newer activity".into();
        app.apply_language_action(action, response());
        assert_eq!(app.notice, "newer status", "{change}");
        assert_eq!(app.language.output, "newer activity", "{change}");
        assert!(!app.active_diagnostics_status().current, "{change}");
        assert_eq!(app.documents[0].text, text);
        assert_eq!(app.documents[0].edit_version, version);
        app.invalidate_diagnostics_refresh();
        assert!(app.language.diagnostic_refresh.is_none(), "{change}");
    }
}

#[test]
fn java_refresh_malformed_ack_never_changes_diagnostics() {
    for response in [
        json!({}),
        json!({"diagnostics_refresh_requested":URI,"version":8,"notification_only":true}),
        json!({"diagnostics_refresh_requested":"file:///other","version":7,"notification_only":true}),
        json!({"diagnostics_refresh_requested":URI,"version":7,"notification_only":false}),
    ] {
        let (mut app, rx) = app();
        let action = dispatch(&mut app, &rx);
        app.apply_language_action(action, response);
        assert!(app
            .error
            .as_ref()
            .unwrap()
            .contains("could not be verified"));
        assert!(!app.active_diagnostics_status().current);
        assert!(app.language.diagnostic_refresh.is_none());
    }
}

#[test]
fn java_refresh_matching_batch_is_independent_of_notification_ack_order() {
    for batch_first in [false, true] {
        let (mut app, rx) = app();
        let action = dispatch(&mut app, &rx);
        if batch_first {
            batch(&mut app, Some(7), 0);
        }
        app.apply_language_action(action, response());
        if !batch_first {
            batch(&mut app, Some(7), 0);
        }
        assert!(app.active_diagnostics_status().current);
        assert!(
            app.language
                .diagnostic_refresh
                .as_ref()
                .unwrap()
                .observation
                == Observation::MatchingVersion
        );
        assert_eq!(app.notice, "Java diagnostic refresh request sent");
    }
}

#[test]
fn java_refresh_event_loss_and_server_exit_never_look_current() {
    for event in [
        json!({"events":[],"truncated":true}),
        json!({"events":[{"type":"lagged"}],"truncated":false}),
        json!({"events":[{"type":"closed"}],"truncated":false}),
    ] {
        let (mut app, _) = app();
        batch(&mut app, Some(7), 0);
        assert!(app.active_diagnostics_status().current);
        app.apply_language_events(&event);
        assert!(!app.active_diagnostics_status().current);
        assert!(app.language.diagnostics.incomplete);
    }
}

#[test]
fn java_refresh_worker_events_ignore_stale_errors_and_payloads_but_keep_transport_failures() {
    for change in ["typing", "session", "close"] {
        for connected in [false, true] {
            for outcome in ["error", "ack", "wrong-payload"] {
                let (mut app, rx) = app();
                app.request_java_diagnostics_refresh();
                let command = rx.try_recv().unwrap();
                match change {
                    "typing" => crate::editor_state::commit(
                        &app.editor_ctx,
                        &mut app.documents[0],
                        "newer draft".into(),
                        0,
                    ),
                    "session" => app.language.reset(),
                    "close" => app.close_language_document(1),
                    _ => unreachable!(),
                }
                let text = app.documents[0].text.clone();
                let edit_version = app.documents[0].edit_version;
                app.error = Some("newer error".into());
                app.notice = "newer status".into();
                app.language.output = "newer activity".into();
                app.apply_event(crate::worker::Event {
                    generation: app.generation,
                    id: command.id,
                    connected,
                    result: match outcome {
                        "error" => Err("late refresh failure".into()),
                        "ack" => Ok(crate::Payload::Language { value: response() }),
                        "wrong-payload" => Ok(crate::Payload::Entries { entries: vec![] }),
                        _ => unreachable!(),
                    },
                });
                if connected {
                    assert!(app.ready(), "{change}/{outcome}");
                    assert_eq!(
                        app.error.as_deref(),
                        Some("newer error"),
                        "{change}/{outcome}"
                    );
                    assert_eq!(app.notice, "newer status", "{change}/{outcome}");
                    assert_eq!(app.language.output, "newer activity", "{change}/{outcome}");
                } else {
                    assert!(!app.ready(), "{change}/{outcome}");
                    assert!(app.error.as_ref().unwrap().contains("connection closed"));
                    assert!(app.language.diagnostic_refresh.is_none());
                }
                assert!(!app.pending.contains_key(&command.id));
                assert_eq!(app.documents[0].text, text);
                assert_eq!(app.documents[0].edit_version, edit_version);
                assert_eq!(app.documents[0].saved_text, SOURCE);
            }
        }
    }
}

#[test]
fn java_refresh_current_error_or_wrong_payload_finishes_sending_without_changing_freshness() {
    for current_batch in [false, true] {
        for wrong_payload in [false, true] {
            let (mut app, rx) = app();
            if current_batch {
                batch(&mut app, Some(7), 0);
            }
            let before = app.active_diagnostics_status();
            app.request_java_diagnostics_refresh();
            let command = rx.try_recv().unwrap();
            app.apply_event(crate::worker::Event {
                generation: app.generation,
                id: command.id,
                connected: true,
                result: if wrong_payload {
                    Ok(crate::Payload::Entries { entries: vec![] })
                } else {
                    Err("refresh failed".into())
                },
            });
            assert!(app.ready());
            assert!(app.error.is_some());
            assert!(app.pending.is_empty());
            assert!(app.language.diagnostic_refresh.is_none());
            let after = app.active_diagnostics_status();
            assert_eq!(after.current, before.current);
            assert_eq!(after.message, before.message);
            assert_eq!(app.documents[0].text, SOURCE);
            assert_eq!(app.documents[0].saved_text, SOURCE);
        }
    }
}

#[test]
fn malformed_or_oversized_diagnostic_replacement_revokes_prior_current_empty_snapshot() {
    for malformed in [
        "missing-array",
        "bad-range",
        "bad-version",
        "too-many",
        "too-large-message",
        "missing-value",
        "missing-uri",
        "oversized-uri",
        "mistyped-uri",
    ] {
        let (mut app, rx) = app();
        let action = dispatch(&mut app, &rx);
        app.apply_language_action(action, response());
        batch(&mut app, Some(7), 0);
        assert!(app.active_diagnostics_status().current);
        assert!(
            app.language
                .diagnostic_refresh
                .as_ref()
                .unwrap()
                .observation
                == Observation::MatchingVersion
        );
        let diagnostic = json!({"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"message":"problem"});
        let value = match malformed {
            "missing-array" => json!({"uri":URI,"version":7}),
            "bad-range" => json!({"uri":URI,"version":7,"diagnostics":[{"message":"problem"}]}),
            "bad-version" => json!({"uri":URI,"version":"7","diagnostics":[]}),
            "too-many" => {
                json!({"uri":URI,"version":7,"diagnostics":vec![diagnostic; language_results::MAX_DIAGNOSTICS + 1]})
            }
            "too-large-message" => {
                json!({"uri":URI,"version":7,"diagnostics":[{"range":diagnostic["range"],"message":"x".repeat(16 * 1024 + 1)}]})
            }
            "missing-value" => Value::Null,
            "missing-uri" => json!({"version":7,"diagnostics":[]}),
            "oversized-uri" => {
                json!({"uri":"x".repeat(language_results::MAX_DIAGNOSTIC_URI_BYTES + 1),"version":7,"diagnostics":[]})
            }
            "mistyped-uri" => json!({"uri":42,"version":7,"diagnostics":[]}),
            _ => unreachable!(),
        };
        let event = if malformed == "missing-value" {
            json!({"type":"diagnostics"})
        } else {
            json!({"type":"diagnostics","value":value})
        };
        app.apply_language_events(&json!({"events":[event],"truncated":false}));
        assert!(!app.active_diagnostics_status().current, "{malformed}");
        assert!(
            app.active_diagnostics_status().message.contains("pending"),
            "{malformed}"
        );
        assert!(app.language.diagnostics.incomplete, "{malformed}");
        assert!(
            !app.language.diagnostics.files.contains_key(URI),
            "{malformed}"
        );
        assert!(app.language.diagnostic_refresh.is_none(), "{malformed}");
        assert_eq!(app.documents[0].text, SOURCE);
    }
}

fn publish_uri(app: &mut CedarApp, uri: &str, version: Option<i32>) {
    app.apply_language_events(&json!({"events":[{"type":"diagnostics","value":{"uri":uri,"version":version,"diagnostics":[]}}],"truncated":false}));
}

#[test]
fn known_java_uri_aliases_track_versioned_and_unversioned_publications_without_false_freshness() {
    for (os, canonical, actual) in [
        (
            "linux",
            "file:///workspace/%E9%9B%AA/Main.java",
            "file:/workspace/雪/Main.java",
        ),
        (
            "windows",
            "file:///C:/workspace%20%E9%9B%AA/Main.java",
            "file:/c:/workspace%20雪/Main.java",
        ),
    ] {
        for version in [None, Some(7)] {
            let (mut app, rx) = app();
            app.agent_info.as_mut().unwrap().os = os.into();
            app.language.sync.opened.get_mut(&1).unwrap().uri = canonical.into();
            let action = dispatch(&mut app, &rx);
            app.apply_language_action(action, json!({"diagnostics_refresh_requested":canonical,"version":7,"notification_only":true}));
            publish_uri(&mut app, actual, version);
            assert!(app.language.diagnostics.files.contains_key(canonical));
            assert!(!app.language.diagnostics.files.contains_key(actual));
            assert_eq!(app.active_diagnostics_status().current, version.is_some());
            let observation = app
                .language
                .diagnostic_refresh
                .as_ref()
                .unwrap()
                .observation;
            assert!(
                observation
                    == if version.is_some() {
                        Observation::MatchingVersion
                    } else {
                        Observation::Unversioned
                    }
            );
            assert_eq!(app.documents[0].text, SOURCE);
        }
    }
}

#[test]
fn known_java_uri_aliases_respect_closed_documents_and_reopened_versions() {
    let (mut app, _rx) = app();
    let actual = "file:/workspace/Main.java";
    publish_uri(&mut app, actual, Some(7));
    assert!(app.active_diagnostics_status().current);
    app.close_language_document(1);
    app.documents.clear();
    app.active_document = None;
    publish_uri(&mut app, actual, Some(7));
    assert!(app.language.diagnostics.files.is_empty());
    app.documents.push(Document::new(
        2,
        "Main.java".into(),
        SOURCE.into(),
        "disk-revision".into(),
    ));
    app.active_document = Some(2);
    app.apply_language_action(
        Action {
            session: app.language.session,
            kind: ActionKind::Sync {
                document: 2,
                version: 8,
                edit_version: 0,
            },
        },
        json!({"opened":URI,"version":8}),
    );
    publish_uri(&mut app, actual, Some(7));
    assert!(app.language.diagnostics.files.is_empty());
    publish_uri(&mut app, actual, Some(8));
    assert!(app.active_diagnostics_status().current);
}

#[test]
fn java_uri_alias_matching_does_not_adopt_unknown_generic_or_ambiguous_targets() {
    for scenario in ["generic", "unknown", "authority", "case", "ambiguous"] {
        let (mut app, _) = app();
        let canonical = "file:///workspace/%E9%9B%AA/Main.java";
        app.language.sync.opened.get_mut(&1).unwrap().uri = canonical.into();
        let actual = match scenario {
            "generic" => {
                app.language.mode = ServerMode::Generic;
                "file:/workspace/雪/Main.java"
            }
            "unknown" => "file:/workspace/other/Main.java",
            "authority" => "file://host/workspace/雪/Main.java",
            "case" => "file:/workspace/雪/main.java",
            "ambiguous" => {
                app.documents.push(Document::new(
                    2,
                    "Other.java".into(),
                    SOURCE.into(),
                    "r".into(),
                ));
                app.language.sync.acknowledge(
                    2,
                    Acknowledged {
                        version: 7,
                        edit_version: 0,
                        uri: "file:///workspace/雪/Main.java".into(),
                    },
                );
                "file:/workspace/雪/Main.java"
            }
            _ => unreachable!(),
        };
        publish_uri(&mut app, actual, Some(7));
        assert!(
            app.language.diagnostics.files.contains_key(actual),
            "{scenario}"
        );
        assert!(
            !app.language.diagnostics.files.contains_key(canonical),
            "{scenario}"
        );
        assert!(!app.active_diagnostics_status().current, "{scenario}");
    }
}

fn render(
    app: &mut CedarApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.language_panel(ui));
        },
    )
}

fn text_shapes(output: &egui::FullOutput) -> Vec<&egui::epaint::TextShape> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn headless_java_refresh_status_is_visible_for_pending_stale_and_empty_batches() {
    for size in [egui::vec2(780.0, 540.0), egui::vec2(1320.0, 880.0)] {
        for state in [
            "pending",
            "current",
            "stale",
            "unversioned",
            "unsupported",
            "awaiting",
        ] {
            let (mut app, rx) = app();
            match state {
                "pending" => {}
                "current" => batch(&mut app, Some(7), 0),
                "stale" => {
                    batch(&mut app, Some(7), 0);
                    app.documents[0].edit_version += 1;
                }
                "unversioned" => batch(&mut app, None, 0),
                "unsupported" => app.language.java_diagnostics_refresh_supported = false,
                "awaiting" => {
                    let action = dispatch(&mut app, &rx);
                    app.apply_language_action(action, response());
                }
                _ => unreachable!(),
            }
            let status = app.active_diagnostics_status();
            let ctx = egui::Context::default();
            let output = render(&mut app, &ctx, size, vec![]);
            let text = text_shapes(&output);
            let rendered = text
                .iter()
                .find(|text| text.galley.text() == status.message)
                .unwrap_or_else(|| panic!("missing status: {state}"));
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            assert!(
                screen.contains_rect(rendered.visual_bounding_rect()),
                "{state}: {:?}",
                rendered.visual_bounding_rect()
            );
            assert!(text
                .iter()
                .any(|text| text.galley.text() == "Refresh Java diagnostics"));
            if state == "stale" {
                assert!(status.message.contains("0 reported problems"));
            }
            if state == "unsupported" {
                assert!(text.iter().any(|text| text
                    .galley
                    .text()
                    .contains("supported Standard server version was not confirmed")));
            }
            if state == "awaiting" {
                assert!(text.iter().any(|text| text
                    .galley
                    .text()
                    .contains("Request sent; awaiting diagnostics")));
            }
        }
    }
}

#[test]
fn headless_refresh_button_dispatches_once_and_requires_sync_after_typing() {
    for synced in [false, true] {
        let (mut app, rx) = app();
        if !synced {
            app.documents[0].edit_version += 1;
        }
        let ctx = egui::Context::default();
        let size = egui::vec2(780.0, 540.0);
        let output = render(&mut app, &ctx, size, vec![]);
        let pos = text_shapes(&output)
            .iter()
            .find(|text| text.galley.text() == "Refresh Java diagnostics")
            .unwrap()
            .visual_bounding_rect()
            .center();
        let _ = render(
            &mut app,
            &ctx,
            size,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let _ = render(
            &mut app,
            &ctx,
            size,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        if synced {
            assert!(matches!(
                rx.try_recv().unwrap().op,
                Operation::LanguageRefreshJavaDiagnostics { version: 7, .. }
            ));
        }
        assert!(rx.try_recv().is_err());
        assert_eq!(app.documents[0].text, SOURCE);
        assert!(!app.documents[0].undo_initialized);
    }
}
