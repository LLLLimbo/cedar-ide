//! Headless transaction and lifetime tests for explicit language features.
use super::*;
use serde_json::json;
const BEFORE: &str = "fn main(){}\n";
const AFTER: &str = "fn main() {\n}\n";
fn app() -> CedarApp {
    let mut app = CedarApp::empty();
    app.state = crate::ConnectionState::Ready;
    app.agent_info = Some(crate::agent_support::full_test_agent());
    app.language.running = true;
    app.language.automatic = false;
    app.language.capabilities = json!({"documentFormattingProvider":true,"referencesProvider":true,"documentSymbolProvider":true});
    app.documents.push(Document::new(
        1,
        "main.rs".into(),
        BEFORE.into(),
        "r0".into(),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    acknowledge(&mut app, 1, 1);
    app
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
fn formatting() -> FeatureKind {
    FeatureKind::Format {
        tab_size: 4,
        insert_spaces: true,
    }
}
fn edits() -> Value {
    json!([{"range":{"start":{"line":0,"character":0},"end":{"line":1,"character":0}},"newText":AFTER}])
}
fn dispatch(app: &mut CedarApp, kind: FeatureKind) -> FeatureRequest {
    app.request_language_navigation_feature(kind);
    match app.next_language_feature_step().unwrap() {
        FeatureStep::Dispatch(request, _) => *request,
        FeatureStep::Sync(_) => panic!("test expected a synchronized source"),
    }
}
fn preview(app: &mut CedarApp) {
    let request = dispatch(app, formatting());
    app.apply_language_feature(request, edits());
    assert!(app.language.features.preview.is_some());
}
#[test]
fn formatting_is_one_undo_redo_with_unchanged_saved_baseline_and_disk() {
    let disk = tempfile::tempdir().unwrap();
    let file = disk.path().join("main.rs");
    std::fs::write(&file, BEFORE).unwrap();
    let mut app = app();
    preview(&mut app);
    assert_eq!(app.documents[0].text, BEFORE);
    app.apply_format_preview();
    let doc = &app.documents[0];
    assert_eq!(doc.text, AFTER);
    assert_eq!(doc.saved_text, BEFORE);
    assert_eq!(doc.revision.as_deref(), Some("r0"));
    assert_eq!(doc.edit_version, 1);
    assert!(doc.dirty());
    assert!(app.pending.is_empty());
    assert_eq!(std::fs::read_to_string(file).unwrap(), BEFORE);
    let state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64))).unwrap();
    let after = (state.cursor.char_range().unwrap(), doc.text.clone());
    let mut history = state.undoer();
    let before = history.undo(&after).unwrap().clone();
    assert_eq!(before.1, BEFORE);
    assert!(history.undo(&before).is_none());
    assert_eq!(history.redo(&before).unwrap().1, AFTER);
    app.apply_format_preview();
    assert_eq!(
        app.documents[0].edit_version, 1,
        "double Apply must be inert"
    );
}
#[test]
fn cursor_movement_preserves_format_preview_and_uses_latest_cursor() {
    let mut app = app();
    preview(&mut app);
    app.documents[0].cursor = (1, 3);
    app.invalidate_language_features();
    assert!(app.language.features.preview.is_some());
    app.apply_format_preview();
    assert_eq!(app.documents[0].cursor, (1, 3));
    assert_eq!(app.documents[0].text, AFTER);
}
#[test]
fn null_empty_and_identical_formatting_do_not_mutate_or_add_undo() {
    for value in [
        Value::Null,
        json!([]),
        json!([{"range":{"start":{"line":0,"character":0},"end":{"line":1,"character":0}},"newText":BEFORE}]),
    ] {
        let mut app = app();
        let request = dispatch(&mut app, formatting());
        app.apply_language_feature(request, value);
        assert!(app.language.features.preview.is_none());
        assert!(!app.documents[0].undo_initialized);
        assert_eq!(app.documents[0].edit_version, 0);
        assert_eq!(app.documents[0].text, BEFORE);
        assert!(!app.documents[0].dirty());
    }
}
#[test]
fn source_changes_tab_roundtrip_cancel_reconnect_restart_and_newer_requests_expire_preview() {
    for change in 0..9 {
        let mut app = app();
        preview(&mut app);
        match change {
            0 => {
                app.documents[0].text.push('x');
                app.documents[0].edit_version += 1;
            }
            1 => {
                app.navigation_changed();
                app.active_document = None;
                app.active_document = Some(1);
            }
            2 => {
                app.language.features.cancel_pending();
            }
            3 => {
                app.generation += 1;
            }
            4 => {
                app.language.reset();
            }
            5 => {
                app.request_language_navigation_feature(FeatureKind::Outline);
            }
            6 => {
                app.documents[0].id = 2;
                app.active_document = Some(2);
            }
            7 => {
                app.documents[0].path = "renamed.rs".into();
            }
            8 => {
                app.language.sync.opened.get_mut(&1).unwrap().version += 1;
            }
            _ => unreachable!(),
        }
        let before_apply = app.documents[0].text.clone();
        app.apply_format_preview();
        assert_eq!(app.documents[0].text, before_apply, "invalidator {change}");
        assert!(app.language.features.preview.is_none());
    }
}
#[test]
fn stale_format_responses_are_ignored_after_new_request_or_cancel_or_server_exit() {
    for change in 0..4 {
        let mut app = app();
        let request = dispatch(&mut app, formatting());
        match change {
            0 => {
                app.request_language_navigation_feature(formatting());
            }
            1 => {
                app.language.features.cancel_pending();
            }
            2 => {
                app.documents[0].edit_version += 1;
            }
            3 => {
                app.apply_language_events(&json!({"events":[{"type":"closed"}],"truncated":false}));
            }
            _ => unreachable!(),
        }
        app.apply_language_feature(request, edits());
        assert!(app.language.features.preview.is_none());
        assert_eq!(app.documents[0].text, BEFORE);
    }
}
#[test]
fn formatting_syncs_exact_draft_and_captures_lsp_version_after_ack() {
    let mut app = app();
    app.documents[0].edit_version = 3;
    app.documents[0].text.push_str("// newer\n");
    app.request_language_navigation_feature(formatting());
    assert!(matches!(
        app.next_language_feature_step(),
        Some(FeatureStep::Sync(1))
    ));
    acknowledge(&mut app, 1, 9);
    let Some(FeatureStep::Dispatch(request, operation)) = app.next_language_feature_step() else {
        panic!("expected dispatch");
    };
    assert_eq!(request.source.lsp_version, Some(9));
    assert!(request.source.text.ends_with("// newer\n"));
    assert!(matches!(
        operation,
        Operation::LanguageFormat {
            version: 9,
            tab_size: 4,
            insert_spaces: true,
            ..
        }
    ));
}
#[test]
fn reference_dispatch_waits_for_every_matching_draft() {
    let mut app = app();
    app.documents.push(Document::new(
        2,
        "other.rs".into(),
        "draft".into(),
        "r".into(),
    ));
    app.documents.push(Document::new(
        3,
        "notes.txt".into(),
        "not this server".into(),
        "r".into(),
    ));
    app.request_language_navigation_feature(FeatureKind::References {
        include_declaration: false,
    });
    assert!(matches!(
        app.next_language_feature_step(),
        Some(FeatureStep::Sync(2))
    ));
    acknowledge(&mut app, 2, 7);
    let Some(FeatureStep::Dispatch(request, op)) = app.next_language_feature_step() else {
        panic!("expected dispatch");
    };
    assert_eq!(request.participants.len(), 2);
    assert_eq!(request.participants[1].lsp_version, Some(7));
    assert!(matches!(
        op,
        Operation::LanguageReferences {
            include_declaration: false,
            ..
        }
    ));
    app.documents[1].text.push_str(" changed while pending");
    app.documents[1].edit_version += 1;
    app.apply_language_feature(*request, json!([]));
    assert!(!app.language.features.references_requested);
}
#[test]
fn references_reject_changed_open_set_and_reopened_document() {
    for change in 0..3 {
        let mut app = app();
        let request = dispatch(
            &mut app,
            FeatureKind::References {
                include_declaration: true,
            },
        );
        match change {
            0 => app.documents.push(Document::new(
                2,
                "second.rs".into(),
                "new".into(),
                "r".into(),
            )),
            1 => {
                app.documents[0].id = 2;
                app.active_document = Some(2);
            }
            2 => {
                app.documents[0].cursor = (1, 2);
            }
            _ => unreachable!(),
        }
        app.apply_language_feature(request, json!([]));
        assert!(!app.language.features.references_requested);
    }
}
#[test]
fn save_ack_during_preview_keeps_latest_baseline_and_newer_formatted_draft() {
    let mut app = app();
    preview(&mut app);
    app.documents[0].acknowledge_save("previously sent snapshot".into(), "r1".into());
    app.apply_format_preview();
    assert_eq!(app.documents[0].text, AFTER);
    assert_eq!(app.documents[0].saved_text, "previously sent snapshot");
    assert_eq!(app.documents[0].revision.as_deref(), Some("r1"));
    app.documents[0].acknowledge_save(BEFORE.into(), "r2".into());
    assert_eq!(app.documents[0].text, AFTER);
    assert_eq!(app.documents[0].saved_text, BEFORE);
    assert!(app.documents[0].dirty());
}
fn symbols() -> Value {
    json!([{"name":"main","kind":12,"range":{"start":{"line":0,"character":0},"end":{"line":1,"character":0}},"selectionRange":{"start":{"line":0,"character":3},"end":{"line":0,"character":7}}}])
}
#[test]
fn outline_ignores_cursor_but_expires_on_edit_and_never_repolls() {
    let mut app = app();
    let request = dispatch(&mut app, FeatureKind::Outline);
    app.documents[0].cursor = (1, 4);
    app.apply_language_feature(request, symbols());
    assert_eq!(
        app.language.features.outline.as_ref().unwrap().items.len(),
        1
    );
    app.documents[0].cursor = (1, 1);
    app.invalidate_language_features();
    assert!(app.language.features.outline.is_some());
    assert!(app.next_language_feature_step().is_none());
    app.documents[0].edit_version += 1;
    app.invalidate_language_features();
    assert!(app.language.features.outline.is_none());
    assert!(app.next_language_feature_step().is_none());
}
#[test]
fn agent_resolved_reference_preserves_dirty_target_and_validates_utf16_range() {
    for end in [2, 3] {
        let mut app = app();
        let mut target = Document::new(2, "other.rs".into(), "disk".into(), "r".into());
        target.text = "🐻dirty target".into();
        target.edit_version = 5;
        app.documents.push(target);
        let location = Location {
            uri: "file:///workspace/other.rs".into(),
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: end,
                },
            },
        };
        app.apply_language_action(
            Action {
                session: 0,
                kind: ActionKind::ResolveUri {
                    sequence: 0,
                    navigation: 0,
                    location,
                },
            },
            json!({"path":"other.rs"}),
        );
        assert_eq!(app.documents[1].text, "🐻dirty target");
        assert_eq!(app.documents[1].saved_text, "disk");
        assert_eq!(app.documents[1].edit_version, 5);
        assert!(app.documents[1].dirty());
        assert_eq!(app.active_document, Some(2));
    }
    let mut app = app();
    app.documents[0].text = "🐻".into();
    let location = Location {
        uri: "file:///workspace/main.rs".into(),
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 1,
            },
        },
    };
    app.apply_language_action(
        Action {
            session: 0,
            kind: ActionKind::ResolveUri {
                sequence: 0,
                navigation: 0,
                location,
            },
        },
        json!({"path":"main.rs"}),
    );
    assert!(app.error.as_deref().unwrap().contains("range"));
    assert_eq!(app.documents[0].text, "🐻");
}
#[test]
fn formatting_does_not_replace_an_unowned_older_recovery_copy() {
    use cedar_recovery::{record_id, Draft, Store, WorkspaceIdentity};
    let temp = tempfile::tempdir().unwrap();
    let storage = temp.path().join("recovery");
    let workspace = WorkspaceIdentity::Local {
        root: "/synthetic/project".into(),
    };
    let draft = Draft {
        workspace: workspace.clone(),
        path: "main.rs".into(),
        text: "older unsaved recovery".into(),
        base_text: "older disk".into(),
        base_revision: Some("old".into()),
        modified_ms: 1,
    };
    {
        Store::open(&storage).unwrap().write(1, &draft).unwrap();
    }
    let mut app = app();
    app.root = "/synthetic/project".into();
    app.active_form = Some(crate::ConnectForm {
        local_root: app.root.clone(),
        ..Default::default()
    });
    app.recovery.start(Ok(storage.clone()), &app.editor_ctx);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !app.recovery.initialized {
        app.recovery_tick(&egui::Context::default());
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    preview(&mut app);
    app.apply_format_preview();
    app.recovery_tick(&egui::Context::default());
    app.recovery.flush();
    assert_eq!(
        app.recovery.status(Some(&workspace), app.active()).0,
        "Older recovery waiting"
    );
    drop(app);
    let store = Store::open(storage).unwrap();
    assert_eq!(
        store
            .read(&record_id(&workspace, "main.rs").unwrap())
            .unwrap(),
        draft
    );
}
#[test]
fn headless_minimum_and_default_layout_cover_features_and_readonly_preview() {
    for size in [[780.0, 540.0], [1320.0, 880.0]] {
        for view in [View::Format, View::References, View::Outline] {
            let mut app = app();
            preview(&mut app);
            app.language.view = view;
            let ctx = egui::Context::default();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(size[0], size[1]),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.language_panel(ui);
                    });
                    app.format_preview_window(ctx);
                },
            );
            assert!(!output.shapes.is_empty());
            assert_eq!(app.documents[0].text, BEFORE);
            assert_eq!(
                app.language
                    .features
                    .preview
                    .as_ref()
                    .unwrap()
                    .request
                    .source
                    .text,
                BEFORE
            );
            assert_eq!(app.language.features.preview.as_ref().unwrap().after, AFTER);
        }
    }
}
#[test]
fn capability_absence_and_oversize_participant_never_dispatch() {
    let mut app = app();
    app.language.capabilities = json!({});
    app.request_language_navigation_feature(formatting());
    assert!(app.language.features.intent.is_none());
    app.language.capabilities = json!({"referencesProvider":true});
    app.documents.push(Document::new(
        2,
        "huge.rs".into(),
        "x".repeat(MAX_FILE_BYTES + 1),
        "r".into(),
    ));
    app.request_language_navigation_feature(FeatureKind::References {
        include_declaration: true,
    });
    assert!(app.language.features.intent.is_none());
    assert_eq!(symbol_kind(999), "symbol");
}

