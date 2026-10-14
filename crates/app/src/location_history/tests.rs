use super::*;
use crate::{
    editor_state,
    model::Document,
    worker::{Event, Worker},
    ConnectionState,
};
use sha2::{Digest, Sha256};

fn app() -> CedarApp {
    let mut app = CedarApp::empty();
    app.editor_ctx.set_fonts(crate::system_fonts::base_fonts());
    app.open_form = false;
    app.state = ConnectionState::Ready;
    app.agent_info = Some(crate::agent_support::full_test_agent());
    app.documents = (1..=3)
        .map(|id| {
            Document::new(
                id,
                format!("{id}.txt"),
                "a😀b\nsecond\n終わり".into(),
                format!("r{id}"),
            )
        })
        .collect();
    app.next_document = 4;
    app.active_document = Some(1);
    for id in 1..=3 {
        set_selection(&mut app, id, range(id as usize, 0, false, true));
    }
    app
}
fn range(primary: usize, secondary: usize, pa: bool, sa: bool) -> CCursorRange {
    let mut range = CCursorRange::two(CCursor::new(secondary), CCursor::new(primary));
    range.primary.prefer_next_row = pa;
    range.secondary.prefer_next_row = sa;
    range
}
fn set_selection(app: &mut CedarApp, document: u64, range: CCursorRange) {
    let doc = app
        .documents
        .iter_mut()
        .find(|doc| doc.id == document)
        .unwrap();
    let mut state = editor_state::load(&app.editor_ctx, doc);
    state.cursor.set_char_range(Some(range));
    state.store(&app.editor_ctx, egui::Id::new(("editor", document)));
    doc.cursor = crate::model::cursor_location(&doc.text, range.primary.index);
}
fn selection(app: &CedarApp, document: u64) -> CCursorRange {
    egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", document)))
        .unwrap()
        .cursor
        .char_range()
        .unwrap()
}
fn signature(entries: &[Location]) -> Vec<(u64, u64, u64, usize, usize, bool, bool)> {
    entries
        .iter()
        .map(|entry| {
            (
                entry.generation,
                entry.document,
                entry.edit_version,
                entry.selection.primary.index,
                entry.selection.secondary.index,
                entry.selection.primary.prefer_next_row,
                entry.selection.secondary.prefer_next_row,
            )
        })
        .collect()
}
fn frame(app: &mut CedarApp, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 640.0),
        )),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native))
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
fn step(app: &mut CedarApp, direction: Direction) -> bool {
    let ctx = app.editor_ctx.clone();
    let mut success = false;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        success = app.history_step(direction, ctx);
        app.finish_history_frame(ctx);
    });
    success
}
fn painted_texts(output: &egui::FullOutput) -> Vec<String> {
    fn collect(shape: &egui::epaint::Shape, texts: &mut Vec<String>) {
        match shape {
            egui::epaint::Shape::Text(text) => texts.push(text.galley.job.text.clone()),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, texts);
                }
            }
            _ => {}
        }
    }
    let mut texts = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, &mut texts);
    }
    texts
}

fn label(
    app: &CedarApp,
    output: &egui::FullOutput,
    label: &str,
    size: egui::Vec2,
    sidebar_width: f32,
) -> egui::Pos2 {
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
        .unwrap_or_else(|| {
            panic!(
                "missing exact label {label:?} at size={size:?}, sidebar_width={sidebar_width}; painted_texts={:?}; {}",
                painted_texts(output),
                compact_history_context(app)
            )
        })
}

const COMPACT_HISTORY_TRACE: &str = "compact_history_trace";
const COMPACT_HISTORY_SIZES: [(egui::Vec2, f32); 3] = [
    (egui::vec2(780.0, 540.0), 180.0),
    (egui::vec2(780.0, 540.0), 246.0),
    (egui::vec2(1000.0, 700.0), 460.0),
];

#[test]
fn compact_editor_scroll_probe_releases_its_context() {
    let (size, width) = COMPACT_HISTORY_SIZES[0];
    let mut app = compact_history_app(size, width);
    let (fonts, deliver) = crate::system_fonts::SystemFonts::pending_failure_for_test();
    app.system_fonts = fonts;
    deliver();
    // Exercise real response capture without a host font worker retaining ctx.
    compact_history_frame(&mut app, size, vec![]);
    compact_history_frame(&mut app, size, vec![]);
    assert!(editor_scroll_probe(&app).5.is_some());
    let witness = std::sync::Arc::new(());
    app.editor_ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("context_drop_witness"), witness.clone());
    });
    assert_eq!(std::sync::Arc::strong_count(&witness), 2);
    drop(app);
    assert_eq!(
        std::sync::Arc::strong_count(&witness),
        1,
        "the test probe must not keep the editor Context alive after the app drops"
    );
}

type EditorScrollProbe = (
    egui::Id,
    egui::Rect,
    egui::Vec2,
    egui::Vec2,
    egui::Rect,
    Option<crate::EditorScrollResponseSnapshot>,
    Option<crate::EditorScrollResponseSnapshot>,
);

fn editor_scroll_probe(app: &CedarApp) -> EditorScrollProbe {
    app.editor_ctx.data(|data| {
        data.get_temp::<EditorScrollProbe>(egui::Id::new("compact_history_editor_scroll"))
            .expect("the production editor ScrollArea must be rendered")
    })
}

fn compact_history_context(app: &CedarApp) -> String {
    let frames = app.editor_ctx.data(|data| {
        data.get_temp::<Vec<String>>(egui::Id::new(COMPACT_HISTORY_TRACE))
            .unwrap_or_default()
    });
    format!("recent production frames: {}", frames.join("\n"))
}

fn compact_history_frame(
    app: &mut CedarApp,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        events,
        ..Default::default()
    };
    let original_events = input.events.clone();
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let retained_events = input.events.clone();
    ctx.run(input, |ctx| {
        eframe::App::update(app, ctx, &mut native);
        let overflow = crate::workspace_access_tests::recorded_response(app, "editor_overflow");
        let scroll = editor_scroll_probe(app);
        let clicked = ctx.interaction_snapshot(|snapshot| snapshot.clicked);
        let trace = format!(
            "pass={}, size={size:?}, fonts={}, error={:?}, overflow={:?}/{:?}, \
             clicked={clicked:?}, drag={:?}, native={original_events:?}, retained={retained_events:?}",
            ctx.cumulative_pass_nr(),
            app.system_fonts.probe_state_for_test(),
            app.error,
            overflow.id,
            overflow.interact_rect,
            scroll.5.as_ref().map(|response| (response.id, response.interact_rect, response.dragged))
        );
        ctx.data_mut(|data| {
            let frames = data.get_temp_mut_or_default::<Vec<String>>(egui::Id::new(COMPACT_HISTORY_TRACE));
            if frames.len() == 24 {
                frames.remove(0);
            }
            frames.push(trace);
        });
    })
}

fn compact_history_app(size: egui::Vec2, sidebar_width: f32) -> CedarApp {
    let mut app = app();
    let form = crate::ConnectForm {
        local_root: "/workspace".into(),
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/workspace".into();
    app.editor_ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(9.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
    });
    app.editor_ctx.data_mut(|data| {
        data.insert_persisted(
            egui::Id::new("explorer"),
            egui::containers::panel::PanelState {
                rect: egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(sidebar_width, size.y),
                ),
            },
        );
    });
    app.activate_history_tab(2);
    app
}

fn compact_visible(app: &CedarApp, size: egui::Vec2, name: &str) -> egui::Rect {
    let rect = crate::workspace_access_tests::recorded_rect(app, name);
    let clip = crate::workspace_access_tests::recorded_clip(app, name);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    assert!(
        rect.is_positive() && screen.contains_rect(rect) && clip.contains_rect(rect),
        "{name} outside/clipped at {size:?}: rect={rect:?}, clip={clip:?}; {}",
        compact_history_context(app)
    );
    rect
}

fn compact_pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn compact_click(app: &mut CedarApp, size: egui::Vec2, name: &str) -> egui::FullOutput {
    let at = compact_visible(app, size, name).center();
    compact_history_frame(
        app,
        size,
        vec![
            egui::Event::PointerMoved(at),
            compact_pointer(at, true),
            compact_pointer(at, false),
        ],
    )
}

fn compact_assert_menu(
    app: &CedarApp,
    output: &egui::FullOutput,
    size: egui::Vec2,
    sidebar_width: f32,
) {
    for (text, widget) in [
        ("Copy draft", "editor_copy_draft"),
        ("Compare with disk", "editor_compare_with_disk"),
    ] {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        assert!(
            screen.contains(label(app, output, text, size, sidebar_width)),
            "{text} outside {size:?}"
        );
        compact_visible(app, size, widget);
    }
}

fn compact_assert_scroll_lane(app: &CedarApp) {
    let (_, _, _, _, lane, drag, _) = editor_scroll_probe(app);
    if let Some(drag) = drag {
        assert!(
            lane.contains_rect(drag.interact_rect),
            "background drag escaped current editor lane: {:?} / {lane:?}; {}",
            drag.interact_rect,
            compact_history_context(app)
        );
        let toolbar = crate::workspace_access_tests::recorded_rect(app, "editor_overflow");
        assert!(
            !drag.interact_rect.intersects(toolbar),
            "background drag covers editor toolbar; {}",
            compact_history_context(app)
        );
    }
}

fn one_buffer_abc_history() -> (CedarApp, [Location; 3]) {
    let mut app = app();
    app.documents.truncate(1);
    frame(&mut app, vec![]);
    set_selection(&mut app, 1, CCursorRange::one(CCursor::new(0)));
    let a = app.history_departure().unwrap();
    app.history_go_to_line(2);
    frame(&mut app, vec![]);
    let b = app.history_departure().unwrap();
    app.history_go_to_line(3);
    frame(&mut app, vec![]);
    let c = app.history_departure().unwrap();
    assert_eq!(signature(&app.location_history.back), signature(&[a, b]));
    assert_eq!(
        [
            a.selection.primary.index,
            b.selection.primary.index,
            c.selection.primary.index,
        ],
        [0, 4, 11]
    );
    (app, [a, b, c])
}

#[test]
fn current_equal_back_reaches_older_same_buffer_location() {
    let (mut app, [a, b, _]) = one_buffer_abc_history();
    // Ordinary caret movement admits no history and does not edit the buffer.
    set_selection(&mut app, 1, b.selection);
    assert!(app.history_departure().unwrap().same(b));
    assert!(step(&mut app, Direction::Back));
    assert!(app.history_departure().unwrap().same(a));
    assert!(app.location_history.back.is_empty());
    assert_eq!(signature(&app.location_history.forward), signature(&[b]));
    assert!(app.location_history.message.is_none());
}

#[test]
fn current_equal_forward_reaches_newer_same_buffer_location() {
    let (mut app, [_, b, c]) = one_buffer_abc_history();
    assert!(step(&mut app, Direction::Back));
    assert!(step(&mut app, Direction::Back));
    assert_eq!(signature(&app.location_history.forward), signature(&[c, b]));
    set_selection(&mut app, 1, b.selection);
    assert!(app.history_departure().unwrap().same(b));
    assert!(step(&mut app, Direction::Forward));
    assert!(app.history_departure().unwrap().same(c));
    assert!(app.location_history.forward.is_empty());
    assert_eq!(signature(&app.location_history.back), signature(&[b]));
    assert!(app.location_history.message.is_none());
}

