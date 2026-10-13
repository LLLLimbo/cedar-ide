//! Complete raw-hook/update journeys for the two armed Project Text surfaces.
//! Setup alone seeds accepted rows and a dirty draft; measured actions use
//! native keys and the exact rendered pointer rectangles with an explicit clock.
use super::*;
use crate::workspace_access_tests::{recorded_clip, recorded_rect, recorded_response};
use egui::text::{CCursor, CCursorRange};

const SAVED: &str = "saved Ω😀\r\nneedle second\nlast\n";
const DRAFT: &str = "dirty Ω😀\r\nneedle second\nlast\n";

fn frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 800.0),
        )),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut raw);
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(raw, |ctx| eframe::App::update(app, ctx, &mut native))
}
fn now(app: &CedarApp) -> f64 {
    app.editor_ctx.input(|input| input.time)
}
fn idle(app: &mut CedarApp, delta: f64) -> egui::FullOutput {
    frame(app, now(app) + delta, vec![])
}
fn key(key: egui::Key) -> Vec<egui::Event> {
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        })
        .collect()
}
fn press(app: &mut CedarApp, key_: egui::Key) {
    frame(app, now(app) + 0.01, key(key_));
}
fn row_probe(bottom: bool, index: usize) -> String {
    if bottom {
        format!("search_row:{index}")
    } else {
        format!("browse_row:{index}")
    }
}
fn focus(app: &CedarApp) -> Option<egui::Id> {
    app.editor_ctx.memory(|memory| memory.focused())
}
fn click(app: &mut CedarApp, probe: &str) {
    let pos = recorded_rect(app, probe)
        .intersect(recorded_clip(app, probe))
        .center();
    assert!(recorded_clip(app, probe).contains(pos));
    frame(app, now(app) + 0.01, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            app,
            now(app) + 0.01,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
    assert!(
        recorded_response(app, probe).clicked_by(egui::PointerButton::Primary),
        "{probe}"
    );
}
fn reply(app: &mut CedarApp, command: &Command, text: &str) {
    let Operation::Read { path } = &command.op else {
        panic!("not Read")
    };
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::File {
            path: path.clone(),
            text: text.into(),
            revision: "b".repeat(64),
        }),
    });
}
fn read(commands: &Receiver<Command>, expected: &str) -> Command {
    let command = commands.try_recv().expect("one expected Read");
    assert!(matches!(&command.op, Operation::Read { path } if path == expected));
    assert!(commands.try_recv().is_err());
    command
}
fn fixture(bottom: bool) -> (CedarApp, Receiver<Command>) {
    fixture_with_target_line(bottom, 2)
}
fn fixture_with_target_line(bottom: bool, target_line: usize) -> (CedarApp, Receiver<Command>) {
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
    app.tools_open = true;
    app.tool = Tool::Search;
    frame(&mut app, 1.0, vec![]);
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 8);
    let mut state = editor_state::load(&app.editor_ctx, &mut app.documents[0]);
    state
        .cursor
        .set_char_range(Some(CCursorRange::two(CCursor::new(1), CCursor::new(8))));
    state.store(&app.editor_ctx, egui::Id::new(("editor", 1_u64)));
    app.search_query = "needle".into();
    app.search();
    let search = commands.try_recv().unwrap();
    assert!(matches!(search.op, Operation::Search { .. }));
    app.apply_event(Event {
        generation: app.generation,
        id: search.id,
        connected: true,
        result: Ok(Payload::Matches {
            matches: ["origin.txt", "a.txt", "b.txt"]
                .into_iter()
                .map(|path| SearchMatch {
                    path: path.into(),
                    line: if path == "a.txt" { target_line } else { 2 },
                    text: "needle historical match".into(),
                })
                .collect(),
            truncated: false,
        }),
    });
    idle(&mut app, 0.01);
    if !bottom {
        app.show_browse(browse::Provider::ProjectText);
    }
    for _ in 0..4 {
        idle(&mut app, 0.01);
    }
    click(&mut app, &row_probe(bottom, 0));
    assert!(commands.try_recv().is_err());
    (app, commands)
}
fn selected(app: &CedarApp, bottom: bool) -> Option<usize> {
    if bottom {
        app.search_state.selected_index()
    } else {
        app.browse.selected_index()
    }
}
fn arm(app: &mut CedarApp, commands: &Receiver<Command>, bottom: bool) {
    press(app, egui::Key::Space);
    idle(app, 0.001);
    assert!(app.project_text_review_armed());
    assert!(!app.project_text_review_pending());
    assert_eq!(
        focus(app),
        Some(recorded_response(app, &row_probe(bottom, 0)).id)
    );
    assert!(
        commands.try_recv().is_err(),
        "dirty draft arm must never Read"
    );
}
fn has_text(output: &egui::FullOutput, expected: &str) -> bool {
    fn check(shape: &egui::epaint::Shape, expected: &str) -> bool {
        match shape {
            egui::epaint::Shape::Text(text) => text.galley.job.text.contains(expected),
            egui::epaint::Shape::Vec(shapes) => shapes.iter().any(|shape| check(shape, expected)),
            _ => false,
        }
    }
    output
        .shapes
        .iter()
        .any(|shape| check(&shape.shape, expected))
}