#[test]
fn newer_format_request_cancels_old_completion_and_rejects_late_popup() {
    let mut app = app();
    let context = QueryContext {
        session: 0,
        document: 1,
        edit_version: 0,
        source: BEFORE.into(),
        cursor: Position {
            line: 0,
            character: 0,
        },
    };
    app.language.completion_popup = true;
    let old_acceptance = app.language.acceptance_sequence;
    app.request_language_navigation_feature(formatting());
    assert!(!app.language.completion_popup);
    assert_ne!(app.language.acceptance_sequence, old_acceptance);
    app.apply_language_action(
        Action {
            session: 0,
            kind: ActionKind::Query {
                context,
                kind: LanguageQueryKind::Completion,
            },
        },
        json!([{"label":"late","insertText":"late"}]),
    );
    assert!(!app.language.completion_popup);
    assert!(app.language.features.intent.is_some());
}

#[test]
fn malformed_format_response_never_creates_preview_or_mutates() {
    let mut app = app();
    let request = dispatch(&mut app, formatting());
    app.apply_language_feature(request, json!([{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"newText":"safe"},{"range":{"start":{"line":999,"character":0},"end":{"line":999,"character":1}},"newText":"bad"}]));
    assert!(app.language.features.preview.is_none());
    assert!(app.error.is_some());
    assert_eq!(app.documents[0].text, BEFORE);
    assert_eq!(app.documents[0].edit_version, 0);
    assert!(!app.documents[0].undo_initialized);
}

