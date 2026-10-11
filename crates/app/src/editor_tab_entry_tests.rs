//! Native focus traversal through the production editor and its raw-input hook.
//! Measured entry routes never request focus, inject ownership, or replay a raw
//! hook in a layout pass. Setup uses ordinary production edit transactions;
//! history inspection uses disposable native Undoer clones.
use super::*;

const BEFORE: &str = "editor_tab_before";
const AFTER: &str = "editor_tab_after";
const SAVED: &str = "saved alpha\nsecond line\n";
const DIRTY: &str = "dirty alpha\nsecond line\n";
const LATER: &str = "later alpha\nsecond line\n";
type Selection = (usize, bool, usize, bool);

#[derive(Debug, PartialEq)]
struct Witness {
    text: String,
    saved: String,
    revision: Option<String>,
    version: u64,
    cursor: (usize, usize),
    selection: Selection,
    history: [Vec<(Selection, String)>; 2],
}

fn selection(range: egui::text::CCursorRange) -> Selection {
    (
        range.primary.index,
        range.primary.prefer_next_row,
        range.secondary.index,
        range.secondary.prefer_next_row,
    )
}

fn witness(app: &CedarApp) -> Witness {
    let doc = app.active().unwrap();
    let state = egui::TextEdit::load_state(&app.editor_ctx, editor_id(app)).unwrap();
    let cursor = state.cursor.char_range().unwrap();
    let mut history: [Vec<(Selection, String)>; 2] = Default::default();
    for (index, redo) in [false, true].into_iter().enumerate() {
        let mut undoer = state.undoer();
        let mut current = (cursor, doc.text.clone());
        for _ in 0..=editor_state::MAX_UNDO_STATES + 1 {
            let next = if redo {
                undoer.redo(&current)
            } else {
                undoer.undo(&current)
            }
            .cloned();
            let Some(next) = next else { break };
            history[index].push((selection(next.0), next.1.clone()));
            current = next;
        }
        assert!(history[index].len() <= editor_state::MAX_UNDO_STATES + 1);
    }
    Witness {
        text: doc.text.clone(),
        saved: doc.saved_text.clone(),
        revision: doc.revision.clone(),
        version: doc.edit_version,
        cursor: doc.cursor,
        selection: selection(cursor),
        history,
    }
}

fn editor_id(app: &CedarApp) -> egui::Id {
    egui::Id::new(("editor", app.active_document.unwrap()))
}

fn focused(app: &CedarApp) -> Option<egui::Id> {
    app.editor_ctx.memory(|memory| memory.focused())
}

fn key_event(key: egui::Key, modifiers: egui::Modifiers, pressed: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat: false,
        modifiers,
    }
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
    vec![
        key_event(key, modifiers, true),
        key_event(key, modifiers, false),
    ]
}

fn tabs(events: &[egui::Event]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::Tab,
                    ..
                }
            )
        })
        .count()
}

#[derive(Debug)]
struct Pass {
    before_editor: Vec<egui::Event>,
    after_editor: Vec<egui::Event>,
    focused: Option<egui::Id>,
    snapshot_frame_matches: Option<bool>,
}

fn raw(app: &CedarApp, events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        time: Some(app.editor_ctx.input(|input| input.time) + 0.05),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 900.0),
        )),
        events,
        ..Default::default()
    }
}

