use super::*;
use crate::{
    model::Document,
    worker::{Command, Worker},
    ConnectForm, ConnectionState,
};
use std::sync::mpsc::Receiver;

fn cursor(primary: usize, secondary: usize) -> egui::text::CCursorRange {
    egui::text::CCursorRange {
        primary: egui::text::CCursor::new(primary),
        secondary: egui::text::CCursor::new(secondary),
    }
}

#[test]
fn one_uses_exact_selected_match_then_next_at_cursor_and_wraps() {
    for selection in [cursor(5, 8), cursor(8, 5)] {
        let result = plan("cat, cat, cat", "cat", "dog", selection, Scope::One).unwrap();
        assert_eq!(result.ranges, vec![5..8]);
        assert_eq!(result.text, "cat, dog, cat");
        assert_eq!(result.cursor_chars, 8);
    }
    for (selection, expected) in [
        (cursor(4, 4), 5..8),
        (cursor(9, 6), 10..13),
        (cursor(13, 13), 0..3),
    ] {
        let result = plan("cat, cat, cat", "cat", "dog", selection, Scope::One).unwrap();
        assert_eq!(result.ranges, vec![expected]);
    }
    // A selection merely containing a match does not authorize replacing it.
    let result = plan("cat, cat, cat", "cat", "dog", cursor(9, 4), Scope::One).unwrap();
    assert_eq!(result.ranges, vec![10..13]);
}

#[test]
fn all_is_case_sensitive_nonoverlapping_and_replacement_is_literal() {
    let result = plan("a.a A.A a.a", "a.a", "$1\\path", cursor(0, 0), Scope::All).unwrap();
    assert_eq!(result.text, "$1\\path A.A $1\\path");
    assert_eq!(result.ranges, vec![0..3, 8..11]);
    assert_eq!(
        plan("aaaaa", "aa", "", cursor(0, 0), Scope::All)
            .unwrap()
            .text,
        "a"
    );
    assert!(plan("abc", "", "x", cursor(0, 0), Scope::All).is_err());
    assert!(plan("abc", "z", "x", cursor(0, 0), Scope::One).is_err());
    assert!(plan("abc", "a", "\0", cursor(0, 0), Scope::All).is_err());
}

#[test]
fn unicode_scalar_ranges_and_exact_crlf_bytes_survive_one_all_and_joining() {
    let source = "é🐻\r\n中🐻\r\n";
    let one = plan(source, "🐻", "$1\\中", cursor(6, 5), Scope::One).unwrap();
    assert_eq!(one.ranges, vec![5..6]);
    assert_eq!(one.text, "é🐻\r\n中$1\\中\r\n");
    assert_eq!(one.cursor_chars, 9);
    let all = plan(source, "🐻", "🦀", cursor(0, 0), Scope::All).unwrap();
    assert_eq!(all.text, "é🦀\r\n中🦀\r\n");
    assert_eq!(all.ranges, vec![1..2, 5..6]);
    assert_eq!(all.cursor_chars, 2);
    let join = plan("éX\nend", "X", "\r", cursor(1, 1), Scope::One).unwrap();
    assert_eq!(join.text, "é\r\nend");
    assert_eq!(join.cursor_chars, 3);
    assert_eq!(
        plan(source, "\r\n", "\n", cursor(0, 0), Scope::All)
            .unwrap()
            .text,
        "é🐻\n中🐻\n"
    );
    assert!(plan(source, "🐻", "x", cursor(99, 0), Scope::One).is_err());
}

