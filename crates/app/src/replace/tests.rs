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

fn assert_selection(actual: egui::text::CCursorRange, expected: egui::text::CCursorRange) {
    assert_eq!(actual, expected);
    assert_eq!(
        actual.primary.prefer_next_row,
        expected.primary.prefer_next_row
    );
    assert_eq!(
        actual.secondary.prefer_next_row,
        expected.secondary.prefer_next_row
    );
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

#[test]
fn selection_forward_and_reversed_preserve_unicode_and_outside_crlf_bytes() {
    let prefix = "🐻x\r\n";
    let selected = "é🐻\r\n中🐻\r\n";
    let suffix = "🐻z\r\n";
    let source = format!("{prefix}{selected}{suffix}");
    let start = prefix.chars().count();
    let end = start + selected.chars().count();
    for range in [cursor(end, start), cursor(start, end)] {
        let result = plan(&source, "🐻", "$1\\🦀", range, Scope::Selection).unwrap();
        assert_eq!(
            result.ranges,
            vec![start + 1..start + 2, start + 5..start + 6]
        );
        assert_eq!(
            result.text,
            format!("{prefix}é$1\\🦀\r\n中$1\\🦀\r\n{suffix}")
        );
        assert_eq!(result.cursor_chars, start + 5);
        let delete = plan(&source, "\r\n", "", range, Scope::Selection).unwrap();
        assert_eq!(delete.text, format!("{prefix}é🐻中🐻{suffix}"));
    }
    let join = plan("éX\nend", "X", "\r", cursor(2, 1), Scope::Selection).unwrap();
    assert_eq!(join.text, "é\r\nend");
    assert_eq!(join.cursor_chars, 3);
    // Selection endpoints are literal scalar boundaries, even inside CRLF.
    // A match crossing an endpoint is excluded; outside CR/LF bytes stay exact.
    assert!(plan("cat\r\ncat", "\r\n", "!", cursor(4, 0), Scope::Selection).is_err());
    assert_eq!(
        plan("cat\r\ncat", "cat", "dog", cursor(4, 0), Scope::Selection)
            .unwrap()
            .text,
        "dog\r\ncat"
    );
    assert_eq!(
        plan("cat\r\ncat", "cat", "dog", cursor(8, 4), Scope::Selection)
            .unwrap()
            .text,
        "cat\r\ndog"
    );
}

#[test]
fn selection_matches_its_slice_with_literal_case_and_overlap_alignment() {
    let result = plan("aaaaaaa", "aa", "X", cursor(6, 1), Scope::Selection).unwrap();
    assert_eq!(result.ranges, vec![1..3, 3..5]);
    assert_eq!(result.text, "aXXaa");
    // Whole-file matches straddling either edge must remain untouched.
    let result = plan(
        "catcatcatcat",
        "cat",
        "dog",
        cursor(2, 10),
        Scope::Selection,
    )
    .unwrap();
    assert_eq!(result.ranges, vec![3..6, 6..9]);
    assert_eq!(result.text, "catdogdogcat");
    assert!(plan("ababa", "aba", "X", cursor(4, 1), Scope::Selection).is_err());
    let result = plan(
        "a.a|a.a A.A a.a|a.a",
        "a.a",
        "$1\\x",
        cursor(4, 15),
        Scope::Selection,
    )
    .unwrap();
    assert_eq!(result.ranges, vec![4..7, 12..15]);
    assert_eq!(result.text, "a.a|$1\\x A.A $1\\x|a.a");
    for range in [
        cursor(0, 0),
        cursor(3, 3),
        cursor(4, 1),
        cursor(1, usize::MAX),
    ] {
        assert!(plan("cat", "cat", "dog", range, Scope::Selection).is_err());
    }
    assert!(plan("cat", "", "dog", cursor(3, 0), Scope::Selection).is_err());
    assert!(plan("cat", "cat", "\0", cursor(3, 0), Scope::Selection).is_err());
}

#[test]
fn selection_exact_match_limit_counts_only_selected_slice_without_truncation() {
    let outside = "x".repeat(model::MAX_FIND_MATCHES + 1);
    let selected = "x".repeat(model::MAX_FIND_MATCHES);
    let source = format!("{outside}|{selected}|{outside}");
    let start = outside.len() + 1;
    let result = plan(
        &source,
        "x",
        "🐻",
        cursor(start + selected.len(), start),
        Scope::Selection,
    )
    .unwrap();
    assert_eq!(result.ranges.len(), model::MAX_FIND_MATCHES);
    assert_eq!(result.ranges.first(), Some(&(start..start + 1)));
    assert_eq!(
        result.ranges.last(),
        Some(&(start + selected.len() - 1..start + selected.len()))
    );
    assert_eq!(
        result.text,
        format!(
            "{outside}|{}|{outside}",
            "🐻".repeat(model::MAX_FIND_MATCHES)
        )
    );
    assert!(plan(
        &source,
        "x",
        "y",
        cursor(outside.len(), 0),
        Scope::Selection
    )
    .unwrap_err()
    .contains("10,000"));
}

#[test]
fn selection_checks_full_source_replacement_and_result_byte_bounds() {
    let source = format!("x{}", "é".repeat(MAX_FILE_BYTES / 2 - 1)) + "a";
    assert_eq!(source.len(), MAX_FILE_BYTES);
    let result = plan(&source, "x", "y", cursor(1, 0), Scope::Selection).unwrap();
    assert_eq!(result.text.len(), MAX_FILE_BYTES);
    assert_eq!(&result.text[1..], &source[1..]);
    assert!(plan(&source, "x", "yy", cursor(1, 0), Scope::Selection).is_err());
    assert!(plan(&(source + "a"), "x", "", cursor(1, 0), Scope::Selection).is_err());
    let replacement = "🐻".repeat(MAX_FILE_BYTES / 4);
    assert_eq!(
        plan("x", "x", &replacement, cursor(1, 0), Scope::Selection)
            .unwrap()
            .text,
        replacement
    );
    assert!(plan("xx", "x", &replacement, cursor(1, 0), Scope::Selection).is_err());
    assert!(plan(
        "x",
        "x",
        &(replacement + "a"),
        cursor(1, 0),
        Scope::Selection
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

fn admitted(app: &CedarApp, pass: u64) -> Option<Activation> {
    InputOwner::capture(app, &app.editor_ctx).map(|owner| Activation { pass, owner })
}

fn preview(app: &mut CedarApp, scope: Scope) {
    let ctx = app.editor_ctx.clone();
    // Snapshot tests construct an already admitted intent. Native button
    // admission is exercised separately through frame(), including its hook.
    app.request_replace_preview(&ctx, scope, admitted(app, input_pass(&ctx)));
    app.finish_replace_frame(&ctx);
    assert!(app.replace.preview.is_some(), "{:?}", app.replace.error);
}

fn apply(app: &mut CedarApp) {
    app.replace.action = Some(Action::Apply(admitted(app, input_pass(&app.editor_ctx))));
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
        9 => set_cursor(app, cursor(4, 4)),
        10 => app.documents[0].jump_to = Some(4),
        11 => app.find_open = false,
        12 => app.navigation_changed(),
        13 => {
            app.workspace_key = Some(crate::WorkspaceKey::Local {
                root: "/other".into(),
            })
        }
        14 => app.root = "/other".into(),
        15 => app.active_form = None,
        16 => app.state = ConnectionState::Ready,
        17 | 18 => {
            let mut changed = selection(&app.editor_ctx, 1);
            let endpoint = if change == 17 {
                &mut changed.primary
            } else {
                &mut changed.secondary
            };
            endpoint.prefer_next_row = !endpoint.prefer_next_row;
            set_cursor(app, changed);
        }
        19 => {
            let mut state = editor_state::load(&app.editor_ctx, &mut app.documents[0]);
            state.cursor.set_char_range(None);
            state.store(&app.editor_ctx, egui::Id::new(("editor", 1u64)));
        }
        20 => app.documents[0].edit_version = u64::MAX,
        _ => unreachable!(),
    }
}

#[test]
fn queued_preview_and_apply_reject_every_changed_snapshot_without_retargeting() {
    for (applying, scope) in [false, true].into_iter().flat_map(|applying| {
        [Scope::One, Scope::All, Scope::Selection].map(|scope| (applying, scope))
    }) {
        for change in 0..21 {
            let (mut app, commands) = app("cat cat");
            set_cursor(&mut app, cursor(3, 0));
            if applying {
                preview(&mut app, scope);
                app.replace.action =
                    Some(Action::Apply(admitted(&app, input_pass(&app.editor_ctx))));
            } else {
                app.request_replace_preview(
                    &app.editor_ctx.clone(),
                    scope,
                    admitted(&app, input_pass(&app.editor_ctx)),
                );
            }
            change_snapshot(&mut app, change);
            let before = app.documents[0].text.clone();
            let version = app.documents[0].edit_version;
            app.finish_replace_frame(&app.editor_ctx.clone());
            assert!(
                app.replace.preview.is_none(),
                "applying={applying}, scope={scope:?}, change={change}"
            );
            assert_eq!(
                app.documents[0].text, before,
                "applying={applying}, scope={scope:?}, change={change}"
            );
            assert_eq!(app.documents[0].edit_version, version);
            assert!(commands.try_recv().is_err());
        }
    }
}

fn frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) -> egui::FullOutput {
    frame_with_hook(app, time, events, true)
}

fn frame_with_hook(
    app: &mut CedarApp,
    time: f64,
    events: Vec<egui::Event>,
    capture: bool,
) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut frame = eframe::Frame::_new_kittest();
    let mut input = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(780.0, 540.0),
        )),
        events,
        ..Default::default()
    };
    if capture {
        eframe::App::raw_input_hook(app, &ctx, &mut input);
    }
    let output = ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut frame));
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

