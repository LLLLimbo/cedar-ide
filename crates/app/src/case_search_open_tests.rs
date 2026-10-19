//! Search-origin Open owns one accepted row and an irreversibly active snapshot.
//! Recording-worker tests preserve ordinary physical Read reuse and inactive caching.
use super::*;
use cedar_protocol::{CaseSearchDescriptor, CaseSearchMatch, CaseSearchResult};
use egui::text::{CCursor, CCursorRange};

const ORIGIN: &str = "origin.txt";
const TARGET: &str = "target.txt";
const TEXT: &str = "first Ω😀\r\nneedle second\nlast\n";

fn fixture() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        local_root: "/project".into(),
        allow_run: false,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/project".into();
    app.agent_info = Some(agent_support::full_test_agent());
    app.case_search = Some(CaseSearchDescriptor::V1);
    app.state = ConnectionState::Ready;
    app.open_form = false;
    app.documents
        .push(Document::new(1, ORIGIN.into(), TEXT.into(), "a".repeat(64)));
    app.active_document = Some(1);
    app.next_document = 2;
    app.search_query = "needle".into();
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn accept(app: &mut CedarApp, commands: &Receiver<Command>, path: &str, line: usize) {
    accept_rows(app, commands, &[(path, line)]);
}

fn accept_rows(app: &mut CedarApp, commands: &Receiver<Command>, rows: &[(&str, usize)]) {
    app.search();
    let command = commands.try_recv().expect("one explicit Search");
    let Some(Job::Search { specification }) = app.pending.remove(&command.id) else {
        panic!("Search owns the request");
    };
    let payload = match command.op {
        Operation::Search { .. } => Payload::Matches {
            matches: rows
                .iter()
                .map(|(path, line)| SearchMatch {
                    path: (*path).into(),
                    line: *line,
                    text: "needle".into(),
                })
                .collect(),
            truncated: false,
        },
        Operation::SearchCase { request } => Payload::CaseMatches {
            result: CaseSearchResult {
                request,
                matches: rows
                    .iter()
                    .map(|(path, line)| CaseSearchMatch {
                        path: (*path).into(),
                        line: *line,
                        text: "Needle".into(),
                    })
                    .collect(),
                incomplete: false,
                reasons: Vec::new(),
                examined_entries: 1,
                read_bytes: TEXT.len(),
            },
        },
        _ => panic!("unexpected search route"),
    };
    app.apply_search_response(command.id, specification, Ok(payload));
    assert!(app.search_rows_active());
    assert!(commands.try_recv().is_err());
}

fn reply(app: &mut CedarApp, id: u64, result: Result<Payload, String>, connected: bool) {
    app.apply_event(Event {
        generation: app.generation,
        id,
        result,
        connected,
    });
}

fn file(path: &str) -> Payload {
    Payload::File {
        path: path.into(),
        text: TEXT.into(),
        revision: "b".repeat(64),
    }
}

fn toggle_back(app: &mut CedarApp) {
    app.search_match_case = !app.search_match_case;
    app.search_filters_changed();
    app.search_match_case = !app.search_match_case;
    app.search_filters_changed();
    assert!(
        !app.search_rows_active(),
        "equal drafts cannot reactivate retired rows"
    );
}

fn pending_read(app: &mut CedarApp, commands: &Receiver<Command>) -> Command {
    app.open_search_result(0);
    let read = commands.try_recv().expect("one ordinary Read");
    assert!(matches!(&read.op, Operation::Read { path } if path == TARGET));
    assert!(matches!(
        app.pending.get(&read.id),
        Some(Job::SearchOpen { .. })
    ));
    assert!(commands.try_recv().is_err());
    read
}