#[test]
fn actual_app_update_escape_cancels_format_preview_before_global_shortcuts() {
    for size in [[780.0, 540.0], [1320.0, 880.0]] {
        let mut app = app();
        preview(&mut app);
        app.open_form = false;
        app.find_open = true;
        let sequence = app.language.features.sequence;
        let ctx = egui::Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        app.editor_ctx = ctx.clone();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(size[0], size[1]),
                )),
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut frame),
        );
        assert!(app.language.features.preview_open);
        assert!(app.language.features.preview.is_some());
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(size[0], size[1]),
                )),
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: Some(egui::Key::Escape),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut frame),
        );
        assert!(!output.shapes.is_empty());
        assert!(app.language.features.preview.is_none());
        assert!(!app.language.features.preview_open);
        assert_ne!(app.language.features.sequence, sequence);
        assert_eq!(app.notice, "Formatting cancelled; draft unchanged");
        assert!(
            app.find_open,
            "modal Escape must not also trigger the global close-find shortcut"
        );
        assert_eq!(app.documents[0].text, BEFORE);
        assert_eq!(app.documents[0].saved_text, BEFORE);
        assert_eq!(app.documents[0].edit_version, 0);
    }
}

fn local_outline(app: &mut CedarApp) -> OutlineLocation {
    let request = dispatch(app, FeatureKind::Outline);
    app.apply_language_feature(request, symbols());
    app.language.features.outline.as_ref().unwrap().items[0]
        .location
        .clone()
}
#[test]
fn local_outline_selection_supersedes_older_uri_resolution() {
    let mut app = app();
    app.documents.push(Document::new(
        2,
        "other.rs".into(),
        "untouched".into(),
        "r".into(),
    ));
    let local = local_outline(&mut app);
    let stale = Action {
        session: app.language.session,
        kind: ActionKind::ResolveUri {
            sequence: app.language.navigation_sequence,
            navigation: app.navigation_epoch,
            location: Location {
                uri: "file:///workspace/other.rs".into(),
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 1,
                    },
                },
            },
        },
    };
    app.navigate_outline_location(local);
    assert_eq!(app.documents[0].cursor, (1, 8));
    app.apply_language_action(stale, json!({"path":"other.rs"}));
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents[0].cursor, (1, 8));
    assert_eq!(app.documents[1].text, "untouched");
    assert!(app.pending.is_empty());
}
#[test]
fn local_outline_selection_supersedes_already_pending_file_open() {
    let mut app = app();
    let local = local_outline(&mut app);
    app.pending.insert(
        77,
        Job::Open {
            path: "other.rs".into(),
            line: None,
            navigation: app.navigation_epoch,
        },
    );
    app.language.deferred_navigation.insert(
        "other.rs".into(),
        DeferredNavigation {
            session: app.language.session,
            sequence: app.language.navigation_sequence,
            navigation: app.navigation_epoch,
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 1,
                },
            },
        },
    );
    app.navigate_outline_location(local);
    assert!(app.language.deferred_navigation.is_empty());
    app.apply_event(crate::worker::Event {
        generation: app.generation,
        id: 77,
        connected: true,
        result: Ok(cedar_protocol::Payload::File {
            path: "other.rs".into(),
            text: "late file response".into(),
            revision: "r".into(),
        }),
    });
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents[0].cursor, (1, 8));
    assert_eq!(app.documents[0].text, BEFORE);
}
#[test]
fn local_outline_cursor_navigation_keeps_format_preview_valid() {
    let mut app = app();
    let local = local_outline(&mut app);
    preview(&mut app);
    let sequence = app.language.features.sequence;
    app.navigate_outline_location(local);
    assert_eq!(app.language.features.sequence, sequence);
    app.invalidate_language_features();
    assert!(app.language.features.preview.is_some());
    app.apply_format_preview();
    assert_eq!(app.documents[0].text, AFTER);
    assert_eq!(app.documents[0].edit_version, 1);
}