#[test]
fn exact_match_and_document_bounds_fail_closed_before_building_output() {
    let limit = "x".repeat(model::MAX_FIND_MATCHES);
    assert_eq!(
        plan(&limit, "x", "y", cursor(0, 0), Scope::All)
            .unwrap()
            .text,
        "y".repeat(model::MAX_FIND_MATCHES)
    );
    for scope in [Scope::One, Scope::All] {
        assert!(plan(&(limit.clone() + "x"), "x", "y", cursor(0, 0), scope)
            .unwrap_err()
            .contains("10,000"));
    }
    let source = format!("x{}", "a".repeat(MAX_FILE_BYTES - 1));
    assert_eq!(
        plan(&source, "x", "b", cursor(0, 0), Scope::All)
            .unwrap()
            .text
            .len(),
        MAX_FILE_BYTES
    );
    assert!(plan(&source, "x", "bb", cursor(0, 0), Scope::All).is_err());
    assert!(plan(&(source + "a"), "x", "", cursor(0, 0), Scope::One).is_err());
    assert!(plan(
        "xx",
        "x",
        &"b".repeat(MAX_FILE_BYTES / 2 + 1),
        cursor(0, 0),
        Scope::All
    )
    .is_err());
    assert!(plan(
        "x",
        "x",
        &"b".repeat(MAX_FILE_BYTES + 1),
        cursor(0, 0),
        Scope::One
    )
    .is_err());
}

fn app(text: &str) -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        local_root: "/workspace".into(),
        allow_run: false,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/workspace".into();
    app.state = ConnectionState::Disconnected;
    app.open_form = false;
    app.find_open = true;
    app.find_query = "cat".into();
    app.replace.replacement = "dog".into();
    app.documents.push(Document::new(
        1,
        "first.rs".into(),
        text.into(),
        "r0".into(),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    set_cursor(&mut app, cursor(0, 0));
    (app, commands)
}

fn set_cursor(app: &mut CedarApp, cursor: egui::text::CCursorRange) {
    let mut state = editor_state::load(&app.editor_ctx, &mut app.documents[0]);
    state.cursor.set_char_range(Some(cursor));
    state.store(&app.editor_ctx, egui::Id::new(("editor", 1u64)));
}

fn step_history(ctx: &egui::Context, doc: &mut Document, redo: bool) -> bool {
    let before = doc.text.clone();
    let modifiers = if redo {
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
    } else {
        egui::Modifiers::COMMAND
    };
    let _ = ctx.run(
        egui::RawInput {
            events: key(egui::Key::Z, modifiers),
            ..Default::default()
        },
        |ctx| {
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", doc.id))));
            editor_state::history_shortcut(ctx, doc);
        },
    );
    doc.text != before
}

fn preview(app: &mut CedarApp, scope: Scope) {
    let ctx = app.editor_ctx.clone();
    app.request_replace_preview(&ctx, scope);
    app.finish_replace_frame(&ctx);
    assert!(app.replace.preview.is_some(), "{:?}", app.replace.error);
}

fn apply(app: &mut CedarApp) {
    app.replace.action = Some(Action::Apply);
    app.finish_replace_frame(&app.editor_ctx.clone());
}

#[test]
fn preview_and_apply_preserve_baseline_interrupted_save_and_other_tabs_offline() {
    let (mut app, commands) = app("cat cat");
    app.documents.push(Document::new(
        2,
        "second.rs".into(),
        "cat untouched".into(),
        "r1".into(),
    ));
    let interrupted =
        crate::interrupted_save::InterruptedSave::capture(&app, &app.documents[0]).unwrap();
    app.documents[0].interrupted_save = Some(interrupted.clone());
    preview(&mut app, Scope::All);
    assert_eq!(app.documents[0].text, "cat cat");
    assert_eq!(app.documents[0].edit_version, 0);
    assert!(!step_history(&app.editor_ctx, &mut app.documents[0], false));
    apply(&mut app);
    assert_eq!(app.documents[0].text, "dog dog");
    assert_eq!(app.documents[0].edit_version, 1);
    assert_eq!(app.documents[0].saved_text, "cat cat");
    assert_eq!(app.documents[0].revision.as_deref(), Some("r0"));
    assert_eq!(app.documents[0].interrupted_save, Some(interrupted));
    assert_eq!(app.documents[1].text, "cat untouched");
    assert!(commands.try_recv().is_err());
    assert!(!app.active_form.as_ref().unwrap().allow_run);
    assert!(step_history(&app.editor_ctx, &mut app.documents[0], false));
    assert_eq!(app.documents[0].text, "cat cat");
    assert!(!step_history(&app.editor_ctx, &mut app.documents[0], false));
    assert!(step_history(&app.editor_ctx, &mut app.documents[0], true));
    assert_eq!(app.documents[0].text, "dog dog");
}

#[test]
fn no_op_apply_preserves_version_selection_and_existing_redo() {
    let (mut app, commands) = app("cat cat");
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        "cat newer".into(),
        4,
    );
    assert!(step_history(&app.editor_ctx, &mut app.documents[0], false));
    frame(
        &mut app,
        1.0,
        key(egui::Key::ArrowRight, egui::Modifiers::NONE),
    );
    let version = app.documents[0].edit_version;
    let selection_before = selection(&app.editor_ctx, 1);
    app.replace.replacement = "cat".into();
    preview(&mut app, Scope::All);
    apply(&mut app);
    assert_eq!(app.documents[0].text, "cat cat");
    assert_eq!(app.documents[0].edit_version, version);
    assert_eq!(selection(&app.editor_ctx, 1), selection_before);
    assert!(step_history(&app.editor_ctx, &mut app.documents[0], true));
    assert_eq!(app.documents[0].text, "cat newer");
    assert!(commands.try_recv().is_err());
}

