//! Find and return journeys through complete native raw-hook/update frames.
//! Fixtures may seed a draft and its selection before the measured journey;
//! query, provider, result, preview, and focus changes use rendered controls.
use super::*;
use crate::workspace_access_tests::{recorded_clip, recorded_rect, recorded_response};
use egui::text::{CCursor, CCursorRange};

const SIZE: egui::Vec2 = egui::vec2(1100.0, 800.0);
const SAVED: &str = "saved Ω😀\r\nneedle second\nlast\n";
const DRAFT: &str = "dirty Ω😀\r\nneedle second\nlast\n";
const DISPATCH_KEYS: &str = "browse_native_dispatch_keys";
type KeyMetadata = (egui::Key, Option<egui::Key>, bool, bool, egui::Modifiers);

fn key_metadata(events: &[egui::Event]) -> Vec<KeyMetadata> {
    events
        .iter()
        .filter_map(|event| match event {
            egui::Event::Key {
                key,
                physical_key,
                pressed,
                repeat,
                modifiers,
            } => Some((*key, *physical_key, *pressed, *repeat, *modifiers)),
            _ => None,
        })
        .collect()
}

// Read-only production observer: preserve key metadata/order, never text or
// paste/IME bodies and never manufactured input ownership.
pub(super) fn record_dispatch_keys(ctx: &egui::Context) {
    let keys = ctx.input(|input| key_metadata(&input.events));
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(DISPATCH_KEYS), keys));
}

fn dispatch_keys(app: &CedarApp) -> Vec<KeyMetadata> {
    app.editor_ctx
        .data(|data| data.get_temp(egui::Id::new(DISPATCH_KEYS)))
        .expect("production Browse dispatcher did not record its residual keys")
}

fn frame_at_size(
    app: &mut CedarApp,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut input = egui::RawInput {
        time: Some(ctx.input(|input| input.time) + 0.1),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let mut native = eframe::Frame::_new_kittest();
    let output = ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native));
    assert!(output
        .shapes
        .iter()
        .all(|shape| shape.clip_rect.is_finite()));
    output
}

fn frame(app: &mut CedarApp, events: Vec<egui::Event>) -> egui::FullOutput {
    frame_at_size(app, SIZE, events)
}

fn settle(app: &mut CedarApp) {
    for _ in 0..3 {
        frame(app, vec![]);
    }
}

fn command_modifiers() -> egui::Modifiers {
    egui::Modifiers {
        ctrl: true,
        command: true,
        ..egui::Modifiers::NONE
    }
}

fn edge(key: egui::Key, pressed: bool, repeat: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat,
        modifiers: egui::Modifiers::NONE,
    }
}

fn modified_key(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
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

fn key(key: egui::Key) -> Vec<egui::Event> {
    modified_key(key, egui::Modifiers::NONE)
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

fn visible_center(app: &CedarApp, probe: &str) -> egui::Pos2 {
    let rect = recorded_rect(app, probe).intersect(recorded_clip(app, probe));
    assert!(rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0);
    rect.center()
}

fn click(app: &mut CedarApp, probe: &str) {
    let pos = visible_center(app, probe);
    let id = recorded_response(app, probe).id;
    let rect = recorded_rect(app, probe);
    let clip = recorded_clip(app, probe);
    // Arrive before pressing: coalescing a large pointer delta with down can
    // begin a native ScrollArea drag instead of clicking the observed row.
    frame(app, vec![egui::Event::PointerMoved(pos)]);
    assert_eq!(
        recorded_rect(app, probe),
        rect,
        "{probe}: hover changed geometry"
    );
    assert_eq!(
        recorded_response(app, probe).id,
        id,
        "{probe}: hover changed identity"
    );
    // Area size and ScrollArea viewport height can converge after the previous
    // frame publishes results or preview. The exact same point must remain
    // inside this unchanged target in both clips; never choose a new point.
    assert!(rect.intersect(clip).contains(pos));
    assert!(rect.intersect(recorded_clip(app, probe)).contains(pos));
    frame(app, pointer(pos, true));
    assert_eq!(
        app.editor_ctx.input(|input| input.pointer.delta()),
        egui::Vec2::ZERO
    );
    assert!(
        recorded_response(app, probe).is_pointer_button_down_on(),
        "{probe}: native press did not reach its current rendered rectangle at {pos:?}"
    );
    frame(app, pointer(pos, false));
    assert!(
        recorded_response(app, probe).clicked_by(egui::PointerButton::Primary),
        "{probe}: native release did not click the rendered control at {pos:?}"
    );
}

fn focused(app: &CedarApp) -> Option<egui::Id> {
    app.editor_ctx.memory(|memory| memory.focused())
}

fn selection(app: &CedarApp) -> CCursorRange {
    egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1_u64)))
        .unwrap()
        .cursor
        .char_range()
        .unwrap()
}

fn selection_signature(range: CCursorRange) -> (usize, usize, bool, bool) {
    (
        range.primary.index,
        range.secondary.index,
        range.primary.prefer_next_row,
        range.secondary.prefer_next_row,
    )
}

fn undo_texts(app: &CedarApp) -> Vec<String> {
    let state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1_u64))).unwrap();
    let mut undoer = state.undoer();
    let mut current = (
        state.cursor.char_range().unwrap(),
        app.documents[0].text.clone(),
    );
    let mut texts = vec![current.1.clone()];
    for _ in 0..editor_state::MAX_UNDO_STATES + 1 {
        let Some(previous) = undoer.undo(&current).cloned() else {
            break;
        };
        current = previous;
        if texts.last() != Some(&current.1) {
            texts.push(current.1.clone());
        }
    }
    texts
}

fn reply(app: &mut CedarApp, id: u64, result: Result<Payload, String>) {
    app.apply_event(Event {
        generation: app.generation,
        id,
        connected: true,
        result,
    });
}

fn file(path: &str, text: &str) -> Payload {
    Payload::File {
        path: path.into(),
        text: text.into(),
        revision: "b".repeat(64),
    }
}

fn fixture() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        local_root: "/workspace".into(),
        allow_run: false,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/workspace".into();
    app.agent_info = Some(agent_support::full_test_agent());
    app.state = ConnectionState::Ready;
    app.open_form = false;
    app.documents.push(Document::new(
        1,
        "origin.txt".into(),
        SAVED.into(),
        "a".repeat(64),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.list(String::new());
    let list = commands.try_recv().unwrap();
    assert!(matches!(list.op, Operation::List { .. }));
    reply(
        &mut app,
        list.id,
        Ok(Payload::Entries {
            entries: ["a.txt", "b.txt", "c.txt", "origin.txt"]
                .into_iter()
                .map(|path| Entry {
                    name: path.into(),
                    path: path.into(),
                    is_dir: false,
                })
                .collect(),
        }),
    );
    seed_dirty_origin(&mut app);
    assert!(commands.try_recv().is_err());
    (app, commands)
}

fn seed_dirty_origin(app: &mut CedarApp) {
    settle(app);
    click(app, "editor");
    assert_eq!(focused(app), Some(egui::Id::new(("editor", 1_u64))));
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 8);
    let mut range = CCursorRange::two(CCursor::new(1), CCursor::new(8));
    range.primary.prefer_next_row = false;
    range.secondary.prefer_next_row = true;
    let mut state = editor_state::load(&app.editor_ctx, &mut app.documents[0]);
    state.cursor.set_char_range(Some(range));
    state.store(&app.editor_ctx, egui::Id::new(("editor", 1_u64)));
    app.documents[0].cursor = model::cursor_location(DRAFT, 8);
    settle(app);
    assert_eq!(undo_texts(app), vec![DRAFT.to_owned(), SAVED.to_owned()]);
}

fn enter(app: &mut CedarApp, text: bool) {
    let modifiers = egui::Modifiers {
        shift: true,
        ..command_modifiers()
    };
    frame(
        app,
        modified_key(if text { egui::Key::F } else { egui::Key::N }, modifiers),
    );
    settle(app);
    assert_eq!(
        focused(app),
        Some(recorded_response(app, "browse_query").id)
    );
}

fn query(app: &mut CedarApp, value: &str) {
    click(app, "browse_query");
    frame(app, modified_key(egui::Key::A, command_modifiers()));
    frame(app, vec![egui::Event::Text(value.into())]);
}

fn submit_text(app: &mut CedarApp, commands: &Receiver<Command>, paths: &[&str]) {
    query(app, "needle");
    assert!(
        commands.try_recv().is_err(),
        "query typing must not perform I/O"
    );
    frame(app, key(egui::Key::Enter));
    let search = commands.try_recv().unwrap();
    assert!(matches!(&search.op, Operation::Search { query, limit: 500 } if query == "needle"));
    assert!(commands.try_recv().is_err());
    reply(
        app,
        search.id,
        Ok(Payload::Matches {
            matches: paths
                .iter()
                .map(|path| SearchMatch {
                    path: (*path).into(),
                    line: 2,
                    text: "needle historical disk match".into(),
                })
                .collect(),
            truncated: false,
        }),
    );
    settle(app);
}

fn has_text(output: &egui::FullOutput, expected: &str) -> bool {
    fn contains(shape: &egui::epaint::Shape, expected: &str) -> bool {
        match shape {
            egui::epaint::Shape::Text(text) => text.galley.job.text.contains(expected),
            egui::epaint::Shape::Vec(shapes) => {
                shapes.iter().any(|shape| contains(shape, expected))
            }
            _ => false,
        }
    }
    output
        .shapes
        .iter()
        .any(|shape| contains(&shape.shape, expected))
}