fn history_frame(app: &mut CedarApp, direction: Direction, button: bool) {
    if !button {
        let shortcut_key = match direction {
            Direction::Back => egui::Key::OpenBracket,
            Direction::Forward => egui::Key::CloseBracket,
        };
        frame(app, key(shortcut_key, egui::Modifiers::COMMAND));
        return;
    }
    frame(app, vec![]);
    let at = crate::workspace_access_tests::recorded_rect(
        app,
        match direction {
            Direction::Back => "Back",
            Direction::Forward => "Forward",
        },
    )
    .center();
    frame(
        app,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    frame(
        app,
        vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
}

#[test]
fn current_equal_native_caret_frames_reach_distinct_target_in_both_directions() {
    for direction in [Direction::Back, Direction::Forward] {
        for button in [false, true] {
            let mut app = app();
            app.documents.truncate(1);
            frame(&mut app, vec![]);
            set_selection(&mut app, 1, CCursorRange::one(CCursor::new(0)));
            let a = app.history_departure().unwrap();
            app.history_go_to_line(2);
            frame(&mut app, vec![]);
            // Native vertical movement prefers the next row. Capture B with
            // that exact affinity before the explicit jump to C records it.
            frame(&mut app, key(egui::Key::Home, egui::Modifiers::NONE));
            let b = app.history_departure().unwrap();
            app.history_go_to_line(3);
            frame(&mut app, vec![]);
            let c = app.history_departure().unwrap();
            assert_eq!(signature(&app.location_history.back), signature(&[a, b]));
            let (caret_key, target, opposite) = match direction {
                Direction::Back => (egui::Key::ArrowUp, a, Direction::Forward),
                Direction::Forward => {
                    history_frame(&mut app, Direction::Back, false);
                    history_frame(&mut app, Direction::Back, false);
                    assert!(app.history_departure().unwrap().same(a));
                    (egui::Key::ArrowDown, c, Direction::Back)
                }
            };
            let source = signature(app.location_history.stack_mut(direction));
            let text = app.documents[0].text.clone();
            let saved_text = app.documents[0].saved_text.clone();
            let revision = app.documents[0].revision.clone();
            frame(&mut app, key(caret_key, egui::Modifiers::NONE));
            assert!(app.history_departure().unwrap().same(b));
            assert_eq!(signature(app.location_history.stack_mut(direction)), source);
            history_frame(&mut app, direction, button);
            assert!(app.history_departure().unwrap().same(target));
            assert!(app.location_history.stack_mut(direction).is_empty());
            assert_eq!(
                signature(app.location_history.stack_mut(opposite)),
                signature(&[b])
            );
            assert_eq!(app.documents[0].text, text);
            assert_eq!(app.documents[0].saved_text, saved_text);
            assert_eq!(app.documents[0].revision, revision);
            assert!(app.location_history.message.is_none());
        }
    }
}

#[test]
fn current_equal_all_equal_pruning_preserves_opposite_and_cancels_pending_read() {
    for direction in [Direction::Back, Direction::Forward] {
        for full_frame in [false, true] {
            let (mut app, [a, b, _]) = one_buffer_abc_history();
            set_selection(&mut app, 1, b.selection);
            let opposite = match direction {
                Direction::Back => Direction::Forward,
                Direction::Forward => Direction::Back,
            };
            *app.location_history.stack_mut(direction) = vec![b, b];
            *app.location_history.stack_mut(opposite) = vec![a];
            let text = app.documents[0].text.clone();
            let (worker, commands) = Worker::recording();
            app.worker = Some(worker);
            app.open("late.txt".into(), None);
            let command = commands.try_recv().unwrap();
            let epoch = app.navigation_epoch;
            assert!(app.location_history.pending.is_some());
            if full_frame {
                history_frame(&mut app, direction, false);
            } else {
                assert!(!step(&mut app, direction));
            }
            assert!(app.location_history.stack_mut(direction).is_empty());
            assert_eq!(
                signature(app.location_history.stack_mut(opposite)),
                signature(&[a])
            );
            assert!(app.history_departure().unwrap().same(b));
            assert_eq!(app.documents[0].text, text);
            assert!(app.location_history.pending.is_none());
            assert_eq!(app.navigation_epoch, epoch.wrapping_add(1));
            assert!(app.location_history.message.is_none());
            app.apply_event(Event {
                id: command.id,
                generation: app.generation,
                connected: true,
                result: Ok(cedar_protocol::Payload::File {
                    path: "late.txt".into(),
                    text: "late".into(),
                    revision: "r".into(),
                }),
            });
            assert!(app.history_departure().unwrap().same(b));
            assert!(app.location_history.stack_mut(direction).is_empty());
            assert_eq!(
                signature(app.location_history.stack_mut(opposite)),
                signature(&[a])
            );
            assert!(app.documents.iter().any(|doc| doc.path == "late.txt"));
        }
    }
}

#[test]
fn current_equal_mixed_stale_entries_count_only_invalid_locations() {
    for direction in [Direction::Back, Direction::Forward] {
        for full_frame in [false, true] {
            for has_target in [false, true] {
                let (mut app, [a, b, c]) = one_buffer_abc_history();
                set_selection(&mut app, 1, b.selection);
                let opposite = match direction {
                    Direction::Back => Direction::Forward,
                    Direction::Forward => Direction::Back,
                };
                let mut wrong_version = b;
                wrong_version.edit_version += 1;
                let mut wrong_generation = b;
                wrong_generation.generation = app.generation.wrapping_add(1);
                let mut entries = Vec::new();
                if has_target {
                    entries.push(a);
                }
                entries.extend([wrong_version, b, wrong_generation, b]);
                *app.location_history.stack_mut(direction) = entries;
                *app.location_history.stack_mut(opposite) = vec![c];
                if full_frame {
                    history_frame(&mut app, direction, false);
                } else {
                    assert_eq!(step(&mut app, direction), has_target);
                }
                assert!(app.location_history.stack_mut(direction).is_empty());
                assert!(app
                    .history_departure()
                    .unwrap()
                    .same(if has_target { a } else { b }));
                let expected = if has_target { vec![c, b] } else { vec![c] };
                assert_eq!(
                    signature(app.location_history.stack_mut(opposite)),
                    signature(&expected)
                );
                assert_eq!(
                    app.location_history.message.as_deref(),
                    Some("Skipped 2 stale editor locations")
                );
            }
        }
    }
}

#[test]
fn current_equal_pruning_keeps_affinity_and_orientation_distinctions() {
    for direction in [Direction::Back, Direction::Forward] {
        for full_frame in [false, true] {
            for target_selection in [
                range(1, 3, true, false),
                range(1, 3, false, true),
                range(3, 1, false, false),
            ] {
                let (mut app, [a, _, _]) = one_buffer_abc_history();
                set_selection(&mut app, 1, range(1, 3, false, false));
                let current = app.history_departure().unwrap();
                let target = Location {
                    selection: target_selection,
                    ..current
                };
                let opposite = match direction {
                    Direction::Back => Direction::Forward,
                    Direction::Forward => Direction::Back,
                };
                *app.location_history.stack_mut(direction) = vec![a, target, current];
                app.location_history.stack_mut(opposite).clear();
                if full_frame {
                    history_frame(&mut app, direction, false);
                } else {
                    assert!(step(&mut app, direction));
                }
                assert!(app.history_departure().unwrap().same(target));
                assert_eq!(
                    signature(app.location_history.stack_mut(direction)),
                    signature(&[a])
                );
                assert_eq!(
                    signature(app.location_history.stack_mut(opposite)),
                    signature(&[current])
                );
                assert!(app.location_history.message.is_none());
            }
        }
    }
}

#[test]
fn same_location_line_navigation_preserves_both_history_stacks() {
    let (mut app, [a, b, c]) = one_buffer_abc_history();
    assert!(step(&mut app, Direction::Back));
    assert!(app.history_departure().unwrap().same(b));
    app.history_go_to_line(2);
    frame(&mut app, vec![]);
    assert!(app.history_departure().unwrap().same(b));
    assert_eq!(signature(&app.location_history.back), signature(&[a]));
    assert_eq!(signature(&app.location_history.forward), signature(&[c]));
    assert!(app.location_history.pending.is_none());
}

#[test]
fn abc_roundtrip_preserves_reversed_unicode_and_both_affinities() {
    let mut app = app();
    let first = range(1, 3, true, false);
    set_selection(&mut app, 1, first);
    app.activate_history_tab(2);
    let second = selection(&app, 2);
    app.activate_history_tab(3);
    let text: Vec<_> = app
        .documents
        .iter()
        .map(|doc| {
            (
                doc.text.clone(),
                doc.saved_text.clone(),
                doc.revision.clone(),
                doc.edit_version,
            )
        })
        .collect();
    assert!(step(&mut app, Direction::Back));
    assert_eq!(app.active_document, Some(2));
    assert!(same_selection(selection(&app, 2), second));
    assert!(step(&mut app, Direction::Back));
    assert_eq!(app.active_document, Some(1));
    assert!(same_selection(selection(&app, 1), first));
    assert!(!step(&mut app, Direction::Back));
    assert!(step(&mut app, Direction::Forward));
    assert!(step(&mut app, Direction::Forward));
    assert_eq!(app.active_document, Some(3));
    assert_eq!(
        app.documents
            .iter()
            .map(|doc| (
                doc.text.clone(),
                doc.saved_text.clone(),
                doc.revision.clone(),
                doc.edit_version
            ))
            .collect::<Vec<_>>(),
        text
    );
}

#[test]
fn branches_clear_forward_only_after_distinct_completed_navigation() {
    let mut app = app();
    app.activate_history_tab(2);
    app.activate_history_tab(3);
    assert!(step(&mut app, Direction::Back));
    let before = signature(&app.location_history.forward);
    app.activate_history_tab(2);
    assert_eq!(signature(&app.location_history.forward), before);
    app.open("2.txt".into(), None);
    assert_eq!(signature(&app.location_history.forward), before);
    app.activate_history_tab(1);
    assert!(app.location_history.forward.is_empty());
    assert_eq!(app.location_history.back.len(), 2);
}

#[test]
fn both_stacks_are_bounded_to_sixty_four() {
    let mut app = app();
    for index in 0..150 {
        app.activate_history_tab(index % 3 + 1);
    }
    assert_eq!(app.location_history.back.len(), LIMIT);
    for _ in 0..LIMIT {
        assert!(step(&mut app, Direction::Back));
    }
    assert_eq!(app.location_history.forward.len(), LIMIT);
    assert!(!step(&mut app, Direction::Back));
}

#[test]
fn partial_and_all_stale_pruning_never_changes_opposite_stack_without_restore() {
    let mut app = app();
    app.activate_history_tab(2);
    app.activate_history_tab(3);
    app.documents[1].edit_version += 1;
    assert!(step(&mut app, Direction::Back));
    assert_eq!(app.active_document, Some(1));
    assert_eq!(
        app.location_history.message.as_deref(),
        Some("Skipped 1 stale editor location")
    );
    assert_eq!(app.location_history.forward.len(), 1);
    app.activate_history_tab(2);
    app.activate_history_tab(3);
    assert!(step(&mut app, Direction::Back));
    let opposite = signature(&app.location_history.forward);
    let before = selection(&app, 2);
    app.documents[0].edit_version += 1;
    assert!(!step(&mut app, Direction::Back));
    assert_eq!(app.active_document, Some(2));
    assert!(same_selection(selection(&app, 2), before));
    assert_eq!(signature(&app.location_history.forward), opposite);
    assert!(app.location_history.back.is_empty());
    assert_eq!(
        app.location_history.message.as_deref(),
        Some("Skipped 1 stale editor location")
    );
}

#[test]
fn closed_ids_never_revive_and_generation_change_clears_everything() {
    let mut app = app();
    app.activate_history_tab(2);
    app.activate_history_tab(3);
    app.remove_tab(1);
    assert!(app
        .location_history
        .back
        .iter()
        .all(|entry| entry.document != 1));
    app.documents.push(Document::new(
        4,
        "1.txt".into(),
        "a😀b\nsecond\n終わり".into(),
        "r1".into(),
    ));
    assert!(step(&mut app, Direction::Back));
    assert!(!step(&mut app, Direction::Back));
    app.state = ConnectionState::Connecting;
    app.cancel_connection();
    assert!(app.location_history.back.is_empty());
    assert!(app.location_history.forward.is_empty());
    assert!(app.location_history.pending.is_none());
}

#[test]
fn saturated_versions_are_never_admitted_or_restored() {
    let mut app = app();
    app.documents[0].edit_version = u64::MAX;
    app.activate_history_tab(2);
    assert!(app.location_history.back.is_empty());
    app.activate_history_tab(3);
    app.documents[1].edit_version = u64::MAX;
    assert!(!step(&mut app, Direction::Back));
    assert_eq!(app.active_document, Some(3));
}

#[test]
fn line_jump_admits_only_after_selection_and_chooser_cancel_admits_nothing() {
    let mut app = app();
    frame(&mut app, vec![]);
    let departure = selection(&app, 1);
    app.history_go_to_line(2);
    assert!(app.location_history.back.is_empty());
    frame(&mut app, vec![]);
    assert_eq!(app.location_history.back.len(), 1);
    assert!(same_selection(
        app.location_history.back[0].selection,
        departure
    ));
    assert_eq!(selection(&app, 1).primary.index, 4);
    app.history_go_to_line(2);
    frame(&mut app, vec![]);
    assert_eq!(app.location_history.back.len(), 1);
    app.show_file_chooser();
    frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
    frame(&mut app, vec![]);
    assert_eq!(app.location_history.back.len(), 1);
}

#[test]
fn completed_read_admits_once_failed_and_stale_reads_do_not() {
    let mut app = app();
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.open("new.txt".into(), None);
    let command = commands.try_recv().unwrap();
    assert!(app.location_history.back.is_empty());
    app.apply_event(Event {
        id: command.id,
        generation: app.generation,
        connected: true,
        result: Ok(cedar_protocol::Payload::File {
            path: "new.txt".into(),
            text: "new".into(),
            revision: "r".into(),
        }),
    });
    assert_eq!(app.location_history.back.len(), 1);
    assert!(app.location_history.pending.is_none());
    app.open("failure.txt".into(), None);
    let command = commands.try_recv().unwrap();
    app.apply_event(Event {
        id: command.id,
        generation: app.generation,
        connected: true,
        result: Err("not_found: missing".into()),
    });
    assert_eq!(app.location_history.back.len(), 1);
    assert!(app.location_history.pending.is_none());
    app.open("slow.txt".into(), Some(1));
    let command = commands.try_recv().unwrap();
    app.activate_history_tab(2);
    let before = signature(&app.location_history.back);
    app.apply_event(Event {
        id: command.id,
        generation: app.generation,
        connected: true,
        result: Ok(cedar_protocol::Payload::File {
            path: "slow.txt".into(),
            text: "slow".into(),
            revision: "r".into(),
        }),
    });
    assert!(app.documents.iter().any(|doc| doc.path == "slow.txt"));
    assert_eq!(app.active_document, Some(2));
    assert_eq!(signature(&app.location_history.back), before);
}

#[test]
fn delayed_source_edit_omits_departure_and_clears_forward_on_success() {
    let mut app = app();
    app.activate_history_tab(2);
    app.activate_history_tab(3);
    assert!(step(&mut app, Direction::Back));
    let before = signature(&app.location_history.back);
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.open("later.txt".into(), None);
    let command = commands.try_recv().unwrap();
    app.documents[1].edit_version += 1;
    app.documents[1].text.push('!');
    app.apply_event(Event {
        id: command.id,
        generation: app.generation,
        connected: true,
        result: Ok(cedar_protocol::Payload::File {
            path: "later.txt".into(),
            text: "later".into(),
            revision: "r".into(),
        }),
    });
    assert_eq!(signature(&app.location_history.back), before);
    assert!(app.location_history.forward.is_empty());
}

#[test]
fn shortcuts_buttons_and_one_shot_focus_use_production_frames() {
    let mut app = app();
    frame(&mut app, vec![]);
    app.activate_history_tab(2);
    frame(
        &mut app,
        key(egui::Key::OpenBracket, egui::Modifiers::COMMAND),
    );
    assert_eq!(app.active_document, Some(1));
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new(("editor", 1u64)))));
    frame(
        &mut app,
        key(egui::Key::CloseBracket, egui::Modifiers::COMMAND),
    );
    assert_eq!(app.active_document, Some(2));
    frame(&mut app, vec![]);
    let at = crate::workspace_access_tests::recorded_rect(&app, "Back").center();
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert_eq!(app.active_document, Some(1));
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new(("editor", 1u64)))));
    let other = egui::Id::new("newer_input");
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(other));
    frame(&mut app, vec![]);
    assert_ne!(
        app.editor_ctx.memory(|memory| memory.focused()),
        Some(egui::Id::new(("editor", 1u64)))
    );
}

