//! Completed Project actions remain truthful after ordinary navigation.
use super::*;

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

fn draft_witness(app: &CedarApp) -> String {
    let doc = &app.documents[0];
    let state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", doc.id))).unwrap();
    let selection = state.cursor.char_range().unwrap();
    let mut history: [Vec<_>; 2] = Default::default();
    for (index, redo) in [false, true].into_iter().enumerate() {
        let mut undoer = state.undoer();
        let mut current = (selection, doc.text.clone());
        for _ in 0..=editor_state::MAX_UNDO_STATES + 1 {
            let next = if redo {
                undoer.redo(&current)
            } else {
                undoer.undo(&current)
            }
            .cloned();
            let Some(next) = next else { break };
            history[index].push(next.clone());
            current = next;
        }
        assert!(
            !history[index].is_empty(),
            "both native history branches are populated"
        );
    }
    format!(
        "{:?}",
        (
            &doc.text,
            &doc.saved_text,
            &doc.revision,
            doc.edit_version,
            doc.cursor,
            selection,
            history
        )
    )
}

fn selected_fixture(long_path: bool) -> (CedarApp, Receiver<Command>, String, String) {
    let scope = if long_path {
        format!("中文/{}", "很长的目录".repeat(120))
    } else {
        "中文".into()
    };
    let target = format!("{scope}/Long.rs");
    let (mut app, commands) = fixture("origin.rs", Mode::Flat);
    app.active_document = None;
    app.recent_connected();
    app.active_document = Some(1);
    settle(&mut app);
    click(&mut app, "editor");
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        "dirty origin 雪\nsecond".into(),
        2,
    );
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        "later origin 雪\nsecond".into(),
        3,
    );
    frame(&mut app, key(egui::Key::Z, egui::Modifiers::COMMAND));
    editor_state::move_selection(
        &app.editor_ctx,
        &mut app.documents[0],
        egui::text::CCursorRange::two(egui::text::CCursor::new(2), egui::text::CCursor::new(8)),
    );
    settle(&mut app);
    assert!(app.documents[0].dirty());
    let selected = egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1_u64)))
        .unwrap()
        .cursor
        .char_range()
        .unwrap();
    assert_eq!((selected.primary.index, selected.secondary.index), (8, 2));
    let before = draft_witness(&app);
    app.documents.push(Document::new(
        2,
        target.clone(),
        "Long saved".into(),
        "r1".into(),
    ));
    app.next_document = 3;
    app.list(scope.clone());
    let command = expect_list(&commands, &scope);
    listed(
        &mut app,
        command,
        vec![
            entry(&target, false),
            entry(&format!("{scope}/Needle.rs"), false),
        ],
    );
    settle(&mut app);
    click(&mut app, "editor_tab:2");
    settle(&mut app);
    assert_eq!(app.active().unwrap().path, target);
    let locations = format!(
        "{:?}{:?}",
        app.location_history.back, app.location_history.forward
    );
    click(&mut app, "project_select_current");
    settle(&mut app);
    assert!(!app.project_reveal_pending());
    assert_eq!(app.explorer.selected, target);
    assert_eq!(
        format!(
            "{:?}{:?}",
            app.location_history.back, app.location_history.forward
        ),
        locations
    );
    assert_eq!(draft_witness(&app), before);
    assert!(commands.try_recv().is_err());
    (app, commands, target, before)
}