fn pointer(at: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn selection_app() -> (CedarApp, Receiver<Command>) {
    let (mut app, commands) = app("cat cat cat");
    frame(&mut app, 0.0, vec![]);
    editor_state::move_selection(&app.editor_ctx, &mut app.documents[0], cursor(7, 4));
    frame(&mut app, 0.1, vec![]);
    (app, commands)
}

fn focus_replace_field(app: &mut CedarApp, time: f64, field: &str) {
    let id = egui::Id::new(field);
    let at = app.editor_ctx.read_response(id).unwrap().rect.center();
    frame(
        app,
        time,
        vec![egui::Event::PointerMoved(at), pointer(at, true)],
    );
    frame(app, time + 0.01, vec![pointer(at, false)]);
    assert!(app.editor_ctx.memory(|memory| memory.has_focus(id)));
}

fn selection_app_after_field_entry() -> (CedarApp, Receiver<Command>, egui::text::CCursorRange) {
    let (mut app, commands) = app("cat cat cat");
    app.find_open = false;
    app.find_query.clear();
    app.replace.replacement.clear();
    frame(&mut app, 0.0, vec![]);
    let mut before = cursor(7, 4);
    before.primary.prefer_next_row = true;
    editor_state::move_selection(&app.editor_ctx, &mut app.documents[0], before);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
    frame(&mut app, 0.1, key(egui::Key::F, egui::Modifiers::COMMAND));
    assert!(app.find_open);
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new(FIND_INPUT))));
    // Ctrl+F now seeds the selected literal; native select-all keeps this
    // fixture exercising actual Find-field typing before Replace authority.
    assert_eq!(app.find_query, "cat");
    frame(&mut app, 0.15, key(egui::Key::A, egui::Modifiers::COMMAND));
    for (index, (key_name, text)) in [
        (egui::Key::C, "c"),
        (egui::Key::A, "a"),
        (egui::Key::T, "t"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut events = key(key_name, egui::Modifiers::NONE);
        events.push(egui::Event::Text(text.into()));
        frame(&mut app, 0.2 + index as f64 * 0.1, events);
    }
    focus_replace_field(&mut app, 0.5, REPLACE_INPUT);
    for (index, (key_name, text)) in [
        (egui::Key::D, "d"),
        (egui::Key::O, "o"),
        (egui::Key::G, "g"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut events = key(key_name, egui::Modifiers::NONE);
        events.push(egui::Event::Text(text.into()));
        frame(&mut app, 0.6 + index as f64 * 0.1, events);
    }
    assert_eq!(app.find_query, "cat");
    assert_eq!(app.replace.replacement, "dog");
    assert_selection(selection(&app.editor_ctx, 1), before);
    (app, commands, before)
}

fn selection_checked_frame(
    app: &mut CedarApp,
    time: f64,
    events: Vec<egui::Event>,
    retry: bool,
    case: &str,
    check: impl FnOnce(&CedarApp),
) {
    let ctx = app.editor_ctx.clone();
    let mut input = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(780.0, 540.0),
        )),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    assert_eq!(
        app.replace.selection_input.as_ref().unwrap().pass,
        ctx.cumulative_pass_nr(),
        "{case}: hook captured the current pass"
    );
    check(app);
    let mut native = eframe::Frame::_new_kittest();
    let mut passes = 0;
    let output = ctx.run(input, |ctx| {
        assert_eq!(
            app.replace.selection_input.as_ref().unwrap().pass,
            input_pass(ctx),
            "{case}: activation pass including sizing retry"
        );
        eframe::App::update(app, ctx, &mut native);
        passes += 1;
        if retry && passes == 1 {
            ctx.request_discard("exercise selection lifecycle sizing retry");
        }
    });
    if retry {
        assert_eq!(passes, 2, "{case}: forced sizing retry ran");
    }
    assert!(output
        .shapes
        .iter()
        .all(|shape| shape.clip_rect.is_finite()));
}