fn run(
    app: &mut CedarApp,
    mut input: egui::RawInput,
    hook: bool,
    discard: bool,
    after_hook: impl FnOnce(&mut CedarApp, &mut egui::RawInput),
) -> Vec<Pass> {
    let ctx = app.editor_ctx.clone();
    ctx.options_mut(|options| options.max_passes = 2.try_into().unwrap());
    if hook {
        let original_tabs = tabs(&input.events);
        eframe::App::raw_input_hook(app, &ctx, &mut input);
        assert_eq!(tabs(&input.events), original_tabs, "raw hook deleted Tab");
    }
    after_hook(app, &mut input);
    let mut passes = Vec::new();
    let _ = ctx.run(input, |ctx| {
        workspace_access_tests::begin(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            let before = ui.button("Before editor");
            workspace_access_tests::record(ui, BEFORE, &before);
            let before_editor = ctx.input(|input| input.events.clone());
            let snapshot_frame_matches = app
                .editor_tab_input
                .as_ref()
                .map(|proof| proof.matches_frame(ctx));
            ui.allocate_ui(
                egui::vec2(ui.available_width(), ui.available_height() - 45.0),
                |ui| app.editor(ui),
            );
            let after_editor = ctx.input(|input| input.events.clone());
            let after = ui.button("After editor");
            workspace_access_tests::record(ui, AFTER, &after);
            passes.push(Pass {
                before_editor,
                after_editor,
                focused: focused(app),
                snapshot_frame_matches,
            });
        });
        if discard && passes.len() == 1 {
            ctx.request_discard("editor Tab entry with an eventless repeated pass");
        }
    });
    passes
}

fn frame(app: &mut CedarApp, events: Vec<egui::Event>) -> Vec<Pass> {
    let input = raw(app, events);
    run(app, input, true, false, |_, _| {})
}

fn click(app: &mut CedarApp, name: &str) {
    let previous = workspace_access_tests::recorded_response(app, name);
    let initial = workspace_access_tests::recorded_rect(app, name)
        .intersect(workspace_access_tests::recorded_clip(app, name));
    assert!(previous.enabled() && initial.is_finite() && initial.is_positive());
    frame(app, vec![egui::Event::PointerMoved(initial.center())]);
    let response = workspace_access_tests::recorded_response(app, name);
    let rect = workspace_access_tests::recorded_rect(app, name)
        .intersect(workspace_access_tests::recorded_clip(app, name));
    assert_eq!(response.id, previous.id);
    assert!(response.enabled() && rect.is_finite() && rect.is_positive());
    let at = rect.center();
    assert_eq!(at, initial.center(), "target moved after pointer arrival");
    for pressed in [true, false] {
        frame(
            app,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let current = workspace_access_tests::recorded_response(app, name);
        assert_eq!(current.id, response.id);
        assert!(current.rect.contains(at));
        if pressed {
            assert!(current.is_pointer_button_down_on());
        } else {
            assert!(current.clicked_by(egui::PointerButton::Primary));
        }
    }
    assert_eq!(focused(app), Some(response.id));
}

fn app(text: &str, selected: bool) -> CedarApp {
    let mut app = CedarApp::empty();
    app.open_form = false;
    app.state = ConnectionState::Ready;
    app.workspace_key = Some(WorkspaceKey::Local {
        root: "/fixture".into(),
    });
    app.root = "/fixture".into();
    app.documents.push(Document::new(
        1,
        "draft.txt".into(),
        text.into(),
        "rev".into(),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    for _ in 0..3 {
        frame(&mut app, vec![]);
    }
    click(&mut app, "editor");
    let range = if selected {
        egui::text::CCursorRange::two(egui::text::CCursor::new(1), egui::text::CCursor::new(9))
    } else {
        egui::text::CCursorRange::one(egui::text::CCursor::new(1))
    };
    editor_state::move_selection(&app.editor_ctx, &mut app.documents[0], range);
    frame(&mut app, vec![]);
    assert_eq!(focused(&app), Some(editor_id(&app)));
    app
}

fn dirty_app(selected: bool) -> CedarApp {
    let mut app = app(SAVED, selected);
    // These ordinary production edit transactions establish a real Undo/Redo
    // branch while the editor is already natively focused. Entry tests then
    // leave through a visible button and return solely through native Tab.
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DIRTY.into(), 2);
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], LATER.into(), 3);
    frame(&mut app, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, DIRTY);
    let range = if selected {
        egui::text::CCursorRange::two(egui::text::CCursor::new(2), egui::text::CCursor::new(9))
    } else {
        egui::text::CCursorRange::one(egui::text::CCursor::new(2))
    };
    editor_state::move_selection(&app.editor_ctx, &mut app.documents[0], range);
    frame(&mut app, vec![]);
    let before = witness(&app);
    assert!(!before.history[0].is_empty() && !before.history[1].is_empty());
    app
}