fn assert_draft(app: &CedarApp, expected: (usize, usize, bool, bool), undo: &[String]) {
    assert_draft_revision(app, expected, undo, &"a".repeat(64));
}

fn assert_draft_revision(
    app: &CedarApp,
    expected: (usize, usize, bool, bool),
    undo: &[String],
    revision: &str,
) {
    assert_eq!(app.documents[0].text, DRAFT);
    assert_eq!(app.documents[0].saved_text, SAVED);
    assert_eq!(app.documents[0].revision.as_deref(), Some(revision));
    assert!(app.documents[0].dirty());
    assert_eq!(selection_signature(selection(app)), expected);
    assert_eq!(undo_texts(app).as_slice(), undo);
}

#[test]
fn browse_files_preview_preserves_dirty_selection_undo_and_exact_escape_focus() {
    for use_button in [false, true] {
        let (mut app, commands) = fixture();
        let departure = selection_signature(selection(&app));
        let undo = undo_texts(&app);
        enter(&mut app, false);
        let epoch = app.navigation_epoch;
        assert!(has_text(&frame(&mut app, vec![]), "Open and loaded files"));
        query(&mut app, "origin");
        frame(&mut app, key(egui::Key::Enter));
        assert!(commands.try_recv().is_err());
        settle(&mut app);
        click(&mut app, "browse_row:0");
        let row = recorded_response(&app, "browse_row:0").id;
        assert_eq!(focused(&app), Some(row));
        assert_eq!(app.navigation_epoch, epoch, "single click only selects");
        assert_draft(&app, departure, &undo);
        if use_button {
            click(&mut app, "browse_preview");
        } else {
            let mut space = key(egui::Key::Space);
            space.insert(1, egui::Event::Text(" ".into()));
            frame(&mut app, space);
        }
        settle(&mut app);
        assert!(has_text(&frame(&mut app, vec![]), "dirty Ω😀"));
        assert!(
            commands.try_recv().is_err(),
            "a draft preview must issue zero Reads"
        );
        assert_eq!(app.documents.len(), 1);
        assert_eq!(app.navigation_epoch, epoch);
        assert_draft(&app, departure, &undo);
        frame(&mut app, key(egui::Key::Escape));
        settle(&mut app);
        assert_eq!(
            focused(&app),
            Some(row),
            "preview Escape returns to the exact result"
        );
        if use_button {
            frame(&mut app, key(egui::Key::Enter));
            assert!(
                app.location_history.back.is_empty(),
                "filename-only reopen of the same location adds no phantom history"
            );
            assert!(
                commands.try_recv().is_err(),
                "filename-only dirty reopen issues zero Reads"
            );
        } else {
            frame(&mut app, key(egui::Key::Escape));
        }
        settle(&mut app);
        assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1_u64))));
        assert_draft(&app, departure, &undo);
        frame(&mut app, modified_key(egui::Key::Z, command_modifiers()));
        assert_eq!(
            app.documents[0].text, SAVED,
            "one Undo still reverses the original edit"
        );
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn browse_provider_switch_query_submit_result_open_and_back_are_distinct_actions() {
    let (mut app, commands) = fixture();
    let departure = selection_signature(selection(&app));
    let undo = undo_texts(&app);
    enter(&mut app, false);
    let epoch = app.navigation_epoch;
    click(&mut app, "browse_text");
    settle(&mut app);
    submit_text(&mut app, &commands, &["origin.txt"]);
    assert_eq!(app.navigation_epoch, epoch, "query Enter must only submit");
    assert_draft(&app, departure, &undo);
    // A clean Down from the query is the direct native route to the first row.
    frame(&mut app, key(egui::Key::ArrowDown));
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "browse_row:0").id)
    );
    frame(&mut app, key(egui::Key::Space));
    settle(&mut app);
    assert!(commands.try_recv().is_err());
    assert_draft(&app, departure, &undo);
    frame(&mut app, key(egui::Key::Escape));
    frame(&mut app, key(egui::Key::Enter));
    settle(&mut app);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents[0].cursor, (2, 1));
    assert_eq!(app.location_history.back.len(), 1);
    assert_eq!(app.navigation_epoch, epoch + 1);
    assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1_u64))));
    assert!(
        commands.try_recv().is_err(),
        "opening an existing draft must issue zero Reads"
    );
    frame(
        &mut app,
        modified_key(egui::Key::OpenBracket, command_modifiers()),
    );
    settle(&mut app);
    assert_draft(&app, departure, &undo);
    assert!(app.location_history.back.is_empty());
    assert_eq!(app.location_history.forward.len(), 1);
    assert!(commands.try_recv().is_err());
}

#[test]
fn browse_unopened_preview_and_ordinary_open_have_separate_read_ledgers() {
    let (mut app, commands) = fixture();
    let departure = selection_signature(selection(&app));
    let undo = undo_texts(&app);
    enter(&mut app, true);
    submit_text(&mut app, &commands, &["a.txt"]);
    click(&mut app, "browse_row:0");
    assert!(
        commands.try_recv().is_err(),
        "selection and hover never Read"
    );
    frame(&mut app, vec![edge(egui::Key::Space, true, false)]);
    let preview = commands.try_recv().unwrap();
    assert!(matches!(&preview.op, Operation::Read { path } if path == "a.txt"));
    for repeat in [false, true] {
        frame(&mut app, vec![edge(egui::Key::Space, true, repeat)]);
        assert!(
            commands.try_recv().is_err(),
            "a held preview key must not replay"
        );
    }
    frame(&mut app, vec![edge(egui::Key::Space, false, false)]);
    reply(
        &mut app,
        preview.id,
        Ok(file("a.txt", "preview first\npreview needle\n")),
    );
    settle(&mut app);
    assert_eq!(app.documents.len(), 1, "preview is not an editable tab");
    assert_eq!(app.active_document, Some(1));
    assert_draft(&app, departure, &undo);
    assert!(has_text(&frame(&mut app, vec![]), "preview first"));
    frame(&mut app, key(egui::Key::Escape));
    frame(&mut app, vec![edge(egui::Key::Enter, true, false)]);
    let open = commands.try_recv().unwrap();
    assert_ne!(open.id, preview.id);
    assert!(matches!(&open.op, Operation::Read { path } if path == "a.txt"));
    frame(&mut app, vec![edge(egui::Key::Enter, true, true)]);
    frame(&mut app, vec![edge(egui::Key::Enter, false, false)]);
    assert!(commands.try_recv().is_err());
    reply(
        &mut app,
        open.id,
        Ok(file("a.txt", "opened first\ncurrent needle\n")),
    );
    settle(&mut app);
    assert_eq!(app.active().unwrap().path, "a.txt");
    assert_eq!(app.active().unwrap().text, "opened first\ncurrent needle\n");
    assert_eq!(app.active().unwrap().cursor, (2, 1));
    assert_draft(&app, departure, &undo);
    frame(
        &mut app,
        modified_key(egui::Key::OpenBracket, command_modifiers()),
    );
    settle(&mut app);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1_u64))));
    assert_draft(&app, departure, &undo);
    assert!(commands.try_recv().is_err());
}

#[test]
fn browse_preview_coalesces_one_replacement_and_ignores_superseded_success_or_error() {
    for stale in [
        Ok(file("a.txt", "superseded preview")),
        Err("superseded failure".into()),
    ] {
        let (mut app, commands) = fixture();
        let departure = selection_signature(selection(&app));
        let undo = undo_texts(&app);
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["a.txt", "b.txt", "c.txt"]);
        click(&mut app, "browse_row:0");
        frame(&mut app, key(egui::Key::Space));
        let first = commands.try_recv().unwrap();
        assert!(matches!(&first.op, Operation::Read { path } if path == "a.txt"));
        for index in [1, 2] {
            click(&mut app, &format!("browse_row:{index}"));
            frame(&mut app, key(egui::Key::Space));
            assert!(
                commands.try_recv().is_err(),
                "only one preview Read may be in flight"
            );
        }
        reply(&mut app, first.id, stale);
        settle(&mut app);
        let replacement = commands.try_recv().unwrap();
        assert!(matches!(&replacement.op, Operation::Read { path } if path == "c.txt"));
        assert!(
            commands.try_recv().is_err(),
            "intermediate selections must not queue Reads"
        );
        reply(
            &mut app,
            replacement.id,
            Ok(file("c.txt", "latest selected preview")),
        );
        settle(&mut app);
        let output = frame(&mut app, vec![]);
        assert!(has_text(&output, "latest selected preview"));
        assert!(!has_text(&output, "superseded preview"));
        assert!(!has_text(&output, "superseded failure"));
        assert_eq!(app.documents.len(), 1);
        assert_draft(&app, departure, &undo);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn browse_original_mixed_text_paste_ime_and_pointer_batches_never_activate_rows() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        for companion in [
            egui::Event::Text("x".into()),
            egui::Event::Paste("x".into()),
            egui::Event::Ime(egui::ImeEvent::Preedit("compose".into())),
            egui::Event::Ime(egui::ImeEvent::Commit("x".into())),
            egui::Event::PointerMoved(egui::pos2(4.0, 4.0)),
        ] {
            let (mut app, commands) = fixture();
            let departure = selection_signature(selection(&app));
            let undo = undo_texts(&app);
            enter(&mut app, true);
            submit_text(&mut app, &commands, &["a.txt"]);
            click(&mut app, "browse_row:0");
            let mut events = key(activation);
            events.insert(1, companion);
            frame(&mut app, events);
            assert!(commands.try_recv().is_err());
            assert!(!app.browse.preview_visible());
            assert_eq!(app.documents.len(), 1);
            assert!(app.location_history.back.is_empty());
            assert_draft(&app, departure, &undo);
        }
    }
    // A composition that began in an earlier frame also vetoes a clean key.
    let (mut app, commands) = fixture();
    enter(&mut app, true);
    submit_text(&mut app, &commands, &["a.txt"]);
    click(&mut app, "browse_row:0");
    frame(
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Preedit(String::new()))],
    );
    frame(&mut app, key(egui::Key::Space));
    assert!(commands.try_recv().is_err());
    frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
    frame(&mut app, key(egui::Key::Space));
    assert!(matches!(
        commands.try_recv().unwrap().op,
        Operation::Read { .. }
    ));
    assert!(commands.try_recv().is_err());

    // A native double-click is still not owned by Browse when the original
    // release batch contains text, paste, or composition input.
    for companion in [
        egui::Event::Text("x".into()),
        egui::Event::Paste("x".into()),
        egui::Event::Ime(egui::ImeEvent::Preedit("compose".into())),
    ] {
        let (mut app, commands) = fixture();
        let departure = selection_signature(selection(&app));
        let undo = undo_texts(&app);
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["a.txt"]);
        click(&mut app, "browse_row:0");
        let pos = visible_center(&app, "browse_row:0");
        frame(&mut app, pointer(pos, true));
        assert!(recorded_response(&app, "browse_row:0").is_pointer_button_down_on());
        let mut release = pointer(pos, false);
        release.push(companion);
        frame(&mut app, release);
        assert!(
            recorded_response(&app, "browse_row:0").double_clicked_by(egui::PointerButton::Primary)
        );
        assert!(
            commands.try_recv().is_err(),
            "mixed native release cannot become Browse activation"
        );
        assert!(!app.browse.preview_visible());
        assert_eq!(app.documents.len(), 1);
        assert!(app.location_history.back.is_empty());
        assert_draft(&app, departure, &undo);
    }
}

