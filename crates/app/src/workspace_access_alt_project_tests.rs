use super::*;
use crate::{browsing_shell_tests as shell, workspace_access_tests as probes, *};

const SIZE: [f32; 2] = [1280.0, 900.0];

fn key(key: egui::Key, modifiers: egui::Modifiers, pressed: bool, repeat: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat,
        modifiers,
    }
}
fn pair(repeat: bool) -> Vec<egui::Event> {
    vec![
        key(egui::Key::Num1, egui::Modifiers::ALT, true, repeat),
        egui::Event::Text("1".into()),
    ]
}
fn raw(events: Vec<egui::Event>, modifiers: egui::Modifiers) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE.into())),
        modifiers,
        events,
        ..Default::default()
    }
}
fn frame(
    app: &mut CedarApp,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
) -> Vec<egui::Event> {
    let ctx = app.editor_ctx.clone();
    let mut input = raw(events, modifiers);
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native));
    ctx.input(|input| input.events.clone())
}
fn owner(app: &CedarApp) -> bool {
    app.editor_ctx
        .data(|d| d.get_temp::<Lifetime>(egui::Id::new(LIFETIME)))
        .unwrap()
        .owned_alt_project
}
fn focused(app: &CedarApp) -> Option<egui::Id> {
    app.editor_ctx.memory(|m| m.focused())
}
fn setup(tree: bool) -> (CedarApp, Receiver<Command>) {
    let (mut app, commands) = shell::fixture(13.0);
    if tree {
        app.explorer_set_mode(explorer_tree::Mode::Tree);
    }
    app.explorer_refresh("");
    let list = commands.try_recv().unwrap();
    assert!(matches!(list.op, Operation::List { .. }));
    app.apply_event(Event {
        generation: app.generation,
        id: list.id,
        connected: true,
        result: Ok(Payload::Entries {
            entries: (0..3)
                .map(|i| Entry {
                    name: format!("row-{i}"),
                    path: format!("row-{i}"),
                    is_dir: true,
                })
                .collect(),
        }),
    });
    if tree {
        app.explorer_select("row-1");
    }
    app.search_scope.clear();
    app.search_masks.clear();
    app.search();
    let search = commands.try_recv().unwrap();
    assert!(matches!(search.op, Operation::Search { .. }));
    app.apply_event(Event {
        generation: app.generation,
        id: search.id,
        connected: true,
        result: Ok(Payload::Matches {
            matches: (1..=2)
                .map(|line| SearchMatch {
                    path: "src/origin.rs".into(),
                    line,
                    text: "needle".into(),
                })
                .collect(),
            truncated: false,
        }),
    });
    app.select_search_row(1);
    shell::settle(&mut app, SIZE);
    shell::click(&mut app, SIZE, "editor");
    frame(
        &mut app,
        vec![egui::Event::Text(" typed".into())],
        egui::Modifiers::NONE,
    );
    shell::key(&mut app, SIZE, egui::Key::A, egui::Modifiers::COMMAND);
    let draft = shell::draft_snapshot(&app);
    assert_ne!(draft.4 .0, draft.4 .2, "actual Ctrl+A selects dirty text");
    assert!(app.documents[0].dirty());
    assert!(commands.try_recv().is_err());
    (app, commands)
}
fn snapshot(app: &CedarApp) -> String {
    let state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64))).unwrap();
    let start = (
        state.cursor.char_range().unwrap(),
        app.documents[0].text.clone(),
    );
    let mut history: [Vec<_>; 2] = Default::default();
    for (index, redo) in [false, true].into_iter().enumerate() {
        let mut undoer = state.undoer();
        let mut value = start.clone();
        for _ in 0..=crate::editor_state::MAX_UNDO_STATES + 1 {
            let next = if redo {
                undoer.redo(&value)
            } else {
                undoer.undo(&value)
            }
            .cloned();
            let Some(next) = next else { break };
            history[index].push(next.clone());
            value = next;
        }
    }
    format!(
        "{:?}/{:?}/{:?}/{:?}/{:?}/{:?}",
        shell::draft_snapshot(app),
        history,
        (
            app.generation,
            app.navigation_epoch,
            app.language.session,
            app.explorer.epoch,
            app.explorer.mode_epoch
        ),
        (
            &app.search_query,
            &app.search_scope,
            &app.search_masks,
            app.search_state.accepted_snapshot_id(),
            app.search_state.selected_index()
        ),
        app.search_results
            .iter()
            .map(|r| (&r.path, r.line, &r.text))
            .collect::<Vec<_>>(),
        (std::mem::discriminant(&app.tool), app.tools_open)
    )
}