fn change_snapshot(app: &mut CedarApp, change: usize) {
    match change {
        0 => app.generation += 1,
        1 => app.navigation_epoch += 1,
        2 => app.documents[0].edit_version += 1,
        3 => app.documents[0].text.push('!'),
        4 => app.documents[0].path = "renamed.rs".into(),
        5 => app.documents[0].id = 2,
        6 => app.active_document = None,
        7 => app.find_query = "Cat".into(),
        8 => app.replace.replacement = "Dog".into(),
        9 => set_cursor(app, cursor(3, 0)),
        10 => app.documents[0].jump_to = Some(4),
        11 => app.find_open = false,
        12 => app.navigation_changed(),
        _ => unreachable!(),
    }
}

#[test]
fn queued_preview_and_apply_reject_every_changed_snapshot_without_retargeting() {
    for applying in [false, true] {
        for change in 0..13 {
            let (mut app, commands) = app("cat cat");
            if applying {
                preview(&mut app, Scope::All);
                app.replace.action = Some(Action::Apply);
            } else {
                app.request_replace_preview(&app.editor_ctx.clone(), Scope::All);
            }
            change_snapshot(&mut app, change);
            let before = app.documents[0].text.clone();
            let version = app.documents[0].edit_version;
            app.finish_replace_frame(&app.editor_ctx.clone());
            assert!(
                app.replace.preview.is_none(),
                "applying={applying}, change={change}"
            );
            assert_eq!(
                app.documents[0].text, before,
                "applying={applying}, change={change}"
            );
            assert_eq!(app.documents[0].edit_version, version);
            assert!(commands.try_recv().is_err());
        }
    }
}

fn frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut frame = eframe::Frame::_new_kittest();
    let output = ctx.run(
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
    );
    assert!(output
        .shapes
        .iter()
        .all(|shape| shape.clip_rect.is_finite()));
    output
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

fn text_rect(output: &egui::FullOutput, expected: &str) -> Option<egui::Rect> {
    fn find(shape: &egui::epaint::Shape, expected: &str) -> Option<egui::Rect> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text == expected => {
                Some(text.visual_bounding_rect())
            }
            egui::epaint::Shape::Vec(shapes) => {
                shapes.iter().find_map(|shape| find(shape, expected))
            }
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, expected))
}