#[test]
fn browse_preview_rejects_unreadable_mismatched_binary_oversized_and_invalid_revisions() {
    for result in [
        Err("Permission denied for a.txt".into()),
        Ok(file("wrong.txt", "must never display")),
        Ok(file("a.txt", "binary\0must never display")),
        Ok(file(
            "a.txt",
            &"x".repeat(cedar_protocol::MAX_FILE_BYTES + 1),
        )),
        Ok(Payload::File {
            path: "a.txt".into(),
            text: "must never display".into(),
            revision: "invalid".into(),
        }),
        Ok(Payload::GitStatus {
            text: "wrong payload".into(),
        }),
    ] {
        let (mut app, commands) = fixture();
        let departure = selection_signature(selection(&app));
        let undo = undo_texts(&app);
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["a.txt"]);
        click(&mut app, "browse_row:0");
        click(&mut app, "browse_preview");
        let preview = commands.try_recv().unwrap();
        reply(&mut app, preview.id, result);
        settle(&mut app);
        assert!(app.browse.preview_visible());
        assert!(!app.browse.busy());
        assert!(app.browse.preview_text().is_none());
        let message = app
            .browse
            .preview_message()
            .expect("failure needs an explicit visible state")
            .to_owned();
        assert!(has_text(&frame(&mut app, vec![]), &message));
        assert_eq!(app.documents.len(), 1);
        assert_draft(&app, departure, &undo);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn browse_preview_reply_cannot_survive_query_provider_document_session_or_navigation_change() {
    for change in 0..6 {
        for failed in [false, true] {
            let (mut app, commands) = fixture();
            let departure = selection_signature(selection(&app));
            let undo = undo_texts(&app);
            enter(&mut app, true);
            submit_text(&mut app, &commands, &["a.txt"]);
            click(&mut app, "browse_row:0");
            frame(&mut app, key(egui::Key::Space));
            let preview = commands.try_recv().unwrap();
            let generation = app.generation;
            match change {
                0 => query(&mut app, "replacement query"),
                1 => click(&mut app, "browse_files"),
                2 => {
                    frame(&mut app, key(egui::Key::Escape));
                    frame(&mut app, key(egui::Key::Escape));
                }
                // These model transitions represent external lifecycle changes,
                // not injected input tickets or manufactured focus ownership.
                3 => app.documents[0].edit_version += 1,
                4 => app.generation += 1,
                _ => app.navigation_changed(),
            }
            app.apply_event(Event {
                generation,
                id: preview.id,
                connected: true,
                result: if failed {
                    Err("late preview failure".into())
                } else {
                    Ok(file("a.txt", "late preview bytes"))
                },
            });
            settle(&mut app);
            assert!(!app.browse.preview_visible(), "change {change}");
            assert!(app.browse.preview_text().is_none());
            assert!(app
                .browse
                .preview_message()
                .is_none_or(|text| !text.contains("late preview")));
            assert_eq!(app.active_document, Some(1));
            assert_eq!(app.documents.len(), 1);
            assert_draft(&app, departure, &undo);
            assert!(
                commands.try_recv().is_err(),
                "invalidation must not pump another Read"
            );
        }
    }
}

#[test]
fn browse_dismiss_and_reopen_keep_inflight_limit_and_late_reply_cannot_restore_old_view() {
    let (mut app, commands) = fixture();
    enter(&mut app, true);
    submit_text(&mut app, &commands, &["a.txt"]);
    click(&mut app, "browse_row:0");
    frame(&mut app, key(egui::Key::Space));
    let first = commands.try_recv().unwrap();
    frame(&mut app, key(egui::Key::Escape));
    frame(&mut app, key(egui::Key::Escape));
    settle(&mut app);
    enter(&mut app, false);
    query(&mut app, "b.txt");
    frame(&mut app, key(egui::Key::Enter));
    settle(&mut app);
    click(&mut app, "browse_row:0");
    frame(&mut app, key(egui::Key::Space));
    assert!(
        commands.try_recv().is_err(),
        "closing the view cannot free the in-flight Read slot"
    );
    reply(
        &mut app,
        first.id,
        Ok(file("a.txt", "old dismissed preview")),
    );
    settle(&mut app);
    let replacement = commands.try_recv().unwrap();
    assert!(matches!(&replacement.op, Operation::Read { path } if path == "b.txt"));
    reply(
        &mut app,
        replacement.id,
        Ok(file("b.txt", "new view preview")),
    );
    settle(&mut app);
    assert_eq!(app.browse.preview_text(), Some("new view preview"));
    assert_eq!(app.browse.query(), "b.txt");
    assert_eq!(app.documents.len(), 1);
    assert!(commands.try_recv().is_err());
}

#[test]
fn browse_narrow_viewport_keeps_selected_row_and_preview_visible_after_resize() {
    let (mut app, commands) = fixture();
    enter(&mut app, true);
    submit_text(&mut app, &commands, &vec!["origin.txt"; 80]);
    assert_eq!(
        app.browse.paths().len(),
        80,
        "Project Text retains its 500-row provider bound"
    );
    let last = app.browse.paths().len() - 1;
    click(&mut app, "browse_row:0");
    let size = egui::vec2(700.0, 480.0);
    frame_at_size(&mut app, size, vec![]);
    frame_at_size(&mut app, size, key(egui::Key::End));
    // The pinned egui ScrollArea applies the requested reveal next frame.
    for _ in 0..3 {
        frame_at_size(&mut app, size, vec![]);
    }
    assert_eq!(app.browse.selected_index(), Some(last));
    let last_row = format!("browse_row:{last}");
    let rect = recorded_rect(&app, &last_row);
    let clip = recorded_clip(&app, &last_row);
    assert!(
        clip.contains_rect(rect),
        "selected row {rect:?} outside {clip:?}"
    );
    assert_eq!(focused(&app), Some(recorded_response(&app, &last_row).id));
    frame_at_size(&mut app, size, key(egui::Key::Space));
    for _ in 0..3 {
        frame_at_size(&mut app, size, vec![]);
    }
    let preview = recorded_rect(&app, "browse_preview_content")
        .intersect(recorded_clip(&app, "browse_preview_content"));
    assert!(preview.width() > 0.0 && preview.height() > 0.0);
    assert!(commands.try_recv().is_err());
    frame_at_size(&mut app, size, key(egui::Key::Escape));
    assert_eq!(focused(&app), Some(recorded_response(&app, &last_row).id));
}

#[test]
fn browse_query_keeps_focus_across_character_frames_and_native_tab_reaches_results() {
    let (mut app, commands) = fixture();
    enter(&mut app, false);
    let query_id = recorded_response(&app, "browse_query").id;
    let mut typed = String::new();
    for character in ["o", "r", "i", "g", "i", "n"] {
        typed.push_str(character);
        frame(&mut app, vec![egui::Event::Text(character.into())]);
        assert_eq!(app.browse.query(), typed);
        assert_eq!(focused(&app), Some(query_id));
        assert!(commands.try_recv().is_err());
    }
    frame(&mut app, key(egui::Key::Enter));
    assert_eq!(app.browse.paths(), vec!["origin.txt"]);
    assert_eq!(focused(&app), Some(query_id));
    frame(&mut app, key(egui::Key::Tab));
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "browse_submit").id)
    );
    frame(&mut app, key(egui::Key::Tab));
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "browse_row:0").id)
    );
    frame(&mut app, key(egui::Key::Space));
    assert!(app.browse.preview_visible());
    assert!(commands.try_recv().is_err());
}

