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

pub(super) fn record_results_scroll(
    ctx: &egui::Context,
    id: egui::Id,
    inner: egui::Rect,
    content: egui::Vec2,
) {
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("browse_results_scroll_probe"),
            (id, inner, content),
        )
    });
}

pub(super) fn record_bottom_results_scroll(
    ctx: &egui::Context,
    id: egui::Id,
    inner: egui::Rect,
    content: egui::Vec2,
) {
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("bottom_results_scroll_probe"),
            (id, inner, content),
        )
    });
}

fn recorded_results_geometry(app: &CedarApp, bottom: bool) -> (egui::Id, egui::Rect, egui::Vec2) {
    let key = if bottom {
        "bottom_results_scroll_probe"
    } else {
        "browse_results_scroll_probe"
    };
    app.editor_ctx
        .data(|data| data.get_temp(egui::Id::new(key)))
        .expect("production Find results scroll was not rendered")
}

fn recorded_results_scroll(app: &CedarApp) -> egui::Id {
    recorded_results_geometry(app, false).0
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
    assert!(
        rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0,
        "probe={probe}, rect={:?}, clip={:?}, screen={:?}, pass={}, focus={:?}, tools_preference={:?}, tools_panel={:?}",
        recorded_rect(app, probe),
        recorded_clip(app, probe),
        app.editor_ctx.screen_rect(),
        app.editor_ctx.cumulative_pass_nr(),
        focused(app),
        app.tools_height,
        egui::containers::panel::PanelState::load(&app.editor_ctx, egui::Id::new("tools"))
            .map(|state| state.rect)
    );
    rect.center()
}

fn click(app: &mut CedarApp, probe: &str) {
    click_at_size(app, SIZE, probe);
}

fn click_at_size(app: &mut CedarApp, size: egui::Vec2, probe: &str) {
    let pos = visible_center(app, probe);
    let id = recorded_response(app, probe).id;
    let rect = recorded_rect(app, probe);
    let clip = recorded_clip(app, probe);
    // Arrive before pressing: coalescing a large pointer delta with down can
    // begin a native ScrollArea drag instead of clicking the observed row.
    frame_at_size(app, size, vec![egui::Event::PointerMoved(pos)]);
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
    frame_at_size(app, size, pointer(pos, true));
    assert_eq!(
        app.editor_ctx.input(|input| input.pointer.delta()),
        egui::Vec2::ZERO
    );
    assert!(
        recorded_response(app, probe).is_pointer_button_down_on(),
        "{probe}: native press did not reach its current rendered rectangle at {pos:?}"
    );
    frame_at_size(app, size, pointer(pos, false));
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
            Ok(file("c.txt", "first\nlatest selected preview")),
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
                        Err("中文 superseded search failure".into())
                    } else {
                        Ok(Payload::Matches {
                            matches: vec![SearchMatch {
                                path: "未読/obsolete.txt".into(),
                                line: 2,
                                text: "中文 superseded search result".into(),
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
            assert!(!app.cjk_seen);
            assert!(app.system_fonts.needs_probe());
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

// These journeys keep egui's real clock and global click classifier. In
// particular, a negative double-click assertion must not pass just because
// a third click was classified as triple instead.
#[derive(Clone, Debug)]
struct ContinuityResponse {
    id: egui::Id,
    clicked: bool,
    double: bool,
    triple: bool,
}

pub(super) fn record_row_click_response(ui: &egui::Ui, name: &str, response: &egui::Response) {
    let probe = egui::Id::new("browse_continuity_response");
    if !ui.is_sizing_pass()
        && ui
            .ctx()
            .data(|data| data.get_temp::<String>(probe).as_deref() == Some(name))
    {
        let snapshot = ContinuityResponse {
            id: response.id,
            clicked: response.clicked_by(egui::PointerButton::Primary),
            double: response.double_clicked_by(egui::PointerButton::Primary),
            triple: response.triple_clicked_by(egui::PointerButton::Primary),
        };
        ui.ctx().data_mut(|data| {
            let rows =
                data.get_temp_mut_or_default::<Vec<ContinuityResponse>>(probe.with("passes"));
            assert!(rows.len() < 8);
            rows.push(snapshot);
        });
    }
}

fn continuity_frame(
    app: &mut CedarApp,
    dt: f64,
    events: Vec<egui::Event>,
    discard: bool,
    probe: Option<&str>,
) -> Vec<ContinuityResponse> {
    let ctx = app.editor_ctx.clone();
    ctx.options_mut(|options| options.max_passes = 2.try_into().unwrap());
    let mut input = egui::RawInput {
        time: Some(ctx.input(|input| input.time) + dt),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE)),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let mut native = eframe::Frame::_new_kittest();
    let probe_id = egui::Id::new("browse_continuity_response");
    ctx.data_mut(|data| {
        data.insert_temp(probe_id, probe.unwrap_or("").to_owned());
        data.insert_temp(probe_id.with("passes"), Vec::<ContinuityResponse>::new());
    });
    let mut passes = 0;
    let output = ctx.run(input, |ctx| {
        eframe::App::update(app, ctx, &mut native);
        passes += 1;
        if discard && passes == 1 {
            ctx.request_discard("exercise original Browse release across layout passes");
        }
    });
    assert!(output
        .shapes
        .iter()
        .all(|shape| shape.clip_rect.is_finite()));
    if discard {
        assert_eq!(
            passes, 2,
            "the original raw hook runs once, update runs twice"
        );
    }
    ctx.data_mut(|data| {
        data.insert_temp(probe_id, String::new());
        data.get_temp::<Vec<ContinuityResponse>>(probe_id.with("passes"))
            .unwrap()
    })
}

fn continuity_tick(app: &mut CedarApp, dt: f64, events: Vec<egui::Event>) {
    continuity_frame(app, dt, events, false, None);
}

fn continuity_click(app: &mut CedarApp, probe: &str, discard: bool) -> ContinuityResponse {
    let pos = visible_center(app, probe);
    let id = recorded_response(app, probe).id;
    continuity_tick(app, 0.01, vec![egui::Event::PointerMoved(pos)]);
    assert_eq!(recorded_response(app, probe).id, id);
    assert!(recorded_rect(app, probe)
        .intersect(recorded_clip(app, probe))
        .contains(pos));
    continuity_tick(app, 0.01, pointer(pos, true));
    assert!(
        recorded_response(app, probe).is_pointer_button_down_on(),
        "probe={probe}, enabled={}, rows={:?}",
        recorded_response(app, probe).enabled(),
        app.browse.paths()
    );
    let responses = continuity_frame(app, 0.01, pointer(pos, false), discard, Some(probe));
    let response = responses[0].clone();
    assert_eq!(response.id, id);
    assert!(response.clicked);
    response
}

fn assert_continuity_selection(
    app: &CedarApp,
    commands: &Receiver<Command>,
    index: usize,
    epoch: u64,
) {
    assert_eq!(app.browse.selected_index(), Some(index));
    assert!(app.browse_marker_open());
    assert_eq!(app.navigation_epoch, epoch);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents.len(), 1);
    assert!(app.location_history.back.is_empty());
    assert!(
        commands.try_recv().is_err(),
        "selection must dispatch no Read/Open"
    );
}

#[test]
fn browse_row_continuity_group_to_unselected_row_only_selects_even_across_passes() {
    for discard in [false, true] {
        for delay in [0.0, 0.7] {
            let (mut app, commands) = fixture();
            let departure = selection_signature(selection(&app));
            let undo = undo_texts(&app);
            enter(&mut app, true);
            submit_text(&mut app, &commands, &["a.txt", "nested/查询-Ω😀.txt"]);
            click(&mut app, "browse_group:1");
            settle(&mut app);
            continuity_tick(&mut app, 1.0, vec![]);
            let group = continuity_click(&mut app, "browse_group:1", false);
            assert!(!group.double);
            for _ in 0..3 {
                continuity_tick(&mut app, 0.01, vec![]);
            }
            if delay > 0.0 {
                continuity_tick(&mut app, delay, vec![]);
            }
            assert_eq!(app.browse.selected_index(), Some(0));
            let epoch = app.navigation_epoch;
            let row = continuity_click(&mut app, "browse_row:1", discard);
            assert_ne!(group.id, row.id);
            assert_eq!(row.double, delay == 0.0);
            assert!(!row.triple);
            assert_continuity_selection(&app, &commands, 1, epoch);
            assert!(!app.browse.preview_visible());
            assert_draft(&app, departure, &undo);
        }
    }
}

#[test]
fn browse_row_continuity_distinct_rows_and_duplicate_hits_only_select() {
    for paths in [["a.txt", "b.txt"], ["a.txt", "a.txt"]] {
        let (mut app, commands) = fixture();
        enter(&mut app, true);
        submit_text(&mut app, &commands, &paths);
        continuity_tick(&mut app, 1.0, vec![]);
        let first = continuity_click(&mut app, "browse_row:0", false);
        let epoch = app.navigation_epoch;
        let second = continuity_click(&mut app, "browse_row:1", false);
        assert_ne!(first.id, second.id);
        assert!(second.double);
        assert!(!second.triple);
        assert_continuity_selection(&app, &commands, 1, epoch);
    }
}

#[test]
fn browse_row_continuity_same_row_double_opens_once_for_each_provider_and_repeated_pass() {
    for text in [false, true] {
        for discard in [false, true] {
            let (mut app, commands) = fixture();
            enter(&mut app, text);
            if text {
                submit_text(&mut app, &commands, &["a.txt"]);
            } else {
                query(&mut app, "a.txt");
                frame(&mut app, key(egui::Key::Enter));
                settle(&mut app);
            }
            continuity_tick(&mut app, 1.0, vec![]);
            let first = continuity_click(&mut app, "browse_row:0", discard);
            assert!(!first.double);
            assert!(commands.try_recv().is_err());
            let second = continuity_click(&mut app, "browse_row:0", discard);
            assert_eq!(first.id, second.id);
            assert!(second.double);
            assert!(!second.triple);
            assert!(!app.browse_marker_open());
            let open = commands.try_recv().unwrap();
            assert!(matches!(&open.op, Operation::Read { path } if path == "a.txt"));
            assert!(
                commands.try_recv().is_err(),
                "repeated pass must not dispatch twice"
            );
            reply(&mut app, open.id, Ok(file("a.txt", "first\nneedle\n")));
            settle(&mut app);
            assert_eq!(app.active().unwrap().path, "a.txt");
            assert_eq!(app.location_history.back.len(), 1);
        }
    }
}

#[test]
fn browse_row_continuity_triple_click_selects_without_synthesizing_double_open() {
    let (mut app, commands) = fixture();
    enter(&mut app, true);
    submit_text(&mut app, &commands, &["a.txt", "b.txt"]);
    continuity_tick(&mut app, 1.0, vec![]);
    continuity_click(&mut app, "browse_row:0", false);
    let second = continuity_click(&mut app, "browse_row:1", false);
    assert!(second.double);
    let epoch = app.navigation_epoch;
    let third = continuity_click(&mut app, "browse_row:1", true);
    assert!(third.triple);
    assert!(!third.double);
    assert_continuity_selection(&app, &commands, 1, epoch);
    // A later genuine same-row double can use that accepted triple's completed
    // row identity, provided egui itself now reports double rather than triple.
    continuity_tick(&mut app, 0.7, vec![]);
    continuity_click(&mut app, "browse_row:1", false);
    let next = continuity_click(&mut app, "browse_row:1", false);
    assert!(next.double);
    assert!(!app.browse_marker_open());
    assert!(matches!(
        commands.try_recv().unwrap().op,
        Operation::Read { .. }
    ));
    assert!(commands.try_recv().is_err());
}

#[test]
fn browse_row_continuity_mixed_and_focus_events_break_prior_completion() {
    for event in [
        egui::Event::Text("x".into()),
        egui::Event::Paste("x".into()),
        egui::Event::Ime(egui::ImeEvent::Preedit("compose".into())),
        egui::Event::WindowFocused(false),
        egui::Event::PointerGone,
        edge(egui::Key::Tab, true, false),
    ] {
        let (mut app, commands) = fixture();
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["a.txt"]);
        continuity_tick(&mut app, 1.0, vec![]);
        continuity_click(&mut app, "browse_row:0", false);
        let cleanup = match event {
            egui::Event::Ime(_) => vec![egui::Event::Ime(egui::ImeEvent::Disabled)],
            egui::Event::WindowFocused(_) => vec![egui::Event::WindowFocused(true)],
            egui::Event::Key { .. } => vec![edge(egui::Key::Tab, false, false)],
            _ => vec![],
        };
        continuity_tick(&mut app, 0.01, vec![event]);
        if !cleanup.is_empty() {
            continuity_tick(&mut app, 0.01, cleanup);
        }
        let epoch = app.navigation_epoch;
        let row = continuity_click(&mut app, "browse_row:0", false);
        assert!(row.double);
        assert!(!row.triple);
        assert_continuity_selection(&app, &commands, 0, epoch);
    }
}

#[test]
fn browse_row_continuity_rejected_release_and_multiedge_batch_cannot_seed_double() {
    for kind in ["modified", "mixed", "outside", "multiple"] {
        let (mut app, commands) = fixture();
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["a.txt"]);
        continuity_tick(&mut app, 1.0, vec![]);
        continuity_click(&mut app, "browse_row:0", false);
        continuity_tick(&mut app, 0.7, vec![]);
        let pos = visible_center(&app, "browse_row:0");
        continuity_tick(&mut app, 0.01, vec![egui::Event::PointerMoved(pos)]);
        continuity_tick(&mut app, 0.01, pointer(pos, true));
        let mut release = pointer(pos, false);
        match kind {
            "modified" => {
                if let egui::Event::PointerButton { modifiers, .. } = &mut release[1] {
                    modifiers.shift = true;
                }
            }
            "mixed" => release.push(egui::Event::Paste("x".into())),
            "outside" => release = pointer(egui::pos2(4.0, 4.0), false),
            "multiple" => {
                release.extend(pointer(pos, true));
                release.extend(pointer(pos, false));
            }
            _ => unreachable!(),
        }
        continuity_frame(&mut app, 0.01, release, true, None);
        assert!(commands.try_recv().is_err());
        // For the multi-pair batch, expire the global clicks; a fresh first row
        // completion must still be selection-only before the next real double.
        if kind == "multiple" {
            continuity_tick(&mut app, 0.7, vec![]);
        }
        let epoch = app.navigation_epoch;
        let row = continuity_click(&mut app, "browse_row:0", true);
        if kind == "modified" || kind == "mixed" {
            assert!(row.double);
            assert!(!row.triple);
        }
        assert_continuity_selection(&app, &commands, 0, epoch);
    }
}

#[test]
fn browse_row_continuity_pane_and_hide_reopen_break_same_row_completion() {
    for change in ["preview", "reopen", "foreign_modal"] {
        let (mut app, commands) = fixture();
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["origin.txt"]);
        continuity_tick(&mut app, 1.0, vec![]);
        continuity_click(&mut app, "browse_row:0", false);
        match change {
            "preview" => {
                continuity_tick(&mut app, 0.01, key(egui::Key::Space));
                assert!(app.browse.preview_visible());
                continuity_tick(&mut app, 0.01, key(egui::Key::Escape));
                assert!(!app.browse.preview_visible());
            }
            "reopen" => {
                continuity_tick(&mut app, 0.01, key(egui::Key::Escape));
                assert!(!app.browse_marker_open());
                continuity_tick(
                    &mut app,
                    0.01,
                    modified_key(
                        egui::Key::F,
                        egui::Modifiers {
                            shift: true,
                            ..command_modifiers()
                        },
                    ),
                );
                assert!(app.browse_marker_open());
            }
            "foreign_modal" => {
                app.open_form = true;
                continuity_tick(&mut app, 0.01, vec![]);
                app.open_form = false;
                app.show_browse(browse::Provider::ProjectText);
            }
            _ => unreachable!(),
        }
        for _ in 0..3 {
            continuity_tick(&mut app, 0.01, vec![]);
        }
        let epoch = app.navigation_epoch;
        let row = continuity_click(&mut app, "browse_row:0", false);
        assert!(row.double);
        assert!(!row.triple);
        assert_continuity_selection(&app, &commands, 0, epoch);
    }
}

#[test]
fn browse_row_continuity_duplicate_press_poison_survives_split_release() {
    for split in [false, true] {
        let (mut app, commands) = fixture();
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["a.txt"]);
        continuity_tick(&mut app, 1.0, vec![]);
        let pos = visible_center(&app, "browse_row:0");
        continuity_tick(&mut app, 0.01, vec![egui::Event::PointerMoved(pos)]);
        if split {
            continuity_tick(&mut app, 0.01, pointer(pos, true));
            continuity_tick(&mut app, 0.01, pointer(pos, true));
        } else {
            let mut presses = pointer(pos, true);
            presses.extend(pointer(pos, true));
            continuity_tick(&mut app, 0.01, presses);
        }
        continuity_frame(&mut app, 0.01, pointer(pos, false), true, None);
        assert!(commands.try_recv().is_err());
        let epoch = app.navigation_epoch;
        let row = continuity_click(&mut app, "browse_row:0", false);
        assert!(row.double);
        assert!(!row.triple);
        assert_continuity_selection(&app, &commands, 0, epoch);
    }
}

#[test]
fn browse_row_continuity_controls_query_provider_and_result_replacement_retire_prior_click() {
    for change in ["control", "query", "filter", "provider", "result"] {
        let (mut app, commands) = fixture();
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["a.txt"]);
        continuity_tick(&mut app, 1.0, vec![]);
        continuity_click(&mut app, "browse_row:0", false);
        if change != "result" {
            // The final row click is genuinely double with the intervening
            // control; the old row release is outside the triple threshold.
            continuity_tick(&mut app, 0.7, vec![]);
            continuity_click(
                &mut app,
                if change == "provider" {
                    "browse_files"
                } else if change == "filter" {
                    "browse_masks"
                } else {
                    "browse_query"
                },
                false,
            );
        }
        if change == "query" {
            continuity_tick(
                &mut app,
                0.01,
                modified_key(egui::Key::A, command_modifiers()),
            );
            continuity_tick(&mut app, 0.01, vec![egui::Event::Text("new needle".into())]);
            continuity_tick(&mut app, 0.01, key(egui::Key::Enter));
        } else if change == "filter" {
            continuity_tick(&mut app, 0.01, vec![egui::Event::Text("x".into())]);
            assert_eq!(app.search_masks, "x");
            continuity_tick(&mut app, 0.01, key(egui::Key::Backspace));
            assert!(app.search_masks.is_empty());
            continuity_tick(&mut app, 0.01, key(egui::Key::Enter));
        } else if change == "result" {
            // An externally completed rerun replaces exact result authority
            // while Browse's epoch/widget ID need not change.
            app.search();
        }
        if change == "query" || change == "filter" || change == "result" {
            let search = commands.try_recv().unwrap();
            assert!(matches!(search.op, Operation::Search { .. }));
            let result = Ok(Payload::Matches {
                matches: vec![SearchMatch {
                    path: "a.txt".into(),
                    line: 2,
                    text: "new needle result".into(),
                }],
                truncated: false,
            });
            let specification = app.search_specification().unwrap();
            reply(&mut app, search.id, result.clone());
            if change == "result" {
                // Apply the real requested provider result synchronously to
                // isolate result identity without a query/epoch retirement.
                app.apply_search_response(search.id, specification, result);
            }
        }
        for _ in 0..3 {
            continuity_tick(&mut app, 0.01, vec![]);
        }
        let epoch = app.navigation_epoch;
        let row = continuity_click(&mut app, "browse_row:0", false);
        assert!(row.double, "change={change}");
        assert!(!row.triple, "change={change}");
        assert_continuity_selection(&app, &commands, 0, epoch);
    }
}