fn click(app: &mut CedarApp, time: f64, label: &str) {
    let output = frame(app, time, vec![]);
    let at = text_rect(&output, label)
        .unwrap_or_else(|| panic!("missing button {label}"))
        .center();
    for (index, pressed) in [true, false].into_iter().enumerate() {
        frame(
            app,
            time + (index + 1) as f64 * 0.01,
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

#[test]
fn actual_frames_click_preview_apply_cancel_and_atomic_undo_redo() {
    let (mut app, commands) = app("cat é🐻\r\ncat\r\n");
    frame(&mut app, 0.0, vec![]);
    click(&mut app, 1.0, "Preview all");
    assert!(app.replace.preview.is_some(), "{:?}", app.replace.error);
    assert_eq!(app.documents[0].text, "cat é🐻\r\ncat\r\n");
    click(&mut app, 2.0, "Cancel");
    assert!(app.replace.preview.is_none());
    assert_eq!(app.documents[0].edit_version, 0);
    click(&mut app, 3.0, "Preview all");
    click(&mut app, 4.0, "Apply");
    assert_eq!(app.documents[0].text, "dog é🐻\r\ndog\r\n");
    assert_eq!(app.documents[0].edit_version, 1);
    frame(&mut app, 5.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, "cat é🐻\r\ncat\r\n");
    frame(
        &mut app,
        6.0,
        key(
            egui::Key::Z,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        ),
    );
    assert_eq!(app.documents[0].text, "dog é🐻\r\ndog\r\n");
    assert_eq!(app.documents[0].saved_text, "cat é🐻\r\ncat\r\n");
    assert!(commands.try_recv().is_err());
}

#[test]
fn actual_find_next_previous_and_manual_selection_choose_reviewed_one() {
    let (mut app, _) = app("cat cat cat");
    frame(&mut app, 0.0, vec![]);
    click(&mut app, 1.0, "Next");
    assert_eq!(selection(&app.editor_ctx, 1), cursor(3, 0));
    click(&mut app, 2.0, "Previous");
    assert_eq!(selection(&app.editor_ctx, 1), cursor(11, 8));
    click(&mut app, 3.0, "Preview one");
    assert_eq!(
        app.replace.preview.as_ref().unwrap().plan.ranges,
        vec![8..11]
    );
    set_cursor(&mut app, cursor(4, 7));
    frame(&mut app, 4.0, vec![]);
    assert!(app.replace.preview.is_none());
    click(&mut app, 5.0, "Preview one");
    assert_eq!(
        app.replace.preview.as_ref().unwrap().plan.ranges,
        vec![4..7]
    );
    click(&mut app, 6.0, "Apply");
    assert_eq!(app.documents[0].text, "cat dog cat");
}

#[test]
fn actual_frames_find_open_type_enter_escape_and_navigation_do_not_leak_source_input() {
    let (mut app, commands) = app("cat cat");
    app.find_open = false;
    app.find_query.clear();
    frame(&mut app, 0.0, vec![]);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
    let mut events = key(egui::Key::F, egui::Modifiers::COMMAND);
    events.push(egui::Event::Text("cat".into()));
    frame(&mut app, 1.0, events);
    assert_eq!(app.find_query, "cat");
    assert_eq!(app.documents[0].text, "cat cat");
    let mut events = key(egui::Key::Enter, egui::Modifiers::NONE);
    events.push(egui::Event::Paste("late field input".into()));
    frame(&mut app, 2.0, events);
    assert_eq!(app.documents[0].text, "cat cat");
    let mut events = key(egui::Key::Escape, egui::Modifiers::NONE);
    events.push(egui::Event::Paste("must not leak".into()));
    frame(&mut app, 3.0, events);
    assert!(!app.find_open);
    assert_eq!(app.documents[0].text, "cat cat");
    frame(&mut app, 4.0, vec![]);
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new(("editor", 1u64)))));
    app.find_open = true;
    app.find_query = "cat".into();
    preview(&mut app, Scope::All);
    let mut events = key(egui::Key::P, egui::Modifiers::COMMAND);
    events.push(egui::Event::Text("another".into()));
    frame(&mut app, 5.0, events);
    assert!(app.navigation.blocks_editor());
    assert!(app.replace.preview.is_none());
    assert_eq!(app.documents[0].text, "cat cat");
    let mut events = key(egui::Key::Escape, egui::Modifiers::NONE);
    events.push(egui::Event::Paste("chooser input".into()));
    frame(&mut app, 6.0, events);
    frame(&mut app, 7.0, vec![]);
    assert!(app.find_open, "chooser Escape must not close Find");
    assert_eq!(app.documents[0].text, "cat cat");
    assert_eq!(app.documents[0].edit_version, 0);
    assert!(commands.try_recv().is_err());
}