#[test]
fn browse_queued_search_success_and_error_yield_to_same_frame_query_provider_or_escape() {
    for change in 0..3 {
        for failed in [false, true] {
            let (mut app, commands) = fixture();
            enter(&mut app, true);
            submit_text(&mut app, &commands, &["origin.txt"]);
            let accepted = app.search_state.accepted.clone();
            query(&mut app, "next");
            frame(&mut app, key(egui::Key::Enter));
            let pending = commands.try_recv().unwrap();
            assert!(matches!(&pending.op, Operation::Search { query, .. } if query == "next"));
            let events = match change {
                0 => vec![egui::Event::Text("x".into())],
                1 => {
                    // Search submission replaces the result list with its
                    // pending state. Measure the provider's new rectangle.
                    settle(&mut app);
                    let pos = visible_center(&app, "browse_files");
                    frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
                    frame(&mut app, pointer(pos, true));
                    assert!(recorded_response(&app, "browse_files").is_pointer_button_down_on());
                    pointer(pos, false)
                }
                _ => key(egui::Key::Escape),
            };
            app.error = Some("current error must survive".into());
            app.notice = "current notice must survive".into();
            app.result_tx
                .send(WorkerEvent::Response(Event {
                    generation: app.generation,
                    id: pending.id,
                    connected: true,
                    result: if failed {
                        Err("superseded search failure".into())
                    } else {
                        Ok(Payload::Matches {
                            matches: vec![SearchMatch {
                                path: "obsolete.txt".into(),
                                line: 2,
                                text: "superseded search result".into(),
                            }],
                            truncated: false,
                        })
                    },
                }))
                .unwrap();
            frame(&mut app, events);
            if change == 1 {
                assert!(recorded_response(&app, "browse_files")
                    .clicked_by(egui::PointerButton::Primary));
                assert!(
                    app.browse.paths().contains(&"a.txt"),
                    "provider release must actually switch to Files"
                );
            }
            settle(&mut app);
            assert_eq!(
                app.search_state.accepted, accepted,
                "change {change}, error {failed}"
            );
            assert_eq!(app.search_results[0].path, "origin.txt");
            assert_eq!(app.error.as_deref(), Some("current error must survive"));
            assert_eq!(app.notice, "current notice must survive");
            assert!(!app.browse.preview_visible());
            assert_eq!(app.active_document, Some(1));
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn browse_exhausted_request_counter_refuses_preview_and_search_without_dispatch() {
    for exhausted in [0, u64::MAX] {
        for text in [false, true] {
            let (mut app, commands) = fixture();
            let departure = selection_signature(selection(&app));
            let undo = undo_texts(&app);
            app.next_request = exhausted;
            enter(&mut app, text);
            query(&mut app, if text { "needle" } else { "a.txt" });
            frame(&mut app, key(egui::Key::Enter));
            if !text {
                settle(&mut app);
                click(&mut app, "browse_row:0");
                click(&mut app, "browse_preview");
            }
            settle(&mut app);
            assert!(
                commands.try_recv().is_err(),
                "exhausted identities must not be reused"
            );
            assert!(!app.browse.busy());
            let message = app
                .browse
                .preview_message()
                .expect("counter exhaustion needs an explicit refusal")
                .to_owned();
            assert!(!message.is_empty());
            assert!(has_text(&frame(&mut app, vec![]), &message));
            assert_draft(&app, departure, &undo);
        }
    }
}

#[test]
fn browse_exhausted_view_identity_refuses_native_submit_before_dispatch() {
    let (mut app, commands) = fixture();
    let departure = selection_signature(selection(&app));
    let undo = undo_texts(&app);
    // Entry retires once, then the one native edit retires once more. The
    // measured submit must refuse to wrap MAX into a previously used identity.
    app.browse.set_epoch_for_test(u64::MAX - 2);
    enter(&mut app, true);
    frame(&mut app, vec![egui::Event::Text("needle".into())]);
    assert_eq!(app.browse.query(), "needle");
    let epoch = app.navigation_epoch;
    frame(&mut app, key(egui::Key::Enter));
    settle(&mut app);
    assert!(
        commands.try_recv().is_err(),
        "retirement exhaustion must precede Search dispatch"
    );
    assert!(!app.browse_marker_open());
    assert!(!app.browse.busy());
    let message = app
        .error
        .clone()
        .expect("identity exhaustion needs an explicit refusal");
    assert!(has_text(&frame(&mut app, vec![]), &message));
    assert_eq!(app.navigation_epoch, epoch);
    assert!(app.location_history.back.is_empty());
    assert!(app.location_history.forward.is_empty());
    assert_draft(&app, departure, &undo);
}

#[test]
fn browse_double_click_opens_current_row_and_replaced_batch_cannot_retarget_release() {
    for replace_between_edges in [false, true] {
        let (mut app, commands) = fixture();
        let departure = selection_signature(selection(&app));
        let undo = undo_texts(&app);
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["a.txt"]);
        click(&mut app, "browse_row:0");
        assert!(commands.try_recv().is_err(), "the first click only selects");
        let row_id = recorded_response(&app, "browse_row:0").id;
        let pos = visible_center(&app, "browse_row:0");
        // The pointer has already arrived. Two 0.1-second frames put this
        // release 0.2 seconds after the first, inside pinned egui's threshold.
        frame(&mut app, pointer(pos, true));
        assert_eq!(recorded_response(&app, "browse_row:0").id, row_id);
        assert!(recorded_response(&app, "browse_row:0").is_pointer_button_down_on());
        assert_eq!(
            app.editor_ctx.input(|input| input.pointer.delta()),
            egui::Vec2::ZERO
        );
        if replace_between_edges {
            // A genuine new provider response changes row authority while the
            // button is held. Do not inject or rewrite a captured pointer owner.
            app.search();
            let search = commands.try_recv().unwrap();
            assert!(matches!(search.op, Operation::Search { .. }));
            reply(
                &mut app,
                search.id,
                Ok(Payload::Matches {
                    matches: vec![SearchMatch {
                        path: "b.txt".into(),
                        line: 2,
                        text: "replacement provider row".into(),
                    }],
                    truncated: false,
                }),
            );
        }
        frame(&mut app, pointer(pos, false));
        if replace_between_edges {
            assert!(
                commands.try_recv().is_err(),
                "old double-click release must not open replacement b.txt"
            );
            assert_eq!(app.active_document, Some(1));
            assert_eq!(app.documents.len(), 1);
            assert!(app.location_history.back.is_empty());
            assert_draft(&app, departure, &undo);
        } else {
            assert!(recorded_response(&app, "browse_row:0")
                .double_clicked_by(egui::PointerButton::Primary));
            let open = commands.try_recv().unwrap();
            assert!(matches!(&open.op, Operation::Read { path } if path == "a.txt"));
            assert!(commands.try_recv().is_err());
            reply(
                &mut app,
                open.id,
                Ok(file("a.txt", "first\nopened needle\n")),
            );
            settle(&mut app);
            assert_eq!(app.active().unwrap().path, "a.txt");
            assert_eq!(app.active().unwrap().cursor, (2, 1));
            frame(
                &mut app,
                modified_key(egui::Key::OpenBracket, command_modifiers()),
            );
            settle(&mut app);
            assert_eq!(app.active_document, Some(1));
            assert_draft(&app, departure, &undo);
            assert!(commands.try_recv().is_err());
        }
    }
}

fn discovery_fixture() -> (CedarApp, Receiver<Command>) {
    let (mut app, commands) = fixture();
    app.file_discovery = Some(cedar_protocol::FileDiscoveryDescriptor::V1);
    (app, commands)
}

fn discovery_result(
    query: &str,
    scope: &str,
    paths: &[&str],
    reasons: Vec<cedar_protocol::FileDiscoveryIncompleteReason>,
) -> Payload {
    Payload::FilePaths {
        result: cedar_protocol::FileDiscoveryResult {
            query: query.into(),
            scope: scope.into(),
            paths: paths.iter().map(|path| (*path).into()).collect(),
            incomplete: !reasons.is_empty(),
            reasons,
            examined_entries: paths.len() + 4,
        },
    }
}

fn expect_discovery(
    commands: &Receiver<Command>,
    expected_query: &str,
    expected_scope: &str,
) -> Command {
    let command = commands
        .try_recv()
        .expect("explicit submit must dispatch one FindFiles");
    assert!(
        matches!(&command.op, Operation::FindFiles { query, scope, limit }
        if query == expected_query && scope == expected_scope
            && *limit == cedar_protocol::DEFAULT_FILE_DISCOVERY_RESULTS)
    );
    assert!(commands.try_recv().is_err());
    command
}

#[test]
fn browse_project_files_discovers_untouched_path_then_previews_opens_and_returns() {
    let (mut app, commands) = discovery_fixture();
    let departure = selection_signature(selection(&app));
    let undo = undo_texts(&app);
    let target = "untouched/deep/Needle.rs";
    assert!(!app.entries.iter().any(|entry| entry.path == target));
    assert!(!app.execution_trusted());
    enter(&mut app, false);
    assert!(
        commands.try_recv().is_err(),
        "empty entry must not scan the project"
    );
    assert!(has_text(&frame(&mut app, vec![]), "Project files"));
    query(&mut app, "Needle.rs");
    assert!(
        commands.try_recv().is_err(),
        "typing must not traverse or Read"
    );
    frame(&mut app, key(egui::Key::Enter));
    let discovery = expect_discovery(&commands, "Needle.rs", "");
    reply(
        &mut app,
        discovery.id,
        Ok(discovery_result("Needle.rs", "", &[target], vec![])),
    );
    settle(&mut app);
    assert_eq!(app.browse.paths(), vec![target]);
    assert!(
        !app.entries.iter().any(|entry| entry.path == target),
        "discovery must not pretend to load Explorer folders"
    );
    click(&mut app, "browse_row:0");
    frame(&mut app, key(egui::Key::Space));
    let preview = commands.try_recv().unwrap();
    assert!(matches!(&preview.op, Operation::Read { path } if path == target));
    reply(&mut app, preview.id, Ok(file(target, "preview version\n")));
    settle(&mut app);
    assert_eq!(app.browse.preview_text(), Some("preview version\n"));
    assert_eq!(app.documents.len(), 1);
    assert_draft(&app, departure, &undo);
    frame(&mut app, key(egui::Key::Escape));
    frame(&mut app, key(egui::Key::Enter));
    let open = commands.try_recv().unwrap();
    assert_ne!(open.id, preview.id);
    assert!(matches!(&open.op, Operation::Read { path } if path == target));
    reply(
        &mut app,
        open.id,
        Ok(file(target, "ordinary Open version\n")),
    );
    settle(&mut app);
    assert_eq!(app.active().unwrap().path, target);
    assert_eq!(app.active().unwrap().text, "ordinary Open version\n");
    frame(
        &mut app,
        modified_key(egui::Key::OpenBracket, command_modifiers()),
    );
    settle(&mut app);
    assert_eq!(app.active_document, Some(1));
    assert_draft(&app, departure, &undo);
    assert!(!app.execution_trusted());
    assert!(commands.try_recv().is_err());
}

#[test]
fn browse_absent_or_future_discovery_descriptor_keeps_truthful_loaded_fallback_without_findfiles() {
    for descriptor in [
        None,
        Some(cedar_protocol::FileDiscoveryDescriptor { version: 2 }),
    ] {
        let (mut app, commands) = fixture();
        app.file_discovery = descriptor;
        app.agent_info
            .as_mut()
            .unwrap()
            .capabilities
            .push("find_files".into());
        enter(&mut app, false);
        assert!(has_text(&frame(&mut app, vec![]), "Open and loaded files"));
        query(&mut app, "origin");
        frame(&mut app, key(egui::Key::Enter));
        assert_eq!(app.browse.paths(), vec!["origin.txt"]);
        assert!(
            commands.try_recv().is_err(),
            "a flat claim or future descriptor must send zero FindFiles"
        );
        assert!(!app.execution_trusted());
    }
}

#[test]
fn browse_project_files_coalesces_explicit_queries_and_retires_superseded_success_or_error() {
    for failed in [false, true] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        query(&mut app, "alpha");
        frame(&mut app, key(egui::Key::Enter));
        let first = expect_discovery(&commands, "alpha", "");
        for value in ["bravo", "charlie"] {
            query(&mut app, value);
            frame(&mut app, key(egui::Key::Enter));
            assert!(
                commands.try_recv().is_err(),
                "only one FindFiles may be in flight"
            );
            assert!(
                app.browse.paths().is_empty(),
                "new intent retires old result activation immediately"
            );
        }
        app.error = Some("current error survives stale discovery".into());
        reply(
            &mut app,
            first.id,
            if failed {
                Err("superseded discovery failure".into())
            } else {
                Ok(discovery_result(
                    "alpha",
                    "",
                    &["untouched/alpha.rs"],
                    vec![],
                ))
            },
        );
        settle(&mut app);
        let latest = expect_discovery(&commands, "charlie", "");
        assert_eq!(
            app.error.as_deref(),
            Some("current error survives stale discovery")
        );
        assert!(app.browse.paths().is_empty());
        reply(
            &mut app,
            latest.id,
            Ok(discovery_result(
                "charlie",
                "",
                &["untouched/charlie.rs"],
                vec![],
            )),
        );
        settle(&mut app);
        assert_eq!(app.browse.paths(), vec!["untouched/charlie.rs"]);
        assert!(commands.try_recv().is_err());
        assert_eq!(app.documents.len(), 1);
    }
}

#[test]
fn browse_project_files_distinguishes_incomplete_empty_scan_from_complete_no_match() {
    for incomplete in [false, true] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        query(&mut app, "missing");
        frame(&mut app, key(egui::Key::Enter));
        let search = expect_discovery(&commands, "missing", "");
        let mut payload = discovery_result(
            "missing",
            "",
            &[],
            if incomplete {
                vec![cedar_protocol::FileDiscoveryIncompleteReason::EntryLimit]
            } else {
                vec![]
            },
        );
        if incomplete {
            let Payload::FilePaths { result } = &mut payload else {
                unreachable!()
            };
            result.examined_entries = cedar_protocol::MAX_FILE_DISCOVERY_ENTRIES;
        }
        reply(&mut app, search.id, Ok(payload));
        settle(&mut app);
        assert!(app.browse.paths().is_empty());
        let output = frame(&mut app, vec![]);
        if incomplete {
            assert!(has_text(&output, "Incomplete scan:"));
            assert!(has_text(
                &output,
                "No matches observed in an incomplete scan; more files may exist."
            ));
            assert!(!has_text(
                &output,
                "No matching project files in the searched scope and exclusions."
            ));
        } else {
            assert!(has_text(
                &output,
                "No matching project files in the searched scope and exclusions."
            ));
            assert!(!has_text(&output, "Incomplete scan:"));
        }
        assert!(commands.try_recv().is_err());
        assert_eq!(app.documents.len(), 1);
    }
}