fn editor_frame(app: &mut CedarApp, ctx: &egui::Context, time: f64, events: Vec<egui::Event>) {
    let mut frame = eframe::Frame::_new_kittest();
    let _ = ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1320.0, 880.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut frame),
    );
}
fn key_events(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
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
#[test]
fn real_frames_format_undo_after_cursor_outline_and_tab_navigation() {
    for navigation in ["outline", "cursor", "tab"] {
        let mut app = app();
        let ctx = egui::Context::default();
        app.editor_ctx = ctx.clone();
        app.open_form = false;
        app.documents.push(Document::new(
            2,
            "other.rs".into(),
            "other draft".into(),
            "r".into(),
        ));
        app.next_document = 3;
        editor_frame(&mut app, &ctx, 0.0, vec![]);
        preview(&mut app);
        app.apply_format_preview();
        editor_frame(&mut app, &ctx, 1.0, vec![]);
        editor_frame(
            &mut app,
            &ctx,
            1.1,
            key_events(egui::Key::Z, egui::Modifiers::COMMAND),
        );
        assert_eq!(app.documents[0].text, BEFORE, "initial undo, {navigation}");
        editor_frame(
            &mut app,
            &ctx,
            1.2,
            key_events(
                egui::Key::Z,
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            ),
        );
        assert_eq!(app.documents[0].text, AFTER, "initial redo, {navigation}");
        match navigation {
            "outline" => {
                acknowledge(&mut app, 1, 9);
                let local = local_outline(&mut app);
                app.navigate_outline_location(local);
                editor_frame(&mut app, &ctx, 1.3, vec![]);
            }
            "cursor" => editor_frame(
                &mut app,
                &ctx,
                1.3,
                key_events(egui::Key::ArrowRight, egui::Modifiers::NONE),
            ),
            _ => editor_frame(&mut app, &ctx, 1.3, vec![]),
        }
        editor_frame(&mut app, &ctx, 2.5, vec![]);
        let action = Action {
            session: app.language.session,
            kind: ActionKind::ResolveUri {
                sequence: app.language.navigation_sequence,
                navigation: app.navigation_epoch,
                location: Location {
                    uri: "file:///workspace/other.rs".into(),
                    range: Range {
                        start: Position {
                            line: 0,
                            character: 0,
                        },
                        end: Position {
                            line: 0,
                            character: 1,
                        },
                    },
                },
            },
        };
        app.apply_language_action(action, json!({"path":"other.rs"}));
        editor_frame(&mut app, &ctx, 2.6, vec![]);
        app.open("main.rs".into(), None);
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
        editor_frame(&mut app, &ctx, 2.7, vec![]);
        assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(("editor", 1u64)))));
        editor_frame(
            &mut app,
            &ctx,
            2.8,
            key_events(egui::Key::Z, egui::Modifiers::COMMAND),
        );
        assert_eq!(
            app.documents[0].text, BEFORE,
            "first undo after {navigation} must undo formatting"
        );
        assert_eq!(app.documents[0].saved_text, BEFORE);
        editor_frame(
            &mut app,
            &ctx,
            2.9,
            key_events(
                egui::Key::Z,
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            ),
        );
        assert_eq!(
            app.documents[0].text, AFTER,
            "first redo after {navigation} must restore formatting"
        );
        assert_eq!(app.documents[0].saved_text, BEFORE);
        editor_frame(&mut app, &ctx, 3.0, vec![egui::Event::Text("x".into())]);
        let typed = app.documents[0].text.clone();
        assert_ne!(typed, AFTER);
        editor_frame(
            &mut app,
            &ctx,
            3.1,
            key_events(egui::Key::Z, egui::Modifiers::COMMAND),
        );
        assert_eq!(
            app.documents[0].text, AFTER,
            "typing stays a separate undo transaction"
        );
        editor_frame(
            &mut app,
            &ctx,
            3.2,
            key_events(egui::Key::Z, egui::Modifiers::COMMAND),
        );
        assert_eq!(app.documents[0].text, BEFORE);
        editor_frame(
            &mut app,
            &ctx,
            3.3,
            key_events(egui::Key::Y, egui::Modifiers::COMMAND),
        );
        assert_eq!(app.documents[0].text, AFTER);
        editor_frame(
            &mut app,
            &ctx,
            3.4,
            key_events(egui::Key::Y, egui::Modifiers::COMMAND),
        );
        assert_eq!(app.documents[0].text, typed);
        assert_eq!(app.documents[0].saved_text, BEFORE);
    }
}