/// Discover egui's actual forward focus stops, including native scrollbars.
/// Finish at the last foreign stop using only native pointer and key input.
fn before_entry(app: &mut CedarApp) -> usize {
    click(app, BEFORE);
    let baseline = witness(app);
    let mut steps = 0;
    loop {
        steps += 1;
        assert!(steps <= 20, "native Tab did not reach the editor");
        frame(app, key(egui::Key::Tab, egui::Modifiers::NONE));
        if focused(app) == Some(editor_id(app)) {
            break;
        }
        assert_eq!(witness(app), baseline);
    }
    assert_eq!(
        witness(app),
        baseline,
        "discovering the route edited the buffer"
    );
    click(app, BEFORE);
    for _ in 1..steps {
        frame(app, key(egui::Key::Tab, egui::Modifiers::NONE));
        assert_ne!(focused(app), Some(editor_id(app)));
    }
    steps
}

fn assert_local_tab(pass: &Pass) {
    assert!(tabs(&pass.before_editor) > 0);
    assert_eq!(
        tabs(&pass.after_editor),
        tabs(&pass.before_editor),
        "unowned Tab was deleted globally"
    );
}

#[test]
fn editor_tab_entry_preserves_dirty_selection_version_and_both_history_branches() {
    for selected in [false, true] {
        let mut app = dirty_app(selected);
        before_entry(&mut app);
        let before = witness(&app);
        let passes = frame(&mut app, key(egui::Key::Tab, egui::Modifiers::NONE));
        assert_eq!(focused(&app), Some(editor_id(&app)));
        assert_local_tab(&passes[0]);
        assert_eq!(witness(&app), before);
        // This is the very next native frame, with no settling frame inserted.
        frame(&mut app, key(egui::Key::Z, egui::Modifiers::COMMAND));
        assert_eq!(app.documents[0].text, SAVED);
        frame(&mut app, key(egui::Key::Y, egui::Modifiers::COMMAND));
        assert_eq!(app.documents[0].text, DIRTY);
        frame(&mut app, key(egui::Key::Y, egui::Modifiers::COMMAND));
        assert_eq!(app.documents[0].text, LATER);
    }
}

#[test]
fn editor_shift_tab_entry_preserves_history_through_native_deferred_focus() {
    for selected in [false, true] {
        let mut app = dirty_app(selected);
        click(&mut app, AFTER);
        let before = witness(&app);
        let mut reached = false;
        for _ in 0..20 {
            let passes = frame(&mut app, key(egui::Key::Tab, egui::Modifiers::SHIFT));
            assert_local_tab(&passes[0]);
            assert_eq!(witness(&app), before);
            if focused(&app) != Some(editor_id(&app)) {
                // egui 0.31.1 applies backward focus at the following begin_pass.
                frame(&mut app, vec![]);
            }
            assert_eq!(witness(&app), before);
            if focused(&app) == Some(editor_id(&app)) {
                reached = true;
                break;
            }
        }
        assert!(reached, "native Shift+Tab did not reach the editor");
        frame(&mut app, key(egui::Key::Z, egui::Modifiers::COMMAND));
        assert_eq!(app.documents[0].text, SAVED);
    }
}