#[test]
fn case_search_open_pending_read_toggle_back_only_caches_without_navigation() {
    // Characterizes the original gap: generic Open retained its line/navigation
    // after a case draft changed and was changed back before the Read completed.
    for match_case in [true, false] {
        let (mut app, commands) = fixture();
        app.search_match_case = match_case;
        accept(&mut app, &commands, TARGET, 2);
        let accepted = app.search_state.accepted.clone();
        let read = pending_read(&mut app, &commands);
        let epoch = app.navigation_epoch;
        assert!(app.location_history.pending.is_some());
        toggle_back(&mut app);
        assert!(
            app.pending.contains_key(&read.id),
            "retirement must drain the physical Read"
        );
        assert!(app.location_history.pending.is_none());
        app.error = Some("current error".into());
        app.notice = "current notice".into();
        reply(&mut app, read.id, Ok(file(TARGET)), true);
        assert_eq!(app.navigation_epoch, epoch);
        assert_eq!(app.active_document, Some(1));
        let cached = app
            .documents
            .iter()
            .find(|doc| doc.path == TARGET)
            .expect("inactive cache");
        assert_eq!(cached.text, TEXT);
        assert!(cached.jump_to.is_none());
        assert!(app.location_history.back.is_empty());
        assert!(app.location_history.forward.is_empty());
        assert_eq!(app.error.as_deref(), Some("current error"));
        assert_eq!(app.notice, "current notice");
        assert_eq!(app.search_state.accepted, accepted);
        assert!(!app.pending.contains_key(&read.id));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn case_search_open_retired_errors_and_wrong_payloads_leave_newer_history_untouched() {
    for result in [
        Err("old read failed".into()),
        Ok(Payload::Matches {
            matches: vec![],
            truncated: false,
        }),
        Ok(file("wrong.txt")),
    ] {
        let (mut app, commands) = fixture();
        accept(&mut app, &commands, TARGET, 2);
        let read = pending_read(&mut app, &commands);
        toggle_back(&mut app);
        app.open("newer.txt".into(), None);
        let newer = commands.try_recv().unwrap();
        let epoch = app.navigation_epoch;
        assert!(app.location_history.pending.is_some());
        app.error = Some("newer error".into());
        app.notice = "newer notice".into();
        reply(&mut app, read.id, result, true);
        assert!(
            app.location_history.pending.is_some(),
            "only the retired history ticket may be canceled"
        );
        assert_eq!(app.navigation_epoch, epoch);
        assert_eq!(app.active_document, Some(1));
        assert_eq!(app.documents.len(), 1);
        assert_eq!(app.error.as_deref(), Some("newer error"));
        assert_eq!(app.notice, "newer notice");
        assert!(!app.pending.contains_key(&read.id));
        assert!(app.pending.contains_key(&newer.id));
    }
}

#[test]
fn case_search_open_newer_same_path_filename_open_reuses_read_and_owns_activation() {
    let (mut app, commands) = fixture();
    accept(&mut app, &commands, TARGET, 2);
    let read = pending_read(&mut app, &commands);
    toggle_back(&mut app);
    app.open(TARGET.into(), None);
    assert!(
        matches!(app.pending.get(&read.id), Some(Job::Open { line: None, navigation, .. }) if *navigation == app.navigation_epoch)
    );
    assert!(
        commands.try_recv().is_err(),
        "newest intent must reuse one physical Read"
    );
    reply(&mut app, read.id, Ok(file(TARGET)), true);
    assert_eq!(app.active().unwrap().path, TARGET);
    assert!(
        app.active().unwrap().jump_to.is_none(),
        "old Search line must not leak into filename Open"
    );
    assert_eq!(app.location_history.back.len(), 1);
    assert!(app.location_history.pending.is_none());
    assert!(app.pending.is_empty());
}

#[test]
fn case_search_open_newer_same_path_search_reuses_read_and_replaces_source() {
    let (mut app, commands) = fixture();
    accept(&mut app, &commands, TARGET, 2);
    let read = pending_read(&mut app, &commands);
    app.search_match_case = false;
    app.search_filters_changed();
    accept(&mut app, &commands, TARGET, 3);
    app.open_search_result(0);
    assert!(
        matches!(app.pending.get(&read.id), Some(Job::SearchOpen { line: Some(3), navigation, .. }) if *navigation == app.navigation_epoch)
    );
    assert!(commands.try_recv().is_err());
    reply(&mut app, read.id, Ok(file(TARGET)), true);
    assert_eq!(app.active().unwrap().path, TARGET);
    assert_eq!(app.active().unwrap().jump_to, Some(line_start(TEXT, 3)));
    assert!(app.location_history.pending.is_some());
    assert!(app.pending.is_empty());
}

#[test]
fn case_search_open_retirement_does_not_revoke_newer_same_path_generic_open() {
    let (mut app, commands) = fixture();
    accept(&mut app, &commands, TARGET, 2);
    let read = pending_read(&mut app, &commands);
    app.open(TARGET.into(), None);
    toggle_back(&mut app);
    assert!(app.location_history.pending.is_some());
    reply(&mut app, read.id, Ok(file(TARGET)), true);
    assert_eq!(app.active().unwrap().path, TARGET);
    assert!(app.active().unwrap().jump_to.is_none());
    assert_eq!(app.location_history.back.len(), 1);
    assert!(commands.try_recv().is_err());
}

#[test]
fn case_search_open_retired_transport_failure_still_disconnects() {
    for result in [Err("transport failed".into()), Ok(file(TARGET))] {
        let (mut app, commands) = fixture();
        accept(&mut app, &commands, TARGET, 2);
        let read = pending_read(&mut app, &commands);
        toggle_back(&mut app);
        reply(&mut app, read.id, result, false);
        assert!(app.state == ConnectionState::Disconnected);
        assert!(app.case_search.is_none());
        assert!(app.search_match_case);
        assert_eq!(app.documents.len(), 1);
        assert!(app.pending.is_empty());
        assert!(app.location_history.pending.is_none());
    }
}

#[test]
fn case_search_open_rejects_inactive_missing_or_changed_rows_before_open() {
    for changed in 0..5 {
        let (mut app, commands) = fixture();
        accept(&mut app, &commands, TARGET, 2);
        let epoch = app.navigation_epoch;
        match changed {
            0 => toggle_back(&mut app),
            1 => {
                app.search_results.clear();
            }
            2 => {
                app.search_state.accepted.as_mut().unwrap().root = "/different".into();
            }
            3 => {
                app.search_state
                    .accepted
                    .as_mut()
                    .unwrap()
                    .specification
                    .case_sensitive = false;
            }
            _ => {
                app.search_state
                    .accepted
                    .as_mut()
                    .unwrap()
                    .specification
                    .route = search_keyboard::Route::CaseV1;
            }
        }
        app.open_search_result(0);
        assert_eq!(app.navigation_epoch, epoch);
        assert!(commands.try_recv().is_err());
        assert!(app.location_history.pending.is_none());
    }
}

#[test]
fn case_search_open_generic_open_remains_independent_of_search_retirement() {
    let (mut app, commands) = fixture();
    accept(&mut app, &commands, TARGET, 2);
    app.open(TARGET.into(), Some(3));
    let read = commands.try_recv().unwrap();
    toggle_back(&mut app);
    reply(&mut app, read.id, Ok(file(TARGET)), true);
    assert_eq!(app.active().unwrap().path, TARGET);
    assert_eq!(app.active().unwrap().jump_to, Some(line_start(TEXT, 3)));
    assert!(app.location_history.pending.is_some());
}

fn settle(app: &mut CedarApp) {
    for _ in 0..3 {
        let ctx = app.editor_ctx.clone();
        let mut input = egui::RawInput {
            time: Some(ctx.input(|input| input.time) + 0.1),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 800.0),
            )),
            focused: true,
            ..Default::default()
        };
        eframe::App::raw_input_hook(app, &ctx, &mut input);
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native));
    }
}