#[test]
fn actual_frames_typing_or_cursor_movement_expires_preview_before_queued_apply() {
    for events in [
        vec![egui::Event::Text("!".into())],
        key(egui::Key::ArrowRight, egui::Modifiers::NONE),
    ] {
        let (mut app, _) = app("cat cat");
        frame(&mut app, 0.0, vec![]);
        preview(&mut app, Scope::All);
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
        app.replace.action = Some(Action::Apply);
        frame(&mut app, 1.0, events);
        assert!(app.replace.preview.is_none());
        assert!(!app.documents[0].text.contains("dog"));
    }
}

#[test]
fn bounded_preview_excerpts_escape_controls_without_altering_literal_plan() {
    assert_eq!(excerpt("a\r\n\tb"), "a\\r\\n\\tb");
    assert!(excerpt(&"🐻".repeat(PREVIEW_CHARS + 1)).ends_with('…'));
    assert_eq!(
        excerpt(&"🐻".repeat(PREVIEW_CHARS + 1)).chars().count(),
        PREVIEW_CHARS + 1
    );
}

#[test]
fn actual_frames_find_or_replacement_input_expires_queued_apply() {
    for input in [FIND_INPUT, REPLACE_INPUT] {
        let (mut app, commands) = app("cat cat");
        frame(&mut app, 0.0, vec![]);
        preview(&mut app, Scope::All);
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new(input)));
        app.replace.action = Some(Action::Apply);
        frame(&mut app, 1.0, vec![egui::Event::Text("!".into())]);
        assert!(app.replace.preview.is_none());
        assert_eq!(app.documents[0].text, "cat cat");
        assert_eq!(app.documents[0].edit_version, 0);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn minimum_window_keeps_apply_and_cancel_visible_for_expanded_control_excerpts() {
    let source = "\u{0001}".repeat(PREVIEW_CHARS * 3);
    let (mut app, _) = app(&source);
    app.find_query = "\u{0001}".repeat(PREVIEW_CHARS);
    app.replace.replacement = "\u{0002}".repeat(PREVIEW_CHARS * 3);
    frame(&mut app, 0.0, vec![]);
    preview(&mut app, Scope::All);
    let output = frame(&mut app, 1.0, vec![]);
    for label in ["Apply", "Cancel"] {
        let rect = text_rect(&output, label).unwrap();
        assert!(
            rect.top() > 0.0 && rect.bottom() < 510.0,
            "{label}: {rect:?}"
        );
    }
    assert_eq!(
        app.replace.preview.as_ref().unwrap().plan.text,
        "\u{0002}".repeat(PREVIEW_CHARS * 9)
    );
}

#[test]
fn late_recovery_and_run_modals_block_pending_preview_and_apply() {
    for applying in [false, true] {
        for recovery in [false, true] {
            let (mut app, commands) = app("cat cat");
            if applying {
                preview(&mut app, Scope::All);
                app.replace.action = Some(Action::Apply);
            } else {
                app.request_replace_preview(&app.editor_ctx.clone(), Scope::All);
            }
            // These states can arise after the editor and navigation frame
            // checks, so the final transaction barrier must inspect them too.
            assert!(!app.navigation.blocks_editor());
            if recovery {
                app.recovery.remove_confirmation =
                    Some(cedar_recovery::RecordId::parse(&"a".repeat(64)).unwrap());
            } else {
                app.run_state.snapshot = Some(cedar_tasks::TaskSnapshot {
                    id: 1,
                    state: cedar_tasks::TaskState::Running,
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: None,
                    windows_exit_code: None,
                    truncated: false,
                    error: None,
                });
                assert!(!app.guard_run_transition(crate::run_ui::Transition::Close));
            }
            app.finish_replace_frame(&app.editor_ctx.clone());
            assert!(app.replace.preview.is_none());
            assert_eq!(app.documents[0].text, "cat cat");
            assert_eq!(app.documents[0].edit_version, 0);
            assert!(commands.try_recv().is_err());
        }
    }
}