#[test]
fn editor_tab_entry_repeats_and_release_repress_remain_local_navigation() {
    for selected in [false, true] {
        for repeated in [false, true] {
            let mut app = dirty_app(selected);
            before_entry(&mut app);
            let before = witness(&app);
            let mut events = key(egui::Key::Tab, egui::Modifiers::NONE);
            let mut again = key_event(egui::Key::Tab, egui::Modifiers::NONE, true);
            if let egui::Event::Key { repeat, .. } = &mut again {
                *repeat = repeated;
            }
            if repeated {
                events.pop();
            }
            events.push(again);
            events.push(key_event(egui::Key::Tab, egui::Modifiers::NONE, false));
            let passes = frame(&mut app, events);
            assert_eq!(focused(&app), Some(editor_id(&app)));
            assert_local_tab(&passes[0]);
            assert_eq!(witness(&app), before);
        }
    }
}

#[test]
fn editor_tab_existing_focus_keeps_collapsed_native_and_selected_block_behavior() {
    let mut collapsed = app("\talpha\nsecond", false);
    frame(&mut collapsed, key(egui::Key::Tab, egui::Modifiers::NONE));
    assert_eq!(collapsed.documents[0].text, "\t\talpha\nsecond");
    frame(&mut collapsed, key(egui::Key::Tab, egui::Modifiers::SHIFT));
    assert_eq!(collapsed.documents[0].text, "\talpha\nsecond");
    assert_eq!(collapsed.documents[0].edit_version, 2);

    let mut selected = app("alpha\nsecond", true);
    let before = witness(&selected);
    frame(&mut selected, key(egui::Key::Tab, egui::Modifiers::NONE));
    assert_eq!(selected.documents[0].text, "\talpha\n\tsecond");
    frame(&mut selected, key(egui::Key::Tab, egui::Modifiers::SHIFT));
    assert_eq!(selected.documents[0].text, before.text);
    assert_eq!(witness(&selected).selection, before.selection);
    assert_eq!(selected.documents[0].edit_version, 2);
}

#[test]
fn editor_tab_eventless_second_pass_restores_native_code_editor_filter() {
    for selected in [false, true] {
        let mut app = app("alpha\nsecond", selected);
        before_entry(&mut app);
        let before = witness(&app);
        let input = raw(&app, key(egui::Key::Tab, egui::Modifiers::NONE));
        let passes = run(&mut app, input, true, true, |_, _| {});
        assert_eq!(passes.len(), 2);
        assert_eq!(passes[0].snapshot_frame_matches, Some(true));
        assert_eq!(passes[1].snapshot_frame_matches, Some(true));
        assert_local_tab(&passes[0]);
        assert!(passes[1].before_editor.is_empty());
        assert!(passes[1].after_editor.is_empty());
        assert_eq!(passes[1].focused, Some(editor_id(&app)));
        assert_eq!(witness(&app), before);
        frame(&mut app, key(egui::Key::Tab, egui::Modifiers::NONE));
        assert_eq!(focused(&app), Some(editor_id(&app)));
        assert_eq!(app.documents[0].edit_version, before.version + 1);
        assert_eq!(
            app.documents[0].text,
            if selected {
                "\talpha\n\tsecond"
            } else {
                "a\tlpha\nsecond"
            }
        );
    }
}

#[test]
fn editor_tab_immediate_next_tab_retains_pinned_native_focus_timing_limitation() {
    // A newly focused egui 0.31.1 widget cannot install its focus lock until a
    // subsequent pass. Keep this native behavior observable; do not repair it
    // by requesting focus or setting the filter from the raw-input hook.
    let mut app = app("alpha\nsecond", false);
    before_entry(&mut app);
    frame(&mut app, key(egui::Key::Tab, egui::Modifiers::NONE));
    assert_eq!(focused(&app), Some(editor_id(&app)));
    let before = witness(&app);
    frame(&mut app, key(egui::Key::Tab, egui::Modifiers::NONE));
    assert_ne!(focused(&app), Some(editor_id(&app)));
    assert_eq!(witness(&app), before);

    before_entry(&mut app);
    frame(&mut app, key(egui::Key::Tab, egui::Modifiers::NONE));
    frame(&mut app, vec![]);
    frame(&mut app, key(egui::Key::Tab, egui::Modifiers::NONE));
    assert_eq!(focused(&app), Some(editor_id(&app)));
    assert_eq!(app.documents[0].text, "a\tlpha\nsecond");
}