#[test]
fn repeat_mixed_modal_plain_bracket_and_altgr_input_never_traverse() {
    for events in [
        {
            let mut events = key(egui::Key::OpenBracket, egui::Modifiers::COMMAND);
            events.push(egui::Event::Paste("x".into()));
            events
        },
        key(egui::Key::OpenBracket, egui::Modifiers::NONE),
        key(
            egui::Key::OpenBracket,
            egui::Modifiers::CTRL | egui::Modifiers::ALT,
        ),
        key(
            egui::Key::OpenBracket,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        ),
    ] {
        let mut app = app();
        app.activate_history_tab(2);
        frame(&mut app, events);
        assert_eq!(app.active_document, Some(2));
        assert_eq!(app.location_history.back.len(), 1);
    }
    let mut app = app();
    app.activate_history_tab(2);
    app.show_file_chooser();
    frame(
        &mut app,
        key(egui::Key::OpenBracket, egui::Modifiers::COMMAND),
    );
    assert_eq!(app.active_document, Some(2));
}

#[test]
fn edit_then_undo_to_same_text_does_not_revive_old_location() {
    let mut app = app();
    frame(&mut app, vec![]);
    app.activate_history_tab(2);
    let initial = app.documents[0].text.clone();
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        format!("{initial}!"),
        0,
    );
    app.activate_history_tab(1);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
    frame(&mut app, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, initial);
    assert_eq!(app.documents[0].edit_version, 2);
    app.activate_history_tab(3);
    assert!(step(&mut app, Direction::Back));
    assert!(step(&mut app, Direction::Back));
    assert!(!step(&mut app, Direction::Back));
    assert_eq!(app.active_document, Some(2));
    assert_eq!(
        app.location_history.message.as_deref(),
        Some("Skipped 1 stale editor location")
    );
}

#[test]
fn delayed_read_keeps_original_departure_after_caret_only_movement() {
    let mut app = app();
    let original = selection(&app, 1);
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.open("later.txt".into(), None);
    let command = commands.try_recv().unwrap();
    set_selection(&mut app, 1, range(5, 4, false, true));
    app.apply_event(Event {
        id: command.id,
        generation: app.generation,
        connected: true,
        result: Ok(cedar_protocol::Payload::File {
            path: "later.txt".into(),
            text: "later".into(),
            revision: "r".into(),
        }),
    });
    assert_eq!(app.location_history.back.len(), 1);
    assert!(same_selection(
        app.location_history.back[0].selection,
        original
    ));
}

#[test]
fn unrelated_pending_jump_cannot_complete_a_read_or_language_ticket() {
    for language in [false, true] {
        let mut app = app();
        let departure = app.history_departure();
        app.history_begin(departure, language);
        app.documents[0].jump_to = Some(4);
        frame(&mut app, vec![]);
        assert!(app.location_history.back.is_empty());
        assert!(app.location_history.pending.is_some());
        assert_eq!(selection(&app, 1).primary.index, 4);
    }
}

#[test]
fn active_tab_click_cancels_pending_read_without_admitting_or_focusing_its_reply() {
    let mut app = app();
    frame(&mut app, vec![]);
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.open("later.txt".into(), None);
    let command = commands.try_recv().unwrap();
    let tab = crate::workspace_access_tests::recorded_response(&app, "editor_tab:1").id;
    let at = crate::workspace_access_tests::recorded_rect(&app, "editor_tab:1").center();
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert_eq!(
        app.editor_ctx
            .interaction_snapshot(|snapshot| snapshot.clicked),
        Some(tab)
    );
    assert!(app.location_history.pending.is_none());
    app.apply_event(Event {
        id: command.id,
        generation: app.generation,
        connected: true,
        result: Ok(cedar_protocol::Payload::File {
            path: "later.txt".into(),
            text: "later".into(),
            revision: "r".into(),
        }),
    });
    assert!(app.location_history.back.is_empty());
    assert_eq!(app.active_document, Some(1));
    assert!(app.documents.iter().any(|doc| doc.path == "later.txt"));
}

#[test]
fn tab_and_enter_activate_rendered_history_buttons() {
    let mut app = app();
    app.activate_history_tab(2);
    frame(&mut app, vec![]);
    let back = crate::workspace_access_tests::recorded_response(&app, "Back").id;
    app.editor_ctx
        .memory_mut(|memory| memory.surrender_focus(memory.focused().unwrap_or(back)));
    let mut reached = false;
    for _ in 0..40 {
        frame(&mut app, key(egui::Key::Tab, egui::Modifiers::NONE));
        if app.editor_ctx.memory(|memory| memory.has_focus(back)) {
            reached = true;
            break;
        }
    }
    assert!(reached, "ordinary Tab navigation must reach Back");
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert_eq!(app.active_document, Some(1));
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new(("editor", 1u64)))));
}

#[test]
fn held_history_press_cannot_act_on_new_navigation_generation_or_edited_target() {
    for mutation in 0..3 {
        let mut app = app();
        app.activate_history_tab(2);
        app.activate_history_tab(3);
        frame(&mut app, vec![]);
        let at = crate::workspace_access_tests::recorded_rect(&app, "Back").center();
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        match mutation {
            0 => app.activate_history_tab(1),
            1 => app.generation += 1,
            _ => app.documents[1].edit_version += 1,
        }
        let before = app.active_document;
        let stack = signature(&app.location_history.back);
        frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(app.active_document, before);
        assert_eq!(signature(&app.location_history.back), stack);
    }
}

#[test]
fn full_editor_frames_preserve_both_affinities_and_orientation() {
    let mut app = app();
    frame(&mut app, vec![]);
    let original = range(1, 3, true, false);
    set_selection(&mut app, 1, original);
    app.activate_history_tab(2);
    frame(&mut app, vec![]);
    frame(
        &mut app,
        key(egui::Key::OpenBracket, egui::Modifiers::COMMAND),
    );
    assert!(same_selection(selection(&app, 1), original));
    frame(
        &mut app,
        key(egui::Key::CloseBracket, egui::Modifiers::COMMAND),
    );
    frame(
        &mut app,
        key(egui::Key::OpenBracket, egui::Modifiers::COMMAND),
    );
    assert!(same_selection(selection(&app, 1), original));
}

#[test]
fn concrete_control_and_command_tuples_match_without_alt_shift_or_both() {
    let windows = egui::Modifiers {
        ctrl: true,
        command: true,
        ..egui::Modifiers::NONE
    };
    let mac = egui::Modifiers {
        mac_cmd: true,
        command: true,
        ..egui::Modifiers::NONE
    };
    for modifiers in [windows, mac] {
        assert_eq!(
            shortcut(&key(egui::Key::OpenBracket, modifiers)[0]),
            Some(Direction::Back)
        );
        assert_eq!(
            shortcut(&key(egui::Key::CloseBracket, modifiers)[0]),
            Some(Direction::Forward)
        );
        for changed in [
            egui::Modifiers {
                alt: true,
                ..modifiers
            },
            egui::Modifiers {
                shift: true,
                ..modifiers
            },
            egui::Modifiers {
                ctrl: true,
                mac_cmd: true,
                ..modifiers
            },
        ] {
            assert_eq!(shortcut(&key(egui::Key::OpenBracket, changed)[0]), None);
        }
    }
}

#[test]
fn plain_brackets_altgr_and_mixed_paste_keep_native_editor_input() {
    for modifiers in [
        egui::Modifiers::NONE,
        egui::Modifiers::CTRL | egui::Modifiers::ALT,
    ] {
        let mut app = app();
        app.activate_history_tab(2);
        frame(&mut app, vec![]);
        set_selection(&mut app, 2, range(0, 0, false, false));
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 2u64))));
        let mut events = key(egui::Key::OpenBracket, modifiers);
        events.push(egui::Event::Text("[".into()));
        frame(&mut app, events);
        assert_eq!(app.active_document, Some(2));
        assert!(app.documents[1].text.starts_with("[a😀b"));
    }
    let mut app = app();
    app.activate_history_tab(2);
    frame(&mut app, vec![]);
    set_selection(&mut app, 2, range(0, 0, false, false));
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 2u64))));
    let mut events = key(egui::Key::OpenBracket, egui::Modifiers::COMMAND);
    events.push(egui::Event::Paste("queued".into()));
    frame(&mut app, events);
    assert_eq!(app.active_document, Some(2));
    assert!(app.documents[1].text.starts_with("queueda😀b"));
}

#[test]
fn opening_a_background_buffer_waits_for_its_retained_line_jump() {
    let mut app = app();
    app.documents[1].jump_to = Some(4);
    app.open("2.txt".into(), None);
    assert!(app.location_history.back.is_empty());
    assert!(app.location_history.pending.is_some());
    frame(&mut app, vec![]);
    assert_eq!(app.location_history.back.len(), 1);
    assert_eq!(selection(&app, 2).primary.index, 4);
    assert!(app.location_history.pending.is_none());
}

#[test]
fn all_stale_back_cancels_an_older_read_without_location_or_opposite_mutation() {
    let mut app = app();
    app.activate_history_tab(2);
    app.documents[0].edit_version += 1;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.open("late.txt".into(), None);
    let command = commands.try_recv().unwrap();
    let before = selection(&app, 2);
    assert!(!step(&mut app, Direction::Back));
    assert!(app.location_history.pending.is_none());
    assert!(app.location_history.forward.is_empty());
    app.apply_event(Event {
        id: command.id,
        generation: app.generation,
        connected: true,
        result: Ok(cedar_protocol::Payload::File {
            path: "late.txt".into(),
            text: "late".into(),
            revision: "r".into(),
        }),
    });
    assert_eq!(app.active_document, Some(2));
    assert!(same_selection(selection(&app, 2), before));
    assert!(app.location_history.back.is_empty());
    assert!(app.location_history.forward.is_empty());
    assert!(app.documents.iter().any(|doc| doc.path == "late.txt"));
}

#[test]
fn native_held_bracket_repeat_never_performs_a_second_traversal() {
    let mut app = app();
    app.activate_history_tab(2);
    app.activate_history_tab(3);
    let pressed = egui::Event::Key {
        key: egui::Key::OpenBracket,
        physical_key: Some(egui::Key::OpenBracket),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    };
    frame(&mut app, vec![pressed.clone()]);
    assert_eq!(app.active_document, Some(2));
    let before = signature(&app.location_history.back);
    // Pinned egui rewrites repeat from its held-key set, even in raw.events.
    // A second pressed event with no release exercises that native contract.
    frame(&mut app, vec![pressed]);
    assert_eq!(app.active_document, Some(2));
    assert_eq!(signature(&app.location_history.back), before);
    frame(
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::OpenBracket,
            physical_key: Some(egui::Key::OpenBracket),
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }],
    );
    frame(
        &mut app,
        key(egui::Key::OpenBracket, egui::Modifiers::COMMAND),
    );
    assert_eq!(app.active_document, Some(1));
}