#[test]
fn alt_project_pair_actual_tree_and_flat_preserve_dirty_selection_undo_find_and_operations() {
    for (tree, find_open) in [(true, false), (false, false), (true, true), (false, true)] {
        let (mut app, commands) = setup(tree);
        if find_open {
            shell::click(&mut app, SIZE, "find_results_entry");
            assert!(app.tools_open && app.tool == Tool::Search);
            shell::click(&mut app, SIZE, "editor");
            shell::key(&mut app, SIZE, egui::Key::A, egui::Modifiers::COMMAND);
        }
        let before = snapshot(&app);
        let target = probes::recorded_response(&app, if tree { "row-1" } else { "row-0" }).id;
        let mut events = pair(false);
        if find_open {
            events.insert(0, egui::Event::PointerMoved(egui::pos2(900.0, 700.0)));
            events.insert(2, egui::Event::MouseMoved(egui::vec2(0.0, 0.0)));
        }
        let remaining = frame(&mut app, events, egui::Modifiers::ALT);
        assert!(!remaining.iter().any(|e| matches!(e, egui::Event::Text(_))));
        assert_eq!(focused(&app), Some(target));
        assert!(owner(&app));
        assert_eq!(snapshot(&app), before);
        frame(
            &mut app,
            vec![key(egui::Key::Num1, egui::Modifiers::ALT, false, false)],
            egui::Modifiers::ALT,
        );
        assert!(!owner(&app));
        if tree {
            for (key, row) in [
                (egui::Key::ArrowDown, "row-2"),
                (egui::Key::ArrowUp, "row-1"),
            ] {
                shell::key(&mut app, SIZE, key, egui::Modifiers::NONE);
                assert_eq!(app.explorer.selected, row);
                assert_eq!(focused(&app), Some(probes::recorded_response(&app, row).id));
                assert_eq!(app.explorer_loaded_directory_count(), 1);
                assert_eq!(snapshot(&app), before);
            }
        }
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn alt_project_successful_held_pair_is_fenced_without_refocus_until_real_release() {
    let (mut app, commands) = setup(true);
    frame(&mut app, pair(false), egui::Modifiers::ALT);
    assert!(owner(&app));
    shell::click(&mut app, SIZE, "editor");
    let before = snapshot(&app);
    let focus = focused(&app);
    for repeat in [false, true, false] {
        assert!(frame(&mut app, pair(repeat), egui::Modifiers::ALT).is_empty());
        assert_eq!(snapshot(&app), before);
        assert_eq!(focused(&app), focus, "held pair must not refocus Project");
    }
    let mut fresh = vec![key(egui::Key::Num1, egui::Modifiers::ALT, false, false)];
    fresh.extend([
        key(egui::Key::Num1, egui::Modifiers::NONE, true, false),
        egui::Event::Text("1".into()),
        key(egui::Key::Num1, egui::Modifiers::NONE, false, false),
    ]);
    let remaining = frame(&mut app, fresh, egui::Modifiers::NONE);
    assert!(remaining.contains(&egui::Event::Text("1".into())));
    assert_ne!(snapshot(&app), before);
    assert!(!owner(&app));
    assert_eq!(focused(&app), focus);
    assert!(commands.try_recv().is_err());
}

#[test]
fn alt_project_trailing_release_and_later_invalidated_focus_never_arm_repeat_ownership() {
    for (invalidate, release_modifiers) in [
        (false, egui::Modifiers::ALT),
        (false, egui::Modifiers::NONE),
        (true, egui::Modifiers::ALT),
    ] {
        let (mut app, commands) = setup(true);
        let before = snapshot(&app);
        if !invalidate {
            let mut events = pair(false);
            events.push(key(egui::Key::Num1, release_modifiers, false, false));
            frame(&mut app, events, egui::Modifiers::ALT);
            assert_eq!(
                focused(&app),
                Some(probes::recorded_response(&app, "row-1").id)
            );
            assert_eq!(snapshot(&app), before);
        } else {
            let ctx = app.editor_ctx.clone();
            let source = focused(&app);
            let mut input = raw(pair(false), egui::Modifiers::ALT);
            eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
            let _ = ctx.run(input, |ctx| {
                app.workspace_access_shortcuts(ctx);
                assert!(
                    app.workspace_access
                        .intent
                        .as_ref()
                        .unwrap()
                        .alt_project_companion
                );
                assert!(!ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(_)))));
                app.sidebar(ctx);
                egui::CentralPanel::default().show(ctx, |ui| app.editor(ui));
                app.navigation_epoch += 1;
                app.finish_workspace_access_frame(ctx);
            });
            assert_eq!(focused(&app), source);
            app.navigation_epoch -= 1;
            assert_eq!(snapshot(&app), before);
            assert!(!owner(&app));
            let remaining = frame(&mut app, pair(false), egui::Modifiers::ALT);
            assert!(
                remaining.contains(&egui::Event::Text("1".into())),
                "failed completion owns no later text"
            );
            assert_eq!(focused(&app), source);
        }
        assert!(!owner(&app));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn alt_project_ordered_pair_rejects_arbitrary_mixed_modified_and_unowned_repeat_input() {
    let mut cases: Vec<(&str, Vec<egui::Event>, egui::Modifiers)> = Vec::new();
    let alt = egui::Modifiers::ALT;
    for (label, text) in [("wrong", "x"), ("empty", ""), ("multiple chars", "11")] {
        cases.push((
            label,
            vec![
                key(egui::Key::Num1, alt, true, false),
                egui::Event::Text(text.into()),
            ],
            alt,
        ));
    }
    cases.push((
        "reversed",
        vec![
            egui::Event::Text("1".into()),
            key(egui::Key::Num1, alt, true, false),
        ],
        alt,
    ));
    cases.push((
        "release before text",
        vec![
            key(egui::Key::Num1, alt, true, false),
            key(egui::Key::Num1, alt, false, false),
            egui::Event::Text("1".into()),
        ],
        alt,
    ));
    cases.push(("unowned repeat", pair(true), alt));
    cases.push(("raw modifier mismatch", pair(false), egui::Modifiers::NONE));
    for modifier in [
        egui::Modifiers::CTRL,
        egui::Modifiers::SHIFT,
        egui::Modifiers::MAC_CMD,
    ] {
        let mods = alt | modifier;
        cases.push((
            "extra modifier",
            vec![
                key(egui::Key::Num1, mods, true, false),
                egui::Event::Text("1".into()),
            ],
            mods,
        ));
    }
    for extra in [
        egui::Event::Text("1".into()),
        egui::Event::Text("other".into()),
        egui::Event::Paste("paste".into()),
        egui::Event::Ime(egui::ImeEvent::Enabled),
        egui::Event::Ime(egui::ImeEvent::Disabled),
        egui::Event::Ime(egui::ImeEvent::Preedit("".into())),
        egui::Event::Ime(egui::ImeEvent::Preedit("composition".into())),
        egui::Event::Ime(egui::ImeEvent::Commit("".into())),
        egui::Event::Ime(egui::ImeEvent::Commit("commit".into())),
        key(egui::Key::ArrowDown, egui::Modifiers::NONE, true, false),
        egui::Event::PointerButton {
            pos: egui::pos2(1250.0, 850.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: alt,
        },
    ] {
        let mut events = pair(false);
        events.push(extra);
        cases.push(("mixed", events, alt));
    }
    for (label, events, mods) in cases {
        let (mut app, commands) = setup(true);
        let wanted: Vec<_> = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    egui::Event::Text(_) | egui::Event::Paste(_) | egui::Event::Ime(_)
                )
            })
            .cloned()
            .collect();
        let remaining = frame(&mut app, events, mods);
        assert!(!owner(&app), "{label}");
        assert_ne!(
            focused(&app),
            Some(probes::recorded_response(&app, "row-1").id),
            "{label}"
        );
        for event in wanted {
            assert!(
                remaining.contains(&event),
                "{label}: original content retained"
            );
        }
        assert!(commands.try_recv().is_err(), "{label}");
    }
}

