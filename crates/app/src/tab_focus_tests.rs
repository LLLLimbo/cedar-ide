use super::*;

const REPLACE_INPUT: &str = "literal_replace_text";

fn app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    app.state = ConnectionState::Ready;
    app.open_form = false;
    app.documents = vec![
        Document::new(1, "first.txt".into(), "first saved".into(), "r1".into()),
        Document::new(2, "second.txt".into(), "second saved".into(), "r2".into()),
    ];
    app.active_document = Some(1);
    app.next_document = 3;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut frame = eframe::Frame::_new_kittest();
    ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(780.0, 540.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut frame),
    )
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

fn label_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    fn find(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.visual_bounding_rect().center())
            }
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, label))
        .unwrap_or_else(|| panic!("missing tab {label}"))
}

fn click_at(app: &mut CedarApp, time: f64, at: egui::Pos2) {
    for (index, pressed) in [true, false].into_iter().enumerate() {
        frame(
            app,
            time + index as f64 * 0.01,
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

fn click_tab(app: &mut CedarApp, time: f64, label: &str) {
    let output = frame(app, time, vec![]);
    click_at(app, time + 0.01, label_position(&output, label));
}

fn focused(app: &CedarApp, id: egui::Id) -> bool {
    app.editor_ctx.memory(|memory| memory.has_focus(id))
}

fn selection(app: &CedarApp, document: u64) -> egui::text::CCursorRange {
    egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", document)))
        .unwrap()
        .cursor
        .char_range()
        .unwrap()
}

#[test]
fn tab_pointer_activation_accepts_typing_and_undo_without_an_editor_click() {
    let (mut app, commands) = app();
    click_tab(&mut app, 0.0, "second.txt");
    assert_eq!(app.active_document, Some(2));
    assert!(focused(&app, egui::Id::new(("editor", 2u64))));
    frame(&mut app, 1.0, vec![egui::Event::Text("!".into())]);
    assert_eq!(app.documents[1].text, "second saved!");
    assert!(app.documents[1].dirty());
    assert_eq!(app.documents[0].text, "first saved");
    frame(&mut app, 2.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[1].text, "second saved");
    assert!(!app.documents[1].dirty());
    assert!(commands.try_recv().is_err());
}

#[test]
fn tab_batched_press_release_keeps_focus_for_switch_and_same_tab_activation() {
    let (mut app, commands) = app();
    app.find_open = true;
    for (index, document) in [2, 2, 1, 1].into_iter().enumerate() {
        let time = index as f64 * 10.0;
        let path = if document == 1 {
            "first.txt"
        } else {
            "second.txt"
        };
        let output = frame(&mut app, time, vec![]);
        let at = label_position(&output, path);
        let input_id = egui::Id::new(REPLACE_INPUT);
        let field = app
            .editor_ctx
            .read_response(input_id)
            .unwrap()
            .rect
            .center();
        click_at(&mut app, time + 1.0, field);
        assert!(focused(&app, input_id));
        // Native backends can deliver a fast click in one input frame. The
        // editor must retain focus even though that frame has an outside press.
        let mut events = vec![egui::Event::PointerMoved(at)];
        events.extend(
            [true, false]
                .into_iter()
                .map(|pressed| egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }),
        );
        frame(&mut app, time + 2.0, events);
        assert_eq!(app.active_document, Some(document));
        assert!(focused(&app, egui::Id::new(("editor", document))));
        frame(&mut app, time + 3.0, vec![egui::Event::Text("!".into())]);
        let active = document as usize - 1;
        assert!(app.documents[active].text.ends_with('!'));
        frame(
            &mut app,
            time + 4.0,
            key(egui::Key::Z, egui::Modifiers::COMMAND),
        );
        assert!(app.documents.iter().all(|doc| !doc.dirty()));
        assert!(focused(&app, egui::Id::new(("editor", document))));
    }
    assert!(commands.try_recv().is_err());
}

#[test]
fn switching_dirty_tabs_preserves_each_cursor_and_complete_undo_redo_branch() {
    let (mut app, commands) = app();
    let ctx = app.editor_ctx.clone();
    for (index, prefix, cursor) in [(0, "first", 3), (1, "second", 5)] {
        app.active_document = Some(index as u64 + 1);
        frame(&mut app, index as f64, vec![]);
        editor_state::commit(
            &ctx,
            &mut app.documents[index],
            format!("{prefix} draft"),
            cursor,
        );
        editor_state::commit(
            &ctx,
            &mut app.documents[index],
            format!("{prefix} newer"),
            cursor + 1,
        );
    }
    frame(&mut app, 2.0, vec![]);
    let selections = [selection(&app, 1), selection(&app, 2)];
    let versions = [app.documents[0].edit_version, app.documents[1].edit_version];
    click_tab(&mut app, 3.0, "first.txt  *");
    assert_eq!(selection(&app, 1), selections[0]);
    assert_eq!(app.documents[0].edit_version, versions[0]);
    frame(&mut app, 4.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, "first draft");
    let first_undone_cursor = selection(&app, 1);
    click_tab(&mut app, 5.0, "second.txt  *");
    assert_eq!(selection(&app, 2), selections[1]);
    assert_eq!(app.documents[1].edit_version, versions[1]);
    frame(&mut app, 6.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[1].text, "second draft");
    click_tab(&mut app, 7.0, "first.txt  *");
    assert_eq!(selection(&app, 1), first_undone_cursor);
    let redo = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
    frame(&mut app, 8.0, key(egui::Key::Z, redo));
    assert_eq!(app.documents[0].text, "first newer");
    assert_eq!(app.documents[1].text, "second draft");
    click_tab(&mut app, 9.0, "second.txt  *");
    frame(&mut app, 10.0, key(egui::Key::Z, redo));
    assert_eq!(app.documents[1].text, "second newer");
    assert_eq!(app.documents[0].saved_text, "first saved");
    assert_eq!(app.documents[1].saved_text, "second saved");
    assert_eq!(app.documents[0].revision.as_deref(), Some("r1"));
    assert_eq!(app.documents[1].revision.as_deref(), Some("r2"));
    assert!(app.documents.iter().all(Document::dirty));
    assert!(commands.try_recv().is_err());
}

#[test]
fn same_tab_click_focuses_editor_but_idle_frames_preserve_find_and_replace_focus() {
    for input in [replace::FIND_INPUT, REPLACE_INPUT] {
        let (mut app, commands) = app();
        app.find_open = true;
        app.find_query = "find".into();
        app.replace.replacement = "replacement".into();
        editor_state::commit(
            &app.editor_ctx,
            &mut app.documents[0],
            "first dirty".into(),
            5,
        );
        click_tab(&mut app, 0.0, "first.txt  *");
        let cursor = selection(&app, 1);
        let input_id = egui::Id::new(input);
        let at = app
            .editor_ctx
            .read_response(input_id)
            .unwrap()
            .rect
            .center();
        click_at(&mut app, 1.0, at);
        assert!(focused(&app, input_id));
        for time in 2..5 {
            frame(&mut app, f64::from(time), vec![]);
            assert!(focused(&app, input_id));
        }
        frame(&mut app, 5.0, vec![egui::Event::Text("!".into())]);
        assert!(focused(&app, input_id));
        assert!(if input == replace::FIND_INPUT {
            app.find_query.contains('!')
        } else {
            app.replace.replacement.contains('!')
        });
        frame(&mut app, 6.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
        assert_eq!(app.documents[0].text, "first dirty");
        assert_eq!(app.documents[0].edit_version, 1);
        click_tab(&mut app, 7.0, "first.txt  *");
        assert!(focused(&app, egui::Id::new(("editor", 1u64))));
        assert_eq!(selection(&app, 1), cursor);
        frame(&mut app, 8.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
        assert_eq!(app.documents[0].text, "first saved");
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn late_open_after_tab_activation_cannot_reactivate_or_steal_field_focus() {
    let (mut app, commands) = app();
    app.find_open = true;
    app.open("late.txt".into(), Some(2));
    let command = commands.try_recv().unwrap();
    click_tab(&mut app, 0.0, "second.txt");
    let input_id = egui::Id::new(REPLACE_INPUT);
    let at = app
        .editor_ctx
        .read_response(input_id)
        .unwrap()
        .rect
        .center();
    click_at(&mut app, 1.0, at);
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::File {
            path: "late.txt".into(),
            text: "late contents\nsecond line".into(),
            revision: "late revision".into(),
        }),
    });
    frame(&mut app, 2.0, vec![egui::Event::Text("replacement".into())]);
    assert_eq!(app.active_document, Some(2));
    assert_eq!(app.documents.len(), 3);
    assert!(focused(&app, input_id));
    assert_eq!(app.replace.replacement, "replacement");
    assert!(app.documents.iter().all(|doc| !doc.dirty()));
    assert!(commands.try_recv().is_err());
}

#[test]
fn navigation_and_foreign_modals_block_tab_activation_without_delayed_focus() {
    for navigation in [true, false] {
        let (mut app, commands) = app();
        app.find_open = true;
        let output = frame(&mut app, 0.0, vec![]);
        let at = label_position(&output, "second.txt");
        if navigation {
            app.show_file_chooser();
        } else {
            app.confirm = Some(Confirm::CloseWindow);
        }
        click_at(&mut app, 1.0, at);
        assert_eq!(app.active_document, Some(1));
        assert!(!focused(&app, egui::Id::new(("editor", 2u64))));
        assert!(app.documents.iter().all(|doc| !doc.dirty()));
        if navigation {
            app.dismiss_navigation();
        } else {
            app.confirm = None;
        }
        // Allow egui's previous modal layer and normal navigation restore to
        // settle, then deliberately focus Find. No blocked tab intent remains.
        frame(&mut app, 2.0, vec![]);
        frame(&mut app, 3.0, vec![]);
        let input_id = egui::Id::new(replace::FIND_INPUT);
        let field = app
            .editor_ctx
            .read_response(input_id)
            .unwrap()
            .rect
            .center();
        click_at(&mut app, 4.0, field);
        frame(&mut app, 5.0, vec![egui::Event::Text("find".into())]);
        assert!(focused(&app, input_id));
        assert_eq!(app.find_query, "find");
        assert_eq!(app.active_document, Some(1));
        assert!(app.documents.iter().all(|doc| !doc.dirty()));
        assert!(commands.try_recv().is_err());
    }
}