#[test]
fn browse_row_continuity_context_change_and_held_motion_are_distinct() {
    for context_change in [false, true] {
        let (mut app, commands) = fixture();
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["a.txt"]);
        continuity_tick(&mut app, 1.0, vec![]);
        continuity_click(&mut app, "browse_row:0", false);
        if context_change {
            app.documents[0].edit_version += 1;
            continuity_tick(&mut app, 0.01, vec![]);
            assert!(!app.browse_marker_open());
            app.show_browse(browse::Provider::ProjectText);
            for _ in 0..3 {
                continuity_tick(&mut app, 0.01, vec![]);
            }
        }
        let pos = visible_center(&app, "browse_row:0");
        continuity_tick(&mut app, 0.01, pointer(pos, true));
        continuity_tick(&mut app, 0.01, vec![]);
        continuity_tick(
            &mut app,
            0.01,
            vec![egui::Event::MouseMoved(egui::Vec2::ZERO)],
        );
        let epoch = app.navigation_epoch;
        let row = continuity_frame(
            &mut app,
            0.01,
            pointer(pos, false),
            false,
            Some("browse_row:0"),
        )[0]
        .clone();
        assert!(row.clicked && row.double && !row.triple);
        if context_change {
            assert_continuity_selection(&app, &commands, 0, epoch);
        } else {
            assert!(
                !app.browse_marker_open(),
                "ordinary held frames preserve same-row continuity"
            );
            assert!(matches!(
                commands.try_recv().unwrap().op,
                Operation::Read { .. }
            ));
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn browse_row_continuity_session_workspace_and_navigation_reopen_need_new_click_pair() {
    for change in ["session", "workspace", "navigation"] {
        let (mut app, commands) = fixture();
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["a.txt"]);
        continuity_tick(&mut app, 1.0, vec![]);
        continuity_click(&mut app, "browse_row:0", false);
        match change {
            "session" => app.generation += 1,
            "workspace" => {
                app.workspace_key = Some(WorkspaceKey::Local {
                    root: "/replacement".into(),
                });
                app.root = "/replacement".into();
            }
            "navigation" => app.navigation_changed(),
            _ => unreachable!(),
        }
        continuity_tick(&mut app, 0.01, vec![]);
        assert!(!app.browse_marker_open());
        app.show_browse(browse::Provider::ProjectText);
        continuity_tick(&mut app, 0.01, vec![]);
        if change != "navigation" {
            continuity_tick(&mut app, 0.01, key(egui::Key::Enter));
            let search = commands.try_recv().unwrap();
            reply(
                &mut app,
                search.id,
                Ok(Payload::Matches {
                    matches: vec![SearchMatch {
                        path: "a.txt".into(),
                        line: 2,
                        text: "needle replacement context".into(),
                    }],
                    truncated: false,
                }),
            );
        }
        for _ in 0..3 {
            continuity_tick(&mut app, 0.01, vec![]);
        }
        let epoch = app.navigation_epoch;
        let row = continuity_click(&mut app, "browse_row:0", false);
        assert!(row.double && !row.triple, "change={change}");
        assert_continuity_selection(&app, &commands, 0, epoch);
        continuity_tick(&mut app, 0.7, vec![]);
        continuity_click(&mut app, "browse_row:0", false);
        let second = continuity_click(&mut app, "browse_row:0", false);
        assert!(second.double && !second.triple);
        assert!(!app.browse_marker_open());
        assert!(matches!(
            commands.try_recv().unwrap().op,
            Operation::Read { .. }
        ));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn browse_row_continuity_carried_release_then_press_preserves_native_double_click() {
    for discard in [false, true] {
        let (mut app, commands) = fixture();
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["a.txt"]);
        continuity_tick(&mut app, 1.0, vec![]);
        let pos = visible_center(&app, "browse_row:0");
        continuity_tick(&mut app, 0.01, vec![egui::Event::PointerMoved(pos)]);
        continuity_tick(&mut app, 0.01, pointer(pos, true));
        let mut boundary = pointer(pos, false);
        boundary.extend(pointer(pos, true));
        let first =
            continuity_frame(&mut app, 0.01, boundary, discard, Some("browse_row:0"))[0].clone();
        assert!(first.clicked && !first.double && !first.triple);
        assert!(
            commands.try_recv().is_err(),
            "first completed gesture only selects"
        );
        let second = continuity_frame(
            &mut app,
            0.01,
            pointer(pos, false),
            discard,
            Some("browse_row:0"),
        )[0]
        .clone();
        assert!(second.clicked && second.double && !second.triple);
        assert!(!app.browse_marker_open());
        assert!(matches!(
            commands.try_recv().unwrap().op,
            Operation::Read { .. }
        ));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn browse_row_continuity_bottom_click_cannot_qualify_reopened_browse_row() {
    let (mut app, commands) = fixture();
    enter(&mut app, true);
    submit_text(&mut app, &commands, &["a.txt"]);
    continuity_tick(&mut app, 1.0, vec![]);
    continuity_click(&mut app, "browse_row:0", false);
    continuity_tick(&mut app, 0.01, key(egui::Key::Escape));
    click(&mut app, "find_results_entry");
    for _ in 0..3 {
        continuity_tick(&mut app, 0.01, vec![]);
    }
    assert!(app.tools_open && app.tool == Tool::Search);
    continuity_tick(&mut app, 0.7, vec![]);
    let pos = visible_center(&app, "search_row:0");
    let bottom_id = recorded_response(&app, "search_row:0").id;
    continuity_tick(&mut app, 0.01, vec![egui::Event::PointerMoved(pos)]);
    continuity_tick(&mut app, 0.01, pointer(pos, true));
    assert!(recorded_response(&app, "search_row:0").is_pointer_button_down_on());
    continuity_tick(&mut app, 0.01, pointer(pos, false));
    assert!(recorded_response(&app, "search_row:0").clicked_by(egui::PointerButton::Primary));
    assert!(commands.try_recv().is_err());
    continuity_tick(
        &mut app,
        0.01,
        modified_key(
            egui::Key::F,
            egui::Modifiers {
                shift: true,
                ..command_modifiers()
            },
        ),
    );
    for _ in 0..3 {
        continuity_tick(&mut app, 0.01, vec![]);
    }
    let epoch = app.navigation_epoch;
    let row = continuity_click(&mut app, "browse_row:0", false);
    assert_ne!(row.id, bottom_id);
    assert!(row.double && !row.triple);
    assert_continuity_selection(&app, &commands, 0, epoch);
}

#[test]
fn browse_row_continuity_direct_move_press_double_keeps_dirty_buffer_open() {
    let (mut app, commands) = fixture();
    let departure = selection_signature(selection(&app));
    let undo = undo_texts(&app);
    enter(&mut app, true);
    submit_text(&mut app, &commands, &["origin.txt"]);
    continuity_tick(&mut app, 1.0, vec![egui::Event::PointerGone]);
    continuity_tick(&mut app, 0.01, key(egui::Key::ArrowDown));
    let row_id = recorded_response(&app, "browse_row:0").id;
    assert_eq!(focused(&app), Some(row_id));
    let pos = visible_center(&app, "browse_row:0");
    continuity_tick(&mut app, 0.1, pointer(pos, true));
    assert!(recorded_response(&app, "browse_row:0").is_pointer_button_down_on());
    let first = continuity_frame(
        &mut app,
        0.1,
        pointer(pos, false),
        false,
        Some("browse_row:0"),
    )[0]
    .clone();
    assert!(first.clicked && !first.double && !first.triple);
    assert_eq!(focused(&app), Some(row_id));
    assert!(commands.try_recv().is_err());
    continuity_tick(&mut app, 0.1, pointer(pos, true));
    assert!(recorded_response(&app, "browse_row:0").is_pointer_button_down_on());
    // Browse currently retains focus in this geometry; the bottom Find route
    // also exercises egui's transient None. Either state must preserve this
    // actual owned gesture, while Some(other widget) never qualifies.
    assert!(focused(&app).is_none_or(|id| id == row_id));
    let epoch = app.navigation_epoch;
    let second = continuity_frame(
        &mut app,
        0.1,
        pointer(pos, false),
        false,
        Some("browse_row:0"),
    )[0]
    .clone();
    assert!(second.clicked && second.double && !second.triple);
    assert!(!app.browse_marker_open());
    assert!(app.navigation_epoch > epoch);
    // Ordinary Open commits its coordinate jump/history on the next frame.
    settle(&mut app);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.location_history.back.len(), 1);
    assert_eq!(app.documents[0].text, DRAFT);
    assert_eq!(undo_texts(&app), undo);
    assert!(
        commands.try_recv().is_err(),
        "dirty-buffer Open adds no Read or Write"
    );
    frame(
        &mut app,
        modified_key(egui::Key::OpenBracket, command_modifiers()),
    );
    settle(&mut app);
    assert_draft(&app, departure, &undo);
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
        "typing before the 250 ms pause must not traverse or Read"
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
    files_live::edit(app, "browse_scope", value);
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
        "scope and query edits before settlement cannot perform I/O"
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
    let edited = app.editor_ctx.input(|input| input.time);
    files_live::at(&mut app, edited + 0.249, vec![]);
    assert_eq!(app.browse.scope(), "other");
    assert!(
        app.browse.paths().is_empty(),
        "scope edit wins over a same-frame reply"
    );
    assert!(
        commands.try_recv().is_err(),
        "new scope needs its own full interval"
    );
    files_live::at(&mut app, edited + 0.25, vec![]);
    let current = expect_discovery(&commands, "needle", "other");
    frame(&mut app, key(egui::Key::Enter));
    assert!(
        commands.try_recv().is_err(),
        "Enter must not duplicate the live request"
    );
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
        scoped_search: None,
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
        Some(recorded_response(&app, "origin.txt").id)
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

// B2 keeps the accepted Project Text payload separate from each short-lived
// Find presentation. These journeys act through rendered controls and observe
// the shared snapshot without manufacturing focus, pointer owners, or tickets.
fn retained_snapshot(app: &CedarApp) -> egui::Id {
    app.search_state
        .accepted_snapshot_id()
        .expect("the explicit Search must have one retained snapshot")
}

fn assert_retained_rows(app: &CedarApp, snapshot: egui::Id, paths: &[&str]) {
    assert_eq!(retained_snapshot(app), snapshot);
    assert_eq!(app.browse.paths(), paths);
    assert_eq!(
        app.search_results
            .iter()
            .map(|row| row.path.as_str())
            .collect::<Vec<_>>(),
        paths,
        "grouping must not reorder the accepted backend payload"
    );
}

#[test]
fn browse_retained_groups_keep_duplicate_indices_and_visible_order() {
    let (mut app, commands) = fixture();
    let departure = selection_signature(selection(&app));
    let undo = undo_texts(&app);
    let paths = ["origin.txt", "a.txt", "origin.txt", "b.txt", "a.txt"];
    enter(&mut app, true);
    submit_text(&mut app, &commands, &paths);
    let snapshot = retained_snapshot(&app);
    assert_eq!(
        app.search_state.group_rows(),
        vec![vec![0, 2], vec![1, 4], vec![3]]
    );
    assert_retained_rows(&app, snapshot, &paths);
    assert_ne!(
        recorded_response(&app, "browse_row:0").id,
        recorded_response(&app, "browse_row:2").id,
        "identical path, line and snippet still have distinct original indices"
    );
    for pair in [0, 2, 1, 4, 3].windows(2) {
        assert!(
            recorded_rect(&app, &format!("browse_row:{}", pair[0])).top()
                < recorded_rect(&app, &format!("browse_row:{}", pair[1])).top()
        );
    }
    click(&mut app, "browse_row:0");
    for expected in [2, 1, 4, 3, 3] {
        frame(&mut app, key(egui::Key::ArrowDown));
        assert_eq!(app.browse.selected_index(), Some(expected));
        assert_eq!(app.search_state.selected_index(), Some(expected));
        assert_eq!(
            focused(&app),
            Some(recorded_response(&app, &format!("browse_row:{expected}")).id)
        );
    }
    frame(&mut app, key(egui::Key::Home));
    frame(&mut app, key(egui::Key::ArrowUp));
    assert_eq!(app.browse.selected_index(), Some(0));
    click(&mut app, "browse_group:0");
    settle(&mut app);
    assert!(!app.search_state.group_expanded(0));
    assert_eq!(app.browse.selected_index(), Some(0));
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "browse_group:0").id)
    );
    assert!(!recorded_response(&app, "browse_preview").enabled());
    assert!(!recorded_response(&app, "browse_open").enabled());
    click(&mut app, "browse_row:1");
    frame(&mut app, key(egui::Key::Home));
    assert_eq!(app.browse.selected_index(), Some(1));
    frame(&mut app, key(egui::Key::ArrowDown));
    assert_eq!(app.browse.selected_index(), Some(4));
    frame(&mut app, key(egui::Key::End));
    assert_eq!(app.browse.selected_index(), Some(3));
    click(&mut app, "browse_group:0");
    settle(&mut app);
    assert!(app.search_state.group_expanded(0));
    assert_retained_rows(&app, snapshot, &paths);
    assert_draft(&app, departure, &undo);
    assert!(app.location_history.back.is_empty());
    assert!(app.location_history.forward.is_empty());
    assert!(
        commands.try_recv().is_err(),
        "grouping and selection perform no operation"
    );
}

#[test]
fn browse_retained_snapshot_is_shared_across_open_back_hide_and_reopen() {
    let (mut app, commands) = fixture();
    let departure = selection_signature(selection(&app));
    let undo = undo_texts(&app);
    let paths = ["origin.txt", "a.txt", "origin.txt"];
    enter(&mut app, true);
    submit_text(&mut app, &commands, &paths);
    let snapshot = retained_snapshot(&app);
    click(&mut app, "browse_group:1");
    settle(&mut app);
    click(&mut app, "browse_row:2");
    frame(&mut app, key(egui::Key::Space));
    settle(&mut app);
    assert!(app.browse.preview_visible());
    assert_draft(&app, departure, &undo);
    frame(&mut app, key(egui::Key::Escape));
    frame(&mut app, key(egui::Key::Enter));
    settle(&mut app);
    assert_eq!(app.documents[0].cursor, (2, 1));
    assert_eq!(app.location_history.back.len(), 1);
    frame(
        &mut app,
        modified_key(egui::Key::OpenBracket, command_modifiers()),
    );
    settle(&mut app);
    assert_draft(&app, departure, &undo);
    assert!(app.location_history.back.is_empty());
    assert_eq!(app.location_history.forward.len(), 1);
    enter(&mut app, true);
    assert_retained_rows(&app, snapshot, &paths);
    assert_eq!(app.browse.selected_index(), Some(2));
    assert!(!app.search_state.group_expanded(1));
    assert!(
        !app.browse.preview_visible(),
        "reopen must not revive a preview ticket"
    );
    frame(&mut app, key(egui::Key::Escape));
    settle(&mut app);
    frame(&mut app, modified_key(egui::Key::J, command_modifiers()));
    settle(&mut app);
    assert!(app.tools_open && app.tool == Tool::Search);
    assert_eq!(retained_snapshot(&app), snapshot);
    assert_eq!(app.search_state.selected_index(), Some(2));
    assert!(!app.search_state.group_expanded(1));
    assert!(!app.has_search_preview());
    assert!(recorded_response(&app, "search_row:2").enabled());
    click(&mut app, "search_group:1");
    settle(&mut app);
    assert!(app.search_state.group_expanded(1));
    click(&mut app, "close_tools");
    settle(&mut app);
    assert!(!app.tools_open);
    assert_eq!(retained_snapshot(&app), snapshot);
    enter(&mut app, true);
    assert_retained_rows(&app, snapshot, &paths);
    assert!(app.search_state.group_expanded(1));
    assert_eq!(app.browse.selected_index(), Some(2));
    assert!(!app.browse.preview_visible());
    assert_draft(&app, departure, &undo);
    assert!(app.location_history.back.is_empty());
    assert_eq!(app.location_history.forward.len(), 1);
    assert!(
        commands.try_recv().is_err(),
        "Open on a dirty tab, grouping, hide and reopen add no I/O"
    );
}

#[test]
fn browse_retained_query_round_trip_and_failed_rerun_require_explicit_search() {
    let (mut app, commands) = fixture();
    let departure = selection_signature(selection(&app));
    let undo = undo_texts(&app);
    enter(&mut app, true);
    submit_text(&mut app, &commands, &["origin.txt", "a.txt"]);
    let snapshot = retained_snapshot(&app);
    for edited in ["changed", "needle"] {
        query(&mut app, edited);
        settle(&mut app);
        assert_retained_rows(&app, snapshot, &["origin.txt", "a.txt"]);
        assert!(!recorded_response(&app, "browse_row:0").enabled());
        assert!(!recorded_response(&app, "browse_preview").enabled());
        assert!(!recorded_response(&app, "browse_open").enabled());
        assert!(commands.try_recv().is_err());
    }
    frame(&mut app, key(egui::Key::Enter));
    let failed = commands.try_recv().unwrap();
    assert!(matches!(&failed.op, Operation::Search { query, limit: 500 } if query == "needle"));
    settle(&mut app);
    assert!(!recorded_response(&app, "browse_row:0").enabled());
    reply(
        &mut app,
        failed.id,
        Err("bounded retained rerun failed".into()),
    );
    settle(&mut app);
    assert_retained_rows(&app, snapshot, &["origin.txt", "a.txt"]);
    assert!(!recorded_response(&app, "browse_row:0").enabled());
    assert!(has_text(&frame(&mut app, vec![]), "needle"));
    frame(&mut app, key(egui::Key::Escape));
    enter(&mut app, true);
    assert_retained_rows(&app, snapshot, &["origin.txt", "a.txt"]);
    assert!(!recorded_response(&app, "browse_row:0").enabled());
    frame(&mut app, key(egui::Key::Enter));
    let accepted = commands.try_recv().unwrap();
    assert!(matches!(&accepted.op, Operation::Search { query, limit: 500 } if query == "needle"));
    reply(
        &mut app,
        accepted.id,
        Ok(Payload::Matches {
            matches: vec![SearchMatch {
                path: "origin.txt".into(),
                line: 3,
                text: "replacement needle".into(),
            }],
            truncated: false,
        }),
    );
    settle(&mut app);
    assert_ne!(retained_snapshot(&app), snapshot);
    assert!(recorded_response(&app, "browse_row:0").enabled());
    assert_eq!(app.search_results[0].line, 3);
    assert_draft(&app, departure, &undo);
    assert!(app.location_history.back.is_empty());
    assert!(commands.try_recv().is_err());
}

#[test]
fn browse_retained_reconnect_cannot_revive_preview_or_search_authority() {
    for failed in [false, true] {
        let (mut app, commands) = fixture();
        let departure = selection_signature(selection(&app));
        let undo = undo_texts(&app);
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["origin.txt", "a.txt"]);
        let snapshot = retained_snapshot(&app);
        click(&mut app, "browse_row:1");
        frame(&mut app, key(egui::Key::Space));
        let preview = commands.try_recv().unwrap();
        assert!(matches!(&preview.op, Operation::Read { path } if path == "a.txt"));
        let old_generation = app.generation;
        let entries = app.entries.clone();
        app.disconnected("retained Find connection changed".into());
        app.generation += 1;
        let (worker, commands) = Worker::recording();
        app.worker = Some(worker);
        app.state = ConnectionState::Connecting;
        app.connecting_form = app.active_form.clone();
        reply(
            &mut app,
            0,
            Ok(Payload::Hello {
                protocol: cedar_protocol::PROTOCOL_VERSION,
                root: "/workspace".into(),
                agent: Some(agent_support::full_test_agent()),
                file_discovery: None,
                scoped_search: None,
            }),
        );
        let list = commands.try_recv().unwrap();
        assert!(matches!(&list.op, Operation::List { path } if path.is_empty()));
        reply(&mut app, list.id, Ok(Payload::Entries { entries }));
        settle(&mut app);
        enter(&mut app, true);
        assert_retained_rows(&app, snapshot, &["origin.txt", "a.txt"]);
        assert!(!recorded_response(&app, "browse_row:1").enabled());
        assert!(!app.browse.preview_visible());
        app.error = Some("new connection status".into());
        app.apply_event(Event {
            generation: old_generation,
            id: preview.id,
            connected: true,
            result: if failed {
                Err("stale connection preview error".into())
            } else {
                Ok(file("a.txt", "stale connection preview bytes"))
            },
        });
        settle(&mut app);
        assert!(!app.browse.preview_visible());
        assert_eq!(app.error.as_deref(), Some("new connection status"));
        assert_eq!(retained_snapshot(&app), snapshot);
        assert!(
            commands.try_recv().is_err(),
            "reconnect and stale outcomes must not re-search"
        );
        submit_text(&mut app, &commands, &["origin.txt", "a.txt"]);
        assert_ne!(retained_snapshot(&app), snapshot);
        assert!(recorded_response(&app, "browse_row:1").enabled());
        assert_draft(&app, departure, &undo);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn browse_retained_group_keys_reject_mixed_composition_and_held_replay() {
    for companion in [
        egui::Event::Text("x".into()),
        egui::Event::Paste("x".into()),
        egui::Event::Ime(egui::ImeEvent::Preedit("composition".into())),
        egui::Event::Ime(egui::ImeEvent::Commit("x".into())),
        egui::Event::PointerMoved(egui::pos2(4.0, 4.0)),
    ] {
        let (mut app, commands) = fixture();
        let departure = selection_signature(selection(&app));
        let undo = undo_texts(&app);
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["origin.txt", "a.txt", "origin.txt"]);
        let snapshot = retained_snapshot(&app);
        click(&mut app, "browse_group:0");
        settle(&mut app);
        assert!(!app.search_state.group_expanded(0));
        let mut events = key(egui::Key::Space);
        events.insert(1, companion);
        frame(&mut app, events);
        assert!(
            !app.search_state.group_expanded(0),
            "mixed native input cannot toggle a group"
        );
        assert_eq!(retained_snapshot(&app), snapshot);
        assert!(!app.browse.preview_visible());
        assert_draft(&app, departure, &undo);
        assert!(commands.try_recv().is_err());
    }
    let (mut app, commands) = fixture();
    enter(&mut app, true);
    submit_text(&mut app, &commands, &["origin.txt", "a.txt"]);
    click(&mut app, "browse_group:0");
    settle(&mut app);
    frame(
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Preedit(String::new()))],
    );
    frame(&mut app, key(egui::Key::Space));
    assert!(!app.search_state.group_expanded(0));
    frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
    frame(&mut app, vec![edge(egui::Key::Space, true, false)]);
    assert!(app.search_state.group_expanded(0));
    for repeat in [false, true] {
        frame(&mut app, vec![edge(egui::Key::Space, true, repeat)]);
        assert!(
            app.search_state.group_expanded(0),
            "held key lifetime cannot toggle twice"
        );
    }
    frame(&mut app, vec![edge(egui::Key::Space, false, false)]);
    frame(&mut app, key(egui::Key::Space));
    assert!(!app.search_state.group_expanded(0));
    assert!(commands.try_recv().is_err());
}