#[test]
fn saturated_versions_and_same_sum_document_replacement_cancel_held_buttons() {
    for mutation in 0..3 {
        let mut app = app();
        app.activate_history_tab(2);
        app.activate_history_tab(3);
        if mutation == 0 {
            app.documents[2].edit_version = u64::MAX;
        }
        if mutation == 2 {
            let mut unrelated =
                Document::new(4, "unrecorded.txt".into(), "unrecorded".into(), "r".into());
            unrelated.edit_version = 1;
            app.documents.push(unrelated);
        }
        frame(&mut app, vec![]);
        let at = crate::workspace_access_tests::recorded_rect(&app, "Back").center();
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        match mutation {
            0 => app.documents[2].text.push('!'),
            1 => app.documents[2].edit_version = u64::MAX,
            _ => {
                app.documents[2].edit_version += 1;
                app.remove_tab(4);
            }
        }
        let before = signature(&app.location_history.back);
        frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(app.active_document, Some(3));
        assert_eq!(signature(&app.location_history.back), before);
    }
}

#[test]
fn local_history_traverses_disconnected_without_emitting_operations() {
    let mut app = app();
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.activate_history_tab(2);
    app.disconnected("offline".into());
    frame(
        &mut app,
        key(egui::Key::OpenBracket, egui::Modifiers::COMMAND),
    );
    assert_eq!(app.active_document, Some(1));
    frame(
        &mut app,
        key(egui::Key::CloseBracket, egui::Modifiers::COMMAND),
    );
    assert_eq!(app.active_document, Some(2));
    assert!(commands.try_recv().is_err());
}

#[test]
fn typing_query_cursor_undo_and_save_ack_only_explicit_find_movement_admits_navigation() {
    let mut app = app();
    frame(&mut app, vec![]);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
    frame(&mut app, key(egui::Key::ArrowRight, egui::Modifiers::NONE));
    frame(&mut app, vec![egui::Event::Text("typed".into())]);
    frame(&mut app, key(egui::Key::Z, egui::Modifiers::COMMAND));
    frame(&mut app, key(egui::Key::F, egui::Modifiers::COMMAND));
    frame(&mut app, vec![egui::Event::Text("second".into())]);
    assert!(app.location_history.back.is_empty());
    let departure = selection(&app, 1);
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert_eq!(app.location_history.back.len(), 1);
    assert!(same_selection(
        app.location_history.back[0].selection,
        departure
    ));
    let history = signature(&app.location_history.back);
    let text = app.documents[0].text.clone();
    app.documents[0].acknowledge_save(text, "saved".into());
    assert_eq!(signature(&app.location_history.back), history);
    assert!(app.location_history.forward.is_empty());
    assert!(app.location_history.pending.is_none());
}

#[test]
fn selected_search_result_admits_after_its_line_selection() {
    let mut app = app();
    settle_system_fonts(&mut app);
    app.tools_open = true;
    app.tool = crate::Tool::Search;
    // Admit a real submitted search batch before exercising its native row.
    let form = crate::ConnectForm {
        local_root: "/workspace".into(),
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.root = "/workspace".into();
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.search_query = "second".into();
    app.search();
    let search = commands.try_recv().unwrap();
    app.apply_event(Event {
        generation: app.generation,
        id: search.id,
        connected: true,
        result: Ok(cedar_protocol::Payload::Matches {
            matches: vec![cedar_protocol::SearchMatch {
                path: "2.txt".into(),
                line: 2,
                text: "second".into(),
            }],
            truncated: false,
        }),
    });
    // Admit the staged batch, then observe the automatic panel until the real
    // row allocation and clip agree across consecutive frames. A fixed three
    // frames can still be in the middle of content-sized panel settlement.
    let mut previous_geometry = None;
    let mut stable_frames = 0;
    for index in 0..16 {
        frame(&mut app, vec![]);
        let panel =
            egui::containers::panel::PanelState::load(&app.editor_ctx, egui::Id::new("tools"))
                .map(|state| state.rect);
        let row = app.search_state.accepted.as_ref().and_then(|source| {
            app.editor_ctx
                .read_response(crate::search_keyboard::row_id(source, 0))
        });
        let row_geometry = row.map(|_| {
            (
                crate::workspace_access_tests::recorded_rect(&app, "search_row:0"),
                crate::workspace_access_tests::recorded_clip(&app, "search_row:0"),
            )
        });
        let scroll = app.editor_ctx.data(|data| {
            data.get_temp::<(egui::Id, egui::Rect, egui::Vec2)>(egui::Id::new(
                "bottom_results_scroll_probe",
            ))
        });
        eprintln!(
            "surface=Bottom case=selected_search_result_admits frame={index} body_font={:?} button_font={:?} window={:?} pass={} focus={:?} preference={:?} panel={panel:?} row_rect_clip={row_geometry:?} query_rect={:?} scope_rect={:?} masks_rect={:?} scroll={scroll:?}",
            egui::TextStyle::Body.resolve(&app.editor_ctx.style()),
            egui::TextStyle::Button.resolve(&app.editor_ctx.style()),
            app.editor_ctx.screen_rect(),
            app.editor_ctx.cumulative_pass_nr(),
            app.editor_ctx.memory(|memory| memory.focused()),
            app.tools_height,
            crate::workspace_access_tests::recorded_rect(&app, "search_query"),
            crate::workspace_access_tests::recorded_rect(&app, "search_scope"),
            crate::workspace_access_tests::recorded_rect(&app, "search_masks"),
        );
        let geometry = (panel, row_geometry);
        if row_geometry.is_some() && previous_geometry == Some(geometry) {
            stable_frames += 1;
        } else {
            stable_frames = 0;
        }
        previous_geometry = Some(geometry);
        if stable_frames == 2 {
            break;
        }
    }
    assert_eq!(
        stable_frames, 2,
        "the native automatic results panel did not settle before pointer input"
    );
    let response = crate::workspace_access_tests::recorded_response(&app, "search_row:0");
    // After end_pass, egui read_response can prefer the preceding pass's rect.
    // Pair the current recorded allocation with its current recorded clip.
    let rect = crate::workspace_access_tests::recorded_rect(&app, "search_row:0");
    let clip = crate::workspace_access_tests::recorded_clip(&app, "search_row:0");
    let visible = rect.intersect(clip);
    assert!(response.enabled() && response.sense.senses_click());
    assert!(
        visible.is_finite() && visible.width() > 0.0 && visible.height() > 0.0,
        "case=selected_search_result_admits probe=search_row:0 rect={:?} clip={clip:?} pass={} focus={:?} preference={:?}",
        rect,
        app.editor_ctx.cumulative_pass_nr(),
        app.editor_ctx.memory(|memory| memory.focused()),
        app.tools_height
    );
    let at = visible.center();
    let epoch = app.navigation_epoch;
    let departure = selection(&app, 1);
    let target_selection = selection(&app, 2);
    frame(&mut app, vec![egui::Event::PointerMoved(at)]);
    assert_eq!(
        crate::workspace_access_tests::recorded_rect(&app, "search_row:0"),
        rect,
        "the hover frame moved the settled pointer target"
    );
    assert_eq!(
        crate::workspace_access_tests::recorded_clip(&app, "search_row:0"),
        clip,
        "the hover frame changed the settled pointer clip"
    );
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(
        crate::workspace_access_tests::recorded_response(&app, "search_row:0")
            .is_pointer_button_down_on()
    );
    frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(
        crate::workspace_access_tests::recorded_response(&app, "search_row:0")
            .clicked_by(egui::PointerButton::Primary)
    );
    assert_eq!(app.search_state.selected_index(), Some(0));
    assert_eq!(
        app.editor_ctx.memory(|memory| memory.focused()),
        Some(response.id)
    );
    assert_eq!(app.active_document, Some(1));
    assert!(same_selection(selection(&app, 1), departure));
    assert!(same_selection(selection(&app, 2), target_selection));
    assert_eq!(app.navigation_epoch, epoch);
    assert!(app.location_history.back.is_empty());
    assert!(app.location_history.forward.is_empty());
    assert!(app.location_history.pending.is_none());
    assert!(!app.has_search_preview());
    assert!(commands.try_recv().is_err());
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    frame(&mut app, vec![]);
    assert_eq!(app.active_document, Some(2));
    assert_eq!(selection(&app, 2).primary.index, 4);
    assert_eq!(app.location_history.back.len(), 1);
    assert!(app.location_history.pending.is_none());
}

#[test]
fn traversal_preserves_the_complete_sixteen_state_undo_redo_timeline() {
    let mut app = app();
    for value in 0..15 {
        editor_state::commit(
            &app.editor_ctx,
            &mut app.documents[0],
            format!("value {value}"),
            0,
        );
    }
    frame(&mut app, vec![]);
    for _ in 0..5 {
        frame(&mut app, key(egui::Key::Z, egui::Modifiers::COMMAND));
    }
    assert_eq!(app.documents[0].text, "value 9");
    app.activate_history_tab(2);
    for _ in 0..8 {
        frame(
            &mut app,
            key(egui::Key::OpenBracket, egui::Modifiers::COMMAND),
        );
        frame(
            &mut app,
            key(egui::Key::CloseBracket, egui::Modifiers::COMMAND),
        );
    }
    frame(
        &mut app,
        key(egui::Key::OpenBracket, egui::Modifiers::COMMAND),
    );
    for value in 10..15 {
        frame(
            &mut app,
            key(
                egui::Key::Z,
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            ),
        );
        assert_eq!(app.documents[0].text, format!("value {value}"));
    }
    for value in (0..14).rev() {
        frame(&mut app, key(egui::Key::Z, egui::Modifiers::COMMAND));
        assert_eq!(app.documents[0].text, format!("value {value}"));
    }
    frame(&mut app, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, "a😀b\nsecond\n終わり");
    assert_eq!(app.documents[0].saved_text, "a😀b\nsecond\n終わり");
}

#[test]
fn compact_editor_history_controls_tabs_copy_compare_and_find_remain_visible() {
    compact_editor_history_layout_case(None);
}

#[test]
fn compact_editor_history_missing_font_on_first_frame() {
    compact_editor_history_layout_case(Some(1));
}

#[test]
fn compact_editor_history_missing_font_on_second_frame() {
    compact_editor_history_layout_case(Some(2));
}

#[test]
fn compact_editor_history_missing_font_on_click_frame() {
    compact_editor_history_layout_case(Some(3));
}

#[test]
fn compact_editor_history_missing_font_on_menu_inspection_frame() {
    compact_editor_history_layout_case(Some(4));
}

#[test]
fn compact_editor_history_missing_font_on_escape_frame() {
    compact_editor_history_layout_case(Some(5));
}

#[test]
fn compact_editor_history_missing_font_on_find_frame() {
    compact_editor_history_layout_case(Some(6));
}

fn compact_editor_history_layout_case(failure_frame: Option<usize>) {
    for (size, sidebar_width) in COMPACT_HISTORY_SIZES {
        let mut app = compact_history_app(size, sidebar_width);
        let mut deliver_failure = failure_frame.map(|_| {
            let (fonts, deliver) = crate::system_fonts::SystemFonts::pending_failure_for_test();
            app.system_fonts = fonts;
            deliver
        });
        let mut frame_number = 0;
        let mut render = |app: &mut CedarApp, events| {
            frame_number += 1;
            if failure_frame == Some(frame_number) {
                deliver_failure
                    .take()
                    .expect("deliver the controlled font result exactly once")();
            }
            compact_history_frame(app, size, events)
        };
        // Preserve the original two-frame boundary and atomic native click.
        // In particular, the second-frame error must pass without settling.
        render(&mut app, vec![]);
        render(&mut app, vec![]);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        for name in [
            "Back",
            "Forward",
            "editor_tab:1",
            "editor_tab:2",
            "editor_tab:3",
            "editor_overflow",
        ] {
            compact_visible(&app, size, name);
        }
        let back = crate::workspace_access_tests::recorded_response(&app, "Back");
        let forward = crate::workspace_access_tests::recorded_response(&app, "Forward");
        assert!(back.rect.right() < forward.rect.left());
        assert!(screen.contains_rect(back.rect) && screen.contains_rect(forward.rect));
        compact_assert_scroll_lane(&app);
        let at = compact_visible(&app, size, "editor_overflow").center();
        render(
            &mut app,
            vec![
                egui::Event::PointerMoved(at),
                compact_pointer(at, true),
                compact_pointer(at, false),
            ],
        );
        let output = render(&mut app, vec![]);
        compact_assert_menu(&app, &output, size, sidebar_width);
        render(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
        app.find_open = true;
        let output = render(&mut app, vec![]);
        assert!(screen.contains(label(&app, &output, "FIND", size, sidebar_width)));
        compact_visible(&app, size, "find_label");
        let editor = crate::workspace_access_tests::recorded_response(&app, "editor");
        assert!(editor.rect.min.y < screen.max.y - 29.0);

        // Also exercise native Find close/open and error disappearance. These
        // run after the unchanged regression boundary above, never as warmup.
        compact_click(&mut app, size, "find_close");
        assert!(!app.find_open);
        // Close requests deferred editor focus. Establish it explicitly with
        // native input before the next shortcut, rather than bypassing the
        // existing restore-focus fence or claiming immediate automatic focus.
        compact_focus_editor(&mut app, size);
        let output =
            compact_history_frame(&mut app, size, key(egui::Key::F, egui::Modifiers::COMMAND));
        assert!(app.find_open);
        assert!(screen.contains(label(&app, &output, "FIND", size, sidebar_width)));
        compact_click(&mut app, size, "find_close");
        assert!(!app.find_open);
        compact_assert_scroll_lane(&app);
        if failure_frame.is_some() {
            assert!(
                app.error.is_some(),
                "the real font error must remain visible until dismissed"
            );
            compact_visible(&app, size, "error_status");
            let before = compact_visible(&app, size, "editor_overflow");
            compact_click(&mut app, size, "error_dismiss");
            assert!(app.error.is_none());
            compact_history_frame(&mut app, size, vec![]);
            let after = compact_visible(&app, size, "editor_overflow");
            assert!(
                after.top() < before.top(),
                "dismiss must move the real toolbar upward"
            );
            // The first completed frame without the banner supplies the new
            // target; do not add a convergence frame before clicking it.
            compact_click(&mut app, size, "editor_overflow");
            let output = compact_history_frame(&mut app, size, vec![]);
            compact_assert_menu(&app, &output, size, sidebar_width);
        }
        assert!(app
            .documents
            .iter()
            .all(|doc| doc.text == "a😀b\nsecond\n終わり" && doc.saved_text == doc.text));
    }
}

#[test]
fn compact_editor_error_first_frame_and_dismiss_are_visible_at_both_fonts() {
    for (size, sidebar_width) in COMPACT_HISTORY_SIZES {
        for font in [13.0, 18.0] {
            let mut app = compact_history_app(size, sidebar_width);
            app.editor_ctx.style_mut(|style| {
                for text_style in [egui::TextStyle::Body, egui::TextStyle::Button] {
                    style
                        .text_styles
                        .insert(text_style, egui::FontId::proportional(font));
                }
            });
            let (fonts, deliver_failure) =
                crate::system_fonts::SystemFonts::pending_failure_for_test();
            app.system_fonts = fonts;
            compact_history_frame(&mut app, size, vec![]);
            let before = compact_visible(&app, size, "editor_overflow");
            deliver_failure();
            let first_output = compact_history_frame(&mut app, size, vec![]);
            let error = app
                .error
                .as_ref()
                .expect("the controlled receiver must show the real font error");
            assert!(painted_texts(&first_output)
                .iter()
                .any(|text| text == error));
            // These are the FIRST error-frame rectangles. No convergence or
            // wider viewport is allowed to rescue clipped notification text.
            let first = (
                compact_visible(&app, size, "error_status"),
                compact_visible(&app, size, "error_dismiss"),
                compact_visible(&app, size, "editor_overflow"),
            );
            assert!(first.2.top() > before.top());
            compact_assert_scroll_lane(&app);
            compact_history_frame(&mut app, size, vec![]);
            let second = (
                compact_visible(&app, size, "error_status"),
                compact_visible(&app, size, "error_dismiss"),
                compact_visible(&app, size, "editor_overflow"),
            );
            assert_eq!(
                first,
                second,
                "error geometry drifted after first paint at font={font}, size={size:?}; {}",
                compact_history_context(&app)
            );
            compact_click(&mut app, size, "error_dismiss");
            assert!(
                app.error.is_none(),
                "native Dismiss must remove the real error"
            );
            compact_history_frame(&mut app, size, vec![]);
            let disappeared = compact_visible(&app, size, "editor_overflow");
            assert_eq!(
                disappeared, before,
                "first frame without error must restore the toolbar"
            );
            compact_history_frame(&mut app, size, vec![]);
            assert_eq!(
                compact_visible(&app, size, "editor_overflow"),
                disappeared,
                "toolbar drifted after error disappearance"
            );
            assert!(app
                .documents
                .iter()
                .all(|doc| doc.text == "a😀b\nsecond\n終わり" && doc.saved_text == doc.text));
        }
    }
}

#[test]
fn compact_editor_error_single_line_dismiss_wrap_boundary_is_visible_on_first_frame() {
    const ERROR: &str = "Notification layout boundary";

    fn notification_frame(
        app: &mut CedarApp,
        size: egui::Vec2,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let ctx = app.editor_ctx.clone();
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        eframe::App::raw_input_hook(app, &ctx, &mut input);
        ctx.run(input, |ctx| {
            crate::workspace_access_tests::begin(ctx);
            app.notifications(ctx);
        })
    }

    for font in [13.0, 18.0] {
        // A separate font-only context locates the two sides of the native
        // wrap boundary. It never renders or warms the tested error panel.
        let measure = egui::Context::default();
        measure.set_fonts(crate::system_fonts::base_fonts());
        measure.style_mut(|style| {
            for text_style in [egui::TextStyle::Body, egui::TextStyle::Button] {
                style
                    .text_styles
                    .insert(text_style, egui::FontId::proportional(font));
            }
        });
        let mut threshold = 0.0;
        let mut single_row_height = 0.0;
        let _ = measure.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1.0, 1.0),
                )),
                ..Default::default()
            },
            |ctx| {
                let style = ctx.style();
                ctx.fonts(|fonts| {
                    let body = egui::TextStyle::Body.resolve(&style);
                    let label =
                        fonts.layout_no_wrap(ERROR.into(), body.clone(), egui::Color32::RED);
                    let dismiss =
                        fonts.layout_no_wrap("Dismiss".into(), body, egui::Color32::WHITE);
                    single_row_height = label.size().y.max(style.spacing.interact_size.y);
                    threshold = 20.0
                        + label.size().x
                        + style.spacing.item_spacing.x
                        + dismiss.size().x
                        + if style.visuals.button_frame {
                            2.0 * style.spacing.button_padding.x
                        } else {
                            0.0
                        };
                });
            },
        );
        for (width_delta, wraps) in [(-2.0, true), (2.0, false)] {
            let mut app = app();
            app.editor_ctx.set_style((*measure.style()).clone());
            app.error = Some(ERROR.into());
            let size = egui::vec2(threshold + width_delta, 240.0);
            let output = notification_frame(&mut app, size, vec![]);
            assert!(painted_texts(&output).iter().any(|text| text == ERROR));
            let first = (
                compact_visible(&app, size, "error_status"),
                compact_visible(&app, size, "error_dismiss"),
            );
            assert!(
                (first.0.height() - single_row_height).abs() < 1.0,
                "boundary fixture must keep the message on one row: font={font}, rect={:?}",
                first.0
            );
            assert_eq!(
                first.1.top() >= first.0.bottom(),
                wraps,
                "wrong side of Dismiss wrap boundary: font={font}, size={size:?}, rects={first:?}"
            );
            notification_frame(&mut app, size, vec![]);
            assert_eq!(
                first,
                (
                    compact_visible(&app, size, "error_status"),
                    compact_visible(&app, size, "error_dismiss"),
                ),
                "first-paint boundary geometry must not drift: font={font}, size={size:?}"
            );
            let at = first.1.center();
            notification_frame(
                &mut app,
                size,
                vec![
                    egui::Event::PointerMoved(at),
                    compact_pointer(at, true),
                    compact_pointer(at, false),
                ],
            );
            assert!(
                app.error.is_none(),
                "the real Dismiss must remain clickable"
            );
        }
    }
}