fn scope(app: &mut CedarApp, value: &str) {
    click(app, "browse_scope");
    frame(app, modified_key(egui::Key::A, command_modifiers()));
    frame(app, vec![egui::Event::Text(value.into())]);
}

#[test]
fn browse_project_files_scope_validation_and_same_frame_scope_edit_retire_old_connection_results() {
    let (mut app, commands) = discovery_fixture();
    let departure = selection_signature(selection(&app));
    let undo = undo_texts(&app);
    enter(&mut app, false);
    query(&mut app, "needle");
    scope(&mut app, "../escape");
    assert!(
        commands.try_recv().is_err(),
        "scope and query typing cannot perform I/O"
    );
    frame(&mut app, key(egui::Key::Enter));
    assert!(
        commands.try_recv().is_err(),
        "invalid relative scope must be refused before dispatch"
    );
    assert!(app.browse.preview_message().is_some());
    scope(&mut app, "src");
    frame(&mut app, key(egui::Key::Enter));
    let old = expect_discovery(&commands, "needle", "src");
    click(&mut app, "browse_scope");
    frame(&mut app, modified_key(egui::Key::A, command_modifiers()));
    app.error = Some("current status survives obsolete files".into());
    app.result_tx
        .send(WorkerEvent::Response(Event {
            generation: app.generation,
            id: old.id,
            connected: true,
            result: Ok(discovery_result(
                "needle",
                "src",
                &["src/needle.rs"],
                vec![],
            )),
        }))
        .unwrap();
    frame(&mut app, vec![egui::Event::Text("other".into())]);
    settle(&mut app);
    assert_eq!(app.browse.scope(), "other");
    assert!(
        app.browse.paths().is_empty(),
        "scope edit wins over a reply queued for the same frame"
    );
    assert!(
        commands.try_recv().is_err(),
        "editing scope cannot enqueue an implicit replacement"
    );
    frame(&mut app, key(egui::Key::Enter));
    let current = expect_discovery(&commands, "needle", "other");
    let generation = app.generation;
    app.generation += 1;
    app.file_discovery = None;
    app.result_tx
        .send(WorkerEvent::Response(Event {
            generation,
            id: current.id,
            connected: true,
            result: Err("retired connection failure".into()),
        }))
        .unwrap();
    settle(&mut app);
    assert!(!app.browse_marker_open());
    assert!(app.browse.paths().is_empty());
    assert_eq!(
        app.error.as_deref(),
        Some("current status survives obsolete files")
    );
    assert!(commands.try_recv().is_err());
    assert_draft(&app, departure, &undo);
}

#[test]
fn browse_file_discovery_uses_first_hello_and_resets_support_after_disconnect() {
    let hello = |descriptor| Payload::Hello {
        protocol: cedar_protocol::PROTOCOL_VERSION,
        root: "/workspace".into(),
        agent: Some(agent_support::full_test_agent()),
        file_discovery: descriptor,
    };
    for replacement in [
        None,
        Some(cedar_protocol::FileDiscoveryDescriptor { version: 2 }),
    ] {
        let (mut app, commands) = fixture();
        let entries = app.entries.clone();
        app.state = ConnectionState::Connecting;
        app.connecting_form = app.active_form.clone();
        reply(
            &mut app,
            0,
            Ok(hello(Some(cedar_protocol::FileDiscoveryDescriptor::V1))),
        );
        let list = commands.try_recv().unwrap();
        assert!(matches!(&list.op, Operation::List { path } if path.is_empty()));
        reply(
            &mut app,
            list.id,
            Ok(Payload::Entries {
                entries: entries.clone(),
            }),
        );
        assert_eq!(
            app.file_discovery,
            Some(cedar_protocol::FileDiscoveryDescriptor::V1)
        );
        for duplicate in [
            None,
            Some(cedar_protocol::FileDiscoveryDescriptor { version: 2 }),
        ] {
            reply(&mut app, 0, Ok(hello(duplicate)));
            assert_eq!(
                app.file_discovery,
                Some(cedar_protocol::FileDiscoveryDescriptor::V1)
            );
            assert!(
                commands.try_recv().is_err(),
                "duplicate Hello cannot reinitialize a ready connection"
            );
        }
        enter(&mut app, false);
        query(&mut app, "needle");
        frame(&mut app, key(egui::Key::Enter));
        let discovery = expect_discovery(&commands, "needle", "");
        reply(
            &mut app,
            discovery.id,
            Ok(discovery_result("needle", "", &[], vec![])),
        );
        settle(&mut app);
        app.disconnected("fixture reconnect".into());
        assert_eq!(app.file_discovery, None);
        app.generation += 1;
        let (worker, commands) = Worker::recording();
        app.worker = Some(worker);
        app.state = ConnectionState::Connecting;
        app.connecting_form = Some(ConnectForm {
            local_root: "/workspace".into(),
            allow_run: false,
            ..Default::default()
        });
        reply(&mut app, 0, Ok(hello(replacement)));
        let list = commands.try_recv().unwrap();
        assert!(matches!(&list.op, Operation::List { path } if path.is_empty()));
        reply(&mut app, list.id, Ok(Payload::Entries { entries }));
        settle(&mut app);
        assert_eq!(app.file_discovery, replacement);
        enter(&mut app, false);
        query(&mut app, "origin");
        frame(&mut app, key(egui::Key::Enter));
        assert_eq!(app.browse.paths(), vec!["origin.txt"]);
        assert!(
            commands.try_recv().is_err(),
            "new absent/future Hello must keep FindFiles disabled"
        );
    }
}