#[test]
fn browse_retained_search_busy_rejection_keeps_one_physical_request_until_reply() {
    for failed in [false, true] {
        let (mut app, commands) = fixture();
        let departure = selection_signature(selection(&app));
        let undo = undo_texts(&app);
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["origin.txt", "a.txt"]);
        let snapshot = retained_snapshot(&app);
        query(&mut app, "pending A");
        frame(&mut app, key(egui::Key::Enter));
        let pending = commands.try_recv().unwrap();
        assert!(
            matches!(&pending.op, Operation::Search { query, limit: 500 } if query == "pending A")
        );
        for value in ["replacement B", "replacement C"] {
            query(&mut app, value);
            frame(&mut app, key(egui::Key::Enter));
            settle(&mut app);
            assert!(
                commands.try_recv().is_err(),
                "an outstanding physical Search cannot be superseded by a request burst"
            );
            assert_retained_rows(&app, snapshot, &["origin.txt", "a.txt"]);
            assert!(!recorded_response(&app, "browse_row:0").enabled());
        }
        frame(&mut app, key(egui::Key::Escape));
        enter(&mut app, true);
        assert_eq!(app.browse.query(), "replacement C");
        frame(&mut app, key(egui::Key::Enter));
        assert!(
            commands.try_recv().is_err(),
            "hide/reopen does not free a physical Search slot"
        );
        app.error = Some("status from current view".into());
        reply(
            &mut app,
            pending.id,
            if failed {
                Err("obsolete pending A failure".into())
            } else {
                Ok(Payload::Matches {
                    matches: vec![SearchMatch {
                        path: "b.txt".into(),
                        line: 7,
                        text: "obsolete pending A".into(),
                    }],
                    truncated: false,
                })
            },
        );
        settle(&mut app);
        assert_retained_rows(&app, snapshot, &["origin.txt", "a.txt"]);
        assert!(!recorded_response(&app, "browse_row:0").enabled());
        assert_eq!(app.error.as_deref(), Some("status from current view"));
        assert!(
            commands.try_recv().is_err(),
            "busy rejection must not become a deferred automatic Search"
        );
        frame(&mut app, key(egui::Key::Enter));
        let fresh = commands.try_recv().unwrap();
        assert!(
            matches!(&fresh.op, Operation::Search { query, limit: 500 } if query == "replacement C")
        );
        reply(
            &mut app,
            fresh.id,
            Ok(Payload::Matches {
                matches: vec![SearchMatch {
                    path: "origin.txt".into(),
                    line: 3,
                    text: "replacement C".into(),
                }],
                truncated: false,
            }),
        );
        settle(&mut app);
        assert_ne!(retained_snapshot(&app), snapshot);
        assert!(recorded_response(&app, "browse_row:0").enabled());
        assert_draft(&app, departure, &undo);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn browse_retained_preview_tickets_do_not_survive_reopen_or_group_collapse() {
    for collapse in [false, true] {
        for failed in [false, true] {
            let (mut app, commands) = fixture();
            let departure = selection_signature(selection(&app));
            let undo = undo_texts(&app);
            enter(&mut app, true);
            submit_text(&mut app, &commands, &["origin.txt", "a.txt"]);
            let snapshot = retained_snapshot(&app);
            click(&mut app, "browse_row:1");
            frame(&mut app, key(egui::Key::Space));
            let obsolete = commands.try_recv().unwrap();
            assert!(matches!(&obsolete.op, Operation::Read { path } if path == "a.txt"));
            if collapse {
                click(&mut app, "browse_group:1");
                settle(&mut app);
                assert!(!app.search_state.group_expanded(1));
                assert_eq!(app.browse.selected_index(), Some(1));
                assert!(!recorded_response(&app, "browse_open").enabled());
            } else {
                frame(&mut app, key(egui::Key::Escape));
                frame(&mut app, key(egui::Key::Escape));
                enter(&mut app, true);
                assert_eq!(app.browse.selected_index(), Some(1));
            }
            assert_retained_rows(&app, snapshot, &["origin.txt", "a.txt"]);
            assert!(!app.browse.preview_visible());
            app.error = Some("retained current status".into());
            reply(
                &mut app,
                obsolete.id,
                if failed {
                    Err("obsolete preview failure".into())
                } else {
                    Ok(file("a.txt", "obsolete preview bytes"))
                },
            );
            settle(&mut app);
            assert!(!app.browse.preview_visible());
            assert!(app.browse.preview_text().is_none());
            assert_eq!(app.error.as_deref(), Some("retained current status"));
            assert_eq!(retained_snapshot(&app), snapshot);
            assert!(
                commands.try_recv().is_err(),
                "retention does not recreate preview authority"
            );
            if collapse {
                click(&mut app, "browse_group:1");
                settle(&mut app);
            }
            click(&mut app, "browse_row:1");
            frame(&mut app, key(egui::Key::Space));
            let fresh = commands.try_recv().unwrap();
            assert_ne!(fresh.id, obsolete.id);
            assert!(matches!(&fresh.op, Operation::Read { path } if path == "a.txt"));
            reply(
                &mut app,
                fresh.id,
                Ok(file("a.txt", "fresh requested preview")),
            );
            settle(&mut app);
            assert_eq!(app.browse.preview_text(), Some("fresh requested preview"));
            assert_eq!(app.active_document, Some(1));
            assert_eq!(app.documents.len(), 1);
            assert_draft(&app, departure, &undo);
            assert!(app.location_history.back.is_empty());
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn browse_retained_native_tab_and_geometry_at_standard_and_narrow_sizes() {
    for size in [egui::vec2(1280.0, 900.0), egui::vec2(780.0, 540.0)] {
        for font in [13.0, 18.0] {
            let (mut app, commands) = fixture();
            let departure = selection_signature(selection(&app));
            let undo = undo_texts(&app);
            app.editor_ctx.style_mut(|style| {
                style
                    .text_styles
                    .insert(egui::TextStyle::Body, FontId::proportional(font));
                style
                    .text_styles
                    .insert(egui::TextStyle::Button, FontId::proportional(font));
                style
                    .text_styles
                    .insert(egui::TextStyle::Monospace, FontId::monospace(font));
            });
            enter(&mut app, true);
            submit_text(&mut app, &commands, &vec!["origin.txt"; 80]);
            let snapshot = retained_snapshot(&app);
            for _ in 0..4 {
                frame_at_size(&mut app, size, vec![]);
            }
            assert_eq!(
                focused(&app),
                Some(recorded_response(&app, "browse_query").id)
            );
            for probe in ["browse_text_scope", "browse_masks", "browse_submit"] {
                let response = recorded_response(&app, probe);
                assert!(response.enabled() && response.sense.is_focusable());
                frame_at_size(&mut app, size, key(egui::Key::Tab));
                assert_eq!(
                    focused(&app),
                    Some(response.id),
                    "font={font}, size={size:?}: native Tab must visit {probe}"
                );
            }
            let group = recorded_response(&app, "browse_group:0").id;
            let first_row = recorded_response(&app, "browse_row:0").id;
            let mut native_tab_focus = Vec::new();
            for _ in 0..6 {
                frame_at_size(&mut app, size, key(egui::Key::Tab));
                native_tab_focus.push(focused(&app));
                if focused(&app) == Some(first_row) {
                    break;
                }
            }
            assert!(
                native_tab_focus.contains(&Some(group)),
                "native Tab must visit the group before its rows"
            );
            assert_eq!(
                focused(&app),
                Some(first_row),
                "native Tab must reach a real result without injected focus"
            );
            frame_at_size(&mut app, size, key(egui::Key::End));
            for _ in 0..4 {
                frame_at_size(&mut app, size, vec![]);
            }
            assert_eq!(app.browse.selected_index(), Some(79));
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let row = recorded_rect(&app, "browse_row:79");
            assert!(
                recorded_clip(&app, "browse_row:79").contains_rect(row),
                "font {font}, size {size:?}: selected row is clipped"
            );
            assert!(screen.contains_rect(row));
            frame_at_size(&mut app, size, key(egui::Key::Space));
            for _ in 0..4 {
                frame_at_size(&mut app, size, vec![]);
            }
            for probe in [
                "browse_query",
                "browse_preview",
                "browse_open",
                "browse_close",
                "browse_preview_content",
            ] {
                let rect = recorded_rect(&app, probe);
                assert!(rect.is_finite() && rect.is_positive());
                assert!(
                    screen.contains_rect(rect),
                    "font {font}, size {size:?}: {probe} outside screen: {rect:?}"
                );
                assert!(
                    recorded_clip(&app, probe).contains_rect(rect),
                    "font {font}, size {size:?}: {probe} clipped: {rect:?}"
                );
            }
            frame_at_size(&mut app, size, key(egui::Key::Escape));
            assert_eq!(
                focused(&app),
                Some(recorded_response(&app, "browse_row:79").id)
            );
            assert_eq!(retained_snapshot(&app), snapshot);
            assert_draft(&app, departure, &undo);
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn browse_retained_historical_label_tracks_accepted_query_during_failed_replacement() {
    let (mut app, commands) = fixture();
    enter(&mut app, true);
    submit_text(&mut app, &commands, &["origin.txt"]);
    let snapshot = retained_snapshot(&app);
    query(&mut app, "replacement query");
    frame(&mut app, key(egui::Key::Enter));
    let pending = commands.try_recv().unwrap();
    assert!(
        matches!(&pending.op, Operation::Search { query, limit: 500 } if query == "replacement query")
    );
    settle(&mut app);
    let output = frame(&mut app, vec![]);
    assert!(has_text(&output, "Historical results for \"needle\""));
    assert!(!has_text(
        &output,
        "Historical results for \"replacement query\""
    ));
    assert!(!recorded_response(&app, "browse_row:0").enabled());
    reply(&mut app, pending.id, Err("replacement query failed".into()));
    settle(&mut app);
    let output = frame(&mut app, vec![]);
    assert_eq!(retained_snapshot(&app), snapshot);
    assert!(has_text(&output, "Historical results for \"needle\""));
    assert!(!has_text(
        &output,
        "Historical results for \"replacement query\""
    ));
    assert!(!recorded_response(&app, "browse_row:0").enabled());
    frame(&mut app, key(egui::Key::Escape));
    enter(&mut app, true);
    assert_eq!(app.browse.query(), "replacement query");
    assert_eq!(retained_snapshot(&app), snapshot);
    assert!(has_text(
        &frame(&mut app, vec![]),
        "Historical results for \"needle\""
    ));
    assert!(commands.try_recv().is_err());
}

#[test]
fn browse_retained_bottom_search_hide_reopen_retires_unchanged_query_request() {
    for failed in [false, true] {
        let (mut app, commands) = fixture();
        let departure = selection_signature(selection(&app));
        let undo = undo_texts(&app);
        enter(&mut app, true);
        submit_text(&mut app, &commands, &["origin.txt", "a.txt"]);
        let snapshot = retained_snapshot(&app);
        frame(&mut app, key(egui::Key::Escape));
        settle(&mut app);
        frame(&mut app, modified_key(egui::Key::J, command_modifiers()));
        settle(&mut app);
        assert!(app.tools_open && app.tool == Tool::Search);
        click(&mut app, "search_submit");
        let pending = commands.try_recv().unwrap();
        assert!(
            matches!(&pending.op, Operation::Search { query, limit: 500 } if query == "needle")
        );
        assert!(
            !app.search_rows_active(),
            "submission immediately retires old row authority"
        );
        assert_eq!(
            app.search_state
                .pending
                .as_ref()
                .map(|source| source.request),
            Some(pending.id)
        );
        assert!(commands.try_recv().is_err());
        // The original assertion read a reconstructed egui response immediately
        // after submission and disagreed with the final inactive model. Pinned
        // egui read_response uses this/previous-pass WidgetRect data, and repeated
        // ID insertion OR-merges enabled; it is not our stored render response.
        // Check disabled presentation after settling, retaining the immediate
        // authority/request assertions above and the exact native Close hit below.
        settle(&mut app);
        assert!(!recorded_response(&app, "search_row:0").enabled());
        click(&mut app, "close_tools");
        settle(&mut app);
        assert!(!app.tools_open);
        frame(&mut app, modified_key(egui::Key::J, command_modifiers()));
        settle(&mut app);
        assert_eq!(
            app.search_query, "needle",
            "this lifecycle test never edits the pending query"
        );
        assert_eq!(retained_snapshot(&app), snapshot);
        app.error = Some("status after reopening retained results".into());
        reply(
            &mut app,
            pending.id,
            if failed {
                Err("中文 obsolete same-query Search error".into())
            } else {
                Ok(Payload::Matches {
                    matches: vec![SearchMatch {
                        path: "未読/b.txt".into(),
                        line: 7,
                        text: "中文 obsolete same-query result".into(),
                    }],
                    truncated: false,
                })
            },
        );
        settle(&mut app);
        assert_eq!(
            retained_snapshot(&app),
            snapshot,
            "reopening cannot restore a hidden request's authority"
        );
        assert_eq!(app.search_results.len(), 2);
        assert_eq!(app.search_results[0].path, "origin.txt");
        assert!(!app.search_rows_active());
        assert!(!app.cjk_seen);
        assert!(app.system_fonts.needs_probe());
        assert!(!recorded_response(&app, "search_row:0").enabled());
        assert_eq!(
            app.error.as_deref(),
            Some("status after reopening retained results")
        );
        assert!(
            commands.try_recv().is_err(),
            "hide/reopen and stale outcomes must not automatically re-search"
        );
        click(&mut app, "search_submit");
        let fresh = commands.try_recv().unwrap();
        assert!(matches!(&fresh.op, Operation::Search { query, limit: 500 } if query == "needle"));
        reply(
            &mut app,
            fresh.id,
            Ok(Payload::Matches {
                matches: vec![SearchMatch {
                    path: "origin.txt".into(),
                    line: 3,
                    text: "fresh explicit Search".into(),
                }],
                truncated: false,
            }),
        );
        settle(&mut app);
        assert_ne!(retained_snapshot(&app), snapshot);
        assert!(recorded_response(&app, "search_row:0").enabled());
        assert_draft(&app, departure, &undo);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn browse_retained_bottom_insufficient_height_is_safe_and_automatic_restores_journey() {
    for size in [egui::vec2(1280.0, 900.0), egui::vec2(780.0, 540.0)] {
        for font in [13.0, 18.0] {
            let (mut app, commands) = fixture();
            let departure = selection_signature(selection(&app));
            let undo = undo_texts(&app);
            // This is a retained runtime preference, not a hard content cap.
            // Production may grow the panel; assert actual visible allocations.
            app.tools_height = Some(150.0);
            app.editor_ctx.style_mut(|style| {
                style
                    .text_styles
                    .insert(egui::TextStyle::Body, FontId::proportional(font));
                style
                    .text_styles
                    .insert(egui::TextStyle::Button, FontId::proportional(font));
                style
                    .text_styles
                    .insert(egui::TextStyle::Monospace, FontId::monospace(font));
            });
            enter(&mut app, true);
            query(&mut app, "needle");
            frame(&mut app, key(egui::Key::Enter));
            let search = commands.try_recv().unwrap();
            assert!(
                matches!(&search.op, Operation::Search { query, limit: 500 } if query == "needle")
            );
            reply(
                &mut app,
                search.id,
                Ok(Payload::Matches {
                    matches: (0..32)
                        .map(|_| SearchMatch {
                            path: "origin.txt".into(),
                            line: 2,
                            text: "needle historical match".into(),
                        })
                        .collect(),
                    truncated: true,
                }),
            );
            settle(&mut app);
            let snapshot = retained_snapshot(&app);
            frame(&mut app, key(egui::Key::Escape));
            for _ in 0..4 {
                frame_at_size(&mut app, size, vec![]);
            }
            frame_at_size(
                &mut app,
                size,
                modified_key(egui::Key::J, command_modifiers()),
            );
            for _ in 0..4 {
                frame_at_size(&mut app, size, vec![]);
            }
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let positive_visible = |app: &CedarApp, probe: &str, stage: &str| {
                let rect = recorded_rect(app, probe);
                let clip = recorded_clip(app, probe);
                let visible = rect.intersect(clip).intersect(screen);
                let scroll_id = crate::search_keyboard_tests::recorded_scroll_id(app);
                let offset = egui::scroll_area::State::load(&app.editor_ctx, scroll_id)
                    .map(|state| state.offset);
                let selected = app.search_state.selected_index();
                let native_focus = focused(app);
                assert!(visible.is_finite() && visible.width() > 0.0 && visible.height() > 0.0,
                    "stage={stage}, font={font}, size={size:?}, probe={probe}, rect={rect:?}, clip={clip:?}, screen={screen:?}, scroll={scroll_id:?}, offset={offset:?}, selected_original_index={selected:?}, native_focus={native_focus:?}");
            };
            for probe in [
                "search_query",
                "search_submit",
                "close_tools",
                "tools_automatic_height",
            ] {
                positive_visible(&app, probe, "constrained controls");
            }
            let row_zero_visible = recorded_rect(&app, "search_row:0")
                .intersect(recorded_clip(&app, "search_row:0"))
                .intersect(screen)
                .is_positive();
            if size == egui::vec2(1280.0, 900.0) && font == 18.0 {
                // Preserved failed usability boundary: this 150-point preference
                // cannot expose its first hit at the larger font. A safe fallback
                // is not a claim that the constrained result journey passed.
                assert!(!row_zero_visible, "the documented insufficient-height boundary changed; reassess its exact geometry");
            }
            let hidden = (0..32)
                .find(|index| {
                    let probe = format!("search_row:{index}");
                    !recorded_rect(&app, &probe)
                        .intersect(recorded_clip(&app, &probe))
                        .intersect(screen)
                        .is_positive()
                })
                .expect("the constrained long result set must include a clipped row");
            let hidden_probe = format!("search_row:{hidden}");
            let hidden_rect = recorded_rect(&app, &hidden_probe);
            let hidden_clip = recorded_clip(&app, &hidden_probe);
            let hidden_point = hidden_rect.center();
            let selected_before = app.search_state.selected_index();
            let navigation_before = app.navigation_epoch;
            frame_at_size(
                &mut app,
                size,
                vec![egui::Event::PointerMoved(hidden_point)],
            );
            frame_at_size(&mut app, size, pointer(hidden_point, true));
            frame_at_size(&mut app, size, pointer(hidden_point, false));
            assert!(!hidden_rect.intersect(hidden_clip).contains(hidden_point));
            assert_eq!(
                app.search_state.selected_index(),
                selected_before,
                "a clipped row must not acquire pointer selection"
            );
            assert_eq!(app.navigation_epoch, navigation_before);
            assert!(!app.has_search_preview());
            assert!(commands.try_recv().is_err());
            assert_eq!(app.tools_height, Some(150.0));
            assert_draft(&app, departure, &undo);
            // Reach the visible escape hatch by real Tab traversal, without a
            // pointer refocus or fabricated focus/scroll ownership.
            let automatic = recorded_response(&app, "tools_automatic_height").id;
            for _ in 0..24 {
                if focused(&app) == Some(automatic) {
                    break;
                }
                frame_at_size(&mut app, size, key(egui::Key::Tab));
            }
            assert_eq!(
                focused(&app),
                Some(automatic),
                "native Tab must reach Automatic height"
            );
            positive_visible(
                &app,
                "tools_automatic_height",
                "constrained keyboard recovery",
            );
            frame_at_size(&mut app, size, key(egui::Key::Enter));
            for _ in 0..4 {
                frame_at_size(&mut app, size, vec![]);
            }
            assert_eq!(app.tools_height, None);
            for probe in [
                "search_query",
                "search_submit",
                "search_truncated",
                "search_group:0",
                "search_row:0",
                "close_tools",
            ] {
                positive_visible(&app, probe, "automatic before End");
            }
            click_at_size(&mut app, size, "search_row:0");
            frame_at_size(&mut app, size, key(egui::Key::End));
            for _ in 0..4 {
                frame_at_size(&mut app, size, vec![]);
            }
            assert_eq!(app.search_state.selected_index(), Some(31));
            positive_visible(&app, "search_row:31", "after End");
            assert_eq!(
                focused(&app),
                Some(recorded_response(&app, "search_row:31").id)
            );
            frame_at_size(&mut app, size, key(egui::Key::Space));
            for _ in 0..4 {
                frame_at_size(&mut app, size, vec![]);
            }
            assert!(app.has_search_preview());
            assert_eq!(app.search_preview_text(), Some(DRAFT));
            positive_visible(&app, "search_row:31", "after Preview");
            positive_visible(&app, "search_preview_content", "after Preview");
            positive_visible(&app, "search_submit", "after Preview");
            positive_visible(&app, "close_tools", "after Preview");
            assert_eq!(
                app.tools_height, None,
                "only explicit Automatic height clears the preference"
            );
            assert_eq!(retained_snapshot(&app), snapshot);
            assert_draft(&app, departure, &undo);
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn browse_retained_bottom_search_refuses_exhausted_request_identity() {
    for exhausted in [0, u64::MAX] {
        for button in [false, true] {
            let (mut app, commands) = fixture();
            let departure = selection_signature(selection(&app));
            let undo = undo_texts(&app);
            enter(&mut app, true);
            submit_text(&mut app, &commands, &["origin.txt"]);
            let snapshot = retained_snapshot(&app);
            frame(&mut app, key(egui::Key::Escape));
            settle(&mut app);
            frame(&mut app, modified_key(egui::Key::J, command_modifiers()));
            settle(&mut app);
            app.next_request = exhausted;
            if button {
                click(&mut app, "search_submit");
            } else {
                click(&mut app, "search_query");
                frame(&mut app, key(egui::Key::Enter));
            }
            settle(&mut app);
            assert!(
                commands.try_recv().is_err(),
                "bottom Search must not dispatch a reused identity"
            );
            assert_eq!(app.next_request, exhausted);
            assert!(app.search_state.pending.is_none());
            assert_eq!(retained_snapshot(&app), snapshot);
            assert!(!recorded_response(&app, "search_row:0").enabled());
            let message = app
                .error
                .clone()
                .expect("exhaustion needs an explicit refusal");
            assert!(message.contains("request identity counter is exhausted"));
            assert!(has_text(&frame(&mut app, vec![]), &message));
            assert_draft(&app, departure, &undo);
            assert!(app.location_history.back.is_empty());
        }
    }
}

#[test]
fn browse_retained_result_scroll_survives_reopen_but_resets_for_new_batch() {
    for bottom in [false, true] {
        let (mut app, commands) = fixture();
        let departure = selection_signature(selection(&app));
        let undo = undo_texts(&app);
        enter(&mut app, true);
        submit_text(&mut app, &commands, &vec!["origin.txt"; 80]);
        let snapshot = retained_snapshot(&app);
        if bottom {
            frame(&mut app, key(egui::Key::Escape));
            settle(&mut app);
            frame(&mut app, modified_key(egui::Key::J, command_modifiers()));
            settle(&mut app);
        }
        let row = |index| {
            format!(
                "{}:{index}",
                if bottom { "search_row" } else { "browse_row" }
            )
        };
        let query_probe = if bottom {
            "search_query"
        } else {
            "browse_query"
        };
        let scroll = |app: &CedarApp| {
            if bottom {
                crate::search_keyboard_tests::recorded_scroll_id(app)
            } else {
                recorded_results_scroll(app)
            }
        };
        click(&mut app, &row(0));
        frame(&mut app, key(egui::Key::End));
        settle(&mut app);
        assert_eq!(app.search_state.selected_index(), Some(79));
        let old_scroll = scroll(&app);
        let old_offset = egui::scroll_area::State::load(&app.editor_ctx, old_scroll)
            .unwrap()
            .offset;
        assert!(
            old_offset.y > 0.0,
            "the journey must actually scroll the accepted result batch"
        );
        assert!(recorded_clip(&app, &row(79)).contains_rect(recorded_rect(&app, &row(79))));
        let old_geometry = recorded_results_geometry(&app, bottom);
        let old_row_rect = recorded_rect(&app, &row(79));
        let old_row_clip = recorded_clip(&app, &row(79));
        if bottom {
            click(&mut app, "close_tools");
            settle(&mut app);
            frame(&mut app, modified_key(egui::Key::J, command_modifiers()));
            settle(&mut app);
        } else {
            frame(&mut app, key(egui::Key::Escape));
            enter(&mut app, true);
        }
        assert_eq!(retained_snapshot(&app), snapshot);
        assert_eq!(app.search_state.selected_index(), Some(79));
        assert_eq!(
            scroll(&app),
            old_scroll,
            "reopening retains the same batch's actual scroll identity"
        );
        let reopened_offset = egui::scroll_area::State::load(&app.editor_ctx, old_scroll)
            .unwrap()
            .offset;
        let reopened_geometry = recorded_results_geometry(&app, bottom);
        let reopened_row_rect = recorded_rect(&app, &row(79));
        let reopened_row_clip = recorded_clip(&app, &row(79));
        assert_eq!(
            reopened_geometry.2, old_geometry.2,
            "same-batch content size is retained"
        );
        // The preserved original failure measured unchanged 1940-point content,
        // viewport growth from 256 to 280, and offset 1684 -> 1660. Pinned egui
        // clamps to content minus viewport; compare that exact observed geometry,
        // without an epsilon, extra scroll command, or reconstructed scroll ID.
        let maximum_offset =
            (reopened_geometry.2 - reopened_geometry.1.size()).max(egui::Vec2::ZERO);
        let expected_offset = old_offset.max(egui::Vec2::ZERO).min(maximum_offset);
        assert_eq!(reopened_offset, expected_offset,
            "bottom={bottom}, before_scroll_inner_content={old_geometry:?}, after_scroll_inner_content={reopened_geometry:?}, before_row={old_row_rect:?}, before_clip={old_row_clip:?}, after_row={reopened_row_rect:?}, after_clip={reopened_row_clip:?}");
        assert!(recorded_clip(&app, &row(79)).contains_rect(recorded_rect(&app, &row(79))));
        assert!(commands.try_recv().is_err());

        click(&mut app, query_probe);
        frame(&mut app, modified_key(egui::Key::A, command_modifiers()));
        frame(&mut app, vec![egui::Event::Text("replacement".into())]);
        frame(&mut app, key(egui::Key::Enter));
        let pending = commands.try_recv().unwrap();
        assert!(
            matches!(&pending.op, Operation::Search { query, limit: 500 } if query == "replacement")
        );
        settle(&mut app);
        // Enter has normal TextEdit focus behavior on the bottom surface. Use
        // a real query click while the reply is pending, then verify publishing
        // a new batch neither steals this focus nor reuses the deep old offset.
        click(&mut app, query_probe);
        let query_focus = recorded_response(&app, query_probe).id;
        assert_eq!(focused(&app), Some(query_focus));
        reply(
            &mut app,
            pending.id,
            Ok(Payload::Matches {
                matches: (0..80)
                    .map(|_| SearchMatch {
                        path: "origin.txt".into(),
                        line: 2,
                        text: "replacement historical match".into(),
                    })
                    .collect(),
                truncated: false,
            }),
        );
        settle(&mut app);
        assert_ne!(retained_snapshot(&app), snapshot);
        assert_ne!(
            scroll(&app),
            old_scroll,
            "a newly accepted batch has its own actual scroll identity"
        );
        assert_eq!(app.search_state.selected_index(), Some(0));
        let rect = recorded_rect(&app, &row(0));
        let clip = recorded_clip(&app, &row(0));
        assert!(
            clip.contains_rect(rect),
            "bottom={bottom}: new first result {rect:?} outside {clip:?}"
        );
        assert_eq!(focused(&app), Some(query_focus));
        assert_draft(&app, departure, &undo);
        assert!(app.location_history.back.is_empty());
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn browse_retained_focused_clipped_row_rejects_keyboard_until_visible_again() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (mut app, commands) = fixture();
        let departure = selection_signature(selection(&app));
        let undo = undo_texts(&app);
        // With shared filters, the 150-point wide/13pt layout placed the row's
        // bottom at 918 versus content bottom 859: a measured 59-point deficit.
        // Round the sufficient initial allocation up to the next ten points;
        // the negative phase below retains the documented 150-point boundary.
        app.tools_height = Some(210.0);
        let geometry = |app: &CedarApp, font: f32, size: egui::Vec2, stage: &str, frame: usize| {
            let panel =
                egui::containers::panel::PanelState::load(&app.editor_ctx, egui::Id::new("tools"))
                    .map(|state| state.rect);
            let (scroll, viewport, content) = recorded_results_geometry(app, true);
            let offset =
                egui::scroll_area::State::load(&app.editor_ctx, scroll).map(|state| state.offset);
            eprintln!(
                "surface=Bottom case=retained_focused_clipped activation={activation:?} stage={stage} frame={frame} font={font} window={size:?} pass={} focus={:?} preference={:?} panel={panel:?} row_rect={:?} row_clip={:?} group_rect={:?} query_rect={:?} scope_rect={:?} masks_rect={:?} scroll={scroll:?} viewport={viewport:?} content={content:?} offset={offset:?}",
                app.editor_ctx.cumulative_pass_nr(),
                focused(app),
                app.tools_height,
                recorded_rect(app, "search_row:0"),
                recorded_clip(app, "search_row:0"),
                recorded_rect(app, "search_group:0"),
                recorded_rect(app, "search_query"),
                recorded_rect(app, "search_scope"),
                recorded_rect(app, "search_masks"),
            );
        };
        app.editor_ctx.style_mut(|style| {
            style
                .text_styles
                .insert(egui::TextStyle::Body, FontId::proportional(13.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, FontId::proportional(13.0));
            style
                .text_styles
                .insert(egui::TextStyle::Monospace, FontId::monospace(13.0));
        });
        enter(&mut app, true);
        query(&mut app, "needle");
        frame(&mut app, key(egui::Key::Enter));
        let search = commands.try_recv().unwrap();
        assert!(matches!(&search.op, Operation::Search { query, limit: 500 } if query == "needle"));
        reply(
            &mut app,
            search.id,
            Ok(Payload::Matches {
                matches: (0..32)
                    .map(|_| SearchMatch {
                        path: "origin.txt".into(),
                        line: 2,
                        text: "needle historical match".into(),
                    })
                    .collect(),
                truncated: true,
            }),
        );
        settle(&mut app);
        let snapshot = retained_snapshot(&app);
        frame(&mut app, key(egui::Key::Escape));
        let wide = egui::vec2(1280.0, 900.0);
        for _ in 0..4 {
            frame_at_size(&mut app, wide, vec![]);
        }
        frame_at_size(
            &mut app,
            wide,
            modified_key(egui::Key::J, command_modifiers()),
        );
        for index in 0..4 {
            frame_at_size(&mut app, wide, vec![]);
            geometry(&app, 13.0, wide, "initial visible row", index);
        }
        click_at_size(&mut app, wide, "search_row:0");
        let row = recorded_response(&app, "search_row:0").id;
        assert_eq!(focused(&app), Some(row));
        assert_eq!(app.search_state.selected_index(), Some(0));
        assert!(recorded_rect(&app, "search_row:0")
            .intersect(recorded_clip(&app, "search_row:0"))
            .is_positive());
        // Change the actual frame layout while retaining genuine native focus.
        // No test writes a pointer owner, focus owner, scroll state or key ticket.
        app.tools_height = Some(150.0);
        app.editor_ctx.style_mut(|style| {
            style
                .text_styles
                .insert(egui::TextStyle::Body, FontId::proportional(18.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, FontId::proportional(18.0));
            style
                .text_styles
                .insert(egui::TextStyle::Monospace, FontId::monospace(18.0));
        });
        let narrow = egui::vec2(780.0, 540.0);
        for index in 0..4 {
            frame_at_size(&mut app, narrow, vec![]);
            geometry(&app, 18.0, narrow, "fully clipped row", index);
        }
        let clipped_rect = recorded_rect(&app, "search_row:0");
        let clipped_view = recorded_clip(&app, "search_row:0");
        assert!(!clipped_rect.intersect(clipped_view).is_positive(), "the keyboard guard fixture needs a fully clipped row: rect={clipped_rect:?}, clip={clipped_view:?}");
        assert_eq!(
            focused(&app),
            Some(row),
            "the clipped row must retain actual native focus"
        );
        let epoch = app.navigation_epoch;
        frame_at_size(&mut app, narrow, vec![edge(activation, true, false)]);
        for repeat in [false, true] {
            frame_at_size(&mut app, narrow, vec![edge(activation, true, repeat)]);
        }
        frame_at_size(&mut app, narrow, vec![edge(activation, false, false)]);
        assert_eq!(app.navigation_epoch, epoch);
        assert_eq!(app.search_state.selected_index(), Some(0));
        assert!(!app.has_search_preview());
        assert!(app.location_history.back.is_empty());
        assert!(
            commands.try_recv().is_err(),
            "focused but invisible Enter/Space must not activate"
        );
        assert_draft(&app, departure, &undo);
        let automatic = recorded_response(&app, "tools_automatic_height").id;
        for _ in 0..24 {
            if focused(&app) == Some(automatic) {
                break;
            }
            frame_at_size(
                &mut app,
                narrow,
                modified_key(egui::Key::Tab, egui::Modifiers::SHIFT),
            );
            // Pinned egui Focus::interested_in_focus defers Previous into
            // id_next_frame. Observe that production move before issuing the
            // next physical Tab; otherwise the loop queues a second predecessor
            // while its reported focus still belongs to the previous frame.
            frame_at_size(&mut app, narrow, vec![]);
        }
        assert_eq!(
            focused(&app),
            Some(automatic),
            "activation={activation:?}: release and native reverse Tab must remain usable"
        );
        frame_at_size(&mut app, narrow, key(egui::Key::Enter));
        let activation_focus = focused(&app);
        let activation_clicked = recorded_response(&app, "tools_automatic_height").clicked();
        let activation_keys = app.editor_ctx.input(|input| key_metadata(&input.events));
        for _ in 0..4 {
            frame_at_size(&mut app, narrow, vec![]);
        }
        assert_eq!(app.tools_height, None, "blocked={activation:?}, phase=Automatic Enter, target={automatic:?}, focus={activation_focus:?}, reconstructed_clicked={activation_clicked}, residual_keys={activation_keys:?}");
        click_at_size(&mut app, narrow, "search_row:0");
        frame_at_size(&mut app, narrow, key(activation));
        for _ in 0..4 {
            frame_at_size(&mut app, narrow, vec![]);
        }
        if activation == egui::Key::Space {
            assert!(
                app.has_search_preview(),
                "a fresh visible Space is not swallowed after the blocked lifetime"
            );
            assert_eq!(app.search_preview_text(), Some(DRAFT));
            assert_draft(&app, departure, &undo);
        } else {
            assert_eq!(
                app.navigation_epoch,
                epoch + 1,
                "a fresh visible Enter performs ordinary Open"
            );
            assert_eq!(app.documents[0].cursor, (2, 1));
            assert_eq!(app.location_history.back.len(), 1);
            assert_eq!(app.documents[0].text, DRAFT);
            assert_eq!(undo_texts(&app), undo);
        }
        assert_eq!(retained_snapshot(&app), snapshot);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn browse_retained_bottom_preview_reveal_yields_to_newer_query_tab_and_wheel() {
    for newer in ["query", "tab", "wheel"] {
        let (mut app, commands) = fixture();
        let departure = selection_signature(selection(&app));
        let undo = undo_texts(&app);
        enter(&mut app, true);
        submit_text(&mut app, &commands, &vec!["origin.txt"; 80]);
        let snapshot = retained_snapshot(&app);
        frame(&mut app, key(egui::Key::Escape));
        settle(&mut app);
        frame(&mut app, modified_key(egui::Key::J, command_modifiers()));
        settle(&mut app);
        click(&mut app, "search_row:0");
        frame(&mut app, key(egui::Key::End));
        settle(&mut app);
        let row = recorded_response(&app, "search_row:79").id;
        assert_eq!(focused(&app), Some(row));
        let scroll = crate::search_keyboard_tests::recorded_scroll_id(&app);
        let before_preview = egui::scroll_area::State::load(&app.editor_ctx, scroll)
            .unwrap()
            .offset;
        let query_id = recorded_response(&app, "search_query").id;
        let query_pos = visible_center(&app, "search_query");
        let row_visible =
            recorded_rect(&app, "search_row:79").intersect(recorded_clip(&app, "search_row:79"));
        let wheel_pos = egui::pos2(row_visible.left() + 6.0, row_visible.center().y);
        let epoch = app.navigation_epoch;
        frame(&mut app, key(egui::Key::Space));
        assert!(app.has_search_preview());
        assert!(commands.try_recv().is_err());
        // Deliberately no empty presentation frame between Space and the newer
        // native intent: a delayed reveal must neither refocus nor undo a wheel.
        match newer {
            "query" => {
                frame(&mut app, pointer(query_pos, true));
                assert!(recorded_response(&app, "search_query").is_pointer_button_down_on());
                assert_eq!(
                    focused(&app),
                    Some(query_id),
                    "preview reveal cannot overwrite the newer query pointer focus"
                );
                frame(&mut app, pointer(query_pos, false));
                assert!(recorded_response(&app, "search_query")
                    .clicked_by(egui::PointerButton::Primary));
                settle(&mut app);
                assert_eq!(focused(&app), Some(query_id));
            }
            "tab" => {
                frame(&mut app, key(egui::Key::Tab));
                let newer_focus = focused(&app);
                assert_ne!(
                    newer_focus,
                    Some(row),
                    "native Tab must actually leave the previewed row"
                );
                settle(&mut app);
                assert_eq!(
                    focused(&app),
                    newer_focus,
                    "preview reveal cannot restore old row focus after Tab"
                );
            }
            _ => {
                frame(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(wheel_pos),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, 120.0),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
                let after_wheel = egui::scroll_area::State::load(&app.editor_ctx, scroll)
                    .unwrap()
                    .offset;
                assert!(after_wheel.y < before_preview.y, "the actual wheel must move results up: before={before_preview:?}, after={after_wheel:?}");
                settle(&mut app);
                let settled = egui::scroll_area::State::load(&app.editor_ctx, scroll)
                    .unwrap()
                    .offset;
                assert!(settled.y < before_preview.y, "delayed preview reveal must not override newer wheel scrolling: before={before_preview:?}, after={settled:?}");
            }
        }
        assert_eq!(retained_snapshot(&app), snapshot);
        assert_eq!(app.search_state.selected_index(), Some(79));
        assert_eq!(app.navigation_epoch, epoch);
        assert_draft(&app, departure, &undo);
        assert!(app.location_history.back.is_empty());
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn browse_retained_preview_intent_rejects_selection_group_and_reopen_aba() {
    for bottom in [false, true] {
        for change in ["selection", "group", "reopen"] {
            for failed in [false, true] {
                let (mut app, commands) = fixture();
                let departure = selection_signature(selection(&app));
                let undo = undo_texts(&app);
                enter(&mut app, true);
                submit_text(&mut app, &commands, &["a.txt", "b.txt"]);
                let snapshot = retained_snapshot(&app);
                if bottom {
                    frame(&mut app, key(egui::Key::Escape));
                    settle(&mut app);
                    frame(&mut app, modified_key(egui::Key::J, command_modifiers()));
                    settle(&mut app);
                }
                let row = |index| {
                    format!(
                        "{}:{index}",
                        if bottom { "search_row" } else { "browse_row" }
                    )
                };
                let group = format!(
                    "{}:0",
                    if bottom {
                        "search_group"
                    } else {
                        "browse_group"
                    }
                );
                click(&mut app, &row(0));
                frame(&mut app, key(egui::Key::Space));
                let obsolete = commands.try_recv().unwrap();
                assert!(matches!(&obsolete.op, Operation::Read { path } if path == "a.txt"));
                let first_serial = app.browse.preview_serial();
                settle(&mut app);
                if bottom {
                    frame(&mut app, key(egui::Key::Space));
                } else {
                    click(&mut app, "browse_preview");
                }
                assert_eq!(
                    app.browse.preview_serial(),
                    first_serial,
                    "repeating the same live preview is idempotent"
                );
                assert!(commands.try_recv().is_err());
                match change {
                    "selection" => {
                        click(&mut app, &row(1));
                        frame(&mut app, key(egui::Key::Space));
                        settle(&mut app);
                        assert!(
                            commands.try_recv().is_err(),
                            "B remains coalesced behind A's physical Read"
                        );
                    }
                    "group" => {
                        click(&mut app, &group);
                        settle(&mut app);
                        assert!(!app.search_state.group_expanded(0));
                        click(&mut app, &group);
                        settle(&mut app);
                        assert!(app.search_state.group_expanded(0));
                    }
                    _ if bottom => {
                        click(&mut app, "close_tools");
                        settle(&mut app);
                        frame(&mut app, modified_key(egui::Key::J, command_modifiers()));
                        settle(&mut app);
                    }
                    _ => {
                        frame(&mut app, key(egui::Key::Escape));
                        frame(&mut app, key(egui::Key::Escape));
                        enter(&mut app, true);
                    }
                }
                click(&mut app, &row(0));
                frame(&mut app, key(egui::Key::Space));
                let replacement_serial = app.browse.preview_serial();
                assert!(
                    replacement_serial > first_serial,
                    "bottom={bottom}, change={change}: A returning is a new preview intent"
                );
                assert!(app.browse.busy());
                assert!(
                    commands.try_recv().is_err(),
                    "only one physical preview Read may be outstanding"
                );
                app.error = Some("current preview status".into());
                reply(
                    &mut app,
                    obsolete.id,
                    if failed {
                        Err("obsolete A preview failure".into())
                    } else {
                        Ok(file("a.txt", "obsolete A preview bytes"))
                    },
                );
                settle(&mut app);
                assert!(app.browse.preview_text().is_none());
                assert!(app.search_preview_text().is_none());
                assert!(app
                    .browse
                    .preview_message()
                    .is_none_or(|message| !message.contains("obsolete A")));
                assert_eq!(app.error.as_deref(), Some("current preview status"));
                let fresh = commands.try_recv().expect(
                    "draining the retired Read admits exactly the latest explicit A intent",
                );
                assert_ne!(fresh.id, obsolete.id);
                assert!(matches!(&fresh.op, Operation::Read { path } if path == "a.txt"));
                assert!(
                    commands.try_recv().is_err(),
                    "no intermediate B Read or duplicate A Read"
                );
                reply(
                    &mut app,
                    fresh.id,
                    Ok(file("a.txt", "fresh A preview bytes")),
                );
                settle(&mut app);
                assert_eq!(
                    if bottom {
                        app.search_preview_text()
                    } else {
                        app.browse.preview_text()
                    },
                    Some("fresh A preview bytes")
                );
                assert_eq!(app.browse.preview_serial(), replacement_serial);
                assert!(!app.browse.busy());
                if bottom {
                    frame(&mut app, key(egui::Key::Space));
                } else {
                    click(&mut app, "browse_preview");
                }
                assert_eq!(app.browse.preview_serial(), replacement_serial);
                assert!(
                    commands.try_recv().is_err(),
                    "completed live preview remains idempotent"
                );
                assert_eq!(retained_snapshot(&app), snapshot);
                assert_eq!(app.active_document, Some(1));
                assert_eq!(app.documents.len(), 1);
                assert_draft(&app, departure, &undo);
                assert!(app.location_history.back.is_empty());
                assert!(app.location_history.forward.is_empty());
            }
        }
    }
}

#[test]
fn browse_retained_preview_serial_exhaustion_refuses_new_intent_and_drains_old_flight() {
    for bottom in [false, true] {
        for failed in [false, true] {
            let (mut app, commands) = fixture();
            let departure = selection_signature(selection(&app));
            let undo = undo_texts(&app);
            enter(&mut app, true);
            submit_text(&mut app, &commands, &["a.txt", "b.txt"]);
            let snapshot = retained_snapshot(&app);
            if bottom {
                frame(&mut app, key(egui::Key::Escape));
                settle(&mut app);
                frame(&mut app, modified_key(egui::Key::J, command_modifiers()));
                settle(&mut app);
            }
            let row = |index| {
                format!(
                    "{}:{index}",
                    if bottom { "search_row" } else { "browse_row" }
                )
            };
            app.browse.set_preview_serial_for_test(u64::MAX - 1);
            click(&mut app, &row(0));
            frame(&mut app, key(egui::Key::Space));
            let final_identity = commands.try_recv().unwrap();
            assert!(matches!(&final_identity.op, Operation::Read { path } if path == "a.txt"));
            assert_eq!(app.browse.preview_serial(), u64::MAX);
            settle(&mut app);
            if bottom {
                frame(&mut app, key(egui::Key::Space));
            } else {
                click(&mut app, "browse_preview");
            }
            assert_eq!(app.browse.preview_serial(), u64::MAX);
            assert!(
                commands.try_recv().is_err(),
                "the same live final identity does not consume another serial"
            );
            click(&mut app, &row(1));
            frame(&mut app, key(egui::Key::Space));
            settle(&mut app);
            assert_eq!(
                app.browse.preview_serial(),
                u64::MAX,
                "preview identity never wraps"
            );
            assert!(!app.browse.preview_visible());
            assert!(!app.has_search_preview());
            assert!(
                app.browse.busy(),
                "refusal cannot release an outstanding physical Read"
            );
            assert!(commands.try_recv().is_err());
            let refusal = app
                .error
                .clone()
                .expect("preview exhaustion needs a visible refusal");
            assert!(refusal.contains("preview identity counter is exhausted"));
            assert!(has_text(&frame(&mut app, vec![]), &refusal));
            reply(
                &mut app,
                final_identity.id,
                if failed {
                    Err("retired exhausted preview error".into())
                } else {
                    Ok(file("a.txt", "retired exhausted preview bytes"))
                },
            );
            settle(&mut app);
            assert!(
                !app.browse.busy(),
                "the actual old completion drains the physical flight"
            );
            assert!(!app.browse.preview_visible());
            assert!(app.browse.preview_text().is_none());
            assert!(app.search_preview_text().is_none());
            assert_eq!(app.error.as_deref(), Some(refusal.as_str()));
            assert_eq!(app.browse.preview_serial(), u64::MAX);
            assert!(
                commands.try_recv().is_err(),
                "drain must not dispatch a refused replacement"
            );
            assert_eq!(retained_snapshot(&app), snapshot);
            assert_eq!(app.active_document, Some(1));
            assert_eq!(app.documents.len(), 1);
            assert_draft(&app, departure, &undo);
            assert!(app.location_history.back.is_empty());
        }
    }
}

#[path = "files_live_tests.rs"]
mod files_live;

const NORMAL_TARGET: &str = "untouched/deep/Needle.rs";
const NORMAL_TARGET_TEXT: &str = "first\nneedle target\nlast\n";
const NORMAL_ACTIVE: std::time::Duration = std::time::Duration::from_secs(240);
const NORMAL_CALL: std::time::Duration = std::time::Duration::from_secs(30);
const NORMAL_CLEANUP: std::time::Duration = std::time::Duration::from_secs(30);
const NORMAL_TOTAL: std::time::Duration = std::time::Duration::from_secs(300);

// Read-only shell witness shared by the two normal-agent journeys. Observe
// complete native Undo/Redo entries, including cursor affinity, rather than
// equating an unchanged text buffer with an unchanged editor/history state.
pub(super) fn normal_shell_snapshot(app: &CedarApp) -> serde_json::Value {
    let documents: Vec<_> = app
        .documents
        .iter()
        .map(|document| {
            let state =
                egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", document.id)))
                    .expect("normal shell proof document has native editor state");
            let selection = state.cursor.char_range().unwrap();
            let history: Vec<_> = [false, true]
                .into_iter()
                .map(|redo| {
                    let mut undoer = state.undoer();
                    let mut current = (selection, document.text.clone());
                    let mut entries = Vec::new();
                    for _ in 0..=editor_state::MAX_UNDO_STATES + 1 {
                        let next = if redo {
                            undoer.redo(&current)
                        } else {
                            undoer.undo(&current)
                        }
                        .cloned();
                        let Some(next) = next else { break };
                        entries.push((selection_signature(next.0), next.1.clone()));
                        current = next;
                    }
                    assert!(entries.len() <= editor_state::MAX_UNDO_STATES + 1);
                    entries
                })
                .collect();
            serde_json::json!({
                "id": document.id, "path": document.path,
                "text": document.text, "saved_text": document.saved_text,
                "revision": document.revision, "dirty": document.dirty(),
                "cursor": document.cursor, "edit_version": document.edit_version,
                "selection": selection_signature(selection), "history": history,
            })
        })
        .collect();
    let locations = |stack: &[location_history::Location]| {
        stack
            .iter()
            .map(|location| {
                (
                    location.generation,
                    location.document,
                    location.edit_version,
                    selection_signature(location.selection),
                )
            })
            .collect::<Vec<_>>()
    };
    let groups = app.search_state.group_rows();
    let expanded: Vec<_> = groups
        .iter()
        .map(|group| (group[0], app.search_state.group_expanded(group[0])))
        .collect();
    serde_json::json!({
        "active_document": app.active_document, "documents": documents,
        "back": locations(&app.location_history.back),
        "forward": locations(&app.location_history.forward),
        "navigation_pending": app.location_history.pending.is_some(),
        "navigation_message": app.location_history.message,
        "query": app.search_query, "scope": app.search_scope, "masks": app.search_masks,
        "results": app.search_results, "truncated": app.search_truncated,
        "request": app.search_request, "selected": app.search_state.selected_index(),
        "snapshot": app.search_state.accepted_snapshot_id().map(|id| id.value()),
        "rows_active": app.search_rows_active(), "groups": groups, "expanded": expanded,
        "visible": app.search_state.visible_indices(), "preview": app.has_search_preview(),
    })
}

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
    retained_text_snapshot: bool,
    grouped_original_indices: bool,
    text_open_back_exact: bool,
    reopen_keeps_selection: bool,
    grouped_navigation_additional_operations: Option<usize>,
    retained_dirty_preview_additional_reads: Option<usize>,
    shell_project_focus_retains_find: bool,
    shell_find_reopen_active_hit: bool,
    shell_hide_exact_editor_return: bool,
    shell_state_preserved: bool,
    shell_transition_operations: Option<usize>,
    recent_popup_filter_select_operations: Option<usize>,
    recent_closed_reopen_reads: Option<usize>,
    recent_closed_reopen_normal_position: bool,
    recent_live_dirty_reuse_reads: Option<usize>,
    recent_live_dirty_reuse_exact: bool,
    recent_find_state_preserved: bool,
    project_reveal_ancestor_lists: Option<usize>,
    project_reveal_additional_reads: Option<usize>,
    project_reveal_cached_operations: Option<usize>,
    project_reveal_selected_and_focused: bool,
    project_reveal_state_preserved: bool,
    project_reveal_cancelled_lists: Option<usize>,
    project_reveal_cancelled_followup_operations: Option<usize>,
    continuous_preview_unarmed_operations: Option<usize>,
    continuous_preview_browse_armed_follow: bool,
    continuous_preview_bottom_armed_follow: bool,
    continuous_preview_dirty_additional_reads: Option<usize>,
    continuous_preview_latest_candidate: bool,
    continuous_preview_unopened_reads: Option<usize>,
    continuous_preview_open_reads: Option<usize>,
    continuous_preview_open_back_exact: bool,
    continuous_preview_state_preserved: bool,
    current_file_find_selection_seeded: bool,
    current_file_find_next_previous_wrap: bool,
    current_file_find_literal_no_match: bool,
    current_file_find_escape_back_exact: bool,
    current_file_find_undo_redo_preserved: bool,
    current_file_find_state_preserved: bool,
    current_file_find_additional_operations: Option<usize>,
    files_live_burst_latest_only: bool,
    files_live_explicit_flush_deduplicated: bool,
    files_live_automatic_reads: Option<usize>,
    files_live_dirty_preview_open_reads: Option<usize>,
    files_live_open_back_exact: bool,
    files_live_state_preserved: bool,
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
            "normal Browse proof dispatched an operation outside its sixteen-operation contract"
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
            retained_text_snapshot: false,
            grouped_original_indices: false,
            text_open_back_exact: false,
            reopen_keeps_selection: false,
            grouped_navigation_additional_operations: None,
            retained_dirty_preview_additional_reads: None,
            shell_project_focus_retains_find: false,
            shell_find_reopen_active_hit: false,
            shell_hide_exact_editor_return: false,
            shell_state_preserved: false,
            shell_transition_operations: None,
            recent_popup_filter_select_operations: None,
            recent_closed_reopen_reads: None,
            recent_closed_reopen_normal_position: false,
            recent_live_dirty_reuse_reads: None,
            recent_live_dirty_reuse_exact: false,
            recent_find_state_preserved: false,
            project_reveal_ancestor_lists: None,
            project_reveal_additional_reads: None,
            project_reveal_cached_operations: None,
            project_reveal_selected_and_focused: false,
            project_reveal_state_preserved: false,
            project_reveal_cancelled_lists: None,
            project_reveal_cancelled_followup_operations: None,
            continuous_preview_unarmed_operations: None,
            continuous_preview_browse_armed_follow: false,
            continuous_preview_bottom_armed_follow: false,
            continuous_preview_dirty_additional_reads: None,
            continuous_preview_latest_candidate: false,
            continuous_preview_unopened_reads: None,
            continuous_preview_open_reads: None,
            continuous_preview_open_back_exact: false,
            continuous_preview_state_preserved: false,
            current_file_find_selection_seeded: false,
            current_file_find_next_previous_wrap: false,
            current_file_find_literal_no_match: false,
            current_file_find_escape_back_exact: false,
            current_file_find_undo_redo_preserved: false,
            current_file_find_state_preserved: false,
            current_file_find_additional_operations: None,
            files_live_burst_latest_only: false,
            files_live_explicit_flush_deduplicated: false,
            files_live_automatic_reads: None,
            files_live_dirty_preview_open_reads: None,
            files_live_open_back_exact: false,
            files_live_state_preserved: false,
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
        assert!(self.retained_text_snapshot);
        assert!(self.grouped_original_indices);
        assert!(self.text_open_back_exact);
        assert!(self.reopen_keeps_selection);
        assert_eq!(self.grouped_navigation_additional_operations, Some(0));
        assert_eq!(self.retained_dirty_preview_additional_reads, Some(0));
        assert!(self.shell_project_focus_retains_find);
        assert!(self.shell_find_reopen_active_hit);
        assert!(self.shell_hide_exact_editor_return);
        assert!(self.shell_state_preserved);
        assert_eq!(self.shell_transition_operations, Some(0));
        assert_eq!(self.recent_popup_filter_select_operations, Some(0));
        assert_eq!(self.recent_closed_reopen_reads, Some(1));
        assert!(self.recent_closed_reopen_normal_position);
        assert_eq!(self.recent_live_dirty_reuse_reads, Some(0));
        assert!(self.recent_live_dirty_reuse_exact);
        assert!(self.recent_find_state_preserved);
        assert_eq!(self.project_reveal_ancestor_lists, Some(3));
        assert_eq!(self.project_reveal_additional_reads, Some(0));
        assert_eq!(self.project_reveal_cached_operations, Some(0));
        assert!(self.project_reveal_selected_and_focused);
        assert!(self.project_reveal_state_preserved);
        assert_eq!(
            explorer_tree::PROJECT_REVEAL_DEADLINE,
            std::time::Duration::from_secs(30)
        );
        assert_eq!(explorer_tree::MAX_PROJECT_REVEAL_LISTS, 32);
        assert_eq!(self.project_reveal_cancelled_lists, Some(1));
        assert_eq!(self.project_reveal_cancelled_followup_operations, Some(0));
        assert_eq!(self.continuous_preview_unarmed_operations, Some(0));
        assert!(self.continuous_preview_browse_armed_follow);
        assert!(self.continuous_preview_bottom_armed_follow);
        assert_eq!(self.continuous_preview_dirty_additional_reads, Some(0));
        assert!(self.continuous_preview_latest_candidate);
        assert_eq!(self.continuous_preview_unopened_reads, Some(1));
        assert_eq!(self.continuous_preview_open_reads, Some(1));
        assert!(self.continuous_preview_open_back_exact);
        assert!(self.continuous_preview_state_preserved);
        assert!(self.current_file_find_selection_seeded);
        assert!(self.current_file_find_next_previous_wrap);
        assert!(self.current_file_find_literal_no_match);
        assert!(self.current_file_find_escape_back_exact);
        assert!(self.current_file_find_undo_redo_preserved);
        assert!(self.current_file_find_state_preserved);
        assert_eq!(self.current_file_find_additional_operations, Some(0));
        assert!(self.files_live_burst_latest_only);
        assert!(self.files_live_explicit_flush_deduplicated);
        assert_eq!(self.files_live_automatic_reads, Some(0));
        assert_eq!(self.files_live_dirty_preview_open_reads, Some(0));
        assert!(self.files_live_open_back_exact);
        assert!(self.files_live_state_preserved);
        assert_eq!(browse::FILES_DEBOUNCE_SECONDS, 0.250);
        assert_eq!(browse::PROJECT_TEXT_REVIEW_DEBOUNCE_SECONDS, 0.150);
        assert_eq!(browse::MAX_PROJECT_TEXT_CONTEXT_LINES, 41);
        assert_eq!(browse::MAX_PROJECT_TEXT_CONTEXT_BYTES, 64 * 1024);
        assert_eq!(
            self.ledger[..12],
            vec![
                serde_json::json!({"op": "Hello"}),
                serde_json::json!({"op": "List", "path": ""}),
                serde_json::json!({"op": "Read", "path": "origin.txt"}),
                serde_json::json!({"op": "FindFiles", "query": "Needle.rs", "scope": "", "limit": 100}),
                serde_json::json!({"op": "Read", "path": NORMAL_TARGET}),
                serde_json::json!({"op": "Read", "path": NORMAL_TARGET}),
                serde_json::json!({"op": "Search", "query": "needle", "limit": 500}),
                serde_json::json!({"op": "Read", "path": NORMAL_TARGET}),
                serde_json::json!({"op": "List", "path": ""}),
                serde_json::json!({"op": "List", "path": "untouched"}),
                serde_json::json!({"op": "List", "path": "untouched/deep"}),
                serde_json::json!({"op": "List", "path": ""}),
            ]
        );
        assert_eq!(
            self.ledger[12..14],
            [
                serde_json::json!({"op": "Read", "path": NORMAL_TARGET}),
                serde_json::json!({"op": "Read", "path": NORMAL_TARGET}),
            ]
        );
        assert_eq!(
            self.ledger[14..],
            [
                serde_json::json!({"op": "FindFiles", "query": "Needle", "scope": "untouched", "limit": 100}),
                serde_json::json!({"op": "FindFiles", "query": "origin.txt", "scope": "", "limit": 100}),
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
        let mut receipt = serde_json::json!({
            "receipt_version": 6,
            "operation_ledger": self.ledger, "source_sha256": self.hashes,
            "connections": 1, "agents_reaped": 1, "trust": false,
            "active_budget_seconds": 240, "cleanup_timeout_seconds": 30,
            "watchdog_seconds": 300, "dirty_preview_and_open_additional_reads": 0,
            "unopened_preview_reads": 1, "unopened_open_reads": 1,
            "retained_text_snapshot": self.retained_text_snapshot,
            "grouped_original_indices": self.grouped_original_indices,
            "text_open_back_exact": self.text_open_back_exact,
            "reopen_keeps_selection": self.reopen_keeps_selection,
            "grouped_navigation_additional_operations": self.grouped_navigation_additional_operations.unwrap(),
            "retained_dirty_preview_additional_reads": self.retained_dirty_preview_additional_reads.unwrap(),
            "shell_project_focus_retains_find": self.shell_project_focus_retains_find,
            "shell_find_reopen_active_hit": self.shell_find_reopen_active_hit,
            "shell_hide_exact_editor_return": self.shell_hide_exact_editor_return,
            "shell_state_preserved": self.shell_state_preserved,
            "shell_transition_operations": self.shell_transition_operations.unwrap(),
            "recent_popup_filter_select_operations": self.recent_popup_filter_select_operations.unwrap(),
            "recent_closed_reopen_reads": self.recent_closed_reopen_reads.unwrap(),
            "recent_closed_reopen_normal_position": self.recent_closed_reopen_normal_position,
            "recent_live_dirty_reuse_reads": self.recent_live_dirty_reuse_reads.unwrap(),
            "recent_live_dirty_reuse_exact": self.recent_live_dirty_reuse_exact,
            "recent_find_state_preserved": self.recent_find_state_preserved,
            "project_reveal_ancestor_lists": self.project_reveal_ancestor_lists.unwrap(),
            "project_reveal_additional_reads": self.project_reveal_additional_reads.unwrap(),
            "project_reveal_cached_operations": self.project_reveal_cached_operations.unwrap(),
            "project_reveal_selected_and_focused": self.project_reveal_selected_and_focused,
            "project_reveal_state_preserved": self.project_reveal_state_preserved,
            "project_reveal_cancelled_lists": self.project_reveal_cancelled_lists.unwrap(),
            "project_reveal_cancelled_followup_operations": self.project_reveal_cancelled_followup_operations.unwrap(),
            "project_reveal_intent_timeout_seconds": explorer_tree::PROJECT_REVEAL_DEADLINE.as_secs(),
            "project_reveal_max_ancestor_lists": explorer_tree::MAX_PROJECT_REVEAL_LISTS,
        });
        let continuous = serde_json::json!({
            "continuous_preview_unarmed_operations": self.continuous_preview_unarmed_operations.unwrap(),
            "continuous_preview_browse_armed_follow": self.continuous_preview_browse_armed_follow,
            "continuous_preview_bottom_armed_follow": self.continuous_preview_bottom_armed_follow,
            "continuous_preview_dirty_additional_reads": self.continuous_preview_dirty_additional_reads.unwrap(),
            "continuous_preview_latest_candidate": self.continuous_preview_latest_candidate,
            "continuous_preview_unopened_reads": self.continuous_preview_unopened_reads.unwrap(),
            "continuous_preview_open_reads": self.continuous_preview_open_reads.unwrap(),
            "continuous_preview_open_back_exact": self.continuous_preview_open_back_exact,
            "continuous_preview_state_preserved": self.continuous_preview_state_preserved,
            "continuous_preview_debounce_ms": (browse::PROJECT_TEXT_REVIEW_DEBOUNCE_SECONDS * 1000.0) as u64,
            "continuous_preview_max_context_lines": browse::MAX_PROJECT_TEXT_CONTEXT_LINES,
            "continuous_preview_max_context_bytes": browse::MAX_PROJECT_TEXT_CONTEXT_BYTES,
        });
        receipt
            .as_object_mut()
            .unwrap()
            .extend(continuous.as_object().unwrap().clone());
        let current_file = serde_json::json!({
            "current_file_find_selection_seeded": self.current_file_find_selection_seeded,
            "current_file_find_next_previous_wrap": self.current_file_find_next_previous_wrap,
            "current_file_find_literal_no_match": self.current_file_find_literal_no_match,
            "current_file_find_escape_back_exact": self.current_file_find_escape_back_exact,
            "current_file_find_undo_redo_preserved": self.current_file_find_undo_redo_preserved,
            "current_file_find_state_preserved": self.current_file_find_state_preserved,
            "current_file_find_additional_operations": self.current_file_find_additional_operations.unwrap(),
        });
        receipt
            .as_object_mut()
            .unwrap()
            .extend(current_file.as_object().unwrap().clone());
        let files_live = serde_json::json!({
            "files_live_debounce_ms": (browse::FILES_DEBOUNCE_SECONDS * 1000.0) as u64,
            "files_live_burst_latest_only": self.files_live_burst_latest_only,
            "files_live_explicit_flush_deduplicated": self.files_live_explicit_flush_deduplicated,
            "files_live_automatic_reads": self.files_live_automatic_reads.unwrap(),
            "files_live_dirty_preview_open_reads": self.files_live_dirty_preview_open_reads.unwrap(),
            "files_live_open_back_exact": self.files_live_open_back_exact,
            "files_live_state_preserved": self.files_live_state_preserved,
        });
        receipt
            .as_object_mut()
            .unwrap()
            .extend(files_live.as_object().unwrap().clone());
        println!("browse_normal_acceptance {receipt}");
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

// Recent extends the same actual-frame/normal-agent journey. These helpers
// use rendered controls only; the recorded path is never an open shortcut.
fn normal_recent_select(app: &mut CedarApp, query_text: &str, path: &str) {
    frame(app, modified_key(egui::Key::E, command_modifiers()));
    settle(app);
    assert_eq!(
        focused(app),
        Some(recorded_response(app, "recent_query").id)
    );
    click(app, "recent_query");
    frame(app, modified_key(egui::Key::A, command_modifiers()));
    frame(app, vec![egui::Event::Text(query_text.into())]);
    settle(app);
    assert_eq!(app.recent_files.visible_paths(), vec![path]);
    let probe = format!("recent_row:{path}");
    let response = recorded_response(app, &probe);
    assert!(response.enabled() && recorded_clip(app, &probe).contains_rect(response.rect));
    click(app, &probe);
    settle(app);
    assert_eq!(app.recent_files.selected_path(), Some(path));
    assert!(recorded_response(app, "recent_open").enabled());
}

fn normal_editor_position(app: &CedarApp) -> ((usize, usize), (usize, usize, bool, bool)) {
    let doc = app.active().unwrap();
    let selection = egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", doc.id)))
        .unwrap()
        .cursor
        .char_range()
        .unwrap();
    (doc.cursor, selection_signature(selection))
}

fn normal_origin_state(app: &CedarApp) -> serde_json::Value {
    let snapshot = normal_shell_snapshot(app);
    snapshot["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|document| document["path"] == "origin.txt")
        .expect("dirty origin must remain open")
        .clone()
}

// Only the frontend clock is controlled. Input still passes through the real
// raw hook and update; every operation below uses the same normal Client.
fn normal_files_edit(app: &mut CedarApp, probe: &str, text: &str) {
    continuity_click(app, probe, false);
    continuity_tick(app, 0.01, modified_key(egui::Key::A, command_modifiers()));
    continuity_tick(app, 0.01, vec![egui::Event::Text(text.into())]);
}

// Review is allowed to change the selected result and preview visibility only.
// Keep editor selection/affinity, Undo/Redo, navigation and retained Search
// evidence in the same existing shell witness while comparing that journey.
fn normal_continuous_review_state(app: &CedarApp) -> serde_json::Value {
    let mut snapshot = normal_shell_snapshot(app);
    let object = snapshot.as_object_mut().unwrap();
    object.remove("selected");
    object.remove("preview");
    snapshot
}

// Find may move only the active origin's cursor/selection and location stacks.
// Retain every other document field, full Undo/Redo entries and Search state.
fn normal_current_file_find_state(app: &CedarApp) -> serde_json::Value {
    let mut snapshot = normal_shell_snapshot(app);
    let object = snapshot.as_object_mut().unwrap();
    object.remove("back");
    object.remove("forward");
    for document in object["documents"].as_array_mut().unwrap() {
        if document["path"] == "origin.txt" {
            let document = document.as_object_mut().unwrap();
            document.remove("cursor");
            document.remove("selection");
        }
    }
    snapshot
}

fn normal_current_file_find_hit(
    app: &mut CedarApp,
    commands: &Receiver<Command>,
    start: usize,
    end: usize,
    ordinal: usize,
) {
    settle(app);
    assert_eq!(
        selection_signature(selection(app)),
        (end, start, false, false)
    );
    assert_eq!(app.active().unwrap().path, "origin.txt");
    assert_eq!(
        app.active().unwrap().cursor,
        model::cursor_location(DRAFT, end)
    );
    assert_eq!(focused(app), Some(recorded_response(app, "editor").id));
    assert!(has_text(
        &frame(app, vec![]),
        &format!("{ordinal} of 3 matches")
    ));
    assert!(
        commands.try_recv().is_err(),
        "current-file navigation must dispatch no operation"
    );
}

#[test]
fn recent_closed_reopen_matches_independent_ordinary_open_after_native_settling() {
    for current_text in [NORMAL_TARGET_TEXT, "changed shorter\n"] {
        let mut positions = Vec::new();
        for through_recent in [false, true] {
            let (mut app, commands) = fixture();
            let mut closed_id = None;
            let mut closed_selection = None;
            if through_recent {
                app.open(NORMAL_TARGET.into(), None);
                let old = commands.try_recv().unwrap();
                assert!(matches!(&old.op, Operation::Read { path } if path == NORMAL_TARGET));
                reply(
                    &mut app,
                    old.id,
                    Ok(file(NORMAL_TARGET, NORMAL_TARGET_TEXT)),
                );
                settle(&mut app);
                closed_id = app.active_document;
                assert_eq!(focused(&app), Some(recorded_response(&app, "editor").id));
                frame(&mut app, modified_key(egui::Key::Home, command_modifiers()));
                frame(&mut app, key(egui::Key::ArrowDown));
                frame(
                    &mut app,
                    modified_key(egui::Key::ArrowRight, egui::Modifiers::SHIFT),
                );
                settle(&mut app);
                let range = egui::TextEdit::load_state(
                    &app.editor_ctx,
                    egui::Id::new(("editor", closed_id.unwrap())),
                )
                .unwrap()
                .cursor
                .char_range()
                .unwrap();
                assert_ne!(range.primary.index, range.secondary.index);
                closed_selection = Some(selection_signature(range));
                assert!(!app.active().unwrap().dirty());
                frame(&mut app, modified_key(egui::Key::W, command_modifiers()));
                settle(&mut app);
                assert!(!app.documents.iter().any(|doc| doc.path == NORMAL_TARGET));
                normal_recent_select(&mut app, "Needle", NORMAL_TARGET);
                assert!(commands.try_recv().is_err());
                click(&mut app, "recent_open");
            } else {
                app.open(NORMAL_TARGET.into(), None);
            }
            let read = commands.try_recv().unwrap();
            assert!(matches!(&read.op, Operation::Read { path } if path == NORMAL_TARGET));
            assert!(
                matches!(
                    app.pending.get(&read.id),
                    Some(Job::Open { line: None, .. })
                ),
                "ordinary and Recent reads must carry no stored line hint"
            );
            assert!(commands.try_recv().is_err());
            reply(
                &mut app,
                read.id,
                Ok(Payload::File {
                    path: NORMAL_TARGET.into(),
                    text: current_text.into(),
                    revision: "c".repeat(64),
                }),
            );
            settle(&mut app);
            let doc = app.active().unwrap();
            assert_eq!(doc.path, NORMAL_TARGET);
            assert_eq!(doc.text, current_text);
            assert_eq!(doc.revision.as_deref(), Some("c".repeat(64).as_str()));
            assert_ne!(Some(doc.id), closed_id);
            let range =
                egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", doc.id)))
                    .unwrap()
                    .cursor
                    .char_range()
                    .unwrap();
            let position = (doc.cursor, selection_signature(range));
            assert_ne!(
                Some(position.1),
                closed_selection,
                "closed caret is not restored"
            );
            assert_eq!(
                range.primary.index,
                current_text.chars().count(),
                "pinned ordinary TextEdit default"
            );
            assert_eq!(range.primary.index, range.secondary.index);
            assert_eq!(focused(&app), Some(recorded_response(&app, "editor").id));
            assert!(commands.try_recv().is_err());
            positions.push(position);
        }
        assert_eq!(
            positions[0], positions[1],
            "Recent must match the independent ordinary Open default for current bytes"
        );
    }
}

#[test]
#[ignore = "requires CEDAR_BROWSE_AGENT_BIN selecting the normal shipping agent"]
fn browse_normal_agent_find_recent_project_reveal_continuous_preview_and_exact_return() {
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
    // Measure the settled ordinary Open(None) default from the existing
    // first-open operation, independently of the later Recent reopen.
    let ordinary_target_position = normal_editor_position(&app);
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
    let snapshot = retained_snapshot(&app);
    let retained_paths: Vec<String> = app.browse.paths().into_iter().map(str::to_owned).collect();
    let grouping_ledger_start = proof.ledger.len();
    let origin = app
        .browse
        .paths()
        .iter()
        .position(|path| *path == "origin.txt")
        .unwrap();
    let target = app
        .browse
        .paths()
        .iter()
        .position(|path| *path == NORMAL_TARGET)
        .unwrap();
    assert_ne!(origin, target);
    let groups = app.search_state.group_rows();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups, (0..2).map(|index| vec![index]).collect::<Vec<_>>());
    assert_eq!(app.search_results[origin].path, "origin.txt");
    assert_eq!(app.search_results[target].path, NORMAL_TARGET);
    proof.grouped_original_indices = true;
    click(&mut app, &format!("browse_group:{target}"));
    settle(&mut app);
    assert!(!app.search_state.group_expanded(target));
    assert_eq!(retained_snapshot(&app), snapshot);
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
    assert!(app.location_history.back.is_empty());
    assert_eq!(app.location_history.forward.len(), 1);
    assert!(commands.try_recv().is_err());
    enter(&mut app, true);
    assert_eq!(retained_snapshot(&app), snapshot);
    assert_eq!(
        app.browse.paths(),
        retained_paths
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
    assert_eq!(app.browse.selected_index(), Some(origin));
    assert!(!app.search_state.group_expanded(target));
    assert!(!app.browse.preview_visible());
    proof.reopen_keeps_selection = true;
    click(&mut app, &format!("browse_row:{origin}"));
    let preview_ledger_start = proof.ledger.len();
    frame(&mut app, key(egui::Key::Space));
    settle(&mut app);
    assert!(app.browse.preview_visible());
    assert!(has_text(&frame(&mut app, vec![]), "dirty Ω😀"));
    assert_draft_revision(&app, departure, &undo, &revision);
    assert!(
        commands.try_recv().is_err(),
        "retained dirty preview sends no Read"
    );
    proof.retained_dirty_preview_additional_reads = Some(proof.ledger.len() - preview_ledger_start);
    frame(&mut app, key(egui::Key::Escape));
    frame(&mut app, key(egui::Key::Escape));
    settle(&mut app);
    frame(&mut app, modified_key(egui::Key::J, command_modifiers()));
    settle(&mut app);
    assert!(app.tools_open && app.tool == Tool::Search);
    assert_eq!(retained_snapshot(&app), snapshot);
    assert_eq!(app.search_state.selected_index(), Some(origin));
    assert!(!app.search_state.group_expanded(target));
    assert!(!app.has_search_preview());
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, &format!("search_row:{origin}")).id),
        "Ctrl+J reopen must focus the retained active hit"
    );
    click(&mut app, &format!("search_group:{target}"));
    settle(&mut app);
    click(&mut app, &format!("search_row:{target}"));
    assert_eq!(app.search_state.selected_index(), Some(target));
    assert_eq!(
        app.active_document,
        Some(1),
        "single click only selects the existing target"
    );
    assert!(commands.try_recv().is_err());
    frame(&mut app, key(egui::Key::Enter));
    settle(&mut app);
    assert_eq!(app.active().unwrap().path, NORMAL_TARGET);
    assert_eq!(app.active().unwrap().cursor, (2, 1));
    assert_eq!(app.location_history.back.len(), 1);
    assert!(
        commands.try_recv().is_err(),
        "ordinary text Open reuses the already-open target"
    );
    frame(
        &mut app,
        modified_key(egui::Key::OpenBracket, command_modifiers()),
    );
    settle(&mut app);
    assert_draft_revision(&app, departure, &undo, &revision);
    assert!(app.location_history.back.is_empty());
    assert_eq!(app.location_history.forward.len(), 1);
    proof.text_open_back_exact = true;

    let shell_ledger_start = proof.ledger.len();
    let shell_state = normal_shell_snapshot(&app);
    let editor = recorded_response(&app, "editor").id;
    assert_eq!(focused(&app), Some(editor));
    frame(
        &mut app,
        modified_key(egui::Key::Num1, egui::Modifiers::ALT),
    );
    settle(&mut app);
    let project = recorded_response(&app, "origin.txt");
    assert!(project.enabled() && project.sense.is_focusable());
    assert!(recorded_clip(&app, "origin.txt").contains_rect(project.rect));
    assert_eq!(focused(&app), Some(project.id));
    assert!(app.tools_open && app.tool == Tool::Search);
    assert_eq!(normal_shell_snapshot(&app), shell_state);
    assert!(commands.try_recv().is_err());
    proof.shell_project_focus_retains_find = true;

    frame(&mut app, modified_key(egui::Key::J, command_modifiers()));
    settle(&mut app);
    assert!(!app.tools_open);
    assert_eq!(focused(&app), Some(editor));
    assert_eq!(recorded_response(&app, "editor").id, editor);
    assert_eq!(normal_shell_snapshot(&app), shell_state);
    assert!(commands.try_recv().is_err());
    proof.shell_hide_exact_editor_return = true;

    click(&mut app, "find_results_entry");
    settle(&mut app);
    assert!(app.tools_open && app.tool == Tool::Search);
    let hit_probe = format!("search_row:{target}");
    let hit = recorded_response(&app, &hit_probe);
    assert!(hit.enabled() && recorded_clip(&app, &hit_probe).contains_rect(hit.rect));
    assert_eq!(focused(&app), Some(hit.id));
    assert_eq!(normal_shell_snapshot(&app), shell_state);
    assert_draft_revision(&app, departure, &undo, &revision);
    assert!(commands.try_recv().is_err());
    proof.shell_find_reopen_active_hit = true;
    proof.shell_state_preserved = true;
    proof.shell_transition_operations = Some(proof.ledger.len() - shell_ledger_start);

    click(&mut app, "close_tools");
    settle(&mut app);
    assert!(!app.tools_open);
    enter(&mut app, true);
    assert_eq!(retained_snapshot(&app), snapshot);
    assert_eq!(app.browse.selected_index(), Some(target));
    assert!(app.search_state.group_expanded(target));
    assert!(!app.browse.preview_visible());
    proof.retained_text_snapshot = true;
    frame(&mut app, key(egui::Key::Escape));
    settle(&mut app);
    assert!(
        commands.try_recv().is_err(),
        "retained grouping, Open/Back, hide and reopen have zero additional operations"
    );
    proof.grouped_navigation_additional_operations =
        Some(proof.ledger.len() - grouping_ledger_start);
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

    // Keep every earlier Browse assertion and its seven-operation prefix.
    // Recent begins only after the existing Undo/Redo round trip has passed.
    assert_eq!(proof.ledger.len(), 7);
    assert_eq!(app.active().unwrap().path, "origin.txt");
    frame(
        &mut app,
        modified_key(egui::Key::ArrowLeft, egui::Modifiers::SHIFT),
    );
    settle(&mut app);
    assert!(
        selection(&app).primary.index != selection(&app).secondary.index,
        "Recent starts with an actual dirty selection"
    );
    let recent_origin = normal_origin_state(&app);
    let recent_find = retained_snapshot(&app);
    let recent_find_selection = app.search_state.selected_index();
    let recent_groups = app.search_state.group_rows();
    let recent_expanded: Vec<_> = recent_groups
        .iter()
        .map(|group| (group[0], app.search_state.group_expanded(group[0])))
        .collect();
    let recent_ui_start = proof.ledger.len();
    let closed = app
        .documents
        .iter()
        .find(|doc| doc.path == NORMAL_TARGET)
        .unwrap();
    let closed_id = closed.id;
    let closed_revision = closed.revision.clone().unwrap();
    assert!(!closed.dirty());
    click(&mut app, &format!("editor_tab:{closed_id}"));
    settle(&mut app);
    assert_eq!(app.active_document, Some(closed_id));
    assert_eq!(app.active().unwrap().path, NORMAL_TARGET);
    assert!(
        !app.active().unwrap().dirty(),
        "only the clean target may be closed"
    );
    let closed_position = normal_editor_position(&app);
    assert_ne!(
        closed_position, ordinary_target_position,
        "the earlier Search Open must leave a distinct historical caret"
    );
    frame(&mut app, modified_key(egui::Key::W, command_modifiers()));
    settle(&mut app);
    assert!(!app.documents.iter().any(|doc| doc.path == NORMAL_TARGET));
    assert_eq!(app.active().unwrap().path, "origin.txt");
    assert_eq!(normal_origin_state(&app), recent_origin);
    assert!(
        commands.try_recv().is_err(),
        "closing the clean target performs no work"
    );

    normal_recent_select(&mut app, "Needle", NORMAL_TARGET);
    assert_eq!(
        app.active().unwrap().path,
        "origin.txt",
        "single click selects only"
    );
    assert_eq!(normal_origin_state(&app), recent_origin);
    assert!(
        commands.try_recv().is_err(),
        "Recent popup/filter/selection must perform no I/O"
    );
    frame(&mut app, key(egui::Key::Escape));
    settle(&mut app);
    assert_eq!(focused(&app), Some(recorded_response(&app, "editor").id));
    assert_eq!(normal_origin_state(&app), recent_origin);
    normal_recent_select(&mut app, "Needle", NORMAL_TARGET);
    assert!(commands.try_recv().is_err());
    proof.recent_popup_filter_select_operations = Some(proof.ledger.len() - recent_ui_start);
    assert!(has_text(&frame(&mut app, vec![]), "Open closed file"));
    let reopen_start = proof.ledger.len();
    click(&mut app, "recent_open");
    assert!(
        app.pending.values().any(|job| matches!(job,
        Job::Open { path, line: None, .. } if path == NORMAL_TARGET)),
        "closed Recent reopen carries no historical line hint"
    );
    proof.exchange(
        &mut app,
        &commands,
        serde_json::json!({"op": "Read", "path": NORMAL_TARGET}),
    );
    settle(&mut app);
    proof.recent_closed_reopen_reads = Some(proof.ledger.len() - reopen_start);
    let reopened = app.active().unwrap();
    assert_ne!(
        reopened.id, closed_id,
        "closed Recent entry must use an ordinary new document"
    );
    assert_eq!(reopened.path, NORMAL_TARGET);
    assert_eq!(reopened.text, NORMAL_TARGET_TEXT);
    assert_eq!(reopened.revision.as_deref(), Some(closed_revision.as_str()));
    assert!(!reopened.dirty());
    assert_eq!(
        reopened.cursor, ordinary_target_position.0,
        "closed file uses the measured ordinary Open(None) position"
    );
    let position =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", reopened.id)))
            .unwrap()
            .cursor
            .char_range()
            .unwrap();
    assert_eq!(selection_signature(position), ordinary_target_position.1);
    assert_ne!(
        normal_editor_position(&app),
        closed_position,
        "Recent does not restore the historical closed-file caret"
    );
    proof.recent_closed_reopen_normal_position = true;
    assert_eq!(normal_origin_state(&app), recent_origin);
    assert!(commands.try_recv().is_err());

    let live_start = proof.ledger.len();
    normal_recent_select(&mut app, "origin", "origin.txt");
    assert_eq!(
        app.active().unwrap().path,
        NORMAL_TARGET,
        "filtering and selecting do not open"
    );
    assert!(commands.try_recv().is_err());
    proof.recent_popup_filter_select_operations = Some(
        proof.recent_popup_filter_select_operations.unwrap() + proof.ledger.len() - live_start,
    );
    click(&mut app, "recent_open");
    settle(&mut app);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.active().unwrap().path, "origin.txt");
    assert_eq!(
        normal_origin_state(&app),
        recent_origin,
        "live dirty reuse preserves full selection, Undo/Redo and known revision"
    );
    assert_eq!(focused(&app), Some(recorded_response(&app, "editor").id));
    assert!(
        commands.try_recv().is_err(),
        "live dirty Recent reuse must issue no Read or other operation"
    );
    proof.recent_live_dirty_reuse_reads = Some(proof.ledger.len() - live_start);
    proof.recent_live_dirty_reuse_exact = true;
    assert_eq!(retained_snapshot(&app), recent_find);
    assert_eq!(app.search_state.selected_index(), recent_find_selection);
    assert_eq!(app.search_state.group_rows(), recent_groups);
    assert_eq!(
        recent_expanded,
        recent_groups
            .iter()
            .map(|group| (group[0], app.search_state.group_expanded(group[0])))
            .collect::<Vec<_>>()
    );
    proof.recent_find_state_preserved = true;
    assert!(!app.execution_trusted());

    // All v2 observations above retain their exact eight-operation prefix.
    // Reveal reuses that same generated nested file and normal Client owner.
    assert_eq!(proof.ledger.len(), 8);
    let target_id = app
        .documents
        .iter()
        .find(|document| document.path == NORMAL_TARGET)
        .unwrap()
        .id;
    click(&mut app, &format!("editor_tab:{target_id}"));
    settle(&mut app);
    assert_eq!(app.active_document, Some(target_id));
    frame(&mut app, modified_key(egui::Key::End, command_modifiers()));
    frame(&mut app, vec![egui::Event::Text("reveal draft Ω😀".into())]);
    frame(
        &mut app,
        modified_key(egui::Key::Home, egui::Modifiers::SHIFT),
    );
    settle(&mut app);
    assert!(app.active().unwrap().dirty());
    let (_, reveal_selection) = normal_editor_position(&app);
    assert_ne!(reveal_selection.0, reveal_selection.1);
    assert_eq!(normal_origin_state(&app), recent_origin);
    assert!(commands.try_recv().is_err());
    let reveal_state = normal_shell_snapshot(&app);
    let reveal_ledger_start = proof.ledger.len();
    click(&mut app, "explorer_tree_mode");
    settle(&mut app);
    assert_eq!(app.explorer.mode, explorer_tree::Mode::Tree);
    let unloaded = app.explorer_visible_rows();
    assert_eq!(unloaded.len(), 1);
    assert_eq!(unloaded[0].entry.path, "");
    assert!(!unloaded[0].loaded && !unloaded[0].expanded);
    assert_eq!(app.explorer_loaded_directory_count(), 0);
    assert!(commands.try_recv().is_err());
    click(&mut app, "project_select_current");
    let reveal_started = std::time::Instant::now();
    assert!(app.project_reveal_pending());
    for ancestor in ["", "untouched", "untouched/deep"] {
        proof.exchange(
            &mut app,
            &commands,
            serde_json::json!({"op": "List", "path": ancestor}),
        );
        settle(&mut app);
        assert!(reveal_started.elapsed() < std::time::Duration::from_secs(30));
        assert_eq!(normal_shell_snapshot(&app), reveal_state);
    }
    assert!(!app.project_reveal_pending());
    assert_eq!(app.explorer.selected, NORMAL_TARGET);
    let row = recorded_response(&app, "Needle.rs");
    assert_eq!(row.id, explorer_tree::row_id(NORMAL_TARGET));
    assert!(row.enabled() && row.sense.is_focusable());
    assert!(recorded_clip(&app, "Needle.rs").contains_rect(row.rect));
    assert_eq!(focused(&app), Some(row.id));
    assert!(commands.try_recv().is_err());
    proof.project_reveal_ancestor_lists = Some(proof.ledger.len() - reveal_ledger_start);
    proof.project_reveal_selected_and_focused = true;

    let cached_start = proof.ledger.len();
    click(&mut app, "project_select_current");
    settle(&mut app);
    assert!(!app.project_reveal_pending());
    assert_eq!(app.explorer.selected, NORMAL_TARGET);
    assert_eq!(focused(&app), Some(recorded_response(&app, "Needle.rs").id));
    assert_eq!(normal_shell_snapshot(&app), reveal_state);
    assert!(
        commands.try_recv().is_err(),
        "cached reveal performs no operation"
    );
    proof.project_reveal_cached_operations = Some(proof.ledger.len() - cached_start);

    // A new explicit intent uses the same fixed deadline contract. Cancel it
    // before its first actual normal-agent response, then drain that response.
    click(&mut app, "explorer_flat_mode");
    click(&mut app, "explorer_tree_mode");
    settle(&mut app);
    let unloaded = app.explorer_visible_rows();
    assert_eq!(unloaded.len(), 1);
    assert_eq!(unloaded[0].entry.path, "");
    assert!(!unloaded[0].loaded && !unloaded[0].expanded);
    assert_eq!(app.explorer_loaded_directory_count(), 0);
    assert!(commands.try_recv().is_err());
    let cancelled_start = proof.ledger.len();
    click(&mut app, "project_select_current");
    assert!(app.project_reveal_pending());
    // Pinned egui swaps its widget maps at end_pass; read_response first
    // consults the preceding map. Render the newly enabled Cancel once more
    // before reading its response, without input or delivering the pending List.
    frame(&mut app, vec![]);
    assert!(app.project_reveal_pending());
    let cancel = recorded_response(&app, "project_reveal_cancel");
    let cancel_rect = recorded_rect(&app, "project_reveal_cancel");
    let cancel_pos = visible_center(&app, "project_reveal_cancel");
    assert!(cancel.enabled());
    frame(&mut app, vec![egui::Event::PointerMoved(cancel_pos)]);
    assert_eq!(
        recorded_response(&app, "project_reveal_cancel").id,
        cancel.id
    );
    assert_eq!(recorded_rect(&app, "project_reveal_cancel"), cancel_rect);
    assert!(recorded_response(&app, "project_reveal_cancel").enabled());
    assert!(recorded_clip(&app, "project_reveal_cancel").contains(cancel_pos));
    // Raw-input ownership cancels on this real press. The Cancel control then
    // becomes disabled in its stable footprint before the native release.
    frame(&mut app, pointer(cancel_pos, true));
    assert!(!app.project_reveal_pending());
    frame(&mut app, pointer(cancel_pos, false));
    settle(&mut app);
    assert!(!app.project_reveal_pending());
    let cancelled_focus = focused(&app);
    proof.exchange(
        &mut app,
        &commands,
        serde_json::json!({"op": "List", "path": ""}),
    );
    proof.project_reveal_cancelled_lists = Some(proof.ledger.len() - cancelled_start);
    let cancelled_followup_start = proof.ledger.len();
    settle(&mut app);
    assert!(!app.project_reveal_pending());
    assert!(!app.explorer_busy());
    assert_ne!(app.explorer.selected, NORMAL_TARGET);
    assert_eq!(
        focused(&app),
        cancelled_focus,
        "late response cannot steal focus"
    );
    assert_eq!(normal_shell_snapshot(&app), reveal_state);
    assert_eq!(normal_origin_state(&app), recent_origin);
    assert!(
        commands.try_recv().is_err(),
        "cancelled reveal must not dispatch its next ancestor List"
    );
    proof.project_reveal_cancelled_followup_operations =
        Some(proof.ledger.len() - cancelled_followup_start);
    proof.project_reveal_additional_reads = Some(
        proof.ledger[reveal_ledger_start..]
            .iter()
            .filter(|operation| operation["op"] == "Read")
            .count(),
    );
    proof.project_reveal_state_preserved = true;
    assert!(!app.execution_trusted());

    // Preserve the complete v3 proof before extending it. Reuse its existing
    // fixture and undo/close only the target's earlier reveal edit through
    // ordinary controls; no setup Read or extra source file is necessary.
    assert_eq!(proof.ledger.len(), 12);
    click(&mut app, "editor");
    frame(&mut app, modified_key(egui::Key::Z, command_modifiers()));
    settle(&mut app);
    assert_eq!(app.active().unwrap().path, NORMAL_TARGET);
    assert_eq!(app.active().unwrap().text, NORMAL_TARGET_TEXT);
    assert!(!app.active().unwrap().dirty());
    frame(&mut app, modified_key(egui::Key::W, command_modifiers()));
    settle(&mut app);
    assert!(!app
        .documents
        .iter()
        .any(|document| document.path == NORMAL_TARGET));
    assert_eq!(app.active().unwrap().path, "origin.txt");
    assert_eq!(normal_origin_state(&app), recent_origin);
    assert_eq!(retained_snapshot(&app), snapshot);
    assert!(commands.try_recv().is_err());
    // The earlier tab/Recent journey can leave this exact departure atop
    // Back; ordinary history correctly coalesces such duplicates. Choose a
    // distinct selection through editor keys before measuring the new journey.
    let setup_state = normal_shell_snapshot(&app);
    let setup_origin = normal_origin_state(&app);
    let setup_undo = undo_texts(&app);
    click(&mut app, "editor");
    assert_eq!(focused(&app), Some(recorded_response(&app, "editor").id));
    frame(&mut app, modified_key(egui::Key::Home, command_modifiers()));
    frame(
        &mut app,
        modified_key(egui::Key::ArrowRight, egui::Modifiers::SHIFT),
    );
    settle(&mut app);
    let continuous_origin = normal_origin_state(&app);
    let continuous_state = normal_continuous_review_state(&app);
    let continuous_back = continuous_state["back"].clone();
    assert_ne!(
        selection(&app).primary.index,
        selection(&app).secondary.index
    );
    assert!(app.location_history.back.last().is_none_or(|last| {
        last.generation != app.generation
            || Some(last.document) != app.active_document
            || last.edit_version != app.active().unwrap().edit_version
            || !location_history::same_selection(last.selection, selection(&app))
    }));
    assert_eq!(continuous_state["back"], setup_state["back"]);
    assert_eq!(continuous_state["forward"], setup_state["forward"]);
    assert_eq!(undo_texts(&app), setup_undo);
    for key in [
        "id",
        "text",
        "saved_text",
        "revision",
        "dirty",
        "edit_version",
    ] {
        assert_eq!(continuous_origin[key], setup_origin[key]);
    }
    assert!(commands.try_recv().is_err());
    let to_target = if target > origin {
        egui::Key::ArrowDown
    } else {
        egui::Key::ArrowUp
    };
    let to_origin = if target > origin {
        egui::Key::ArrowUp
    } else {
        egui::Key::ArrowDown
    };

    let unarmed_start = proof.ledger.len();
    enter(&mut app, true);
    assert!(!app.project_text_review_armed());
    click(&mut app, &format!("browse_row:{origin}"));
    frame(&mut app, key(to_target));
    settle(&mut app);
    assert_eq!(app.browse.selected_index(), Some(target));
    assert!(!app.project_text_review_armed());
    assert!(!app.browse.preview_visible());
    frame(&mut app, key(to_origin));
    settle(&mut app);
    assert_eq!(app.browse.selected_index(), Some(origin));
    assert!(!app.project_text_review_armed());
    assert!(!app.browse.preview_visible());
    assert_eq!(normal_continuous_review_state(&app), continuous_state);
    assert!(
        commands.try_recv().is_err(),
        "unarmed movement performs no work"
    );
    proof.continuous_preview_unarmed_operations = Some(proof.ledger.len() - unarmed_start);

    let dirty_start = proof.ledger.len();
    frame(&mut app, key(egui::Key::Space));
    settle(&mut app);
    assert!(app.project_text_review_armed());
    assert_eq!(app.browse_preview_text(), Some(DRAFT));
    assert!(commands.try_recv().is_err());
    // Explicit Text preview retains the owned row's keyboard focus. The
    // following adjacent real key frames admit target then origin 100 ms
    // apart, so only the latest candidate may mature at the fixed 150 ms.
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, &format!("browse_row:{origin}")).id)
    );
    assert!(app.project_text_review_armed());
    frame(&mut app, key(to_target));
    let target_time = app.editor_ctx.input(|input| input.time);
    assert_eq!(app.browse.selected_index(), Some(target));
    assert!(app.project_text_review_pending());
    assert!(commands.try_recv().is_err());
    frame(&mut app, key(to_origin));
    let origin_time = app.editor_ctx.input(|input| input.time);
    assert!(origin_time > target_time && origin_time - target_time < 0.150);
    assert_eq!(app.browse.selected_index(), Some(origin));
    assert!(app.project_text_review_pending());
    assert!(commands.try_recv().is_err());
    frame(&mut app, vec![]);
    assert!(app.project_text_review_pending());
    assert!(commands.try_recv().is_err());
    frame(&mut app, vec![]);
    assert!(!app.project_text_review_pending());
    assert!(app.project_text_review_armed());
    assert_eq!(app.browse.selected_index(), Some(origin));
    assert_eq!(app.browse_preview_text(), Some(DRAFT));
    assert_eq!(normal_continuous_review_state(&app), continuous_state);
    assert!(
        commands.try_recv().is_err(),
        "superseded unopened candidate must not Read"
    );
    proof.continuous_preview_latest_candidate = true;
    proof.continuous_preview_dirty_additional_reads = Some(proof.ledger.len() - dirty_start);

    let preview_start = proof.ledger.len();
    frame(&mut app, key(to_target));
    assert_eq!(app.browse.selected_index(), Some(target));
    assert!(app.project_text_review_pending());
    assert!(commands.try_recv().is_err());
    frame(&mut app, vec![]);
    assert!(app.project_text_review_pending());
    assert!(commands.try_recv().is_err());
    frame(&mut app, vec![]);
    proof.exchange(
        &mut app,
        &commands,
        serde_json::json!({"op": "Read", "path": NORMAL_TARGET}),
    );
    settle(&mut app);
    assert!(app.project_text_review_armed());
    assert!(!app.project_text_review_pending());
    assert_eq!(app.browse_preview_text(), Some(NORMAL_TARGET_TEXT));
    assert_eq!(app.browse.selected_index(), Some(target));
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, &format!("browse_row:{target}")).id)
    );
    assert_eq!(normal_continuous_review_state(&app), continuous_state);
    assert_eq!(app.documents.len(), 1, "preview does not open an editor");
    assert!(commands.try_recv().is_err());
    proof.continuous_preview_browse_armed_follow = true;
    proof.continuous_preview_unopened_reads = Some(proof.ledger.len() - preview_start);

    let open_start = proof.ledger.len();
    frame(&mut app, key(egui::Key::Enter));
    proof.exchange(
        &mut app,
        &commands,
        serde_json::json!({"op": "Read", "path": NORMAL_TARGET}),
    );
    settle(&mut app);
    proof.continuous_preview_open_reads = Some(proof.ledger.len() - open_start);
    assert_eq!(app.active().unwrap().path, NORMAL_TARGET);
    assert_eq!(app.active().unwrap().text, NORMAL_TARGET_TEXT);
    assert_eq!(app.active().unwrap().cursor, (2, 1));
    assert!(!app.project_text_review_armed());
    assert_eq!(
        app.location_history.back.len(),
        continuous_back.as_array().unwrap().len() + 1
    );
    frame(
        &mut app,
        modified_key(egui::Key::OpenBracket, command_modifiers()),
    );
    settle(&mut app);
    assert_eq!(app.active().unwrap().path, "origin.txt");
    assert_eq!(normal_origin_state(&app), continuous_origin);
    assert_eq!(normal_shell_snapshot(&app)["back"], continuous_back);
    assert_eq!(app.location_history.forward.len(), 1);
    assert_eq!(retained_snapshot(&app), snapshot);
    assert!(commands.try_recv().is_err());
    proof.continuous_preview_open_back_exact = true;

    // The second Text surface follows the same explicit-arm lifetime, using
    // the documents now open from the measured ordinary Open. Its complete
    // dirty/current-draft review sequence adds no remote operation.
    let bottom_start = proof.ledger.len();
    click(&mut app, "find_results_entry");
    settle(&mut app);
    assert!(app.tools_open && app.tool == Tool::Search);
    assert!(!app.project_text_review_armed());
    let bottom_state = normal_continuous_review_state(&app);
    click(&mut app, &format!("search_row:{origin}"));
    frame(&mut app, key(egui::Key::Space));
    settle(&mut app);
    assert!(app.project_text_review_armed());
    assert_eq!(app.search_preview_text(), Some(DRAFT));
    click(&mut app, &format!("search_row:{target}"));
    settle(&mut app);
    assert!(app.project_text_review_armed());
    assert_eq!(app.search_state.selected_index(), Some(target));
    assert_eq!(app.search_preview_text(), Some(NORMAL_TARGET_TEXT));
    click(&mut app, &format!("search_row:{origin}"));
    settle(&mut app);
    assert!(app.project_text_review_armed());
    assert_eq!(app.search_state.selected_index(), Some(origin));
    assert_eq!(app.search_preview_text(), Some(DRAFT));
    assert_eq!(normal_continuous_review_state(&app), bottom_state);
    assert_eq!(normal_origin_state(&app), continuous_origin);
    assert!(
        commands.try_recv().is_err(),
        "bottom dirty review performs zero Reads"
    );
    proof.continuous_preview_bottom_armed_follow = true;
    proof.continuous_preview_dirty_additional_reads = Some(
        proof.continuous_preview_dirty_additional_reads.unwrap() + proof.ledger.len()
            - bottom_start,
    );
    click(&mut app, "close_tools");
    settle(&mut app);
    assert!(!app.project_text_review_armed());
    assert!(!app.has_search_preview());
    assert_eq!(normal_continuous_review_state(&app), bottom_state);
    enter(&mut app, true);
    assert!(!app.project_text_review_armed());
    assert!(!app.browse.preview_visible());
    assert_eq!(app.browse.selected_index(), Some(origin));
    assert_eq!(retained_snapshot(&app), snapshot);
    frame(&mut app, key(egui::Key::Escape));
    settle(&mut app);
    assert_eq!(normal_continuous_review_state(&app), bottom_state);
    assert_eq!(normal_origin_state(&app), continuous_origin);
    assert!(!app.execution_trusted());
    assert!(commands.try_recv().is_err());
    proof.continuous_preview_state_preserved = true;

    // Append to the already completed v4 journey without another fixture,
    // Read, provider, client or deadline. The normally loaded dirty origin's
    // first character is still selected by the earlier real editor keys.
    assert_eq!(proof.ledger.len(), 14);
    assert_eq!(app.active().unwrap().path, "origin.txt");
    assert_eq!(app.active().unwrap().text, DRAFT);
    assert_eq!(focused(&app), Some(recorded_response(&app, "editor").id));
    let file_find_ledger = proof.ledger.clone();
    let file_find_origin = normal_origin_state(&app);
    let file_find_shell = normal_shell_snapshot(&app);
    let file_find_state = normal_current_file_find_state(&app);
    let file_find_undo = undo_texts(&app);
    let file_find_departure = selection_signature(selection(&app));
    let file_find_back = app.location_history.back.len();
    assert_eq!((file_find_departure.0, file_find_departure.1), (1, 0));
    assert!(commands.try_recv().is_err());
    frame(&mut app, modified_key(egui::Key::F, command_modifiers()));
    settle(&mut app);
    assert!(app.find_open);
    assert_eq!(app.find_query, "d");
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "find_query").id)
    );
    assert!(has_text(&frame(&mut app, vec![]), "1 of 3 matches"));
    assert_eq!(normal_shell_snapshot(&app), file_find_shell);
    assert!(commands.try_recv().is_err());
    proof.current_file_find_selection_seeded = true;

    // These are fixed scalar indices in the original Unicode/CRLF fixture,
    // independent of the production matcher. Repeated actual-key navigation
    // must advance and wrap; a disabled feature cannot satisfy this witness.
    for (modifiers, start, end, ordinal) in [
        (egui::Modifiers::NONE, 13, 14, 2),
        (egui::Modifiers::NONE, 22, 23, 3),
        (egui::Modifiers::NONE, 0, 1, 1),
        (egui::Modifiers::SHIFT, 22, 23, 3),
    ] {
        frame(&mut app, modified_key(egui::Key::F3, modifiers));
        normal_current_file_find_hit(&mut app, &commands, start, end, ordinal);
        assert_eq!(app.location_history.back.len(), file_find_back + 1);
        assert!(app.location_history.forward.is_empty());
        assert_eq!(normal_current_file_find_state(&app), file_find_state);
    }
    click(&mut app, "find_previous");
    normal_current_file_find_hit(&mut app, &commands, 13, 14, 2);
    click(&mut app, "find_next");
    normal_current_file_find_hit(&mut app, &commands, 22, 23, 3);
    click(&mut app, "find_query");
    frame(&mut app, key(egui::Key::Enter));
    normal_current_file_find_hit(&mut app, &commands, 0, 1, 1);
    frame(&mut app, key(egui::Key::F3));
    normal_current_file_find_hit(&mut app, &commands, 13, 14, 2);
    assert_eq!(app.location_history.back.len(), file_find_back + 1);
    assert!(app.location_history.forward.is_empty());
    assert_eq!(normal_current_file_find_state(&app), file_find_state);
    proof.current_file_find_next_previous_wrap = true;

    // Finish the single-departure loop, including Forward, before changing
    // query identity. Escape itself preserves the current exact match.
    let file_find_arrival = normal_origin_state(&app);
    frame(&mut app, key(egui::Key::Escape));
    settle(&mut app);
    assert!(!app.find_open);
    assert_eq!(focused(&app), Some(recorded_response(&app, "editor").id));
    assert_eq!(normal_origin_state(&app), file_find_arrival);
    frame(
        &mut app,
        modified_key(egui::Key::OpenBracket, command_modifiers()),
    );
    settle(&mut app);
    assert_eq!(normal_origin_state(&app), file_find_origin);
    assert_eq!(selection_signature(selection(&app)), file_find_departure);
    assert_eq!(normal_shell_snapshot(&app)["back"], file_find_shell["back"]);
    assert_eq!(app.location_history.forward.len(), 1);
    frame(
        &mut app,
        modified_key(egui::Key::CloseBracket, command_modifiers()),
    );
    settle(&mut app);
    assert_eq!(normal_origin_state(&app), file_find_arrival);
    assert_eq!(app.location_history.back.len(), file_find_back + 1);
    assert!(app.location_history.forward.is_empty());
    frame(
        &mut app,
        modified_key(egui::Key::OpenBracket, command_modifiers()),
    );
    settle(&mut app);
    assert_eq!(normal_origin_state(&app), file_find_origin);
    assert_eq!(normal_shell_snapshot(&app)["back"], file_find_shell["back"]);
    assert_eq!(app.location_history.forward.len(), 1);
    assert!(commands.try_recv().is_err());
    proof.current_file_find_escape_back_exact = true;

    // The source matcher is literal and case-sensitive. Type into the actual
    // field and positively observe zero matches; no-hit activation cannot
    // alter selection, history, the dirty draft or retained project results.
    frame(&mut app, modified_key(egui::Key::F, command_modifiers()));
    settle(&mut app);
    assert!(app.find_open);
    assert_eq!(app.find_query, "d");
    let no_match_state = normal_shell_snapshot(&app);
    for literal in [".*", "DIRTY"] {
        click(&mut app, "find_query");
        frame(&mut app, modified_key(egui::Key::A, command_modifiers()));
        frame(&mut app, vec![egui::Event::Text(literal.into())]);
        settle(&mut app);
        assert_eq!(app.find_query, literal);
        assert!(has_text(&frame(&mut app, vec![]), "0 matches"));
        for events in [
            key(egui::Key::F3),
            modified_key(egui::Key::F3, egui::Modifiers::SHIFT),
            key(egui::Key::Enter),
        ] {
            frame(&mut app, events);
            settle(&mut app);
            assert_eq!(normal_shell_snapshot(&app), no_match_state);
            assert!(commands.try_recv().is_err());
        }
    }
    frame(&mut app, key(egui::Key::Escape));
    settle(&mut app);
    assert!(!app.find_open);
    assert_eq!(focused(&app), Some(recorded_response(&app, "editor").id));
    assert_eq!(normal_shell_snapshot(&app), no_match_state);
    proof.current_file_find_literal_no_match = true;
    assert_eq!(normal_current_file_find_state(&app), file_find_state);
    assert_eq!(retained_snapshot(&app), snapshot);
    assert_eq!(undo_texts(&app), file_find_undo);
    assert!(!app.execution_trusted());
    proof.current_file_find_state_preserved = true;

    frame(&mut app, modified_key(egui::Key::Z, command_modifiers()));
    settle(&mut app);
    assert_eq!(app.active().unwrap().text, SAVED);
    assert!(!app.active().unwrap().dirty());
    assert_eq!(
        app.active().unwrap().revision.as_deref(),
        Some(revision.as_str())
    );
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
    settle(&mut app);
    assert_eq!(app.active().unwrap().text, DRAFT);
    assert_eq!(app.active().unwrap().saved_text, SAVED);
    assert!(app.active().unwrap().dirty());
    assert_eq!(
        app.active().unwrap().revision.as_deref(),
        Some(revision.as_str())
    );
    assert_eq!(undo_texts(&app), file_find_undo);
    assert_eq!(retained_snapshot(&app), snapshot);
    assert!(!app.execution_trusted());
    assert!(commands.try_recv().is_err());
    proof.current_file_find_undo_redo_preserved = true;
    assert_eq!(proof.ledger, file_find_ledger);
    proof.current_file_find_additional_operations =
        Some(proof.ledger.len() - file_find_ledger.len());

    // Preserve the complete v5 prefix. Admit the two new full ordinary calls
    // against the original deadline before beginning the suffix. This is
    // bounded-deadline admission, not a worst-case sum for all sixteen calls.
    assert_eq!(proof.ledger.len(), 14);
    assert!(proof.started.elapsed() + NORMAL_CALL * 2 < NORMAL_ACTIVE);
    let live_origin = normal_origin_state(&app);
    let live_departure = normal_editor_position(&app);
    let live_undo = undo_texts(&app);
    let live_documents = app.documents.len();
    assert!(app
        .documents
        .iter()
        .any(|document| document.path == NORMAL_TARGET));
    let live_start = proof.ledger.len();

    enter(&mut app, false);
    assert!(commands.try_recv().is_err());
    normal_files_edit(&mut app, "browse_scope", "untouched");
    assert!(commands.try_recv().is_err());
    normal_files_edit(&mut app, "browse_query", "N");
    continuity_tick(&mut app, 0.02, vec![egui::Event::Text("ee".into())]);
    assert!(commands.try_recv().is_err());
    continuity_tick(&mut app, 0.02, vec![egui::Event::Text("dle".into())]);
    assert!(commands.try_recv().is_err());
    let last_edit = app.editor_ctx.input(|input| input.time);
    continuity_tick(&mut app, 0.249, vec![]);
    assert!(commands.try_recv().is_err(), "249 ms must not dispatch");
    let to_deadline =
        last_edit + browse::FILES_DEBOUNCE_SECONDS - app.editor_ctx.input(|input| input.time);
    continuity_tick(&mut app, to_deadline, vec![]);
    proof.exchange(
        &mut app,
        &commands,
        serde_json::json!({"op": "FindFiles", "query": "Needle", "scope": "untouched", "limit": 100}),
    );
    settle(&mut app);
    assert_eq!(app.browse.paths(), vec![NORMAL_TARGET]);
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "browse_query").id)
    );
    assert!(commands.try_recv().is_err());
    assert_eq!(proof.ledger.len() - live_start, 1);
    proof.files_live_burst_latest_only = true;

    // The target is already normally loaded by the preserved v5 journey.
    // Explicit preview and ordinary Open reuse it without another Read.
    click(&mut app, "browse_row:0");
    assert_eq!(app.browse.selected_index(), Some(0));
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "browse_row:0").id)
    );
    frame(&mut app, key(egui::Key::Space));
    settle(&mut app);
    assert!(app.browse.preview_visible());
    assert_eq!(app.browse_preview_text(), Some(NORMAL_TARGET_TEXT));
    assert!(commands.try_recv().is_err());
    frame(&mut app, key(egui::Key::Escape));
    frame(&mut app, key(egui::Key::Enter));
    settle(&mut app);
    assert_eq!(app.active().unwrap().path, NORMAL_TARGET);
    assert!(commands.try_recv().is_err());
    frame(
        &mut app,
        modified_key(egui::Key::OpenBracket, command_modifiers()),
    );
    settle(&mut app);
    assert_eq!(app.active().unwrap().path, "origin.txt");
    assert_eq!(normal_editor_position(&app), live_departure);
    assert_eq!(normal_origin_state(&app), live_origin);
    assert_eq!(undo_texts(&app), live_undo);
    assert!(commands.try_recv().is_err());
    proof.files_live_open_back_exact = true;

    enter(&mut app, false);
    normal_files_edit(&mut app, "browse_query", "ori");
    continuity_tick(&mut app, 0.02, vec![egui::Event::Text("gin.txt".into())]);
    assert!(commands.try_recv().is_err());
    // Explicit Enter flushes before 250 ms. Another Enter and the visible
    // Find control refer to the same pending identity and cannot duplicate it.
    continuity_tick(&mut app, 0.01, key(egui::Key::Enter));
    continuity_tick(&mut app, 0.01, key(egui::Key::Enter));
    continuity_click(&mut app, "browse_submit", false);
    proof.exchange(
        &mut app,
        &commands,
        serde_json::json!({"op": "FindFiles", "query": "origin.txt", "scope": "", "limit": 100}),
    );
    settle(&mut app);
    continuity_tick(&mut app, 1.0, vec![]);
    assert_eq!(app.browse.paths(), vec!["origin.txt"]);
    assert!(commands.try_recv().is_err());
    assert_eq!(proof.ledger.len() - live_start, 2);
    proof.files_live_explicit_flush_deduplicated = true;
    proof.files_live_automatic_reads = Some(
        proof.ledger[live_start..]
            .iter()
            .filter(|operation| operation["op"] == "Read")
            .count(),
    );

    let dirty_live_start = proof.ledger.len();
    click(&mut app, "browse_row:0");
    assert_eq!(app.browse.selected_index(), Some(0));
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "browse_row:0").id)
    );
    frame(&mut app, key(egui::Key::Space));
    settle(&mut app);
    assert!(app.browse.preview_visible());
    assert_eq!(app.browse_preview_text(), Some(DRAFT));
    assert_eq!(normal_origin_state(&app), live_origin);
    assert!(commands.try_recv().is_err());
    frame(&mut app, key(egui::Key::Escape));
    frame(&mut app, key(egui::Key::Enter));
    settle(&mut app);
    assert_eq!(app.active().unwrap().path, "origin.txt");
    assert_eq!(normal_editor_position(&app), live_departure);
    assert_eq!(normal_origin_state(&app), live_origin);
    assert_eq!(undo_texts(&app), live_undo);
    assert_eq!(app.documents.len(), live_documents);
    assert_eq!(retained_snapshot(&app), snapshot);
    assert!(!app.execution_trusted());
    assert!(commands.try_recv().is_err());
    proof.files_live_dirty_preview_open_reads = Some(proof.ledger.len() - dirty_live_start);
    proof.files_live_state_preserved = true;
    proof.finish();
}