#[test]
fn alt_project_original_and_live_vectors_cannot_manufacture_or_retarget_a_pair() {
    for change in [
        "original mixed",
        "live removed key",
        "live removed text",
        "live replaced text",
        "live prepended text",
        "stale origin",
        "egui held key",
        "blocked modal",
    ] {
        let (mut app, commands) = setup(true);
        let ctx = app.editor_ctx.clone();
        let mut events = pair(false);
        if change == "original mixed" {
            events.push(key(
                egui::Key::ArrowDown,
                egui::Modifiers::NONE,
                true,
                false,
            ));
        }
        let mut input = raw(events, egui::Modifiers::ALT);
        if change == "egui held key" {
            // Populate egui's independent keys_down without the native hook;
            // Access still captures a fresh original key, while begin_pass
            // must independently mark the live key as a repeat.
            let _ = ctx.run(
                raw(
                    vec![key(egui::Key::Num1, egui::Modifiers::ALT, true, false)],
                    egui::Modifiers::ALT,
                ),
                |_| {},
            );
        }
        eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
        if change == "egui held key" {
            assert!(original_input(&ctx).unwrap().alt_project_companion);
        }
        if change == "original mixed" {
            input.events.pop();
        }
        if change == "stale origin" {
            app.navigation_epoch += 1;
        }
        if change == "blocked modal" {
            app.open_form = true;
        }
        let _ = ctx.run(input, |ctx| {
            ctx.input_mut(|i| match change {
                "live removed key" => {
                    i.events.remove(0);
                }
                "live removed text" => {
                    i.events.pop();
                }
                "live replaced text" => i.events[1] = egui::Event::Text("other".into()),
                "live prepended text" => i.events.insert(0, egui::Event::Text("earlier".into())),
                _ => {}
            });
            let before: Vec<_> = ctx.input(|i| {
                i.events
                    .iter()
                    .filter(|e| matches!(e, egui::Event::Text(_)))
                    .cloned()
                    .collect()
            });
            app.workspace_access_shortcuts(ctx);
            assert!(app.workspace_access.intent.is_none(), "{change}");
            let after: Vec<_> = ctx.input(|i| {
                i.events
                    .iter()
                    .filter(|e| matches!(e, egui::Event::Text(_)))
                    .cloned()
                    .collect()
            });
            assert_eq!(after, before, "{change}");
        });
        assert!(!owner(&app));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn alt_project_prior_held_composition_and_successful_but_mixed_repeats_preserve_content() {
    for prior in [
        "key",
        "pointer",
        "composition",
        "owned mixed",
        "owned plain",
        "owned release repress",
    ] {
        let (mut app, commands) = setup(true);
        if prior.starts_with("owned") {
            frame(&mut app, pair(false), egui::Modifiers::ALT);
            assert!(owner(&app));
            shell::click(&mut app, SIZE, "editor");
        } else {
            let event = match prior {
                "key" => key(egui::Key::ArrowLeft, egui::Modifiers::NONE, true, false),
                "pointer" => egui::Event::PointerButton {
                    pos: egui::pos2(1250.0, 850.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                _ => egui::Event::Ime(egui::ImeEvent::Preedit("composition".into())),
            };
            frame(&mut app, vec![event], egui::Modifiers::NONE);
        }
        let mut events = pair(false);
        let mut mods = egui::Modifiers::ALT;
        match prior {
            "owned mixed" => events.push(egui::Event::Text("other".into())),
            "owned plain" => {
                mods = egui::Modifiers::NONE;
                events[0] = key(egui::Key::Num1, mods, true, false);
            }
            "owned release repress" => events.insert(0, key(egui::Key::Num1, mods, false, false)),
            "composition" => events.push(egui::Event::Ime(egui::ImeEvent::Disabled)),
            _ => {}
        }
        let remaining = frame(&mut app, events, mods);
        assert!(
            remaining.contains(&egui::Event::Text("1".into())),
            "{prior}"
        );
        assert_ne!(
            focused(&app),
            Some(probes::recorded_response(&app, "row-1").id),
            "{prior}"
        );
        assert!(commands.try_recv().is_err());
    }
}