#[test]
fn project_text_context_bounds_unicode_long_lines_and_unavailable_eof() {
    let many = (1..=120)
        .map(|line| format!("Ω😀行 {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let context = browse::project_text_context(&many, 60);
    assert_eq!(
        context.text.lines().count(),
        browse::MAX_PROJECT_TEXT_CONTEXT_LINES
    );
    assert!(context.text.contains("▶ 60 | Ω😀行 60"));
    assert!(context.note.contains("context truncated"));
    assert!(!context.text.contains("行 39"));
    let huge = format!("{}\n{}\nend", "😀".repeat(40_000), "界".repeat(40_000));
    let context = browse::project_text_context(&huge, 2);
    assert!(context.text.len() <= browse::MAX_PROJECT_TEXT_CONTEXT_BYTES);
    assert!(context.text.contains("▶ 2 | 界"));
    assert!(context.text.contains("[line truncated]"));
    assert!(context.note.contains("context truncated"));
    let eof = browse::project_text_context("one\ntwo", 9);
    assert!(eof.text.is_empty());
    assert!(eof.note.contains("historical line 9 is beyond"));
    assert!(browse::project_text_context("one", 0).text.is_empty());
    assert_eq!(
        browse::project_text_context("Ω\r\n😀", 2).text,
        "  1 | Ω\n▶ 2 | 😀"
    );
}

#[test]
fn continuous_preview_both_surfaces_unarmed_zero_read_fixed_deadline_and_repaint() {
    for bottom in [false, true] {
        let (mut app, commands) = fixture(bottom);
        press(&mut app, egui::Key::ArrowDown);
        idle(&mut app, 0.5);
        assert_eq!(selected(&app, bottom), Some(1));
        assert!(commands.try_recv().is_err(), "unarmed selection");
        press(&mut app, egui::Key::Home);
        arm(&mut app, &commands, bottom);
        press(&mut app, egui::Key::ArrowDown);
        let changed = now(&app);
        let recent = app.recent_files.preview_history_snapshot();
        assert!(app.project_text_review_pending());
        let ticket = app.find_preview_identity();
        for offset in [0.02, 0.07, 0.12, 0.149] {
            frame(&mut app, changed + offset, vec![]);
            assert!(commands.try_recv().is_err(), "settlement fired early");
            assert!(
                app.find_preview_identity() == ticket,
                "repaint changed ticket"
            );
        }
        frame(&mut app, changed + 0.150, vec![]);
        let preview = read(&commands, "a.txt");
        assert!(!app.project_text_review_pending());
        let row_focus = focus(&app);
        reply(&mut app, &preview, "alpha\ncurrent line\nlast");
        let output = idle(&mut app, 0.01);
        assert_eq!(focus(&app), row_focus, "Read reply must not steal focus");
        assert!(has_text(
            &idle(&mut app, 0.01),
            "Historical requested line 2"
        ));
        assert!(app
            .project_text_preview_context()
            .unwrap()
            .text
            .contains("▶ 2 | current line"));
        assert!(!has_text(&output, "current match"));
        assert!(app.recent_files.preview_history_snapshot() == recent);
        for _ in 0..4 {
            idle(&mut app, 0.2);
        }
        assert!(
            commands.try_recv().is_err(),
            "one Read per selection, never per repaint"
        );
    }
}

#[test]
fn continuous_preview_both_surfaces_latest_candidate_one_flight_and_aba() {
    for bottom in [false, true] {
        let (mut app, commands) = fixture(bottom);
        arm(&mut app, &commands, bottom);
        press(&mut app, egui::Key::ArrowDown);
        idle(&mut app, 0.151);
        let old_a = read(&commands, "a.txt");
        press(&mut app, egui::Key::ArrowDown);
        idle(&mut app, 0.151);
        assert!(commands.try_recv().is_err(), "one physical Read flight");
        press(&mut app, egui::Key::ArrowUp);
        let fresh_a = app.find_preview_identity();
        idle(&mut app, 0.151);
        assert!(commands.try_recv().is_err());
        reply(&mut app, &old_a, "old\nstale A");
        idle(&mut app, 0.01);
        let new_a = read(&commands, "a.txt");
        assert!(app.find_preview_identity() == fresh_a);
        assert!(app.project_text_preview_context().is_none());
        reply(&mut app, &new_a, "new\nlatest A");
        idle(&mut app, 0.01);
        assert!(app
            .project_text_preview_context()
            .unwrap()
            .text
            .contains("latest A"));
    }
}

#[test]
fn continuous_preview_both_surfaces_escape_hidden_reopen_and_surface_invalidation() {
    for bottom in [false, true] {
        let (mut app, commands) = fixture(bottom);
        arm(&mut app, &commands, bottom);
        press(&mut app, egui::Key::ArrowDown);
        idle(&mut app, 0.151);
        let pending = read(&commands, "a.txt");
        press(&mut app, egui::Key::Escape);
        assert!(!app.project_text_review_armed());
        assert!(!app.project_text_review_pending());
        assert_eq!(
            focus(&app),
            Some(recorded_response(&app, &row_probe(bottom, 1)).id)
        );
        assert!(app.tools_open);
        if bottom {
            app.tools_open = false;
        } else {
            app.browse_dismiss();
        }
        idle(&mut app, 0.01);
        if bottom {
            app.tools_open = true;
        } else {
            app.show_browse(browse::Provider::ProjectText);
        }
        for _ in 0..3 {
            idle(&mut app, 0.01);
        }
        let before_focus = focus(&app);
        reply(&mut app, &pending, "old\nretired");
        idle(&mut app, 0.2);
        assert_eq!(focus(&app), before_focus);
        assert!(app.project_text_preview_context().is_none());
        assert!(!app.project_text_review_armed());
        click(&mut app, &row_probe(bottom, 0));
        arm(&mut app, &commands, bottom);
        if bottom {
            app.show_browse(browse::Provider::ProjectText);
        } else {
            app.browse_dismiss();
        }
        for _ in 0..3 {
            idle(&mut app, 0.01);
        }
        assert!(!app.project_text_review_armed());
        click(&mut app, &row_probe(!bottom, 1));
        idle(&mut app, 0.2);
        assert!(commands.try_recv().is_err(), "arm must not cross surfaces");
    }
}

#[test]
fn continuous_preview_both_surfaces_mixed_input_ime_hover_never_select_or_read() {
    for bottom in [false, true] {
        let (mut app, commands) = fixture(bottom);
        arm(&mut app, &commands, bottom);
        let probe = row_probe(bottom, 1);
        let pos = recorded_rect(&app, &probe)
            .intersect(recorded_clip(&app, &probe))
            .center();
        let time = now(&app) + 0.01;
        frame(&mut app, time, vec![egui::Event::PointerMoved(pos)]);
        for mut events in [
            vec![egui::Event::Text("x".into())],
            vec![egui::Event::Paste("x".into())],
            vec![egui::Event::Ime(egui::ImeEvent::Preedit("文".into()))],
        ] {
            events.extend(key(egui::Key::ArrowDown));
            let time = now(&app) + 0.01;
            frame(&mut app, time, events);
            idle(&mut app, 0.2);
            assert_eq!(selected(&app, bottom), Some(0));
            assert!(commands.try_recv().is_err());
        }
        let time = now(&app) + 0.01;
        frame(
            &mut app,
            time,
            vec![egui::Event::Ime(egui::ImeEvent::Disabled)],
        );
        press(&mut app, egui::Key::ArrowDown);
        idle(&mut app, 0.151);
        read(&commands, "a.txt");
    }
}

#[test]
fn continuous_preview_both_surfaces_dirty_context_preserves_editor_undo_and_history() {
    for bottom in [false, true] {
        let (mut app, commands) = fixture(bottom);
        let before = editor_state::load(&app.editor_ctx, &mut app.documents[0]);
        let cursor = before.cursor.char_range();
        let version = app.documents[0].edit_version;
        let recent = app.recent_files.preview_history_snapshot();
        assert!(app.location_history.back.is_empty());
        assert!(app.location_history.forward.is_empty());
        arm(&mut app, &commands, bottom);
        press(&mut app, egui::Key::ArrowDown);
        press(&mut app, egui::Key::ArrowUp);
        idle(&mut app, 0.151);
        assert!(
            commands.try_recv().is_err(),
            "latest dirty candidate avoids Read"
        );
        assert!(app
            .project_text_preview_context()
            .unwrap()
            .text
            .contains("▶ 2 | needle second"));
        assert_eq!(app.documents.len(), 1);
        assert_eq!(app.documents[0].text, DRAFT);
        assert_eq!(app.documents[0].saved_text, SAVED);
        assert_eq!(app.documents[0].edit_version, version);
        assert!(app.recent_files.preview_history_snapshot() == recent);
        assert_eq!(
            editor_state::load(&app.editor_ctx, &mut app.documents[0])
                .cursor
                .char_range(),
            cursor
        );
        assert!(app.location_history.back.is_empty());
        assert!(app.location_history.forward.is_empty());
        assert!(app.location_history.pending.is_none());
        let mut undoer = before.undoer();
        assert_eq!(
            undoer.undo(&(cursor.unwrap(), DRAFT.into())).unwrap().1,
            SAVED
        );
    }
}

#[test]
fn continuous_preview_same_row_double_click_survives_settlement_but_cross_row_does_not() {
    for bottom in [false, true] {
        let (mut app, commands) = fixture(bottom);
        arm(&mut app, &commands, bottom);
        idle(&mut app, 0.5);
        click(&mut app, &row_probe(bottom, 1));
        let reserved = app.find_preview_identity();
        idle(&mut app, 0.151);
        let preview = read(&commands, "a.txt");
        assert!(app.find_preview_identity() == reserved);
        click(&mut app, &row_probe(bottom, 1));
        assert!(recorded_response(&app, &row_probe(bottom, 1))
            .double_clicked_by(egui::PointerButton::Primary));
        let opened = read(&commands, "a.txt");
        assert_ne!(
            preview.id, opened.id,
            "preview and ordinary Open are independent"
        );

        let (mut app, commands) = fixture(bottom);
        arm(&mut app, &commands, bottom);
        click(&mut app, &row_probe(bottom, 1));
        click(&mut app, &row_probe(bottom, 2));
        assert!(
            commands.try_recv().is_err(),
            "global cross-row double timing cannot Open"
        );
        idle(&mut app, 0.151);
        read(&commands, "b.txt");
        assert_eq!(app.documents.len(), 1);
    }
}

#[test]
fn continuous_preview_armed_view_invalidations_cannot_revive() {
    for bottom in [false, true] {
        for case in [
            "query",
            "filter",
            "rerun",
            "group",
            "provider",
            "hide",
            "navigation",
            "reconnect",
        ] {
            let (mut app, commands) = fixture(bottom);
            arm(&mut app, &commands, bottom);
            match case {
                "query" | "filter" => {
                    let probe = match (bottom, case) {
                        (true, "query") => "search_query",
                        (false, "query") => "browse_query",
                        (true, _) => "search_scope",
                        (false, _) => "browse_text_scope",
                    };
                    click(&mut app, probe);
                    let time = now(&app) + 0.01;
                    frame(&mut app, time, vec![egui::Event::Text("x".into())]);
                }
                "rerun" => {
                    click(
                        &mut app,
                        if bottom {
                            "search_submit"
                        } else {
                            "browse_submit"
                        },
                    );
                    let command = commands.try_recv().expect("explicit rerun");
                    assert!(matches!(command.op, Operation::Search { .. }));
                }
                "group" => click(
                    &mut app,
                    if bottom {
                        "search_group:0"
                    } else {
                        "browse_group:0"
                    },
                ),
                "provider" => {
                    if bottom {
                        app.tool = Tool::Git;
                    } else {
                        click(&mut app, "browse_files");
                    }
                }
                "hide" => {
                    if bottom {
                        app.tools_open = false;
                    } else {
                        app.browse_dismiss();
                    }
                    idle(&mut app, 0.01);
                    if bottom {
                        app.tools_open = true;
                    } else {
                        app.show_browse(browse::Provider::ProjectText);
                    }
                }
                "navigation" => app.navigation_epoch += 1,
                "reconnect" => app.generation += 1,
                _ => unreachable!(),
            }
            idle(&mut app, 0.2);
            assert!(!app.project_text_review_armed(), "bottom={bottom} {case}");
            assert!(!app.project_text_review_pending(), "bottom={bottom} {case}");
            assert!(
                commands.try_recv().is_err(),
                "invalidation must not issue Read: {case}"
            );
        }
    }
}

#[test]
fn continuous_preview_nonactive_target_draft_change_disarms_before_owned_selection() {
    for bottom in [false, true] {
        let (mut app, commands) = fixture(bottom);
        app.documents.push(Document::new(
            2,
            "a.txt".into(),
            "one\ntarget".into(),
            "c".repeat(64),
        ));
        arm(&mut app, &commands, bottom);
        press(&mut app, egui::Key::ArrowDown);
        idle(&mut app, 0.151);
        assert!(commands.try_recv().is_err());
        assert!(app.project_text_review_armed());
        app.documents[1].text.push_str(" changed");
        app.documents[1].edit_version += 1;
        press(&mut app, egui::Key::ArrowDown);
        idle(&mut app, 0.2);
        assert!(!app.project_text_review_armed());
        assert!(!app.project_text_review_pending());
        assert!(
            commands.try_recv().is_err(),
            "old target version cannot arm next row"
        );
    }
}

#[test]
fn continuous_preview_fixed_clock_roundoff_and_backward_clock_fail_closed() {
    for bottom in [false, true] {
        let (mut app, commands) = fixture(bottom);
        arm(&mut app, &commands, bottom);
        frame(&mut app, 1.95, key(egui::Key::ArrowDown));
        assert!(app.project_text_review_pending());
        frame(&mut app, 2.099, vec![]);
        assert!(commands.try_recv().is_err());
        frame(&mut app, 2.100, vec![]);
        read(&commands, "a.txt");

        // egui itself panics before app update on backward native time.
        // Exercise our fail-closed guard directly for malformed clock values.
        assert!(browse::preview_settlement_remaining(1.94, 1.95, 2.1).is_none());
        assert!(browse::preview_settlement_remaining(f64::NAN, 1.95, 2.1).is_none());
        assert!(browse::preview_settlement_remaining(f64::INFINITY, 1.95, 2.1).is_none());
        assert!(browse::preview_settlement_remaining(0.05, 0.05, 0.05 + 0.15).unwrap() > 0.0);
    }
}

fn context_scroll(app: &CedarApp, bottom: bool) -> (egui::Id, egui::Rect) {
    app.editor_ctx
        .data(|data| data.get_temp(egui::Id::new(("project_text_context_scroll", bottom))))
        .unwrap()
}
#[test]
fn continuous_preview_target_reveals_once_and_wheel_or_new_focus_cancels_late_reveal() {
    for bottom in [false, true] {
        for interrupt in ["none", "wheel", "focus", "space"] {
            let (mut app, commands) = fixture_with_target_line(bottom, 60);
            arm(&mut app, &commands, bottom);
            press(&mut app, egui::Key::ArrowDown);
            idle(&mut app, 0.151);
            let preview = read(&commands, "a.txt");
            let row_focus = focus(&app);
            if interrupt == "space" {
                let ticket = app.find_preview_identity();
                press(&mut app, egui::Key::Space);
                assert!(app.find_preview_identity() == ticket);
                assert!(commands.try_recv().is_err());
            } else if interrupt == "focus" {
                press(&mut app, egui::Key::Tab);
            } else if interrupt == "wheel" {
                let (_, rect) = context_scroll(&app, bottom);
                let time = now(&app) + 0.01;
                frame(
                    &mut app,
                    time,
                    vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -25.0),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            let text = (1..=120)
                .map(|line| {
                    if line == 60 {
                        "界".repeat(20_000)
                    } else {
                        format!("source line {line}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let before_focus = focus(&app);
            reply(&mut app, &preview, &text);
            for _ in 0..3 {
                idle(&mut app, 0.01);
            }
            assert_eq!(focus(&app), before_focus);
            let target = recorded_rect(&app, "project_text_context_target");
            let clip = recorded_clip(&app, "project_text_context_target");
            let (id, _) = context_scroll(&app, bottom);
            let state = egui::scroll_area::State::load(&app.editor_ctx, id).unwrap();
            if matches!(interrupt, "none" | "space") {
                assert_eq!(focus(&app), row_focus);
                assert!(clip.contains(egui::pos2(target.min.x + 1.0, target.center().y)), "target marker must be visibly revealed: bottom={bottom} target={target:?} clip={clip:?} offset={:?}", state.offset);
                assert!(state.offset.y > 0.0);
                let (_, viewport) = context_scroll(&app, bottom);
                let time = now(&app) + 0.01;
                frame(
                    &mut app,
                    time,
                    vec![
                        egui::Event::PointerMoved(viewport.center()),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, 180.0),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
                for _ in 0..8 {
                    idle(&mut app, 0.05);
                }
                let manual = egui::scroll_area::State::load(&app.editor_ctx, id)
                    .unwrap()
                    .offset;
                assert!(manual.y < state.offset.y, "actual wheel must move preview");
                for _ in 0..3 {
                    idle(&mut app, 0.05);
                }
                let later = egui::scroll_area::State::load(&app.editor_ctx, id)
                    .unwrap()
                    .offset;
                assert!(
                    (later.y - manual.y).abs() < 0.01,
                    "repaint must preserve manual scroll"
                );
            } else {
                assert!(
                    state.offset.y < target.height(),
                    "new user intent vetoes late target snapback: {interrupt}"
                );
            }
            assert!(commands.try_recv().is_err());
        }
    }
}