#[test]
fn browse_project_files_terminal_error_and_refusal_clear_searching_status() {
    for result in [
        Err("discovery permission failure".into()),
        Ok(Payload::GitStatus {
            text: "unexpected payload".into(),
        }),
        Ok(discovery_result("wrong query", "", &[], vec![])),
    ] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        query(&mut app, "needle");
        frame(&mut app, key(egui::Key::Enter));
        let request = expect_discovery(&commands, "needle", "");
        assert!(has_text(&frame(&mut app, vec![]), "Finding project files"));
        reply(&mut app, request.id, result);
        settle(&mut app);
        let output = frame(&mut app, vec![]);
        assert!(
            !has_text(&output, "Finding project files"),
            "terminal outcomes must not leave a permanent busy label"
        );
        let message = app
            .browse
            .preview_message()
            .expect("terminal discovery failure needs an explicit state");
        assert!(has_text(&output, message));
        assert!(app.browse.paths().is_empty());
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn browse_project_files_narrow_scoped_incomplete_results_keep_last_row_and_preview_visible() {
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    query(&mut app, "needle");
    scope(&mut app, "src");
    frame(&mut app, key(egui::Key::Enter));
    let search = expect_discovery(&commands, "needle", "src");
    let paths: Vec<_> = (0..80)
        .map(|index| format!("src/deep/needle-{index:03}.rs"))
        .collect();
    let borrowed: Vec<_> = paths.iter().map(String::as_str).collect();
    reply(
        &mut app,
        search.id,
        Ok(discovery_result(
            "needle",
            "src",
            &borrowed,
            vec![cedar_protocol::FileDiscoveryIncompleteReason::TimeLimit],
        )),
    );
    settle(&mut app);
    assert_eq!(app.browse.paths(), borrowed);
    click(&mut app, "browse_row:0");
    let size = egui::vec2(700.0, 480.0);
    frame_at_size(&mut app, size, vec![]);
    frame_at_size(&mut app, size, key(egui::Key::End));
    for _ in 0..3 {
        frame_at_size(&mut app, size, vec![]);
    }
    let last = paths.len() - 1;
    assert_eq!(app.browse.selected_index(), Some(last));
    let probe = format!("browse_row:{last}");
    let rect = recorded_rect(&app, &probe);
    let clip = recorded_clip(&app, &probe);
    assert!(
        clip.contains_rect(rect),
        "scoped result {rect:?} outside {clip:?}"
    );
    let output = frame_at_size(&mut app, size, key(egui::Key::Space));
    assert!(has_text(&output, "Incomplete scan:"));
    let preview = commands.try_recv().unwrap();
    assert!(matches!(&preview.op, Operation::Read { path } if path == &paths[last]));
    reply(
        &mut app,
        preview.id,
        Ok(file(&paths[last], "visible preview\n")),
    );
    for _ in 0..3 {
        frame_at_size(&mut app, size, vec![]);
    }
    let content = recorded_rect(&app, "browse_preview_content");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    assert!(
        screen.contains_rect(content),
        "preview {content:?} is outside {screen:?}"
    );
    assert!(recorded_clip(&app, "browse_preview_content").contains_rect(content));
    frame_at_size(&mut app, size, key(egui::Key::Escape));
    assert_eq!(focused(&app), Some(recorded_response(&app, &probe).id));
    assert!(commands.try_recv().is_err());
}

#[test]
fn browse_accepted_cjk_paths_and_preview_bytes_trigger_existing_font_probe_flag() {
    for preview_bytes in [false, true] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        query(&mut app, "needle");
        frame(&mut app, key(egui::Key::Enter));
        let search = expect_discovery(&commands, "needle", "");
        let path = if preview_bytes {
            "untouched/needle.rs"
        } else {
            "未読/needle.rs"
        };
        assert!(!app.cjk_seen);
        reply(
            &mut app,
            search.id,
            Ok(discovery_result("needle", "", &[path], vec![])),
        );
        frame(&mut app, vec![]);
        if preview_bytes {
            settle(&mut app);
            click(&mut app, "browse_row:0");
            frame(&mut app, key(egui::Key::Space));
            let preview = commands.try_recv().unwrap();
            assert!(!app.cjk_seen);
            reply(&mut app, preview.id, Ok(file(path, "草稿 preview\n")));
            frame(&mut app, vec![]);
        }
        assert!(app.cjk_seen);
        assert!(
            app.system_fonts.needs_probe(),
            "the flag is asserted before any subsequent frame can start optional font probing"
        );
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn browse_windows_history_and_project_aliases_keep_original_mixed_and_repeat_ownership() {
    let (mut app, commands) = fixture();
    let departure = selection_signature(selection(&app));
    let undo = undo_texts(&app);
    enter(&mut app, true);
    submit_text(&mut app, &commands, &["origin.txt"]);
    frame(&mut app, key(egui::Key::ArrowDown));
    frame(&mut app, key(egui::Key::Enter));
    settle(&mut app);
    assert_eq!(app.documents[0].cursor, (2, 1));
    let destination = selection_signature(selection(&app));
    let epoch = app.navigation_epoch;
    let history_modifiers = egui::Modifiers {
        alt: true,
        ..command_modifiers()
    };
    let find_modifiers = egui::Modifiers {
        shift: true,
        ..command_modifiers()
    };
    let project_modifiers = egui::Modifiers {
        alt: true,
        ..egui::Modifiers::NONE
    };
    for mut events in [
        modified_key(egui::Key::ArrowLeft, history_modifiers),
        modified_key(egui::Key::Num1, project_modifiers),
    ] {
        events.extend(modified_key(egui::Key::N, find_modifiers));
        frame(&mut app, events);
        assert!(!app.browse_marker_open());
        assert_eq!(
            app.navigation_epoch, epoch,
            "filtering Browse must not manufacture a clean history shortcut"
        );
        assert_eq!(selection_signature(selection(&app)), destination);
        assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1_u64))));
        assert_eq!(app.location_history.back.len(), 1);
        assert!(commands.try_recv().is_err());
    }
    let mut back = modified_key(egui::Key::ArrowLeft, history_modifiers);
    let back_release = back.pop().unwrap();
    frame(&mut app, back.clone());
    settle(&mut app);
    assert_draft(&app, departure, &undo);
    let after_back = app.navigation_epoch;
    if let egui::Event::Key { repeat, .. } = &mut back[0] {
        *repeat = true;
    }
    frame(&mut app, back);
    assert_draft(&app, departure, &undo);
    assert_eq!(
        app.navigation_epoch, after_back,
        "native held repeat must not traverse again"
    );
    frame(&mut app, vec![back_release]);
    frame(
        &mut app,
        modified_key(egui::Key::ArrowRight, history_modifiers),
    );
    settle(&mut app);
    assert_eq!(selection_signature(selection(&app)), destination);
    frame(
        &mut app,
        modified_key(egui::Key::ArrowLeft, history_modifiers),
    );
    settle(&mut app);
    assert_draft(&app, departure, &undo);
    let mut project = modified_key(egui::Key::Num1, project_modifiers);
    let project_release = project.pop().unwrap();
    frame(&mut app, project.clone());
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "explorer_refresh").id)
    );
    click(&mut app, "editor");
    let editor_selection = selection_signature(selection(&app));
    if let egui::Event::Key { repeat, .. } = &mut project[0] {
        *repeat = true;
    }
    frame(&mut app, project);
    assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1_u64))));
    assert_draft(&app, editor_selection, &undo);
    frame(&mut app, vec![project_release]);
    assert!(commands.try_recv().is_err());
}