#[test]
fn editor_tab_missing_or_stale_snapshot_cannot_authorize_surviving_tab() {
    for selected in [false, true] {
        for stale in 0..11 {
            let mut app = dirty_app(selected);
            let input = raw(&app, key(egui::Key::Tab, egui::Modifiers::NONE));
            let mut baseline = None;
            let passes = run(&mut app, input, stale != 10, false, |app, _| {
                match stale {
                    0 => app.editor_tab_input = None,
                    1 => app.documents[0].path = "renamed.txt".into(),
                    2 => app.documents[0].edit_version += 1,
                    3 => app.generation += 1,
                    4 => app.navigation_epoch += 1,
                    5 => {
                        app.workspace_key = Some(WorkspaceKey::Local {
                            root: "/different".into(),
                        })
                    }
                    6 => app.root = "/different".into(),
                    7 => app.language.session += 1,
                    8 => app.state = ConnectionState::Disconnected,
                    9 => app.workspace_key = None,
                    _ => assert!(
                        app.editor_tab_input.is_none(),
                        "fixture omitted the raw hook"
                    ),
                }
                baseline = Some(witness(app));
            });
            assert_local_tab(&passes[0]);
            assert_eq!(witness(&app), baseline.unwrap(), "stale case {stale}");
        }
    }
}

#[test]
fn editor_tab_entry_preserves_native_text_and_paste_order_without_global_consumption() {
    for selected in [false, true] {
        for position in 0..3 {
            let mut app = app("alpha\nsecond", selected);
            before_entry(&mut app);
            let before = witness(&app);
            let mut events = vec![
                egui::Event::Text("A".into()),
                egui::Event::Paste("B".into()),
            ];
            events.splice(
                position..position,
                key(egui::Key::Tab, egui::Modifiers::NONE),
            );
            let passes = frame(&mut app, events);
            assert_local_tab(&passes[0]);
            assert_eq!(passes[0].after_editor, passes[0].before_editor);
            assert_eq!(
                app.documents[0].text,
                if selected {
                    "aABond"
                } else {
                    "aABlpha\nsecond"
                }
            );
            assert_eq!(app.documents[0].edit_version, before.version + 1);
        }
    }
}

fn composition(app: &CedarApp) -> Option<bool> {
    app.editor_ctx
        .data(|data| data.get_temp::<bool>(editor_id(app).with("block_indent_composition")))
}

#[test]
fn editor_tab_entry_retains_ordered_native_ime_transition_even_without_enabled() {
    let batches = [
        (
            vec![egui::ImeEvent::Preedit("draft".into())],
            true,
            "adraftond",
        ),
        (
            vec![
                egui::ImeEvent::Enabled,
                egui::ImeEvent::Preedit("draft".into()),
            ],
            true,
            "adraftond",
        ),
        (
            vec![
                egui::ImeEvent::Preedit("draft".into()),
                egui::ImeEvent::Commit("done".into()),
            ],
            false,
            "adoneond",
        ),
        (
            vec![
                egui::ImeEvent::Commit("done".into()),
                egui::ImeEvent::Preedit("draft".into()),
            ],
            true,
            "alpha\nsecdraftond",
        ),
        (
            vec![
                egui::ImeEvent::Preedit("draft".into()),
                egui::ImeEvent::Disabled,
            ],
            false,
            "adraftond",
        ),
        (
            vec![
                egui::ImeEvent::Preedit("draft".into()),
                egui::ImeEvent::Preedit(String::new()),
            ],
            false,
            "aond",
        ),
    ];
    for (ime, active, expected) in batches {
        let mut app = app("alpha\nsecond", true);
        before_entry(&mut app);
        let mut events = key(egui::Key::Tab, egui::Modifiers::NONE);
        events.extend(ime.into_iter().map(egui::Event::Ime));
        let passes = frame(&mut app, events);
        assert_eq!(focused(&app), Some(editor_id(&app)));
        assert_local_tab(&passes[0]);
        assert_eq!(composition(&app), Some(active));
        // These are pinned TextEdit semantics, including Commit without an
        // established IME range collapsing the selection before a Preedit.
        assert_eq!(app.documents[0].text, expected);
        assert_eq!(app.documents[0].edit_version, 1);
        // TextEdit may clear Enabled IME input on gained focus and collapse its
        // native preedit range. Select through an ordinary native shortcut so
        // the following selected-block Tab tests Cedar's retained transition.
        frame(&mut app, key(egui::Key::A, egui::Modifiers::COMMAND));
        let before = witness(&app);
        assert_ne!(before.selection.0, before.selection.2);
        frame(&mut app, key(egui::Key::Tab, egui::Modifiers::NONE));
        assert_eq!(composition(&app), Some(active));
        if active {
            assert_eq!(witness(&app), before);
        } else {
            assert_eq!(app.documents[0].edit_version, before.version + 1);
            assert_eq!(
                app.documents[0].text,
                format!("\t{}", before.text.replace('\n', "\n\t"))
            );
        }
    }
}