fn selection(app: &CedarApp, document: u64) -> CCursorRange {
    egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", document)))
        .unwrap()
        .cursor
        .char_range()
        .unwrap()
}

#[test]
fn case_search_open_dirty_reuse_preserves_draft_and_exact_back_without_read() {
    let (mut app, commands) = fixture();
    app.documents
        .push(Document::new(2, TARGET.into(), TEXT.into(), "b".repeat(64)));
    app.next_document = 3;
    settle(&mut app);
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[1],
        "dirty Ω😀\r\nneedle draft\nlast\n".into(),
        7,
    );
    let mut range = CCursorRange::two(CCursor::new(1), CCursor::new(7));
    range.primary.prefer_next_row = false;
    range.secondary.prefer_next_row = true;
    let mut state = editor_state::load(&app.editor_ctx, &mut app.documents[0]);
    state.cursor.set_char_range(Some(range));
    state.store(&app.editor_ctx, egui::Id::new(("editor", 1_u64)));
    settle(&mut app);
    let before = selection(&app, 1);
    let draft = (
        app.documents[1].text.clone(),
        app.documents[1].saved_text.clone(),
        app.documents[1].edit_version,
        app.documents[1].revision.clone(),
    );
    accept(&mut app, &commands, TARGET, 2);
    app.open_search_result(0);
    assert_eq!(app.active_document, Some(2));
    assert_eq!(app.active().unwrap().jump_to, Some(line_start(&draft.0, 2)));
    settle(&mut app);
    assert_eq!(app.location_history.back.len(), 1);
    let ctx = app.editor_ctx.clone();
    assert!(app.history_step(location_history::Direction::Back, &ctx));
    settle(&mut app);
    assert_eq!(app.active_document, Some(1));
    assert!(location_history::same_selection(selection(&app, 1), before));
    assert_eq!(
        (
            app.documents[1].text.clone(),
            app.documents[1].saved_text.clone(),
            app.documents[1].edit_version,
            app.documents[1].revision.clone()
        ),
        draft
    );
    assert!(
        commands.try_recv().is_err(),
        "dirty reuse and Back issue no Reads or writes"
    );
}