fn app_with_formatted_history() -> (CedarApp, egui::Context) {
    let mut app = app();
    let ctx = egui::Context::default();
    app.editor_ctx = ctx.clone();
    app.open_form = false;
    editor_frame(&mut app, &ctx, 0.0, vec![]);
    preview(&mut app);
    app.apply_format_preview();
    editor_frame(&mut app, &ctx, 1.0, vec![]);
    assert_eq!(app.documents[0].text, AFTER);
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(("editor", 1u64)))));
    (app, ctx)
}
#[test]
fn actual_frames_preserve_queued_text_paste_and_history_order() {
    for paste in [false, true] {
        for edit_first in [false, true] {
            let (mut app, ctx) = app_with_formatted_history();
            let edit = if paste {
                egui::Event::Paste("x".into())
            } else {
                egui::Event::Text("x".into())
            };
            let mut events = key_events(egui::Key::Z, egui::Modifiers::COMMAND);
            if edit_first {
                events.insert(0, edit);
            } else {
                events.push(edit);
            }
            editor_frame(&mut app, &ctx, 1.1, events);
            assert_eq!(
                app.documents[0].text,
                if edit_first {
                    AFTER.to_owned()
                } else {
                    format!("x{BEFORE}")
                },
                "paste={paste}, edit_first={edit_first}"
            );
            assert_eq!(app.documents[0].saved_text, BEFORE);
        }
    }
}
#[test]
fn actual_frames_preserve_undo_redo_order_in_one_input_batch() {
    for undo_first in [false, true] {
        let (mut app, ctx) = app_with_formatted_history();
        let undo = key_events(egui::Key::Z, egui::Modifiers::COMMAND);
        let redo = key_events(egui::Key::Y, egui::Modifiers::COMMAND);
        let events: Vec<_> = if undo_first {
            undo.into_iter().chain(redo).collect()
        } else {
            redo.into_iter().chain(undo).collect()
        };
        editor_frame(&mut app, &ctx, 1.1, events);
        assert_eq!(
            app.documents[0].text,
            if undo_first { AFTER } else { BEFORE },
            "undo_first={undo_first}"
        );
        assert_eq!(app.documents[0].saved_text, BEFORE);
    }
}
#[test]
fn actual_frames_preserve_multiple_history_presses_and_repeats() {
    for repeat in [false, true] {
        let (mut app, ctx) = app_with_formatted_history();
        let third = format!("{AFTER}// third transaction\n");
        crate::editor_state::commit(&ctx, &mut app.documents[0], third.clone(), 0);
        editor_frame(&mut app, &ctx, 1.1, vec![]);
        let mut undos = key_events(egui::Key::Z, egui::Modifiers::COMMAND);
        let mut second = key_events(egui::Key::Z, egui::Modifiers::COMMAND);
        if let egui::Event::Key {
            repeat: repeated, ..
        } = &mut second[0]
        {
            *repeated = repeat;
        }
        undos.extend(second);
        editor_frame(&mut app, &ctx, 1.2, undos);
        assert_eq!(app.documents[0].text, BEFORE, "repeat={repeat}");
        let redos = key_events(egui::Key::Y, egui::Modifiers::COMMAND)
            .into_iter()
            .chain(key_events(
                egui::Key::Z,
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            ))
            .collect();
        editor_frame(&mut app, &ctx, 1.3, redos);
        assert_eq!(app.documents[0].text, third, "repeat={repeat}");
        assert_eq!(app.documents[0].saved_text, BEFORE);
    }
}

#[test]
fn optional_agent_operations_are_independent_of_server_and_core_lifecycle() {
    for (missing, kind) in [
        ("language_format", formatting()),
        (
            "language_references",
            FeatureKind::References {
                include_declaration: true,
            },
        ),
        ("language_document_symbols", FeatureKind::Outline),
    ] {
        let mut app = app();
        app.agent_info
            .as_mut()
            .unwrap()
            .capabilities
            .retain(|name| name != missing);
        assert!(app.backend_language_supported());
        app.request_language_navigation_feature(kind);
        assert!(app.language.features.intent.is_none());
        assert!(app.error.as_ref().unwrap().contains(missing));
    }
    let mut app = app();
    app.agent_info
        .as_mut()
        .unwrap()
        .capabilities
        .retain(|name| !matches!(name.as_str(), "language_query" | "language_resolve_uri"));
    preview(&mut app);
    assert!(app.language.features.preview.is_some());
}