#[test]
fn editor_tab_stale_identity_cannot_adopt_entry_composition() {
    for stale in 0..5 {
        let mut app = app("alpha\nsecond", false);
        before_entry(&mut app);
        let mut events = key(egui::Key::Tab, egui::Modifiers::NONE);
        events.push(egui::Event::Ime(egui::ImeEvent::Preedit("draft".into())));
        let input = raw(&app, events);
        let passes = run(&mut app, input, true, false, |app, _| match stale {
            0 => app.documents[0].path = "renamed.txt".into(),
            1 => app.generation += 1,
            2 => app.navigation_epoch += 1,
            3 => app.language.session += 1,
            _ => app.workspace_key = None,
        });
        assert_eq!(focused(&app), Some(editor_id(&app)));
        assert_local_tab(&passes[0]);
        assert_eq!(composition(&app), None, "stale composition case {stale}");
        assert!(!app.documents[0].text.contains('\t'));
    }
}

#[test]
fn editor_tab_owned_blocked_selection_consumes_only_the_tab_press() {
    for blocker in 0..4 {
        let mut app = app("alpha\nsecond", true);
        let before = witness(&app);
        match blocker {
            0 => app.open_form = true,
            1 => app.new_file = true,
            2 => app.documents[0].jump_to = Some(1),
            _ => {}
        }
        let mut input = raw(&app, key(egui::Key::Tab, egui::Modifiers::NONE));
        if blocker == 3 {
            input.focused = false;
        }
        let passes = run(&mut app, input, true, false, |_, _| {});
        assert_eq!(tabs(&passes[0].before_editor), 2);
        assert_eq!(tabs(&passes[0].after_editor), 1, "owned blocker {blocker}");
        assert!(matches!(
            passes[0].after_editor.as_slice(),
            [egui::Event::Key {
                key: egui::Key::Tab,
                pressed: false,
                ..
            }]
        ));
        assert_eq!(app.documents[0].text, before.text);
        assert_eq!(app.documents[0].edit_version, before.version);
        if blocker != 2 {
            assert_eq!(witness(&app), before);
        }
    }
}

#[test]
fn editor_tab_original_mixed_batch_cannot_become_a_clean_selected_action() {
    let mut app = app("alpha\nsecond", true);
    let before = witness(&app);
    let mut events = key(egui::Key::Tab, egui::Modifiers::NONE);
    events.push(egui::Event::Text("original companion".into()));
    let input = raw(&app, events);
    let passes = run(&mut app, input, true, false, |app, input| {
        assert!(!app.editor_tab_input.as_ref().unwrap().block_action);
        // Model a different input interceptor consuming a companion after the
        // immutable prehook snapshot. It must not create a clean block command.
        input
            .events
            .retain(|event| !matches!(event, egui::Event::Text(_)));
    });
    assert_eq!(tabs(&passes[0].before_editor), 2);
    assert_eq!(tabs(&passes[0].after_editor), 1);
    assert_eq!(witness(&app), before);
}