fn compact_visible_editor(app: &CedarApp, size: egui::Vec2) -> egui::Rect {
    let rect = crate::workspace_access_tests::recorded_rect(app, "editor")
        .intersect(crate::workspace_access_tests::recorded_clip(app, "editor"))
        .intersect(egui::Rect::from_min_size(egui::Pos2::ZERO, size));
    assert!(rect.is_positive(), "editor has no visible native target");
    rect
}

fn compact_focus_editor(app: &mut CedarApp, size: egui::Vec2) {
    let at = compact_visible_editor(app, size).left_top() + egui::vec2(18.0, 8.0);
    compact_history_frame(
        app,
        size,
        vec![
            egui::Event::PointerMoved(at),
            compact_pointer(at, true),
            compact_pointer(at, false),
        ],
    );
    let editor = crate::workspace_access_tests::recorded_response(app, "editor").id;
    assert!(
        app.editor_ctx.memory(|memory| memory.has_focus(editor)),
        "native editor click must own focus; {}",
        compact_history_context(app)
    );
}

fn compact_go_to_line(app: &mut CedarApp, size: egui::Vec2, line: usize) {
    compact_history_frame(app, size, key(egui::Key::G, egui::Modifiers::COMMAND));
    compact_history_frame(app, size, vec![]);
    assert!(app.navigation.dialog_open());
    compact_history_frame(app, size, key(egui::Key::A, egui::Modifiers::COMMAND));
    compact_history_frame(app, size, vec![egui::Event::Text(line.to_string())]);
    compact_history_frame(app, size, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(!app.navigation.dialog_open());
    // Enter queues the line jump, but closing the dialog deliberately keeps
    // the editor blocked until egui releases its previous modal layer. Wait
    // for that actual navigation transaction, not a guessed cursor delay.
    for _ in 0..4 {
        if app.active().unwrap().jump_to.is_none()
            && app.location_history.pending.is_none()
            && !app.navigation.blocks_editor()
            && !app.navigation.restore_focus
        {
            break;
        }
        compact_history_frame(app, size, vec![]);
    }
    assert!(
        app.active().unwrap().jump_to.is_none()
            && app.location_history.pending.is_none()
            && !app.navigation.blocks_editor()
            && !app.navigation.restore_focus,
        "native Go-to did not finish after modal release; {}",
        compact_history_context(app)
    );
    assert_eq!(app.active().unwrap().cursor.0, line);
}

fn compact_caret_visible(app: &mut CedarApp, size: egui::Vec2) {
    // Wait only for egui's own scroll animation to put the real caret inside
    // its current viewport; never enlarge the window or alter the offset.
    for _ in 0..60 {
        compact_history_frame(app, size, vec![]);
        let caret = app.editor_ctx.data(|data| {
            data.get_temp::<egui::Rect>(egui::Id::new("compact_history_editor_caret"))
                .expect("production TextEdit must report its caret")
        });
        if editor_scroll_probe(app).1.contains_rect(caret) {
            return;
        }
    }
    panic!(
        "native navigation did not reveal the caret; {}",
        compact_history_context(app)
    );
}

#[test]
fn compact_editor_scroll_selection_and_undo_survive_banner_and_find_transitions() {
    for (size, sidebar_width) in COMPACT_HISTORY_SIZES {
        let mut app = compact_history_app(size, sidebar_width);
        let (fonts, deliver_failure) = crate::system_fonts::SystemFonts::pending_failure_for_test();
        app.system_fonts = fonts;
        compact_history_frame(&mut app, size, vec![]);
        deliver_failure();
        compact_history_frame(&mut app, size, vec![]);
        assert!(app.error.is_some());
        compact_visible(&app, size, "error_status");
        compact_assert_scroll_lane(&app);
        compact_focus_editor(&mut app, size);
        let saved = app.active().unwrap().saved_text.clone();
        let revision = app.active().unwrap().revision.clone();
        let draft = (0..180)
            .map(|line| format!("line {line:03} a😀b second 終わり\n"))
            .collect::<String>();
        compact_history_frame(&mut app, size, key(egui::Key::A, egui::Modifiers::COMMAND));
        compact_history_frame(&mut app, size, vec![egui::Event::Text(draft.clone())]);
        assert_eq!(app.active().unwrap().text, draft);
        compact_go_to_line(&mut app, size, 1);
        compact_caret_visible(&mut app, size);
        let history = signature(&app.location_history.back);

        // Native TextEdit capture must continue when the pointer leaves the
        // clipped editor viewport; clipping a background must not clip capture.
        let visible = compact_visible_editor(&app, size);
        let start = visible.left_top() + egui::vec2(18.0, 8.0);
        let inside = start + egui::vec2(90.0, 24.0);
        compact_history_frame(
            &mut app,
            size,
            vec![
                egui::Event::PointerMoved(start),
                compact_pointer(start, true),
            ],
        );
        compact_history_frame(&mut app, size, vec![egui::Event::PointerMoved(inside)]);
        let inside_selection = selection(&app, 2);
        assert_ne!(
            inside_selection.primary.index,
            inside_selection.secondary.index
        );
        let outside = egui::pos2(inside.x, visible.bottom() + 12.0);
        compact_history_frame(&mut app, size, vec![egui::Event::PointerMoved(outside)]);
        let outside_selection = selection(&app, 2);
        assert!(
            outside_selection.primary.index > inside_selection.primary.index,
            "TextEdit lost native selection capture outside its clip; {}",
            compact_history_context(&app)
        );
        compact_history_frame(&mut app, size, vec![compact_pointer(outside, false)]);
        let selected = selection(&app, 2);

        let before_wheel = editor_scroll_probe(&app).2.y;
        let at = compact_visible_editor(&app, size).center();
        compact_history_frame(
            &mut app,
            size,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -120.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(
            editor_scroll_probe(&app).2.y > before_wheel,
            "native wheel must still scroll the editor; {}",
            compact_history_context(&app)
        );
        assert!(same_selection(selection(&app, 2), selected));

        // Use the production ScrollArea's actual vertical scrollbar response.
        let probe = editor_scroll_probe(&app);
        let bar = probe
            .6
            .expect("long draft must render a vertical scrollbar");
        assert_eq!(bar.id, probe.0.with(1usize));
        assert!(bar.interact_rect.is_positive() && probe.4.contains_rect(bar.interact_rect));
        let at = egui::pos2(
            bar.interact_rect.center().x,
            bar.interact_rect.top() + bar.interact_rect.height() * 0.35,
        );
        let before_bar = probe.2.y;
        compact_history_frame(
            &mut app,
            size,
            vec![egui::Event::PointerMoved(at), compact_pointer(at, true)],
        );
        compact_history_frame(&mut app, size, vec![compact_pointer(at, false)]);
        assert!(
            (editor_scroll_probe(&app).2.y - before_bar).abs() > 1.0,
            "native scrollbar must change scroll offset; {}",
            compact_history_context(&app)
        );
        assert!(same_selection(selection(&app, 2), selected));

        // The fix must preserve background drag-to-scroll in the current lane.
        // The gutter is outside TextEdit, so this gesture belongs to ScrollArea.
        let probe = editor_scroll_probe(&app);
        let drag = probe
            .5
            .expect("Find closed must retain editor background dragging");
        assert!(drag.senses_drag);
        compact_assert_scroll_lane(&app);
        let start = egui::pos2(probe.1.left() + 2.0, probe.1.center().y);
        let end = start - egui::vec2(0.0, 55.0);
        let before_drag = probe.2.y;
        compact_history_frame(
            &mut app,
            size,
            vec![
                egui::Event::PointerMoved(start),
                compact_pointer(start, true),
            ],
        );
        compact_history_frame(&mut app, size, vec![egui::Event::PointerMoved(end)]);
        assert!(
            editor_scroll_probe(&app)
                .5
                .as_ref()
                .is_some_and(|response| response.dragged),
            "the real background response must own the drag; {}",
            compact_history_context(&app)
        );
        assert!(
            editor_scroll_probe(&app).2.y > before_drag,
            "background dragging must still pan content; {}",
            compact_history_context(&app)
        );
        compact_history_frame(&mut app, size, vec![compact_pointer(end, false)]);
        assert!(same_selection(selection(&app, 2), selected));
        assert_eq!(signature(&app.location_history.back), history);

        compact_history_frame(&mut app, size, key(egui::Key::F, egui::Modifiers::COMMAND));
        assert!(app.find_open);
        compact_visible(&app, size, "find_label");
        assert!(
            editor_scroll_probe(&app).5.is_none(),
            "Find keeps its existing background-drag exclusion"
        );
        compact_click(&mut app, size, "find_close");
        assert!(!app.find_open);
        compact_history_frame(&mut app, size, vec![]);
        assert!(editor_scroll_probe(&app).5.is_some());
        compact_assert_scroll_lane(&app);
        compact_click(&mut app, size, "error_dismiss");
        assert!(app.error.is_none());
        compact_history_frame(&mut app, size, vec![]);
        compact_click(&mut app, size, "editor_overflow");
        let output = compact_history_frame(&mut app, size, vec![]);
        compact_assert_menu(&app, &output, size, sidebar_width);
        compact_history_frame(
            &mut app,
            size,
            key(egui::Key::Escape, egui::Modifiers::NONE),
        );
        assert_eq!(app.active().unwrap().text, draft);
        assert_eq!(app.active().unwrap().saved_text, saved);
        assert_eq!(app.active().unwrap().revision, revision);

        // Real Go-to and Back/Forward must still scroll a distant caret into
        // view after the lane has expanded, without a pixel-exact offset claim.
        compact_focus_editor(&mut app, size);
        compact_go_to_line(&mut app, size, 1);
        compact_caret_visible(&mut app, size);
        compact_go_to_line(&mut app, size, 160);
        compact_caret_visible(&mut app, size);
        compact_click(&mut app, size, "Back");
        assert_eq!(app.active().unwrap().cursor.0, 1);
        compact_caret_visible(&mut app, size);
        compact_click(&mut app, size, "Forward");
        assert_eq!(app.active().unwrap().cursor.0, 160);
        compact_caret_visible(&mut app, size);

        compact_history_frame(&mut app, size, key(egui::Key::Z, egui::Modifiers::COMMAND));
        assert_eq!(
            app.active().unwrap().text,
            saved,
            "scroll/selection/Find/history must preserve native Undo"
        );
        compact_history_frame(
            &mut app,
            size,
            key(
                egui::Key::Z,
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            ),
        );
        assert_eq!(
            app.active().unwrap().text,
            draft,
            "native Redo must restore the unsaved draft"
        );
        assert_eq!(app.active().unwrap().saved_text, saved);
        assert_eq!(app.active().unwrap().revision, revision);
    }
}

#[test]
fn saturated_destination_reports_unavailable_and_preserves_both_stacks() {
    let mut app = app();
    app.activate_history_tab(2);
    app.activate_history_tab(3);
    assert!(step(&mut app, Direction::Back));
    let before_back = signature(&app.location_history.back);
    let before_forward = signature(&app.location_history.forward);
    app.documents[0].edit_version = u64::MAX;
    app.open("1.txt".into(), None);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(signature(&app.location_history.back), before_back);
    assert_eq!(signature(&app.location_history.forward), before_forward);
    assert!(app
        .location_history
        .message
        .as_deref()
        .unwrap()
        .contains("saturated"));
    assert!(app.location_history.pending.is_none());
    app.open("2.txt".into(), None);
    assert_eq!(signature(&app.location_history.back), before_back);
    assert!(app.location_history.forward.is_empty());
}

fn queue_file(app: &CedarApp, request: u64, path: &str) {
    app.result_tx
        .send(crate::worker::WorkerEvent::Response(Event {
            id: request,
            generation: app.generation,
            connected: true,
            result: Ok(cedar_protocol::Payload::File {
                path: path.into(),
                text: "received".into(),
                revision: "r".into(),
            }),
        }))
        .unwrap();
}

#[test]
fn ready_read_yields_to_same_frame_tab_chooser_text_and_modal_without_unseen_history() {
    for intent in 0..4 {
        let mut app = app();
        frame(&mut app, vec![]);
        let original = selection(&app, 1);
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
        let (worker, commands) = Worker::recording();
        app.worker = Some(worker);
        app.open("late.txt".into(), None);
        let read = commands.try_recv().unwrap();
        queue_file(&app, read.id, "late.txt");
        let events = match intent {
            0 => {
                let at =
                    crate::workspace_access_tests::recorded_rect(&app, "editor_tab:3").center();
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            }
            1 => key(egui::Key::P, egui::Modifiers::COMMAND),
            2 => vec![egui::Event::Text("newer".into())],
            _ => {
                app.confirm = Some(crate::Confirm::CloseTab(1));
                vec![]
            }
        };
        frame(&mut app, events);
        assert!(app.documents.iter().any(|doc| doc.path == "late.txt"));
        assert_eq!(app.active_document, Some(if intent == 0 { 3 } else { 1 }));
        assert_eq!(app.location_history.back.len(), usize::from(intent == 0));
        if intent == 0 {
            assert!(same_selection(
                app.location_history.back[0].selection,
                original
            ));
        }
        assert!(app.location_history.pending.is_none());
        if intent == 2 {
            assert_eq!(app.documents[0].edit_version, 1);
        }
    }
}

#[test]
fn ready_read_hover_key_release_and_earlier_caret_motion_do_not_cancel_completion() {
    for events in [
        vec![],
        vec![egui::Event::PointerMoved(egui::pos2(400.0, 450.0))],
        vec![egui::Event::Key {
            key: egui::Key::OpenBracket,
            physical_key: Some(egui::Key::OpenBracket),
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }],
    ] {
        let mut app = app();
        frame(&mut app, vec![]);
        let original = selection(&app, 1);
        let (worker, commands) = Worker::recording();
        app.worker = Some(worker);
        app.open("late.txt".into(), None);
        let read = commands.try_recv().unwrap();
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
        frame(&mut app, key(egui::Key::ArrowRight, egui::Modifiers::NONE));
        assert!(app.location_history.pending.is_some());
        queue_file(&app, read.id, "late.txt");
        frame(&mut app, events);
        assert_eq!(app.active().unwrap().path, "late.txt");
        assert_eq!(app.location_history.back.len(), 1);
        assert!(same_selection(
            app.location_history.back[0].selection,
            original
        ));
    }
}

#[test]
fn ready_navigation_guard_keeps_response_before_eof_and_ignores_multiple_stale_replies() {
    for eof in [false, true] {
        let mut app = app();
        frame(&mut app, vec![]);
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
        let (worker, commands) = Worker::recording();
        app.worker = Some(worker);
        app.open("older.txt".into(), None);
        let older = commands.try_recv().unwrap();
        app.open("current.txt".into(), None);
        let current = commands.try_recv().unwrap();
        queue_file(&app, current.id, "current.txt");
        queue_file(&app, older.id, "older.txt");
        if eof {
            app.result_tx
                .send(crate::worker::WorkerEvent::TransportLost {
                    generation: app.generation,
                    message: "EOF after completed Reads".into(),
                })
                .unwrap();
        }
        frame(&mut app, vec![egui::Event::Paste("newer".into())]);
        assert_eq!(app.active_document, Some(1));
        assert!(app.documents.iter().any(|doc| doc.path == "current.txt"));
        assert!(app.documents.iter().any(|doc| doc.path == "older.txt"));
        assert!(app.location_history.back.is_empty());
        assert!(app.location_history.forward.is_empty());
        assert!(app.location_history.pending.is_none());
        assert_eq!(app.documents[0].edit_version, 1);
        assert_eq!(app.state == ConnectionState::Disconnected, eof);
    }
}

#[test]
fn ready_read_ime_input_remains_with_source_and_key_release_only_stays_inert() {
    let mut app = app();
    frame(&mut app, vec![]);
    set_selection(&mut app, 1, range(0, 0, false, false));
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.open("late.txt".into(), None);
    let read = commands.try_recv().unwrap();
    queue_file(&app, read.id, "late.txt");
    frame(
        &mut app,
        vec![
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("入力".into())),
        ],
    );
    assert_eq!(app.active_document, Some(1));
    assert!(app.location_history.back.is_empty());
    assert!(app.location_history.pending.is_none());
    assert!(app.documents.iter().any(|doc| doc.path == "late.txt"));
}

#[test]
fn ready_read_on_a_captured_tab_release_uses_visible_departure_and_orphan_release_does_not_cancel()
{
    for captured in [false, true] {
        let mut app = app();
        frame(&mut app, vec![]);
        let at = crate::workspace_access_tests::recorded_rect(&app, "editor_tab:3").center();
        let original = selection(&app, 1);
        let (worker, commands) = Worker::recording();
        app.worker = Some(worker);
        app.open("late.txt".into(), None);
        let read = commands.try_recv().unwrap();
        if captured {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            assert!(app.location_history.pending.is_some());
        }
        queue_file(&app, read.id, "late.txt");
        frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(
            app.active().unwrap().path,
            if captured { "3.txt" } else { "late.txt" }
        );
        assert_eq!(app.location_history.back.len(), 1);
        assert!(same_selection(
            app.location_history.back[0].selection,
            original
        ));
    }
}

#[test]
fn ready_read_disabled_control_release_and_empty_ime_do_not_cancel() {
    for disabled in [false, true] {
        let mut app = app();
        frame(&mut app, vec![]);
        let at = crate::workspace_access_tests::recorded_rect(&app, "Back").center();
        let (worker, commands) = Worker::recording();
        app.worker = Some(worker);
        app.open("late.txt".into(), None);
        let read = commands.try_recv().unwrap();
        if disabled {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        queue_file(&app, read.id, "late.txt");
        frame(
            &mut app,
            if disabled {
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }]
            } else {
                vec![
                    egui::Event::Ime(egui::ImeEvent::Enabled),
                    egui::Event::Ime(egui::ImeEvent::Preedit(String::new())),
                ]
            },
        );
        assert_eq!(app.active().unwrap().path, "late.txt");
        assert_eq!(app.location_history.back.len(), 1);
    }
}