#[test]
fn case_search_open_completion_rechecks_full_source_and_row_identity() {
    for changed in 0..8 {
        let (mut app, commands) = fixture();
        accept(&mut app, &commands, TARGET, 2);
        let read = pending_read(&mut app, &commands);
        match changed {
            0 => app.search_state.accepted.as_mut().unwrap().root = "/other".into(),
            1 => app.search_state.accepted.as_mut().unwrap().request += 1,
            2 => {
                app.search_state
                    .accepted
                    .as_mut()
                    .unwrap()
                    .specification
                    .case_sensitive = false
            }
            3 => {
                app.search_state
                    .accepted
                    .as_mut()
                    .unwrap()
                    .specification
                    .route = search_keyboard::Route::CaseV1
            }
            4 => {
                app.search_state
                    .accepted
                    .as_mut()
                    .unwrap()
                    .specification
                    .request
                    .masks = vec!["*.txt".into()]
            }
            5 => app.search_results[0].path = "different.txt".into(),
            6 => app.search_results[0].line = 3,
            _ => app.search_results[0].text = "changed snippet".into(),
        }
        app.error = Some("current error".into());
        let epoch = app.navigation_epoch;
        reply(&mut app, read.id, Ok(file(TARGET)), true);
        assert_eq!(app.active_document, Some(1), "case {changed}");
        assert_eq!(app.navigation_epoch, epoch);
        assert!(app
            .documents
            .iter()
            .find(|document| document.path == TARGET)
            .unwrap()
            .jump_to
            .is_none());
        assert!(app.location_history.pending.is_none());
        assert!(app.location_history.back.is_empty());
        assert_eq!(app.error.as_deref(), Some("current error"));
        assert!(app.pending.is_empty());
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn case_search_open_retired_capacity_and_document_id_errors_are_silent() {
    for full in [false, true] {
        let (mut app, commands) = fixture();
        accept(&mut app, &commands, TARGET, 2);
        let read = pending_read(&mut app, &commands);
        toggle_back(&mut app);
        if full {
            for id in 2..=32 {
                app.documents.push(Document::new(
                    id,
                    format!("cache-{id}.txt"),
                    TEXT.into(),
                    "c".repeat(64),
                ));
            }
            app.next_document = 33;
        } else {
            app.next_document = u64::MAX;
        }
        let count = app.documents.len();
        let next = app.next_document;
        app.error = Some("current error".into());
        reply(&mut app, read.id, Ok(file(TARGET)), true);
        assert_eq!(app.documents.len(), count);
        assert_eq!(app.next_document, next);
        assert_eq!(app.active_document, Some(1));
        assert_eq!(app.error.as_deref(), Some("current error"));
        assert!(app.pending.is_empty());
    }
}

#[test]
fn case_search_open_retired_success_does_not_replace_a_later_dirty_buffer() {
    let (mut app, commands) = fixture();
    accept(&mut app, &commands, TARGET, 2);
    let read = pending_read(&mut app, &commands);
    toggle_back(&mut app);
    let mut dirty = Document::new(2, TARGET.into(), "saved later".into(), "c".repeat(64));
    dirty.text = "newer unsaved draft".into();
    dirty.edit_version = 3;
    app.documents.push(dirty);
    app.next_document = 3;
    reply(&mut app, read.id, Ok(file(TARGET)), true);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.documents[1].text, "newer unsaved draft");
    assert_eq!(app.documents[1].saved_text, "saved later");
    assert_eq!(app.documents[1].edit_version, 3);
    assert!(app.documents[1].jump_to.is_none());
    assert!(app.pending.is_empty());
    assert!(commands.try_recv().is_err());
}

#[test]
fn case_search_open_only_admits_the_current_selected_result() {
    let (mut app, commands) = fixture();
    accept_rows(&mut app, &commands, &[(TARGET, 2), ("second.txt", 3)]);
    assert_eq!(app.search_state.selected_index(), Some(0));
    let epoch = app.navigation_epoch;
    app.open_search_result(1);
    assert_eq!(app.navigation_epoch, epoch);
    assert!(app.pending.is_empty());
    assert!(commands.try_recv().is_err());
    app.select_search_row(1);
    app.open_search_result(1);
    let read = commands.try_recv().unwrap();
    assert!(matches!(&read.op, Operation::Read { path } if path == "second.txt"));
    reply(&mut app, read.id, Ok(file("second.txt")), true);
    assert_eq!(app.active().unwrap().path, "second.txt");
    assert_eq!(app.active().unwrap().jump_to, Some(line_start(TEXT, 3)));
}

#[test]
fn case_search_open_selecting_away_and_back_cannot_reactivate_pending_open() {
    for back in [false, true] {
        let (mut app, commands) = fixture();
        accept_rows(&mut app, &commands, &[(TARGET, 2), ("second.txt", 3)]);
        let read = pending_read(&mut app, &commands);
        app.select_search_row(1);
        if back {
            app.select_search_row(0);
        }
        assert!(
            app.search_rows_active(),
            "only Open activation retires on row selection"
        );
        assert!(app.pending.contains_key(&read.id));
        assert!(app.location_history.pending.is_none());
        reply(&mut app, read.id, Ok(file(TARGET)), true);
        assert_eq!(app.active_document, Some(1));
        assert!(app
            .documents
            .iter()
            .find(|document| document.path == TARGET)
            .unwrap()
            .jump_to
            .is_none());
        assert!(app.location_history.back.is_empty());
        assert!(app.pending.is_empty());
        assert!(commands.try_recv().is_err());
    }
}