#[test]
fn selection_field_focus_split_pointer_actions_allow_ime_lifecycle_and_sizing_retries() {
    // Synthetic native-shaped batches isolate lifecycle classification; they
    // do not identify which events accompanied the reported desktop refusal.
    for field in [FIND_INPUT, REPLACE_INPUT] {
        for lifecycle in [egui::ImeEvent::Enabled, egui::ImeEvent::Disabled] {
            for (phase, retry) in [
                ("press", false),
                ("held", false),
                ("release", false),
                ("press", true),
                ("release", true),
                ("both", true),
            ] {
                let (mut app, commands, before) = selection_app_after_field_entry();
                app.editor_ctx
                    .options_mut(|options| options.max_passes = 2.try_into().unwrap());
                for (time, name) in [(1.0, "replace_preview_selection"), (2.0, "replace_apply")] {
                    let case = format!(
                        "{name}: field={field}, lifecycle={lifecycle:?}, phase={phase}, retry={retry}"
                    );
                    focus_replace_field(&mut app, time, field);
                    let response = crate::workspace_access_tests::recorded_response(&app, name);
                    let at = crate::workspace_access_tests::recorded_rect(&app, name).center();
                    let mut press = vec![egui::Event::PointerMoved(at), pointer(at, true)];
                    if phase == "press" || phase == "both" {
                        press.push(egui::Event::Ime(lifecycle.clone()));
                    }
                    let mut captured_owner = None;
                    selection_checked_frame(
                        &mut app,
                        time + 0.1,
                        press,
                        retry && (phase == "press" || phase == "both"),
                        &case,
                        |app| {
                            let input = app.replace.selection_input.as_ref().unwrap();
                            assert!(
                                input.pointer == Some(PointerInput::Press),
                                "{case}: press batch"
                            );
                            assert_eq!(
                                input.focus,
                                Some(egui::Id::new(field)),
                                "{case}: field focus"
                            );
                            let owner = input
                                .owner
                                .as_ref()
                                .unwrap_or_else(|| panic!("{case}: missing press candidate owner"));
                            assert!(
                                owner.current(app, &app.editor_ctx),
                                "{case}: press owner current"
                            );
                            assert!(
                                owner.selection == selection_identity(before),
                                "{case}: press selection identity"
                            );
                            captured_owner = Some(owner.clone());
                        },
                    );
                    assert!(
                        crate::workspace_access_tests::recorded_response(&app, name)
                            .is_pointer_button_down_on(),
                        "{case}: native response owns the actual press"
                    );
                    let (pressed_id, pressed_owner) = app
                        .replace
                        .selection_press
                        .clone()
                        .unwrap_or_else(|| panic!("{case}: missing admitted press"));
                    assert_eq!(pressed_id, response.id, "{case}: press button identity");
                    assert!(
                        Arc::ptr_eq(&pressed_owner, &captured_owner.unwrap()),
                        "{case}: press retains the captured owner"
                    );
                    assert_selection(selection(&app.editor_ctx, 1), before);
                    selection_checked_frame(
                        &mut app,
                        time + 0.2,
                        if phase == "held" {
                            vec![egui::Event::Ime(lifecycle.clone())]
                        } else {
                            vec![]
                        },
                        false,
                        &case,
                        |app| {
                            let input = app.replace.selection_input.as_ref().unwrap();
                            assert!(
                                input.pointer.is_none() && !input.keyboard,
                                "{case}: held batch"
                            );
                            assert!(
                                input.owner.is_none(),
                                "{case}: held frame does not copy a draft"
                            );
                        },
                    );
                    let (held_id, held_owner) = app
                        .replace
                        .selection_press
                        .as_ref()
                        .unwrap_or_else(|| panic!("{case}: held frame lost the press"));
                    assert_eq!(*held_id, pressed_id, "{case}: held button identity");
                    assert!(
                        Arc::ptr_eq(held_owner, &pressed_owner),
                        "{case}: held frame retains the same owner allocation"
                    );
                    assert!(
                        held_owner.current(&app, &app.editor_ctx),
                        "{case}: held owner current"
                    );
                    assert_selection(selection(&app.editor_ctx, 1), before);
                    let mut release = vec![pointer(at, false)];
                    if phase == "release" || phase == "both" {
                        release.push(egui::Event::Ime(lifecycle.clone()));
                    }
                    selection_checked_frame(
                        &mut app,
                        time + 0.3,
                        release,
                        retry && (phase == "release" || phase == "both"),
                        &case,
                        |app| {
                            let input = app.replace.selection_input.as_ref().unwrap();
                            assert!(
                                input.pointer == Some(PointerInput::Release),
                                "{case}: release batch"
                            );
                            let owner = input.owner.as_ref().unwrap_or_else(|| {
                                panic!("{case}: missing release candidate owner")
                            });
                            assert!(
                                owner == &pressed_owner,
                                "{case}: release matches the press owner"
                            );
                            assert!(
                                owner.current(app, &app.editor_ctx),
                                "{case}: release owner current"
                            );
                            assert!(
                                owner.selection == selection_identity(before),
                                "{case}: release selection identity"
                            );
                        },
                    );
                    assert!(app.replace.error.is_none(), "{case}: action refused");
                    if name == "replace_preview_selection" {
                        let preview = app
                            .replace
                            .preview
                            .as_ref()
                            .unwrap_or_else(|| panic!("{case}: missing preview"));
                        assert_eq!(preview.scope, Scope::Selection, "{case}: preview scope");
                        assert_eq!(preview.plan.ranges, vec![4..7], "{case}: preview range");
                        assert!(
                            preview.current(&app, &app.editor_ctx),
                            "{case}: preview current"
                        );
                        assert_selection(preview.selection, before);
                        assert_selection(selection(&app.editor_ctx, 1), before);
                        assert_eq!(
                            app.documents[0].text, "cat cat cat",
                            "{case}: preview source"
                        );
                        assert_eq!(app.documents[0].edit_version, 0, "{case}: preview version");
                    } else {
                        assert!(
                            app.replace.preview.is_none(),
                            "{case}: apply consumed preview"
                        );
                        assert_eq!(
                            app.documents[0].text, "cat dog cat",
                            "{case}: applied source"
                        );
                        assert_eq!(app.documents[0].edit_version, 1, "{case}: one transaction");
                    }
                }
                frame(&mut app, 3.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
                assert_eq!(app.documents[0].text, "cat cat cat");
                assert_selection(selection(&app.editor_ctx, 1), before);
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn selection_field_focus_same_batch_pointer_actions_allow_ime_lifecycle() {
    for field in [FIND_INPUT, REPLACE_INPUT] {
        for lifecycle in [egui::ImeEvent::Enabled, egui::ImeEvent::Disabled] {
            let (mut app, commands, before) = selection_app_after_field_entry();
            for (time, name) in [(1.0, "replace_preview_selection"), (2.0, "replace_apply")] {
                let case = format!("{name}: field={field}, lifecycle={lifecycle:?}, same batch");
                focus_replace_field(&mut app, time, field);
                let at = crate::workspace_access_tests::recorded_rect(&app, name).center();
                selection_checked_frame(
                    &mut app,
                    time + 0.1,
                    vec![
                        egui::Event::PointerMoved(at),
                        pointer(at, true),
                        egui::Event::Ime(lifecycle.clone()),
                        pointer(at, false),
                    ],
                    false,
                    &case,
                    |app| {
                        let input = app.replace.selection_input.as_ref().unwrap();
                        assert!(
                            input.pointer == Some(PointerInput::Click),
                            "{case}: click batch"
                        );
                        assert_eq!(
                            input.focus,
                            Some(egui::Id::new(field)),
                            "{case}: field focus"
                        );
                        let owner = input
                            .owner
                            .as_ref()
                            .unwrap_or_else(|| panic!("{case}: missing click candidate owner"));
                        assert!(
                            owner.current(app, &app.editor_ctx),
                            "{case}: click owner current"
                        );
                        assert!(
                            owner.selection == selection_identity(before),
                            "{case}: click selection identity"
                        );
                    },
                );
                assert!(app.replace.error.is_none(), "{case}: action refused");
                if name == "replace_preview_selection" {
                    let preview = app
                        .replace
                        .preview
                        .as_ref()
                        .unwrap_or_else(|| panic!("{case}: missing preview"));
                    assert_eq!(preview.scope, Scope::Selection, "{case}: preview scope");
                    assert_eq!(preview.plan.ranges, vec![4..7], "{case}: preview range");
                    assert!(
                        preview.current(&app, &app.editor_ctx),
                        "{case}: preview current"
                    );
                    assert_selection(preview.selection, before);
                    assert_selection(selection(&app.editor_ctx, 1), before);
                    assert_eq!(
                        app.documents[0].text, "cat cat cat",
                        "{case}: preview source"
                    );
                    assert_eq!(app.documents[0].edit_version, 0, "{case}: preview version");
                } else {
                    assert!(
                        app.replace.preview.is_none(),
                        "{case}: apply consumed preview"
                    );
                    assert_eq!(
                        app.documents[0].text, "cat dog cat",
                        "{case}: applied source"
                    );
                    assert_eq!(app.documents[0].edit_version, 1, "{case}: one transaction");
                }
            }
            frame(&mut app, 3.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
            assert_eq!(app.documents[0].text, "cat cat cat");
            assert_selection(selection(&app.editor_ctx, 1), before);
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn selection_actual_pointer_buttons_reject_same_batch_and_split_mixed_input() {
    for applying in [false, true] {
        for split in [false, true] {
            for focus in [FIND_INPUT, REPLACE_INPUT, "editor"] {
                for (competing, extra) in [
                    ("paste", vec![egui::Event::Paste("!".into())]),
                    ("text", vec![egui::Event::Text("!".into())]),
                    (
                        "empty preedit",
                        vec![egui::Event::Ime(egui::ImeEvent::Preedit(String::new()))],
                    ),
                    (
                        "preedit",
                        vec![egui::Event::Ime(egui::ImeEvent::Preedit("!".into()))],
                    ),
                    (
                        "empty commit",
                        vec![egui::Event::Ime(egui::ImeEvent::Commit(String::new()))],
                    ),
                    (
                        "commit",
                        vec![egui::Event::Ime(egui::ImeEvent::Commit("!".into()))],
                    ),
                    (
                        "lifecycle and text",
                        vec![
                            egui::Event::Ime(egui::ImeEvent::Enabled),
                            egui::Event::Text("!".into()),
                        ],
                    ),
                    (
                        "lifecycle and paste",
                        vec![
                            egui::Event::Ime(egui::ImeEvent::Disabled),
                            egui::Event::Paste("!".into()),
                        ],
                    ),
                    ("undo", key(egui::Key::Z, egui::Modifiers::COMMAND)),
                    ("arrow", key(egui::Key::ArrowRight, egui::Modifiers::NONE)),
                ] {
                    let (mut app, commands) = selection_app();
                    if applying {
                        click(&mut app, 1.0, "Preview selection");
                    }
                    let label = if applying {
                        "Apply"
                    } else {
                        "Preview selection"
                    };
                    let output = frame(&mut app, 2.0, vec![]);
                    let at = text_rect(&output, label).unwrap().center();
                    frame(&mut app, 2.1, vec![egui::Event::PointerMoved(at)]);
                    let id = if focus == "editor" {
                        egui::Id::new(("editor", 1u64))
                    } else {
                        egui::Id::new(focus)
                    };
                    app.editor_ctx.memory_mut(|memory| memory.request_focus(id));
                    let mut events = vec![pointer(at, true)];
                    if !split {
                        events.push(pointer(at, false));
                    }
                    events.extend(extra);
                    frame(&mut app, 3.0, events);
                    if split {
                        frame(&mut app, 3.1, vec![pointer(at, false)]);
                    }
                    assert!(
                        !app.documents[0].text.contains("dog"),
                        "applying={applying}, split={split}, focus={focus}, competing={competing}"
                    );
                    assert!(
                        app.replace.preview.is_none(),
                        "applying={applying}, split={split}, focus={focus}, competing={competing}"
                    );
                    assert!(commands.try_recv().is_err());
                }
            }
        }
    }
}

#[test]
fn selection_actual_owned_enter_and_space_buttons_do_not_leak_activation_text() {
    for key_name in [egui::Key::Enter, egui::Key::Space] {
        for space_text in [false, true] {
            if key_name != egui::Key::Space && space_text {
                continue;
            }
            for lifecycle in [
                None,
                Some(egui::ImeEvent::Enabled),
                Some(egui::ImeEvent::Disabled),
            ] {
                let (mut app, commands) = selection_app();
                for (time, name) in [(1.0, "replace_preview_selection"), (2.0, "replace_apply")] {
                    let case = format!("{name}: key={key_name:?}, space_text={space_text}, lifecycle={lifecycle:?}");
                    frame(&mut app, time, vec![]);
                    let id = crate::workspace_access_tests::recorded_response(&app, name).id;
                    app.editor_ctx.memory_mut(|memory| memory.request_focus(id));
                    let mut events = key(key_name, egui::Modifiers::NONE);
                    if space_text {
                        events.push(egui::Event::Text(" ".into()));
                    }
                    if let Some(lifecycle) = &lifecycle {
                        events.push(egui::Event::Ime(lifecycle.clone()));
                    }
                    selection_checked_frame(&mut app, time + 0.1, events, false, &case, |app| {
                        let input = app.replace.selection_input.as_ref().unwrap();
                        assert!(input.keyboard, "{case}: clean keyboard batch");
                        assert_eq!(
                            input.focus,
                            Some(id),
                            "{case}: focused button owns activation"
                        );
                        let owner = input
                            .owner
                            .as_ref()
                            .unwrap_or_else(|| panic!("{case}: missing keyboard candidate owner"));
                        assert!(
                            owner.current(app, &app.editor_ctx),
                            "{case}: keyboard owner current"
                        );
                        assert!(
                            owner.selection == selection_identity(cursor(7, 4)),
                            "{case}: keyboard selection identity"
                        );
                    });
                    if name == "replace_preview_selection" {
                        assert!(
                            app.replace.preview.is_some(),
                            "{case}: {:?}",
                            app.replace.error
                        );
                        assert_eq!(app.documents[0].text, "cat cat cat", "{case}");
                        assert_selection(selection(&app.editor_ctx, 1), cursor(7, 4));
                    }
                }
                frame(&mut app, 3.0, vec![]);
                assert_eq!(app.documents[0].text, "cat dog cat");
                assert_eq!(app.documents[0].edit_version, 1);
                frame(&mut app, 4.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
                assert_eq!(app.documents[0].text, "cat cat cat");
                assert_selection(selection(&app.editor_ctx, 1), cursor(7, 4));
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn selection_actual_pointer_buttons_reject_composition_during_hold_or_release() {
    for applying in [false, true] {
        for phase in ["held", "release"] {
            for (composition, ime) in [
                ("empty preedit", egui::ImeEvent::Preedit(String::new())),
                ("preedit", egui::ImeEvent::Preedit("!".into())),
                ("empty commit", egui::ImeEvent::Commit(String::new())),
                ("commit", egui::ImeEvent::Commit("!".into())),
            ] {
                let case = format!("applying={applying}, phase={phase}, composition={composition}");
                let (mut app, commands) = selection_app();
                if applying {
                    click(&mut app, 1.0, "Preview selection");
                }
                frame(&mut app, 2.0, vec![]);
                let name = if applying {
                    "replace_apply"
                } else {
                    "replace_preview_selection"
                };
                let at = crate::workspace_access_tests::recorded_rect(&app, name).center();
                frame(
                    &mut app,
                    2.1,
                    vec![egui::Event::PointerMoved(at), pointer(at, true)],
                );
                assert!(
                    crate::workspace_access_tests::recorded_response(&app, name)
                        .is_pointer_button_down_on(),
                    "{case}: native press"
                );
                assert!(
                    app.replace.selection_press.is_some(),
                    "{case}: clean owned press"
                );
                let mut events = vec![
                    egui::Event::Ime(egui::ImeEvent::Enabled),
                    egui::Event::Ime(ime),
                    egui::Event::Ime(egui::ImeEvent::Disabled),
                ];
                if phase == "release" {
                    events.push(pointer(at, false));
                }
                frame(&mut app, 2.2, events);
                assert!(
                    app.replace.selection_press.is_none(),
                    "{case}: composition destroys press ownership"
                );
                if phase == "held" {
                    frame(&mut app, 2.3, vec![pointer(at, false)]);
                }
                assert!(app.replace.preview.is_none(), "{case}: refused action");
                assert_eq!(app.documents[0].text, "cat cat cat", "{case}");
                assert_eq!(app.documents[0].edit_version, 0, "{case}");
                assert_selection(selection(&app.editor_ctx, 1), cursor(7, 4));
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn selection_actual_keyboard_buttons_reject_competing_or_unowned_activation() {
    for applying in [false, true] {
        for (case, (key_name, extra)) in [
            (egui::Key::Enter, vec![egui::Event::Text("\n".into())]),
            (egui::Key::Enter, vec![egui::Event::Paste("!".into())]),
            (
                egui::Key::Enter,
                vec![
                    egui::Event::Ime(egui::ImeEvent::Enabled),
                    egui::Event::Ime(egui::ImeEvent::Preedit(String::new())),
                ],
            ),
            (egui::Key::Space, vec![egui::Event::Text("  ".into())]),
            (
                egui::Key::Space,
                vec![egui::Event::Text(" ".into()), egui::Event::Text(" ".into())],
            ),
            (
                egui::Key::Space,
                vec![key(egui::Key::ArrowRight, egui::Modifiers::NONE)
                    .pop()
                    .unwrap()],
            ),
        ]
        .into_iter()
        .chain(
            [egui::Key::Enter, egui::Key::Space]
                .into_iter()
                .flat_map(|key_name| {
                    [
                        egui::ImeEvent::Preedit(String::new()),
                        egui::ImeEvent::Preedit("!".into()),
                        egui::ImeEvent::Commit(String::new()),
                        egui::ImeEvent::Commit("!".into()),
                    ]
                    .into_iter()
                    .map(move |composition| (key_name, vec![egui::Event::Ime(composition)]))
                }),
        )
        .chain([
            (
                egui::Key::Enter,
                vec![
                    egui::Event::Ime(egui::ImeEvent::Enabled),
                    egui::Event::Text("!".into()),
                ],
            ),
            (
                egui::Key::Space,
                vec![
                    egui::Event::Ime(egui::ImeEvent::Disabled),
                    egui::Event::Paste("!".into()),
                ],
            ),
        ])
        .enumerate()
        {
            let (mut app, commands) = selection_app();
            if applying {
                click(&mut app, 1.0, "Preview selection");
            }
            let name = if applying {
                "replace_apply"
            } else {
                "replace_preview_selection"
            };
            frame(&mut app, 2.0, vec![]);
            let id = crate::workspace_access_tests::recorded_response(&app, name).id;
            app.editor_ctx.memory_mut(|memory| memory.request_focus(id));
            let mut events = key(key_name, egui::Modifiers::NONE);
            events.extend(extra);
            frame(&mut app, 3.0, events);
            assert!(
                app.replace.preview.is_none(),
                "applying={applying}, key={key_name:?}, case={case}"
            );
            assert_eq!(
                app.documents[0].text, "cat cat cat",
                "applying={applying}, key={key_name:?}, case={case}"
            );
            assert_eq!(
                app.documents[0].edit_version, 0,
                "applying={applying}, key={key_name:?}, case={case}"
            );
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn selection_actual_buttons_refuse_missing_stale_capture_and_held_key_repeat() {
    for applying in [false, true] {
        for missing in [false, true] {
            let (mut app, commands) = selection_app();
            if applying {
                click(&mut app, 1.0, "Preview selection");
            }
            let output = frame(&mut app, 2.0, vec![]);
            let at = text_rect(
                &output,
                if applying {
                    "Apply"
                } else {
                    "Preview selection"
                },
            )
            .unwrap()
            .center();
            if missing {
                app.replace.selection_input = None;
            }
            frame_with_hook(
                &mut app,
                3.0,
                vec![
                    egui::Event::PointerMoved(at),
                    pointer(at, true),
                    pointer(at, false),
                ],
                false,
            );
            assert!(app.replace.preview.is_none());
            assert_eq!(app.documents[0].text, "cat cat cat");
            assert!(commands.try_recv().is_err());
        }
    }
    let (mut app, commands) = selection_app();
    let id = crate::workspace_access_tests::recorded_response(&app, "replace_preview_selection").id;
    app.editor_ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &mut app,
        1.0,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE).remove(0)],
    );
    assert!(app.replace.preview.is_some());
    frame(&mut app, 2.0, vec![]);
    let id = crate::workspace_access_tests::recorded_response(&app, "replace_apply").id;
    app.editor_ctx.memory_mut(|memory| memory.request_focus(id));
    // Native InputState enriches this second raw repeat=false into repeat=true.
    frame(
        &mut app,
        3.0,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE).remove(0)],
    );
    assert!(app.replace.preview.is_none());
    assert_eq!(app.documents[0].text, "cat cat cat");
    assert_eq!(app.documents[0].edit_version, 0);
    assert!(commands.try_recv().is_err());
}

#[test]
fn selection_clean_same_batch_pointer_actions_survive_sizing_retries() {
    let (mut app, commands) = selection_app();
    app.editor_ctx
        .options_mut(|options| options.max_passes = 2.try_into().unwrap());
    for (time, label) in [(1.0, "Preview selection"), (2.0, "Apply")] {
        let output = frame(&mut app, time, vec![]);
        let at = text_rect(&output, label).unwrap().center();
        let ctx = app.editor_ctx.clone();
        let mut input = egui::RawInput {
            time: Some(time + 0.1),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(780.0, 540.0),
            )),
            events: vec![
                egui::Event::PointerMoved(at),
                pointer(at, true),
                pointer(at, false),
            ],
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
        let mut native = eframe::Frame::_new_kittest();
        let mut passes = 0;
        let _ = ctx.run(input, |ctx| {
            eframe::App::update(&mut app, ctx, &mut native);
            passes += 1;
            if passes == 1 {
                ctx.request_discard("exercise selection activation sizing retry");
            }
        });
        assert_eq!(passes, 2);
        if label == "Preview selection" {
            assert!(app.replace.preview.is_some(), "{:?}", app.replace.error);
            assert_eq!(app.documents[0].edit_version, 0);
        }
    }
    assert_eq!(app.documents[0].text, "cat dog cat");
    assert_eq!(app.documents[0].edit_version, 1);
    assert!(commands.try_recv().is_err());
}

#[test]
fn selection_press_cannot_retarget_after_snapshot_change_or_late_hook_mutation() {
    for after_hook in [false, true] {
        for change in [0, 1, 2, 3, 4, 7, 8, 9, 13, 14, 15, 16, 17, 18] {
            let (mut app, commands) = selection_app();
            let output = frame(&mut app, 1.0, vec![]);
            let at = text_rect(&output, "Preview selection").unwrap().center();
            frame(
                &mut app,
                1.1,
                vec![egui::Event::PointerMoved(at), pointer(at, true)],
            );
            assert!(app.replace.selection_press.is_some());
            // A quiet repaint during a held clean gesture must not lose its
            // press ownership or allocate another full-source snapshot.
            frame(&mut app, 1.2, vec![]);
            assert!(app.replace.selection_press.is_some());
            assert!(app
                .replace
                .selection_input
                .as_ref()
                .unwrap()
                .owner
                .is_none());
            let ctx = app.editor_ctx.clone();
            let mut input = egui::RawInput {
                time: Some(2.0),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(780.0, 540.0),
                )),
                events: vec![pointer(at, false)],
                ..Default::default()
            };
            if after_hook {
                eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
            }
            change_snapshot(&mut app, change);
            let text = app.documents[0].text.clone();
            let version = app.documents[0].edit_version;
            if !after_hook {
                eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
            }
            let mut native = eframe::Frame::_new_kittest();
            let _ = ctx.run(input, |ctx| eframe::App::update(&mut app, ctx, &mut native));
            assert!(
                app.replace.preview.is_none(),
                "after_hook={after_hook}, change={change}"
            );
            assert_eq!(app.documents[0].text, text);
            assert_eq!(app.documents[0].edit_version, version);
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn selection_capture_does_not_copy_editor_typing_or_unbounded_inputs() {
    let (mut app, _) = selection_app();
    for open in [false, true] {
        app.find_open = open;
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
        let raw = egui::RawInput {
            events: key(egui::Key::Space, egui::Modifiers::NONE),
            ..Default::default()
        };
        app.capture_replace_input(&app.editor_ctx.clone(), &raw);
        assert!(app
            .replace
            .selection_input
            .as_ref()
            .unwrap()
            .owner
            .is_none());
    }
    app.find_open = true;
    let id = crate::workspace_access_tests::recorded_response(&app, "replace_preview_selection").id;
    app.editor_ctx.memory_mut(|memory| memory.request_focus(id));
    for field in 0..3 {
        let oversized = "x".repeat(MAX_FILE_BYTES + 1);
        match field {
            0 => app.documents[0].text = oversized,
            1 => {
                app.documents[0].text = "cat cat cat".into();
                app.find_query = oversized;
            }
            2 => {
                app.find_query = "cat".into();
                app.replace.replacement = oversized;
            }
            _ => unreachable!(),
        }
        let raw = egui::RawInput {
            events: key(egui::Key::Enter, egui::Modifiers::NONE),
            ..Default::default()
        };
        app.capture_replace_input(&app.editor_ctx.clone(), &raw);
        assert!(app
            .replace
            .selection_input
            .as_ref()
            .unwrap()
            .owner
            .is_none());
    }
}

#[test]
fn selection_actual_pointer_rejects_previously_held_keys_and_other_buttons() {
    for held_key in [false, true] {
        let (mut app, commands) = selection_app();
        let output = frame(&mut app, 1.0, vec![]);
        let at = text_rect(&output, "Preview selection").unwrap().center();
        let held = if held_key {
            key(egui::Key::F8, egui::Modifiers::NONE).remove(0)
        } else {
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }
        };
        frame(&mut app, 1.1, vec![egui::Event::PointerMoved(at), held]);
        frame(&mut app, 2.0, vec![pointer(at, true), pointer(at, false)]);
        assert!(app.replace.preview.is_none());
        assert_eq!(app.documents[0].text, "cat cat cat");
        assert_eq!(app.documents[0].edit_version, 0);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn selection_native_edge_hits_and_held_release_after_layout_move_are_admitted() {
    for move_layout in [false, true] {
        let (mut app, commands) = selection_app();
        for (time, name) in [(1.0, "replace_preview_selection"), (3.0, "replace_apply")] {
            frame(&mut app, time, vec![]);
            let response = crate::workspace_access_tests::recorded_response(&app, name);
            let before_rect = crate::workspace_access_tests::recorded_rect(&app, name);
            let at = if move_layout {
                before_rect.center()
            } else {
                egui::pos2(
                    response.interact_rect.right() + 1.0,
                    response.interact_rect.center().y,
                )
            };
            if !move_layout {
                assert!(!response.rect.contains(at));
                assert!(response
                    .interact_rect
                    .expand(app.editor_ctx.style().interaction.interact_radius)
                    .contains(at));
            }
            frame(
                &mut app,
                time + 0.1,
                vec![egui::Event::PointerMoved(at), pointer(at, true)],
            );
            assert!(app.replace.selection_press.is_some());
            let (pressed_id, pressed_owner) = app.replace.selection_press.clone().unwrap();
            assert_eq!(pressed_id, response.id, "{name}: captured button identity");
            if move_layout {
                app.error = if app.error.is_some() {
                    None
                } else {
                    Some("A layout notification appeared during the held click".into())
                };
                frame(&mut app, time + 0.2, vec![]);
                let moved = crate::workspace_access_tests::recorded_response(&app, name);
                // read_response may retain the prior interaction geometry after
                // end_pass swaps pass state. The recorder holds this render's rect.
                let moved_rect = crate::workspace_access_tests::recorded_rect(&app, name);
                assert_eq!(moved.id, response.id, "{name}: moved button identity");
                assert_ne!(
                    before_rect, moved_rect,
                    "{name}: notification must move the rendered button"
                );
                assert!(
                    !moved_rect.contains(at),
                    "{name}: before={before_rect:?}, moved={moved_rect:?}, interaction={:?}, held={at:?}",
                    moved.rect
                );
                let (held_id, held_owner) = app.replace.selection_press.as_ref().unwrap();
                assert_eq!(*held_id, pressed_id, "{name}: held button identity");
                assert!(
                    std::sync::Arc::ptr_eq(held_owner, &pressed_owner),
                    "{name}: captured owner must survive layout movement"
                );
            }
            frame(&mut app, time + 0.3, vec![pointer(at, false)]);
            if name == "replace_preview_selection" {
                assert!(app.replace.preview.is_some(), "{:?}", app.replace.error);
            }
        }
        assert_eq!(app.documents[0].text, "cat dog cat");
        assert_eq!(app.documents[0].edit_version, 1);
        assert!(commands.try_recv().is_err());
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
fn selection_actual_buttons_cancel_apply_and_undo_restore_direction_and_both_affinities() {
    let source = "cat|é🐻\r\ncat🐻\r\n|cat";
    for (primary, secondary) in [(4, 14), (14, 4)] {
        for primary_affinity in [false, true] {
            for secondary_affinity in [false, true] {
                let (mut app, commands) = app(source);
                app.find_query = "🐻".into();
                app.replace.replacement = "🦀🦀".into();
                frame(&mut app, 0.0, vec![]);
                let mut before = cursor(primary, secondary);
                before.primary.prefer_next_row = primary_affinity;
                before.secondary.prefer_next_row = secondary_affinity;
                // Retain a native checkpoint with the same indices but stale
                // affinities. Apply must refresh its precise before-selection.
                let mut stale = before;
                stale.primary.prefer_next_row = !primary_affinity;
                stale.secondary.prefer_next_row = !secondary_affinity;
                editor_state::move_selection(&app.editor_ctx, &mut app.documents[0], stale);
                set_cursor(&mut app, before);
                click(&mut app, 1.0, "Preview selection");
                let reviewed = app.replace.preview.as_ref().unwrap();
                assert_eq!(reviewed.scope, Scope::Selection);
                assert_eq!(reviewed.plan.ranges, vec![5..6, 11..12]);
                assert_selection(reviewed.selection, before);
                assert_eq!(app.documents[0].text, source);
                click(&mut app, 2.0, "Cancel");
                assert!(app.replace.preview.is_none());
                assert_eq!(app.documents[0].edit_version, 0);
                assert_selection(selection(&app.editor_ctx, 1), before);
                click(&mut app, 3.0, "Preview selection");
                click(&mut app, 4.0, "Apply");
                assert_eq!(app.documents[0].text, "cat|é🦀🦀\r\ncat🦀🦀\r\n|cat");
                assert_eq!(app.documents[0].edit_version, 1);
                assert_selection(selection(&app.editor_ctx, 1), cursor(7, 7));
                assert_eq!(app.documents[0].saved_text, source);
                frame(&mut app, 5.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
                assert_eq!(app.documents[0].text, source);
                assert_eq!(app.documents[0].edit_version, 2);
                assert_selection(selection(&app.editor_ctx, 1), before);
                frame(&mut app, 6.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
                assert_eq!(app.documents[0].text, source);
                assert_eq!(app.documents[0].edit_version, 2);
                frame(&mut app, 7.0, key(egui::Key::Y, egui::Modifiers::COMMAND));
                assert_eq!(app.documents[0].text, "cat|é🦀🦀\r\ncat🦀🦀\r\n|cat");
                assert_eq!(app.documents[0].edit_version, 3);
                assert_selection(selection(&app.editor_ctx, 1), cursor(7, 7));
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn selection_no_op_actual_field_and_buttons_preserve_version_selection_and_entire_redo() {
    let source = "cat cat cat";
    let (mut app, commands) = app(source);
    frame(&mut app, 0.0, vec![]);
    for text in ["cat newer", "cat newest"] {
        editor_state::commit(&app.editor_ctx, &mut app.documents[0], text.into(), 4);
    }
    for _ in 0..2 {
        assert!(step_history(&app.editor_ctx, &mut app.documents[0], false));
    }
    let mut before = cursor(7, 4);
    before.primary.prefer_next_row = true;
    editor_state::move_selection(&app.editor_ctx, &mut app.documents[0], before);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(REPLACE_INPUT)));
    let mut events = key(egui::Key::A, egui::Modifiers::COMMAND);
    events.push(egui::Event::Text("cat".into()));
    frame(&mut app, 1.0, events);
    assert_eq!(app.replace.replacement, "cat");
    let version = app.documents[0].edit_version;
    click(&mut app, 2.0, "Preview selection");
    assert_eq!(
        app.replace.preview.as_ref().unwrap().plan.ranges,
        vec![4..7]
    );
    click(&mut app, 3.0, "Apply");
    assert_eq!(app.documents[0].text, source);
    assert_eq!(app.documents[0].edit_version, version);
    assert_selection(selection(&app.editor_ctx, 1), before);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
    for (time, text) in [(4.0, "cat newer"), (5.0, "cat newest")] {
        frame(&mut app, time, key(egui::Key::Y, egui::Modifiers::COMMAND));
        assert_eq!(app.documents[0].text, text);
    }
    for time in [6.0, 7.0] {
        frame(&mut app, time, key(egui::Key::Z, egui::Modifiers::COMMAND));
    }
    assert_eq!(app.documents[0].text, source);
    assert_selection(selection(&app.editor_ctx, 1), before);
    assert!(commands.try_recv().is_err());
}

#[test]
fn selection_preview_refuses_missing_empty_out_of_range_and_exhausted_state() {
    for malformed in 0..5 {
        let (mut app, commands) = app("cat cat");
        match malformed {
            0 => editor_state::forget(&app.editor_ctx, 1),
            1 => change_snapshot(&mut app, 19),
            2 => set_cursor(&mut app, cursor(4, 4)),
            3 => set_cursor(&mut app, cursor(8, 0)),
            4 => {
                set_cursor(&mut app, cursor(3, 0));
                app.documents[0].edit_version = u64::MAX;
            }
            _ => unreachable!(),
        }
        let version = app.documents[0].edit_version;
        app.request_replace_preview(
            &app.editor_ctx.clone(),
            Scope::Selection,
            admitted(&app, input_pass(&app.editor_ctx)),
        );
        app.finish_replace_frame(&app.editor_ctx.clone());
        assert!(app.replace.preview.is_none(), "malformed={malformed}");
        assert!(app.replace.error.is_some(), "malformed={malformed}");
        assert_eq!(app.documents[0].text, "cat cat");
        assert_eq!(app.documents[0].edit_version, version);
        assert!(commands.try_recv().is_err());
    }
    let (mut app, _) = app("cat cat");
    set_cursor(&mut app, cursor(3, 0));
    app.documents[0].edit_version = u64::MAX - 1;
    preview(&mut app, Scope::Selection);
    apply(&mut app);
    assert_eq!(app.documents[0].text, "dog cat");
    assert_eq!(app.documents[0].edit_version, u64::MAX);
    set_cursor(&mut app, cursor(7, 4));
    app.request_replace_preview(
        &app.editor_ctx.clone(),
        Scope::Selection,
        admitted(&app, input_pass(&app.editor_ctx)),
    );
    app.finish_replace_frame(&app.editor_ctx.clone());
    assert!(app.replace.preview.is_none());
    assert_eq!(app.documents[0].text, "dog cat");
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
    frame(&mut app, 0.5, key(egui::Key::F, egui::Modifiers::COMMAND));
    frame(&mut app, 1.0, vec![egui::Event::Text("cat".into())]);
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
    for (scope, events) in [Scope::All, Scope::Selection]
        .into_iter()
        .flat_map(|scope| {
            [
                vec![egui::Event::Text("!".into())],
                key(egui::Key::ArrowRight, egui::Modifiers::NONE),
            ]
            .map(|events| (scope, events))
        })
    {
        let (mut app, _) = app("cat cat");
        frame(&mut app, 0.0, vec![]);
        set_cursor(&mut app, cursor(3, 0));
        preview(&mut app, scope);
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
        app.replace.action = Some(Action::Apply(admitted(
            &app,
            app.editor_ctx.cumulative_pass_nr(),
        )));
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
    for (scope, input) in [Scope::All, Scope::Selection]
        .into_iter()
        .flat_map(|scope| [FIND_INPUT, REPLACE_INPUT].map(|input| (scope, input)))
    {
        let (mut app, commands) = app("cat cat");
        frame(&mut app, 0.0, vec![]);
        set_cursor(&mut app, cursor(3, 0));
        preview(&mut app, scope);
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new(input)));
        app.replace.action = Some(Action::Apply(admitted(
            &app,
            app.editor_ctx.cumulative_pass_nr(),
        )));
        frame(&mut app, 1.0, vec![egui::Event::Text("!".into())]);
        assert!(app.replace.preview.is_none());
        assert_eq!(app.documents[0].text, "cat cat");
        assert_eq!(app.documents[0].edit_version, 0);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn minimum_window_keeps_apply_and_cancel_visible_for_expanded_control_excerpts() {
    for scope in [Scope::All, Scope::Selection] {
        let source = "\u{0001}".repeat(PREVIEW_CHARS * 3);
        let (mut app, _) = app(&source);
        app.find_query = "\u{0001}".repeat(PREVIEW_CHARS);
        app.replace.replacement = "\u{0002}".repeat(PREVIEW_CHARS * 3);
        let output = frame(&mut app, 0.0, vec![]);
        for label in ["Preview one", "Preview all", "Preview selection"] {
            let rect = text_rect(&output, label).unwrap();
            assert!(
                rect.top() > 0.0 && rect.bottom() < 510.0,
                "{label}: {rect:?}"
            );
        }
        set_cursor(&mut app, cursor(source.chars().count(), 0));
        preview(&mut app, scope);
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
        if scope == Scope::Selection {
            let label = format!(
                "Selected range: line 1, column 1 to line 1, column {} (end exclusive)",
                source.chars().count() + 1
            );
            assert!(text_rect(&output, &label).is_some());
        }
    }
}

#[test]
fn late_recovery_and_run_modals_block_pending_preview_and_apply() {
    for (applying, scope) in [false, true]
        .into_iter()
        .flat_map(|applying| [Scope::All, Scope::Selection].map(|scope| (applying, scope)))
    {
        for recovery in [false, true] {
            let (mut app, commands) = app("cat cat");
            set_cursor(&mut app, cursor(3, 0));
            if applying {
                preview(&mut app, scope);
                app.replace.action =
                    Some(Action::Apply(admitted(&app, input_pass(&app.editor_ctx))));
            } else {
                app.request_replace_preview(
                    &app.editor_ctx.clone(),
                    scope,
                    admitted(&app, input_pass(&app.editor_ctx)),
                );
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