#[test]
fn editor_tab_snapshot_matches_exact_document_and_rejects_missing_target() {
    let mut app = app("alpha\nsecond", true);
    let input = raw(&app, key(egui::Key::Tab, egui::Modifiers::NONE));
    let original = input.events.clone();
    let proof = editor_tab::FrameInput::capture(&app, &app.editor_ctx, &input).unwrap();
    assert_eq!(input.events, original);
    assert!(proof.matches_document(&app.documents[0]));
    for changed in 0..3 {
        let mut doc = Document::new(1, "draft.txt".into(), "alpha\nsecond".into(), "rev".into());
        match changed {
            0 => doc.id = 2,
            1 => doc.path = "other.txt".into(),
            _ => doc.edit_version = 1,
        }
        assert!(!proof.matches_document(&doc), "document mismatch {changed}");
    }
    app.documents[0].jump_to = Some(1);
    let blocked = editor_tab::FrameInput::capture(&app, &app.editor_ctx, &input).unwrap();
    assert!(blocked.matches_document(&app.documents[0]));
    assert!(!blocked.original_input_eligible());
    app.documents[0].jump_to = None;
    assert!(
        !blocked.original_input_eligible(),
        "clearing a jump cannot rewrite the original eligibility"
    );
    app.active_document = None;
    assert!(editor_tab::FrameInput::capture(&app, &app.editor_ctx, &input).is_none());
    app.active_document = Some(99);
    assert!(editor_tab::FrameInput::capture(&app, &app.editor_ctx, &input).is_none());
}

#[test]
fn editor_tab_changed_active_document_does_not_transfer_ownership() {
    let mut app = app("alpha\nsecond", true);
    app.documents.push(Document::new(
        2,
        "second.txt".into(),
        "another buffer".into(),
        "rev2".into(),
    ));
    app.active_document = Some(2);
    frame(&mut app, vec![]);
    click(&mut app, "editor");
    let second_before = witness(&app);
    app.active_document = Some(1);
    frame(&mut app, vec![]);
    click(&mut app, "editor");
    frame(&mut app, vec![]);
    let first_before = witness(&app);
    let input = raw(&app, key(egui::Key::Tab, egui::Modifiers::NONE));
    let passes = run(&mut app, input, true, false, |app, _| {
        app.active_document = Some(2)
    });
    assert_local_tab(&passes[0]);
    assert_eq!(witness(&app), second_before);
    app.active_document = Some(1);
    assert_eq!(witness(&app), first_before);
}

#[test]
fn editor_tab_snapshot_rejects_other_viewport_and_later_native_batch() {
    let mut app = app("alpha\nsecond", true);
    let input = raw(&app, key(egui::Key::Tab, egui::Modifiers::NONE));
    let proof = editor_tab::FrameInput::capture(&app, &app.editor_ctx, &input).unwrap();
    let mut other_viewport = input.clone();
    other_viewport.viewport_id = egui::ViewportId::from_hash_of("different editor viewport");
    let foreign = editor_tab::FrameInput::capture(&app, &app.editor_ctx, &other_viewport).unwrap();
    let ctx = app.editor_ctx.clone();
    // This is a proof-only run; no input is routed to an editor or granted by
    // the local snapshots. The ordinary frame below advances native identity.
    let _ = ctx.run(input, |ctx| {
        assert!(proof.matches_context(&app, ctx));
        assert!(!foreign.matches_frame(ctx));
    });
    frame(&mut app, vec![]);
    let _ = ctx.run(raw(&app, vec![]), |ctx| {
        assert!(!proof.matches_frame(ctx));
        assert!(!proof.matches_context(&app, ctx));
    });
}