#[test]
fn captured_tab_release_survives_panel_motion_and_supersedes_ready_read() {
    let mut app = app();
    frame(&mut app, vec![]);
    let at = crate::workspace_access_tests::recorded_rect(&app, "editor_tab:3").center();
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.open("late.txt".into(), None);
    let read = commands.try_recv().unwrap();
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let form = crate::ConnectForm {
        local_root: "/workspace".into(),
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/workspace".into();
    queue_file(&app, read.id, "late.txt");
    frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(
        (crate::workspace_access_tests::recorded_rect(&app, "editor_tab:3")
            .center()
            .x
            - at.x)
            .abs()
            > 100.0
    );
    assert_eq!(app.active_document, Some(3));
    assert_eq!(app.location_history.back.len(), 1);
    assert_eq!(app.location_history.back[0].document, 1);
}

#[test]
fn ready_reply_counts_touch_start_and_nonempty_ime_content_only() {
    let cases = vec![
        (egui::Event::Ime(egui::ImeEvent::Enabled), false),
        (egui::Event::Ime(egui::ImeEvent::Disabled), false),
        (
            egui::Event::Ime(egui::ImeEvent::Preedit(String::new())),
            false,
        ),
        (
            egui::Event::Ime(egui::ImeEvent::Commit(String::new())),
            false,
        ),
        (
            egui::Event::Ime(egui::ImeEvent::Preedit("入力".into())),
            true,
        ),
        (
            egui::Event::Ime(egui::ImeEvent::Commit("入力".into())),
            true,
        ),
        (
            egui::Event::Touch {
                device_id: egui::TouchDeviceId(1),
                id: egui::TouchId(1),
                phase: egui::TouchPhase::Start,
                pos: egui::pos2(400.0, 400.0),
                force: None,
            },
            true,
        ),
        (
            egui::Event::Touch {
                device_id: egui::TouchDeviceId(1),
                id: egui::TouchId(1),
                phase: egui::TouchPhase::Move,
                pos: egui::pos2(400.0, 400.0),
                force: None,
            },
            false,
        ),
        (
            egui::Event::Touch {
                device_id: egui::TouchDeviceId(1),
                id: egui::TouchId(1),
                phase: egui::TouchPhase::End,
                pos: egui::pos2(400.0, 400.0),
                force: None,
            },
            false,
        ),
    ];
    for (event, supersedes) in cases {
        let mut app = app();
        frame(&mut app, vec![]);
        let (worker, commands) = Worker::recording();
        app.worker = Some(worker);
        app.open("late.txt".into(), None);
        let read = commands.try_recv().unwrap();
        queue_file(&app, read.id, "late.txt");
        frame(&mut app, vec![event]);
        assert_eq!(
            app.active().unwrap().path,
            if supersedes { "1.txt" } else { "late.txt" }
        );
        assert_eq!(app.location_history.back.len(), usize::from(!supersedes));
    }
}

#[test]
fn ready_read_preserves_only_the_current_owned_back_or_forward_release() {
    for forward in [false, true] {
        let mut app = app();
        app.activate_history_tab(2);
        if forward {
            assert!(step(&mut app, Direction::Back));
        }
        frame(&mut app, vec![]);
        let at = crate::workspace_access_tests::recorded_rect(
            &app,
            if forward { "Forward" } else { "Back" },
        )
        .center();
        let (worker, commands) = Worker::recording();
        app.worker = Some(worker);
        app.open("late.txt".into(), None);
        let read = commands.try_recv().unwrap();
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        queue_file(&app, read.id, "late.txt");
        frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(app.active_document, Some(if forward { 2 } else { 1 }));
        assert_eq!(app.location_history.back.len(), usize::from(forward));
        assert_eq!(app.location_history.forward.len(), usize::from(!forward));
        assert!(app.documents.iter().any(|doc| doc.path == "late.txt"));
        assert!(app.location_history.pending.is_none());
        let back = signature(&app.location_history.back);
        let forward_stack = signature(&app.location_history.forward);
        frame(&mut app, vec![]);
        assert_eq!(signature(&app.location_history.back), back);
        assert_eq!(signature(&app.location_history.forward), forward_stack);
    }
    let mut app = app();
    app.activate_history_tab(2);
    app.activate_history_tab(3);
    frame(&mut app, vec![]);
    let at = crate::workspace_access_tests::recorded_rect(&app, "Back").center();
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.open("late.txt".into(), None);
    let read = commands.try_recv().unwrap();
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    app.documents[1].edit_version += 1;
    queue_file(&app, read.id, "late.txt");
    let before = signature(&app.location_history.back);
    frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(app.active_document, Some(3));
    assert_eq!(signature(&app.location_history.back), before);
    assert!(app.location_history.forward.is_empty());
}

#[test]
fn captured_history_click_after_save_ack_and_before_eof_keeps_worker_order() {
    let mut app = app();
    app.root = "/project".into();
    let form = crate::ConnectForm {
        local_root: app.root.clone(),
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[1],
        "saved draft".into(),
        2,
    );
    app.activate_history_tab(2);
    frame(&mut app, vec![]);
    let at = crate::workspace_access_tests::recorded_rect(&app, "Back").center();
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.open("late.txt".into(), None);
    let read = commands.try_recv().unwrap();
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    app.documents[1].saving = true;
    app.next_request = 500;
    let submission =
        crate::interrupted_save::InterruptedSave::capture(&app, &app.documents[1]).unwrap();
    app.next_request += 1;
    let revision = format!("{:x}", Sha256::digest(b"saved draft"));
    app.pending.insert(
        500,
        crate::Job::Save {
            document: 2,
            snapshot: "saved draft".into(),
            submission: Some(submission),
        },
    );
    app.result_tx
        .send(crate::worker::WorkerEvent::Response(Event {
            generation: app.generation,
            id: 500,
            connected: true,
            result: Ok(cedar_protocol::Payload::Written {
                revision: revision.clone(),
            }),
        }))
        .unwrap();
    queue_file(&app, read.id, "late.txt");
    app.result_tx
        .send(crate::worker::WorkerEvent::TransportLost {
            generation: app.generation,
            message: "EOF after acknowledgements".into(),
        })
        .unwrap();
    frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents[1].saved_text, "saved draft");
    assert_eq!(
        app.documents[1].revision.as_deref(),
        Some(revision.as_str())
    );
    assert!(!app.documents[1].saving);
    assert!(app.documents[1].interrupted_save.is_none());
    assert!(app.state == ConnectionState::Disconnected);
    assert!(app.location_history.back.is_empty());
    assert_eq!(app.location_history.forward.len(), 1);
    assert!(app.documents.iter().any(|doc| doc.path == "late.txt"));
}