#[test]
fn completed_project_selection_names_historical_target_after_recent_tab_and_back() {
    for route in ["recent", "tab", "back"] {
        let (mut app, commands, target, before) = selected_fixture(false);
        let directory = app.directory.clone();
        let entries = app
            .entries
            .iter()
            .map(|e| (&e.path, e.is_dir))
            .map(|(p, d)| (p.clone(), d))
            .collect::<Vec<_>>();
        match route {
            "recent" => {
                frame(&mut app, key(egui::Key::E, egui::Modifiers::COMMAND));
                settle(&mut app);
                frame(&mut app, vec![egui::Event::Text("origin".into())]);
                settle(&mut app);
                assert_eq!(app.recent_files.visible_paths(), vec!["origin.rs"]);
                frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
            }
            "tab" => click(&mut app, "editor_tab:1"),
            "back" => click(&mut app, "Back"),
            _ => unreachable!(),
        }
        settle(&mut app);
        assert_eq!(app.active().unwrap().path, "origin.rs", "{route}");
        assert_eq!(app.directory, directory);
        assert_eq!(
            app.entries
                .iter()
                .map(|e| (e.path.clone(), e.is_dir))
                .collect::<Vec<_>>(),
            entries
        );
        assert_eq!(app.explorer.selected, target);
        assert_eq!(draft_witness(&app), before);
        assert!(!app.execution_trusted());
        assert!(
            commands.try_recv().is_err(),
            "{route}: no automatic List or Read"
        );
        let expected = format!("Selection completed: /{target}");
        assert_eq!(
            app.explorer.message.as_deref(),
            Some(expected.as_str()),
            "{route}: completed action must identify Long, not imply current origin"
        );
        let output = frame_at(&mut app, [900.0, 740.0], vec![]);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(text) if text.galley.job.text == expected && shape.clip_rect.intersects(text.visual_bounding_rect()))), "historical result remains painted");
    }
}

#[test]
fn completed_project_selection_long_cjk_status_is_one_row_with_full_target_retained() {
    let (mut app, commands, target, before) = selected_fixture(true);
    for font_size in [13.0, 18.0] {
        app.editor_ctx.style_mut(|style| {
            for font in style.text_styles.values_mut() {
                font.size = font_size;
            }
        });
        for _ in 0..5 {
            frame_at(&mut app, [780.0, 540.0], vec![]);
        }
        let output = frame_at(&mut app, [780.0, 540.0], vec![]);
        let expected = format!("Selection completed: /{target}");
        assert_eq!(app.explorer.message.as_deref(), Some(expected.as_str()));
        let (clip, text) = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.job.text == expected => {
                    Some((shape.clip_rect, text))
                }
                _ => None,
            })
            .expect("completed Project status painted");
        assert_eq!(
            text.galley.rows.len(),
            1,
            "long CJK target must not consume sidebar rows"
        );
        assert!(text.galley.elided);
        assert!(clip.contains_rect(text.visual_bounding_rect()));
        let sidebar =
            egui::containers::panel::PanelState::load(&app.editor_ctx, egui::Id::new("explorer"))
                .unwrap()
                .rect;
        assert!(text.galley.rect.width() <= sidebar.width());
        assert_eq!(app.explorer.selected, target);
        assert_eq!(draft_witness(&app), before);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn later_cancel_failure_and_mode_change_replace_completed_project_status() {
    for action in ["cancel", "failure", "mode"] {
        let (mut app, commands, target, before) = selected_fixture(false);
        assert_eq!(
            app.explorer.message,
            Some(format!("Selection completed: /{target}"))
        );
        if action == "mode" {
            click(&mut app, "explorer_tree_mode");
            settle(&mut app);
            assert!(app.explorer.message.is_none());
        } else {
            click(&mut app, "editor_tab:1");
            settle(&mut app);
            click(&mut app, "project_select_current");
            let command = expect_list(&commands, "");
            if action == "cancel" {
                click(&mut app, "project_reveal_cancel");
                listed(&mut app, command, vec![entry("origin.rs", false)]);
                settle(&mut app);
                assert_eq!(
                    app.explorer.message.as_deref(),
                    Some("File selection canceled")
                );
            } else {
                reply(&mut app, command, Err("directory access refused".into()));
                settle(&mut app);
                assert_eq!(app.explorer.message.as_deref(), Some("Current file was not selected: its directory listing failed, was incomplete, or exceeded an Explorer limit. Refresh or Retry explicitly"));
            }
        }
        assert_eq!(draft_witness(&app), before);
        assert!(!app.project_reveal_pending());
        assert!(commands.try_recv().is_err());
    }
}