#[test]
fn browse_global_entry_rejects_repeat_held_and_ime_input_without_stealing_row_activation() {
    let modifiers = egui::Modifiers {
        shift: true,
        ..command_modifiers()
    };
    for veto in 0..3 {
        let (mut app, commands) = fixture();
        let undo = undo_texts(&app);
        if veto == 2 {
            // Empty native IME Preedit deletes a selected range. Collapse it
            // with a real key before measuring the persistent composition veto.
            frame(&mut app, key(egui::Key::ArrowRight));
            let collapsed = selection(&app);
            assert_eq!(collapsed.primary.index, collapsed.secondary.index);
            assert_eq!(app.documents[0].text, DRAFT);
            assert_eq!(undo_texts(&app), undo);
        }
        let mut shortcut = modified_key(egui::Key::N, modifiers);
        match veto {
            0 => {
                for event in &mut shortcut {
                    if let egui::Event::Key {
                        pressed: true,
                        repeat,
                        ..
                    } = event
                    {
                        *repeat = true;
                    }
                }
            }
            1 => {
                frame(&mut app, vec![edge(egui::Key::A, true, false)]);
            }
            _ => {
                frame(
                    &mut app,
                    vec![egui::Event::Ime(egui::ImeEvent::Preedit(String::new()))],
                );
                // Native empty-range deletion gives both endpoints next-row
                // affinity; Browse must preserve this completed IME state.
                assert_eq!(selection_signature(selection(&app)), (8, 8, true, true));
                assert_eq!(app.documents[0].text, DRAFT);
                assert_eq!(undo_texts(&app), undo);
            }
        }
        let departure = selection_signature(selection(&app));
        frame(&mut app, shortcut);
        assert!(!app.browse_marker_open(), "veto {veto}");
        assert_draft(&app, departure, &undo);
        if veto == 1 {
            frame(&mut app, vec![edge(egui::Key::A, false, false)]);
        }
        if veto == 2 {
            frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
        }
        frame(&mut app, modified_key(egui::Key::N, modifiers));
        settle(&mut app);
        assert!(app.browse_marker_open());
        query(&mut app, "origin");
        frame(&mut app, key(egui::Key::Enter));
        frame(&mut app, key(egui::Key::ArrowDown));
        let row = recorded_response(&app, "browse_row:0").id;
        assert_eq!(focused(&app), Some(row));
        let epoch = app.navigation_epoch;
        let mut mixed = modified_key(egui::Key::F, modifiers);
        mixed.extend(key(egui::Key::Enter));
        frame(&mut app, mixed);
        assert_eq!(app.navigation_epoch, epoch);
        assert_eq!(app.browse.query(), "origin");
        assert_eq!(focused(&app), Some(row));
        assert!(commands.try_recv().is_err());
        frame(&mut app, key(egui::Key::Space));
        assert!(app.browse.preview_visible());
        assert_draft(&app, departure, &undo);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn browse_fresh_entry_supersedes_ready_ordinary_open_before_capturing_departure() {
    for provider_key in [egui::Key::N, egui::Key::F] {
        let (mut app, commands) = fixture();
        let departure = selection_signature(selection(&app));
        let undo = undo_texts(&app);
        app.open("a.txt".into(), None);
        let open = commands.try_recv().unwrap();
        assert!(matches!(&open.op, Operation::Read { path } if path == "a.txt"));
        app.result_tx
            .send(WorkerEvent::Response(Event {
                generation: app.generation,
                id: open.id,
                connected: true,
                result: Ok(file("a.txt", "older ready source\n")),
            }))
            .unwrap();
        frame(
            &mut app,
            modified_key(
                provider_key,
                egui::Modifiers {
                    shift: true,
                    ..command_modifiers()
                },
            ),
        );
        settle(&mut app);
        assert!(app.browse_marker_open());
        assert_eq!(
            app.active_document,
            Some(1),
            "ready Open must not replace the origin of a newer Find entry"
        );
        assert!(app.location_history.back.is_empty());
        assert_eq!(
            focused(&app),
            Some(recorded_response(&app, "browse_query").id)
        );
        assert_draft(&app, departure, &undo);
        frame(&mut app, key(egui::Key::Escape));
        settle(&mut app);
        assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1_u64))));
        assert_draft(&app, departure, &undo);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn browse_global_key_consumption_preserves_fresh_same_key_edges_and_metadata_order() {
    let global = egui::Modifiers {
        shift: true,
        ..command_modifiers()
    };
    for fresh_modifiers in [egui::Modifiers::NONE, command_modifiers()] {
        let (mut app, commands) = fixture();
        let mut held = modified_key(egui::Key::N, global);
        held.pop();
        frame(&mut app, held);
        settle(&mut app);
        assert!(app.browse_marker_open());
        let epoch = app.navigation_epoch;
        let fresh = modified_key(egui::Key::N, fresh_modifiers);
        let mut release_repress = Vec::new();
        if fresh_modifiers == command_modifiers() {
            // The prior N remains held: egui normalizes this raw repeat=false
            // press to repeat=true even though its modifiers have changed.
            release_repress.push(modified_key(egui::Key::N, egui::Modifiers::NONE).remove(0));
        }
        release_repress.push(modified_key(egui::Key::N, global).pop().unwrap());
        release_repress.extend(fresh.clone());
        frame(&mut app, release_repress);
        assert_eq!(
            dispatch_keys(&app),
            key_metadata(&fresh),
            "owned release must not consume a fresh same-key lifetime"
        );
        assert!(app.browse_marker_open());
        assert_eq!(app.navigation_epoch, epoch);
        assert!(commands.try_recv().is_err());

        let (mut app, commands) = fixture();
        let epoch = app.navigation_epoch;
        let mut mixed = modified_key(egui::Key::N, global);
        mixed.extend(fresh.clone());
        frame(&mut app, mixed);
        assert_eq!(
            dispatch_keys(&app),
            key_metadata(&fresh),
            "a key-name mask must not erase differently modified same-key input"
        );
        assert!(!app.browse_marker_open());
        assert_eq!(app.navigation_epoch, epoch);
        assert!(app.location_history.back.is_empty());
        assert!(commands.try_recv().is_err());
    }
}

const NORMAL_TARGET: &str = "untouched/deep/Needle.rs";
const NORMAL_TARGET_TEXT: &str = "first\nneedle target\nlast\n";
const NORMAL_ACTIVE: std::time::Duration = std::time::Duration::from_secs(240);
const NORMAL_CALL: std::time::Duration = std::time::Duration::from_secs(30);
const NORMAL_CLEANUP: std::time::Duration = std::time::Duration::from_secs(30);
const NORMAL_TOTAL: std::time::Duration = std::time::Duration::from_secs(300);

// One normal Client owner, sharing the actual-frame helpers above. The generated
// root is retained from creation and deleted only after explicit verified close;
// neither a panic nor the cancellation watchdog can silently delete evidence.
struct NormalBrowse {
    root: std::path::PathBuf,
    client: Option<cedar_client::Client>,
    started: std::time::Instant,
    cancellation: cedar_client::ConnectionCancellation,
    watchdog: Option<(Sender<()>, std::thread::JoinHandle<bool>)>,
    ledger: Vec<serde_json::Value>,
    hashes: Vec<(String, String)>,
    close_verified: bool,
    success: bool,
}

fn source_digest(path: &std::path::Path) -> String {
    use sha2::Digest;
    format!(
        "{:x}",
        sha2::Sha256::digest(std::fs::read(path).expect("read generated source"))
    )
}

fn normal_operation(operation: &Operation) -> serde_json::Value {
    match operation {
        Operation::List { path } => serde_json::json!({"op": "List", "path": path}),
        Operation::Read { path } => serde_json::json!({"op": "Read", "path": path}),
        Operation::FindFiles {
            query,
            scope,
            limit,
        } => serde_json::json!({"op": "FindFiles", "query": query, "scope": scope, "limit": limit}),
        Operation::Search { query, limit } => {
            serde_json::json!({"op": "Search", "query": query, "limit": limit})
        }
        _ => panic!(
            "normal Browse proof dispatched an operation outside its seven-operation contract"
        ),
    }
}

impl NormalBrowse {
    fn new() -> Self {
        let started = std::time::Instant::now();
        let agent = std::path::PathBuf::from(
            std::env::var_os("CEDAR_BROWSE_AGENT_BIN")
                .expect("select the normal cedar-agent with CEDAR_BROWSE_AGENT_BIN"),
        );
        assert!(agent.is_absolute() && agent.is_file());
        assert_eq!(
            agent.file_name().unwrap(),
            if cfg!(windows) {
                "cedar-agent.exe"
            } else {
                "cedar-agent"
            }
        );
        let root = tempfile::Builder::new()
            .prefix("cedar-browse-normal-")
            .tempdir()
            .expect("create generated root")
            .keep();
        eprintln!("browse_normal_fixture {}", root.display());
        std::fs::create_dir_all(root.join("untouched/deep")).expect("create untouched directory");
        std::fs::write(root.join("origin.txt"), SAVED).expect("write generated origin");
        std::fs::write(root.join(NORMAL_TARGET), NORMAL_TARGET_TEXT)
            .expect("write generated target");
        let hashes = ["origin.txt", NORMAL_TARGET]
            .into_iter()
            .map(|path| (path.to_owned(), source_digest(&root.join(path))))
            .collect();
        let cancellation = cedar_client::ConnectionCancellation::new();
        let cancel_on_expiry = cancellation.clone();
        let (stop, stopped) = mpsc::channel();
        let remaining = NORMAL_TOTAL.saturating_sub(started.elapsed());
        let watchdog = std::thread::spawn(move || {
            if matches!(
                stopped.recv_timeout(remaining),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                cancel_on_expiry.cancel();
                true
            } else {
                false
            }
        });
        let mut proof = Self {
            root,
            client: None,
            started,
            cancellation,
            watchdog: Some((stop, watchdog)),
            ledger: vec![],
            hashes,
            close_verified: false,
            success: false,
        };
        proof.admit_call();
        // Client's constructor performs one ordinary 30-second Hello. Register
        // the returned owner before any subsequent assertion can unwind.
        proof.client = Some(
            cedar_client::Client::spawn_agent_with_cancellation(
                &agent,
                &proof.root,
                false,
                proof.cancellation.clone(),
            )
            .expect("normal agent handshake; root retained if constructor cleanup is uncertain"),
        );
        proof.ledger.push(serde_json::json!({"op": "Hello"}));
        assert!(proof.client.as_ref().unwrap().supports_file_discovery());
        proof
    }