#[test]
fn ready_read_all_stale_history_release_is_consumed_once_without_opposite_entry() {
    let mut app = app();
    app.activate_history_tab(2);
    app.documents[0].edit_version += 1;
    frame(&mut app, vec![]);
    let at = crate::workspace_access_tests::recorded_rect(&app, "Back").center();
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.open("late.txt".into(), None);
    let read = commands.try_recv().unwrap();
    let epoch = app.navigation_epoch;
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    queue_file(&app, read.id, "late.txt");
    frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(app.active_document, Some(2));
    assert_eq!(app.navigation_epoch, epoch.wrapping_add(1));
    assert!(app.location_history.back.is_empty());
    assert!(app.location_history.forward.is_empty());
    assert_eq!(
        app.location_history.message.as_deref(),
        Some("Skipped 1 stale editor location")
    );
    frame(&mut app, vec![]);
    assert_eq!(app.navigation_epoch, epoch.wrapping_add(1));
}

#[test]
fn mixed_history_release_and_text_does_not_prehandle_or_replay_back() {
    let mut app = app();
    app.activate_history_tab(2);
    frame(&mut app, vec![]);
    let at = crate::workspace_access_tests::recorded_rect(&app, "Back").center();
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.open("late.txt".into(), None);
    let read = commands.try_recv().unwrap();
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let before = signature(&app.location_history.back);
    queue_file(&app, read.id, "late.txt");
    frame(
        &mut app,
        vec![
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::Text("newer input".into()),
        ],
    );
    assert_eq!(app.active_document, Some(2));
    assert_eq!(signature(&app.location_history.back), before);
    assert!(app.location_history.forward.is_empty());
    frame(&mut app, vec![]);
    assert_eq!(app.active_document, Some(2));
    assert_eq!(signature(&app.location_history.back), before);
}

#[test]
fn editor_focus_ime_housekeeping_preserves_owned_history_pointer_clicks() {
    // Native egui-winit toggles IME eligibility as TextEdit loses focus and
    // can send Disabled even without a preceding Enabled. These are shaped
    // like that native sequence, rather than a claim of a captured raw trace.
    for ime in [
        egui::ImeEvent::Disabled,
        egui::ImeEvent::Enabled,
        egui::ImeEvent::Preedit(String::new()),
        egui::ImeEvent::Commit(String::new()),
    ] {
        for phase in 0..3 {
            for forward in [false, true] {
                for ready in [false, true] {
                    let mut app = app();
                    app.activate_history_tab(2);
                    if forward {
                        assert!(step(&mut app, Direction::Back));
                    }
                    let editor = egui::Id::new(("editor", app.active_document.unwrap()));
                    app.editor_ctx
                        .memory_mut(|memory| memory.request_focus(editor));
                    frame(&mut app, vec![]);
                    assert!(app.editor_ctx.memory(|memory| memory.has_focus(editor)));
                    let at = crate::workspace_access_tests::recorded_rect(
                        &app,
                        if forward { "Forward" } else { "Back" },
                    )
                    .center();
                    let read = if ready {
                        let (worker, commands) = Worker::recording();
                        app.worker = Some(worker);
                        app.open("late.txt".into(), None);
                        Some(commands.try_recv().unwrap().id)
                    } else {
                        None
                    };
                    let pointer = |pressed| egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    };
                    let mut press = vec![egui::Event::PointerMoved(at), pointer(true)];
                    if phase == 0 {
                        press.push(egui::Event::Ime(ime.clone()));
                    }
                    frame(&mut app, press);
                    if phase == 1 {
                        frame(&mut app, vec![egui::Event::Ime(ime.clone())]);
                    }
                    if let Some(read) = read {
                        queue_file(&app, read, "late.txt");
                    }
                    let mut release = vec![pointer(false)];
                    if phase == 2 {
                        release.push(egui::Event::Ime(ime.clone()));
                    }
                    frame(&mut app, release);
                    assert_eq!(
                        app.active_document,
                        Some(if forward { 2 } else { 1 }),
                        "IME {ime:?}, phase {phase}, forward {forward}, ready {ready}"
                    );
                    assert_eq!(app.location_history.back.len(), usize::from(forward));
                    assert_eq!(app.location_history.forward.len(), usize::from(!forward));
                    assert!(app.editor_ctx.memory(|memory| memory
                        .has_focus(egui::Id::new(("editor", app.active_document.unwrap())))));
                    let back = signature(&app.location_history.back);
                    let forward_stack = signature(&app.location_history.forward);
                    frame(&mut app, vec![]);
                    assert_eq!(signature(&app.location_history.back), back);
                    assert_eq!(signature(&app.location_history.forward), forward_stack);
                    if ready {
                        assert!(app.documents.iter().any(|doc| doc.path == "late.txt"));
                    }
                }
            }
        }
    }
}

#[test]
fn substantive_ime_and_paste_still_cancel_owned_history_pointer_clicks() {
    for input in [
        egui::Event::Ime(egui::ImeEvent::Preedit("新".into())),
        egui::Event::Ime(egui::ImeEvent::Commit("新".into())),
        egui::Event::Paste("newer input".into()),
    ] {
        for on_release in [false, true] {
            let mut app = app();
            app.activate_history_tab(2);
            let editor = egui::Id::new(("editor", 2u64));
            app.editor_ctx
                .memory_mut(|memory| memory.request_focus(editor));
            frame(&mut app, vec![]);
            let at = crate::workspace_access_tests::recorded_rect(&app, "Back").center();
            let pointer = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            frame(&mut app, vec![egui::Event::PointerMoved(at), pointer(true)]);
            if !on_release {
                frame(&mut app, vec![input.clone()]);
            }
            let mut release = vec![pointer(false)];
            if on_release {
                release.push(input.clone());
            }
            frame(&mut app, release);
            assert_eq!(app.active_document, Some(2));
            assert_eq!(app.location_history.back.len(), 1);
            assert!(app.location_history.forward.is_empty());
        }
    }
}

fn settle_system_fonts(app: &mut CedarApp) {
    // Let the real CJK loader finish before injecting a result at a fixed
    // gesture boundary. It stays enabled, including on hosts without CJK fonts.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        frame(app, vec![]);
        if app.error.is_some() || app.notice.starts_with("CJK fallback:") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the real system font probe did not complete before the pointer test"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn settled_history_banner_app(forward: bool) -> CedarApp {
    let mut app = app();
    settle_system_fonts(&mut app);
    app.error = None;
    app.notice = "Settled font result".into();
    app.activate_history_tab(2);
    if forward {
        assert!(step(&mut app, Direction::Back));
    }
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    app
}

fn move_history_header_with_banner(app: &mut CedarApp) {
    // Notifications now render below the header. A changed workspace title
    // moves the real header buttons at the same boundary, preserving the
    // geometry-versus-widget-ownership case without changing navigation state.
    app.root = "/workspace/received-workspace-with-a-long-display-name".into();
}

fn history_banner_pointer_case(before_press: bool, ready: bool, stale: bool, stationary: bool) {
    for error in [false, true] {
        for forward in [false, true] {
            let mut app = settled_history_banner_app(forward);
            let name = if forward { "Forward" } else { "Back" };
            let direction = if forward {
                Direction::Forward
            } else {
                Direction::Back
            };
            let before = crate::workspace_access_tests::recorded_response(&app, name);
            let before_rect = crate::workspace_access_tests::recorded_rect(&app, name);
            assert!(before.enabled());
            let at = before_rect.center();
            let source = app.active_document;
            let target = Some(if forward { 2 } else { 1 });
            if stale {
                app.documents
                    .iter_mut()
                    .find(|doc| Some(doc.id) == target)
                    .unwrap()
                    .edit_version += 1;
            }
            let read = if ready {
                let (worker, commands) = Worker::recording();
                app.worker = Some(worker);
                app.open("late.txt".into(), None);
                Some(commands.try_recv().unwrap().id)
            } else {
                None
            };
            let epoch = app.navigation_epoch;
            let pointer = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let inject_result = |app: &mut CedarApp| {
                move_history_header_with_banner(app);
                if error {
                    app.error = Some("Generated font completion error".into());
                } else {
                    app.notice = "Generated font completion notice".into();
                }
            };
            if before_press {
                inject_result(&mut app);
            }
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(at), pointer(at, true)],
            );
            let pressed = crate::workspace_access_tests::recorded_response(&app, name);
            let pressed_rect = crate::workspace_access_tests::recorded_rect(&app, name);
            let egui_owned = pressed.is_pointer_button_down_on();
            let cedar_owned =
                app.location_history.press == app.history_press_stamp(direction, pressed.id);
            eprintln!(
                "history banner: {name}, error={error}, before_press={before_press}, ready={ready}, stale={stale}, \
                 before_id={:?}, press_id={:?}, before_render={before_rect:?}, \
                 press_render={pressed_rect:?}, before_interaction={:?}, \
                 press_interaction={:?}, at={at:?}, render_contains={}, \
                 egui_owned={egui_owned}, cedar_owned={cedar_owned}",
                before.id,
                pressed.id,
                before.rect,
                pressed.rect,
                pressed_rect.contains(at)
            );
            assert_eq!(
                before.id, pressed.id,
                "banner changed the history widget ID"
            );
            if before_press {
                assert!(
                    !pressed_rect.contains(at),
                    "the workspace-title update must move the rendered history button"
                );
            }
            assert!(egui_owned, "the rendered history button must own the press");
            if !before_press {
                inject_result(&mut app);
            }
            if let Some(read) = read {
                queue_file(&app, read, "late.txt");
            }
            // A press-frame layout change releases over the newly rendered
            // button. A release-frame change retains egui's already-owned
            // click, as does the existing captured-tab panel-motion test.
            let release_at = if before_press && !stationary {
                pressed_rect.center()
            } else {
                at
            };
            frame(&mut app, vec![pointer(release_at, false)]);
            let released = crate::workspace_access_tests::recorded_response(&app, name);
            let released_rect = crate::workspace_access_tests::recorded_rect(&app, name);
            let clicked = app
                .editor_ctx
                .interaction_snapshot(|snapshot| snapshot.clicked);
            let release_owned =
                clicked == Some(pressed.id) && released.clicked_by(egui::PointerButton::Primary);
            eprintln!(
                "history banner release: {name}, error={error}, before_press={before_press}, ready={ready}, stale={stale}, \
                 release_id={:?}, release_render={released_rect:?}, release_interaction={:?}, \
                 clicked={clicked:?}, at={release_at:?}, render_contains={}, active={:?}",
                released.id,
                released.rect,
                released_rect.contains(release_at),
                app.active_document
            );
            assert_eq!(
                before.id, released.id,
                "banner changed the history widget ID"
            );
            assert!(
                !released_rect.contains(at),
                "the workspace-title update must move the rendered history button"
            );
            if before_press && !stationary {
                assert!(released_rect.contains(release_at));
            }
            assert!(
                release_owned,
                "egui must retain the actual captured interaction"
            );
            assert_eq!(
                app.active_document,
                if stale { source } else { target },
                "{name}, error={error}, before_press={before_press}, ready={ready}, stale={stale}, cedar_owned={cedar_owned}: \
                 an actual captured history click must navigate"
            );
            assert!(cedar_owned);
            if stale {
                assert!(app.location_history.back.is_empty());
                assert!(app.location_history.forward.is_empty());
                assert_eq!(
                    app.location_history.message.as_deref(),
                    Some("Skipped 1 stale editor location")
                );
            } else {
                assert_eq!(app.location_history.back.len(), usize::from(forward));
                assert_eq!(app.location_history.forward.len(), usize::from(!forward));
            }
            assert!(app.location_history.press.is_none());
            assert_eq!(app.navigation_epoch, epoch.wrapping_add(1));
            if ready {
                assert!(app.documents.iter().any(|doc| doc.path == "late.txt"));
                assert!(app.location_history.pending.is_none());
            }
            let back = signature(&app.location_history.back);
            let forward_stack = signature(&app.location_history.forward);
            frame(&mut app, vec![]);
            assert_eq!(app.active_document, if stale { source } else { target });
            assert_eq!(app.navigation_epoch, epoch.wrapping_add(1));
            assert_eq!(signature(&app.location_history.back), back);
            assert_eq!(signature(&app.location_history.forward), forward_stack);
        }
    }
}

#[test]
fn history_banner_before_press_distinguishes_geometry_from_widget_ownership() {
    history_banner_pointer_case(true, false, false, false);
}

#[test]
fn history_banner_before_release_distinguishes_geometry_from_widget_ownership() {
    history_banner_pointer_case(false, false, false, false);
}

#[test]
fn history_banner_captured_release_supersedes_ready_read_once() {
    for before_press in [false, true] {
        history_banner_pointer_case(before_press, true, false, false);
    }
}

#[test]
fn history_banner_captured_all_stale_release_consumes_ready_read_once() {
    for before_press in [false, true] {
        history_banner_pointer_case(before_press, true, true, false);
    }
}

#[test]
fn history_banner_stationary_release_retains_capture_after_press_frame_motion() {
    // Native-shaped stationary pointer: only the UI moves. Both the ordinary
    // widget path and the ordered ready-reply path must keep the captured ID.
    for ready in [false, true] {
        history_banner_pointer_case(true, ready, false, true);
    }
}

#[test]
fn history_banner_same_frame_primary_click_keeps_actual_widget_ownership() {
    for forward in [false, true] {
        let mut app = settled_history_banner_app(forward);
        let name = if forward { "Forward" } else { "Back" };
        let before = crate::workspace_access_tests::recorded_response(&app, name);
        let at = crate::workspace_access_tests::recorded_rect(&app, name).center();
        move_history_header_with_banner(&mut app);
        app.error = Some("Generated error before a same-frame click".into());
        let pointer = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &mut app,
            vec![egui::Event::PointerMoved(at), pointer(true), pointer(false)],
        );
        assert_eq!(
            app.editor_ctx
                .interaction_snapshot(|snapshot| snapshot.clicked),
            Some(before.id)
        );
        assert!(!crate::workspace_access_tests::recorded_rect(&app, name).contains(at));
        assert_eq!(app.active_document, Some(if forward { 2 } else { 1 }));
        assert_eq!(app.location_history.back.len(), usize::from(forward));
        assert_eq!(app.location_history.forward.len(), usize::from(!forward));
        assert!(app.location_history.press.is_none());
    }
}

#[test]
fn history_banner_orphan_and_disabled_releases_do_not_cancel_ready_read() {
    for disabled in [false, true] {
        let mut app = settled_history_banner_app(false);
        if disabled {
            app.location_history.back.clear();
            frame(&mut app, vec![]);
            // read_response observes the previous interaction snapshot; make
            // the disabled render its input before starting this gesture.
            frame(&mut app, vec![]);
        }
        let at = crate::workspace_access_tests::recorded_rect(&app, "Back").center();
        assert_eq!(
            crate::workspace_access_tests::recorded_response(&app, "Back").enabled(),
            !disabled
        );
        let mut expected_back = app.location_history.back.clone();
        expected_back.push(app.history_departure().unwrap());
        let (worker, commands) = Worker::recording();
        app.worker = Some(worker);
        app.open("late.txt".into(), None);
        let read = commands.try_recv().unwrap().id;
        let pointer = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        move_history_header_with_banner(&mut app);
        app.error = Some("Generated error before an unowned gesture".into());
        if disabled {
            frame(&mut app, vec![egui::Event::PointerMoved(at), pointer(true)]);
        }
        assert!(app.location_history.press.is_none());
        queue_file(&app, read, "late.txt");
        frame(&mut app, vec![pointer(false)]);
        assert_eq!(app.active().unwrap().path, "late.txt");
        assert_eq!(
            signature(&app.location_history.back),
            signature(&expected_back)
        );
        assert!(app.location_history.forward.is_empty());
        assert!(app.location_history.pending.is_none());
        assert!(app.location_history.press.is_none());
    }
}

#[test]
fn history_banner_drag_and_changed_target_cancel_captured_press() {
    for mutation in 0..4 {
        let mut app = settled_history_banner_app(false);
        let before = crate::workspace_access_tests::recorded_response(&app, "Back");
        let at = crate::workspace_access_tests::recorded_rect(&app, "Back").center();
        let pointer = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        move_history_header_with_banner(&mut app);
        app.error = Some("Generated error before a cancelled gesture".into());
        frame(
            &mut app,
            vec![egui::Event::PointerMoved(at), pointer(at, true)],
        );
        assert!(app.location_history.press.is_some());
        let mut release_at = crate::workspace_access_tests::recorded_rect(&app, "Back").center();
        let mut release = vec![];
        match mutation {
            0 => {
                release_at = egui::pos2(600.0, 400.0);
                release.push(egui::Event::PointerMoved(release_at));
            }
            1 => app.activate_history_tab(3),
            2 => app.generation += 1,
            _ => app.documents[0].edit_version += 1,
        }
        let source = app.active_document;
        let back = signature(&app.location_history.back);
        let forward = signature(&app.location_history.forward);
        release.push(pointer(release_at, false));
        frame(&mut app, release);
        assert_eq!(
            app.editor_ctx
                .interaction_snapshot(|snapshot| snapshot.clicked),
            if mutation == 0 { None } else { Some(before.id) }
        );
        assert_eq!(app.active_document, source, "mutation {mutation}");
        assert_eq!(signature(&app.location_history.back), back);
        assert_eq!(signature(&app.location_history.forward), forward);
        assert!(app.location_history.press.is_none());
    }
}

#[test]
fn history_banner_foreign_button_capture_cannot_become_primary_navigation() {
    for foreign in [
        egui::PointerButton::Secondary,
        egui::PointerButton::Middle,
        egui::PointerButton::Extra1,
        egui::PointerButton::Extra2,
    ] {
        for same_frame in [false, true] {
            let mut app = settled_history_banner_app(false);
            let before = crate::workspace_access_tests::recorded_response(&app, "Back");
            let at = crate::workspace_access_tests::recorded_rect(&app, "Back").center();
            let back = signature(&app.location_history.back);
            let pointer = |pos, button, pressed| egui::Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(at), pointer(at, foreign, true)],
            );
            assert!(app.location_history.press.is_none());
            assert!(
                crate::workspace_access_tests::recorded_response(&app, "Back")
                    .is_pointer_button_down_on()
            );
            move_history_header_with_banner(&mut app);
            app.error = Some("Generated error while a foreign button owns the widget".into());
            let elsewhere = egui::pos2(600.0, 400.0);
            let mut press = vec![
                egui::Event::PointerMoved(elsewhere),
                pointer(elsewhere, egui::PointerButton::Primary, true),
            ];
            if same_frame {
                press.push(pointer(elsewhere, egui::PointerButton::Primary, false));
            }
            frame(&mut app, press);
            let stamped = app.location_history.press.is_some();
            if !same_frame {
                frame(
                    &mut app,
                    vec![pointer(elsewhere, egui::PointerButton::Primary, false)],
                );
            }
            let clicked = app
                .editor_ctx
                .interaction_snapshot(|snapshot| snapshot.clicked);
            eprintln!(
                "foreign history capture: {foreign:?}, same_frame={same_frame}, \
                 stamped={stamped}, clicked={clicked:?}, active={:?}",
                app.active_document
            );
            assert_eq!(clicked, Some(before.id));
            assert_eq!(
                app.active_document,
                Some(2),
                "{foreign:?}, same_frame={same_frame}"
            );
            assert_eq!(signature(&app.location_history.back), back);
            assert!(app.location_history.forward.is_empty());
            assert!(app.location_history.press.is_none());
            frame(&mut app, vec![pointer(elsewhere, foreign, false)]);
            let at = crate::workspace_access_tests::recorded_rect(&app, "Back").center();
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(at),
                    pointer(at, egui::PointerButton::Primary, true),
                    pointer(at, egui::PointerButton::Primary, false),
                ],
            );
            assert_eq!(app.active_document, Some(1));
        }
    }
}