    fn admit_call(&self) {
        assert!(!self.cancellation.is_cancelled());
        assert!(
            self.started.elapsed() + NORMAL_CALL < NORMAL_ACTIVE,
            "shared active deadline cannot admit another full ordinary call"
        );
    }

    fn exchange(
        &mut self,
        app: &mut CedarApp,
        commands: &Receiver<Command>,
        expected: serde_json::Value,
    ) {
        self.admit_call();
        let command = commands
            .try_recv()
            .expect("expected one native app dispatch");
        let observed = normal_operation(&command.op);
        self.ledger.push(observed.clone());
        assert_eq!(observed, expected);
        assert!(
            commands.try_recv().is_err(),
            "no extra operation may accompany the expected dispatch"
        );
        let client = self.client.as_mut().unwrap();
        let result = client.request(command.op);
        assert!(
            client.is_connected(),
            "normal agent disconnected during the bounded request"
        );
        assert!(
            result.is_ok(),
            "normal generated operation failed: {result:?}"
        );
        assert!(self.started.elapsed() < NORMAL_ACTIVE);
        app.apply_event(Event {
            generation: app.generation,
            id: command.id,
            connected: true,
            result,
        });
    }

    fn finish(&mut self) {
        assert!(self.started.elapsed() < NORMAL_ACTIVE);
        assert_eq!(
            self.ledger,
            vec![
                serde_json::json!({"op": "Hello"}),
                serde_json::json!({"op": "List", "path": ""}),
                serde_json::json!({"op": "Read", "path": "origin.txt"}),
                serde_json::json!({"op": "FindFiles", "query": "Needle.rs", "scope": "", "limit": 100}),
                serde_json::json!({"op": "Read", "path": NORMAL_TARGET}),
                serde_json::json!({"op": "Read", "path": NORMAL_TARGET}),
                serde_json::json!({"op": "Search", "query": "needle", "limit": 500}),
            ]
        );
        for (path, hash) in &self.hashes {
            assert_eq!(
                &source_digest(&self.root.join(path)),
                hash,
                "generated disk source changed"
            );
        }
        let allowance = NORMAL_TOTAL
            .saturating_sub(self.started.elapsed())
            .min(NORMAL_CLEANUP);
        assert!(!allowance.is_zero());
        self.client
            .take()
            .unwrap()
            .close_and_wait(allowance)
            .expect("normal agent close/reap must be confirmed before fixture deletion");
        self.close_verified = true;
        let (stop, watchdog) = self.watchdog.take().unwrap();
        let _ = stop.send(());
        assert!(!watchdog.join().expect("join shared cancellation watchdog"));
        assert!(self.started.elapsed() < NORMAL_TOTAL);
        std::fs::remove_dir_all(&self.root).expect("remove verified completed fixture");
        self.success = true;
        println!(
            "browse_normal_acceptance {}",
            serde_json::json!({
                "operation_ledger": self.ledger, "source_sha256": self.hashes,
                "connections": 1, "agents_reaped": 1, "trust": false,
                "active_budget_seconds": 240, "cleanup_timeout_seconds": 30,
                "watchdog_seconds": 300, "dirty_preview_and_open_additional_reads": 0,
                "unopened_preview_reads": 1, "unopened_open_reads": 1,
            })
        );
    }
}

impl Drop for NormalBrowse {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(client) = self.client.take() {
            let allowance = NORMAL_TOTAL
                .saturating_sub(self.started.elapsed())
                .min(NORMAL_CLEANUP);
            if !allowance.is_zero() {
                self.close_verified = client.close_and_wait(allowance).is_ok();
            }
        }
        if let Some((stop, watchdog)) = self.watchdog.take() {
            let _ = stop.send(());
            let _ = watchdog.join();
        }
        if !self.success {
            eprintln!(
                "browse_normal_retained {}",
                serde_json::json!({
                    "root": self.root, "close_verified": self.close_verified,
                    "operation_ledger": self.ledger, "source_sha256": self.hashes,
                })
            );
        }
    }
}

#[test]
#[ignore = "requires CEDAR_BROWSE_AGENT_BIN selecting the normal shipping agent"]
fn browse_normal_agent_find_preview_open_and_exact_return() {
    let mut proof = NormalBrowse::new();
    let mut app = CedarApp::empty();
    app.open_form = false;
    let root = proof
        .root
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    app.form.local_root = root.clone();
    app.generation = 1;
    app.state = ConnectionState::Connecting;
    app.connecting_form = Some(ConnectForm {
        local_root: root,
        allow_run: false,
        ..Default::default()
    });
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    reply(
        &mut app,
        0,
        Ok(proof.client.as_ref().unwrap().handshake().clone()),
    );
    proof.exchange(
        &mut app,
        &commands,
        serde_json::json!({"op": "List", "path": ""}),
    );
    assert!(app.ready() && !app.execution_trusted());
    assert_eq!(
        app.file_discovery,
        Some(cedar_protocol::FileDiscoveryDescriptor::V1)
    );
    assert!(!app.entries.iter().any(|entry| entry.path == NORMAL_TARGET));
    app.open("origin.txt".into(), None);
    proof.exchange(
        &mut app,
        &commands,
        serde_json::json!({"op": "Read", "path": "origin.txt"}),
    );
    seed_dirty_origin(&mut app);
    let departure = selection_signature(selection(&app));
    let undo = undo_texts(&app);
    let revision = app.documents[0].revision.clone().unwrap();

    enter(&mut app, false);
    assert_eq!(app.browse.paths(), vec!["origin.txt"]);
    click(&mut app, "browse_row:0");
    frame(&mut app, key(egui::Key::Space));
    settle(&mut app);
    assert!(
        commands.try_recv().is_err(),
        "open draft preview must perform zero Reads"
    );
    assert_draft_revision(&app, departure, &undo, &revision);
    frame(&mut app, key(egui::Key::Escape));
    frame(&mut app, key(egui::Key::Enter));
    settle(&mut app);
    assert!(
        commands.try_recv().is_err(),
        "filename-only draft Open must perform zero Reads"
    );
    assert_draft_revision(&app, departure, &undo, &revision);
    assert!(app.location_history.back.is_empty());

    enter(&mut app, false);
    query(&mut app, "Needle.rs");
    assert!(commands.try_recv().is_err());
    frame(&mut app, key(egui::Key::Enter));
    proof.exchange(
        &mut app,
        &commands,
        serde_json::json!({"op": "FindFiles", "query": "Needle.rs", "scope": "", "limit": 100}),
    );
    settle(&mut app);
    assert_eq!(app.browse.paths(), vec![NORMAL_TARGET]);
    assert!(!app.entries.iter().any(|entry| entry.path == NORMAL_TARGET));
    click(&mut app, "browse_row:0");
    frame(&mut app, key(egui::Key::Space));
    proof.exchange(
        &mut app,
        &commands,
        serde_json::json!({"op": "Read", "path": NORMAL_TARGET}),
    );
    settle(&mut app);
    assert_eq!(app.browse.preview_text(), Some(NORMAL_TARGET_TEXT));
    assert_eq!(app.documents.len(), 1);
    frame(&mut app, key(egui::Key::Escape));
    frame(&mut app, key(egui::Key::Enter));
    proof.exchange(
        &mut app,
        &commands,
        serde_json::json!({"op": "Read", "path": NORMAL_TARGET}),
    );
    settle(&mut app);
    assert_eq!(app.active().unwrap().path, NORMAL_TARGET);
    assert_eq!(app.active().unwrap().text, NORMAL_TARGET_TEXT);
    frame(
        &mut app,
        modified_key(egui::Key::OpenBracket, command_modifiers()),
    );
    settle(&mut app);
    assert_draft_revision(&app, departure, &undo, &revision);

    enter(&mut app, true);
    query(&mut app, "needle");
    frame(&mut app, key(egui::Key::Enter));
    proof.exchange(
        &mut app,
        &commands,
        serde_json::json!({"op": "Search", "query": "needle", "limit": 500}),
    );
    settle(&mut app);
    let origin = app
        .browse
        .paths()
        .iter()
        .position(|path| *path == "origin.txt")
        .unwrap();
    click(&mut app, &format!("browse_row:{origin}"));
    frame(&mut app, key(egui::Key::Space));
    settle(&mut app);
    assert!(commands.try_recv().is_err());
    frame(&mut app, key(egui::Key::Escape));
    frame(&mut app, key(egui::Key::Enter));
    settle(&mut app);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents[0].cursor, (2, 1));
    frame(
        &mut app,
        modified_key(egui::Key::OpenBracket, command_modifiers()),
    );
    settle(&mut app);
    assert_draft_revision(&app, departure, &undo, &revision);
    frame(&mut app, modified_key(egui::Key::Z, command_modifiers()));
    assert_eq!(app.documents[0].text, SAVED);
    frame(
        &mut app,
        modified_key(
            egui::Key::Z,
            egui::Modifiers {
                shift: true,
                ..command_modifiers()
            },
        ),
    );
    assert_eq!(app.documents[0].text, DRAFT);
    assert!(commands.try_recv().is_err());
    assert!(!app.execution_trusted());
    proof.finish();
}
