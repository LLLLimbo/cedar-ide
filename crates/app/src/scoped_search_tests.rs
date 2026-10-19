//! Scoped Find controls and source authority through production native frames.
//! These recording-worker tests issue no filesystem, agent, font or language I/O.
use super::*;
use crate::workspace_access_tests::{recorded_clip, recorded_rect, recorded_response};
use cedar_protocol::{ScopedSearchDescriptor, ScopedSearchRequest, ScopedSearchResult};
use egui::text::{CCursor, CCursorRange};

pub(super) const SIZE: egui::Vec2 = egui::vec2(1100.0, 800.0);
const SAVED: &str = "saved Ω😀\r\nneedle second\nlast\n";
const DRAFT: &str = "dirty Ω😀\r\nneedle second\nlast\n";
pub(super) type SelectionKey = (usize, bool, usize, bool);
pub(super) type HistorySnapshot = [Vec<(SelectionKey, String)>; 2];

#[derive(Clone, Copy, Debug)]
pub(super) enum Surface {
    Browse,
    Bottom,
}
impl Surface {
    pub(super) fn query(self) -> &'static str {
        match self {
            Self::Browse => "browse_query",
            Self::Bottom => "search_query",
        }
    }
    pub(super) fn scope(self) -> &'static str {
        match self {
            Self::Browse => "browse_text_scope",
            Self::Bottom => "search_scope",
        }
    }
    pub(super) fn masks(self) -> &'static str {
        match self {
            Self::Browse => "browse_masks",
            Self::Bottom => "search_masks",
        }
    }
    pub(super) fn submit(self) -> &'static str {
        match self {
            Self::Browse => "browse_submit",
            Self::Bottom => "search_submit",
        }
    }
    pub(super) fn row(self, index: usize) -> String {
        format!(
            "{}:{index}",
            match self {
                Self::Browse => "browse_row",
                Self::Bottom => "search_row",
            }
        )
    }
    pub(super) fn group(self, index: usize) -> String {
        format!(
            "{}:{index}",
            match self {
                Self::Browse => "browse_group",
                Self::Bottom => "search_group",
            }
        )
    }
}

pub(super) fn command_modifiers() -> egui::Modifiers {
    egui::Modifiers {
        ctrl: true,
        command: true,
        ..egui::Modifiers::NONE
    }
}
pub(super) fn key(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
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
pub(super) fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
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
pub(super) fn frame_at(
    app: &mut CedarApp,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    captured_frame(app, size, events, |_| {})
}
pub(super) fn captured_frame(
    app: &mut CedarApp,
    size: egui::Vec2,
    events: Vec<egui::Event>,
    change: impl FnOnce(&mut CedarApp),
) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut input = egui::RawInput {
        time: Some(ctx.input(|input| input.time) + 0.1),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        events,
        focused: true,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    change(app);
    let mut native = eframe::Frame::_new_kittest();
    let output = ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native));
    assert!(output
        .shapes
        .iter()
        .all(|shape| shape.clip_rect.is_finite()));
    output
}
pub(super) fn frame(app: &mut CedarApp, events: Vec<egui::Event>) -> egui::FullOutput {
    frame_at(app, SIZE, events)
}
pub(super) fn settle(app: &mut CedarApp) {
    for _ in 0..3 {
        frame(app, vec![]);
    }
}
pub(super) fn focus(app: &mut CedarApp, probe: &str) {
    recorded_response(app, probe).request_focus();
    frame(app, vec![]);
    assert_eq!(
        app.editor_ctx.memory(|m| m.focused()),
        Some(recorded_response(app, probe).id)
    );
}
pub(super) fn click(app: &mut CedarApp, probe: &str) {
    click_at_size(app, SIZE, probe);
}
pub(super) fn click_at_size(app: &mut CedarApp, size: egui::Vec2, probe: &str) {
    let rect = recorded_rect(app, probe).intersect(recorded_clip(app, probe));
    assert!(
        rect.is_finite() && rect.is_positive(),
        "{probe} has no visible hit target"
    );
    let pos = rect.center();
    frame_at(app, size, vec![egui::Event::PointerMoved(pos)]);
    frame_at(app, size, pointer(pos, true));
    assert!(
        recorded_response(app, probe).is_pointer_button_down_on(),
        "{probe} missed native press"
    );
    frame_at(app, size, pointer(pos, false));
    assert!(
        recorded_response(app, probe).clicked(),
        "{probe} missed native click"
    );
}
pub(super) fn replace(app: &mut CedarApp, probe: &str, value: &str) {
    focus(app, probe);
    frame(app, key(egui::Key::A, command_modifiers()));
    frame(app, key(egui::Key::Backspace, egui::Modifiers::NONE));
    if !value.is_empty() {
        frame(app, vec![egui::Event::Text(value.into())]);
    }
    settle(app);
}
pub(super) fn enter(app: &mut CedarApp, surface: Surface) {
    match surface {
        Surface::Browse => {
            frame(
                app,
                key(
                    egui::Key::F,
                    egui::Modifiers {
                        shift: true,
                        ..command_modifiers()
                    },
                ),
            );
        }
        Surface::Bottom => {
            app.tool = Tool::Search;
            app.tools_open = true;
            app.tools_height = Some(480.0);
        }
    }
    settle(app);
}
pub(super) fn reply(app: &mut CedarApp, id: u64, result: Result<Payload, String>) {
    app.apply_event(Event {
        generation: app.generation,
        id,
        connected: true,
        result,
    });
}
pub(super) fn fixture() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    // Match the production startup fonts and metrics before the first native frame.
    // Optional CJK fallback must not replace a default-font Latin layout mid-gesture.
    app.editor_ctx.set_fonts(system_fonts::base_fonts());
    app.editor_ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(14.0));
    });
    let form = ConnectForm {
        local_root: "/workspace".into(),
        allow_run: false,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/workspace".into();
    app.agent_info = Some(agent_support::full_test_agent());
    app.scoped_search = Some(ScopedSearchDescriptor { version: 1 });
    app.state = ConnectionState::Ready;
    app.open_form = false;
    app.documents.push(Document::new(
        1,
        "src/origin.rs".into(),
        SAVED.into(),
        "a".repeat(64),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    settle(&mut app);
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 8);
    let mut range = CCursorRange::two(CCursor::new(1), CCursor::new(8));
    range.primary.prefer_next_row = false;
    range.secondary.prefer_next_row = true;
    let mut state = editor_state::load(&app.editor_ctx, &mut app.documents[0]);
    state.cursor.set_char_range(Some(range));
    state.store(&app.editor_ctx, egui::Id::new(("editor", 1_u64)));
    settle(&mut app);
    assert!(commands.try_recv().is_err());
    (app, commands)
}
pub(super) fn request(query: &str, scope: &str, masks: &[&str]) -> ScopedSearchRequest {
    ScopedSearchRequest {
        query: query.into(),
        scope: scope.into(),
        masks: masks.iter().map(|v| (*v).into()).collect(),
        limit: 500,
    }
}
pub(super) fn rows() -> Vec<SearchMatch> {
    [
        ("src/origin.rs", 2),
        ("src/origin.rs", 3),
        ("src/target.rs", 2),
    ]
    .into_iter()
    .map(|(path, line)| SearchMatch {
        path: path.into(),
        line,
        text: "needle historical disk match".into(),
    })
    .collect()
}
pub(super) fn payload(request: ScopedSearchRequest, matches: Vec<SearchMatch>) -> Payload {
    Payload::ScopedMatches {
        result: ScopedSearchResult {
            request,
            matches,
            incomplete: false,
            reasons: vec![],
            examined_entries: 4,
            read_bytes: 128,
        },
    }
}
pub(super) fn submit(
    app: &mut CedarApp,
    commands: &Receiver<Command>,
    surface: Surface,
) -> Command {
    click(app, surface.submit());
    let command = commands
        .try_recv()
        .expect("explicit submit must send one command");
    assert!(commands.try_recv().is_err());
    command
}
pub(super) fn scoped(
    app: &mut CedarApp,
    commands: &Receiver<Command>,
    surface: Surface,
) -> Command {
    replace(app, surface.query(), "needle");
    replace(app, surface.scope(), "src");
    replace(app, surface.masks(), "*.rs");
    assert!(commands.try_recv().is_err(), "typing filters must be local");
    let command = submit(app, commands, surface);
    assert!(
        matches!(&command.op, Operation::SearchIn { request: actual } if *actual == request("needle", "src", &["*.rs"]))
    );
    command
}
pub(super) fn accepted(app: &mut CedarApp, commands: &Receiver<Command>, surface: Surface) {
    let command = scoped(app, commands, surface);
    reply(
        app,
        command.id,
        Ok(payload(request("needle", "src", &["*.rs"]), rows())),
    );
    settle(app);
    assert!(recorded_response(app, &surface.row(0)).enabled());
}
pub(super) fn has_text(output: &egui::FullOutput, expected: &str) -> bool {
    fn contains(shape: &egui::epaint::Shape, expected: &str) -> bool {
        match shape {
            egui::epaint::Shape::Text(t) => t.galley.job.text.contains(expected),
            egui::epaint::Shape::Vec(v) => v.iter().any(|s| contains(s, expected)),
            _ => false,
        }
    }
    output.shapes.iter().any(|s| contains(&s.shape, expected))
}
// Status details are reachable through the real bounded native ScrollArea.
// Do not call has_text on clipped-away galleys or remove the old exact counters.
pub(super) fn browse_status_scroll(
    app: &mut CedarApp,
    size: egui::Vec2,
    to_end: bool,
) -> egui::FullOutput {
    let (id, rect, content): (egui::Id, egui::Rect, egui::Vec2) = app
        .editor_ctx
        .data(|data| data.get_temp(egui::Id::new("text_status_scroll_probe")))
        .unwrap();
    frame_at(
        app,
        size,
        vec![
            egui::Event::PointerMoved(rect.center()),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, if to_end { -8000.0 } else { 8000.0 }),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    for _ in 0..3 {
        frame_at(app, size, vec![]);
    }
    let output = frame_at(app, size, vec![]);
    let offset = egui::scroll_area::State::load(&app.editor_ctx, id)
        .unwrap()
        .offset
        .y;
    let expected = if to_end {
        (content.y - rect.height()).max(0.0)
    } else {
        0.0
    };
    assert!(
        (offset - expected).abs() <= 1.0,
        "status details scroll must reach requested endpoint: {offset} vs {expected}"
    );
    output
}

// A one-Body-line plus 4-point-slack details viewport cannot show both the scan
// row and a later incomplete warning at once. The slack prevents fractional
// wheel offsets from clipping either edge of a complete Body row. Prove every
// exact field is fully readable during
// real scrolling, rather than accepting a clipped galley or requiring the tail
// frame to contain all rows. One-point wheel steps cannot skip a complete row.
pub(super) fn browse_assert_status_fields_reachable(
    app: &mut CedarApp,
    size: egui::Vec2,
    expected: &[&str],
    forbidden: &[&str],
) {
    fn full_row(
        shape: &egui::epaint::Shape,
        clip: egui::Rect,
        viewport: egui::Rect,
        expected: &str,
    ) -> Option<egui::Rect> {
        match shape {
            egui::epaint::Shape::Text(draw) => draw.galley.rows.iter().find_map(|row| {
                let rect = row.rect.translate(draw.pos.to_vec2());
                (row.text().contains(expected)
                    && clip.contains_rect(rect)
                    && viewport.contains_rect(rect))
                .then_some(rect)
            }),
            egui::epaint::Shape::Vec(shapes) => shapes
                .iter()
                .find_map(|shape| full_row(shape, clip, viewport, expected)),
            _ => None,
        }
    }
    fn log_tail_rows(
        shape: &egui::epaint::Shape,
        clip: egui::Rect,
        viewport: egui::Rect,
        expected: &[&str],
        following_frame: bool,
        offset: f32,
    ) {
        match shape {
            egui::epaint::Shape::Text(draw)
                if expected
                    .iter()
                    .any(|field| draw.galley.text().contains(field)) =>
            {
                for row in &draw.galley.rows {
                    let rect = row.rect.translate(draw.pos.to_vec2());
                    eprintln!("SCOPED_STATUS_ROW_PAINT following_frame={following_frame} stored_offset={offset} text={:?} row={rect:?} clip={clip:?} viewport={viewport:?} full={}", row.text(), clip.contains_rect(rect) && viewport.contains_rect(rect));
                }
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    log_tail_rows(shape, clip, viewport, expected, following_frame, offset);
                }
            }
            _ => {}
        }
    }
    let mut output = browse_status_scroll(app, size, false);
    let (id, viewport, content): (egui::Id, egui::Rect, egui::Vec2) = app
        .editor_ctx
        .data(|data| data.get_temp(egui::Id::new("text_status_scroll_probe")))
        .unwrap();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    assert!(screen.contains_rect(viewport));
    let tail = (content.y - viewport.height()).max(0.0);
    let mut reached = vec![false; expected.len()];
    let mut tail_repainted = false;
    // Native smoothing can lag the wheel by a few frames. The bound is derived
    // from the actual content extent, not a timeout or direct ScrollArea write.
    for step in 0..=(tail.ceil() as usize + 20) {
        let offset = egui::scroll_area::State::load(&app.editor_ctx, id)
            .unwrap()
            .offset
            .y;
        if reached.iter().any(|reached| !reached) {
            for shape in &output.shapes {
                log_tail_rows(
                    &shape.shape,
                    shape.clip_rect,
                    viewport,
                    expected,
                    step > 0,
                    offset,
                );
            }
        }
        for field in forbidden {
            assert!(
                !has_text(&output, field),
                "unexpected status field {field:?} at offset {offset}"
            );
        }
        for (index, field) in expected.iter().enumerate() {
            if reached[index] {
                continue;
            }
            if let Some((rect, clip)) = output.shapes.iter().find_map(|shape| {
                full_row(&shape.shape, shape.clip_rect, viewport, field)
                    .map(|rect| (rect, shape.clip_rect))
            }) {
                reached[index] = true;
                eprintln!("SCOPED_STATUS_FIELD field={field:?} window={size:?} step={step} offset={offset} row={rect:?} clip={clip:?} viewport={viewport:?}");
            }
        }
        if step > 0 && (offset - tail).abs() <= 0.01 {
            for shape in &output.shapes {
                log_tail_rows(
                    &shape.shape,
                    shape.clip_rect,
                    viewport,
                    expected,
                    tail_repainted,
                    offset,
                );
            }
            if tail_repainted {
                break;
            }
            // ScrollArea handles wheel input in end(), after laying out and
            // painting this frame's children. Its stored offset can already be
            // at the tail while these shapes still use the previous offset.
            // Observe the one following native frame, without another wheel.
            output = frame_at(app, size, vec![]);
            tail_repainted = true;
            continue;
        }
        frame_at(
            app,
            size,
            vec![
                egui::Event::PointerMoved(viewport.center()),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -1.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        // Observe the user's stop after each small wheel action, not only at
        // the tail. end() has now committed this action's scroll position.
        output = frame_at(app, size, vec![]);
    }
    let offset = egui::scroll_area::State::load(&app.editor_ctx, id)
        .unwrap()
        .offset
        .y;
    assert!(
        (offset - tail).abs() <= 0.01,
        "real details wheel must reach the tail: {offset} vs {tail}"
    );
    let missing: Vec<_> = expected
        .iter()
        .zip(reached)
        .filter_map(|(field, reached)| (!reached).then_some(*field))
        .collect();
    assert!(
        missing.is_empty(),
        "status fields never fully painted in one Body row: {missing:?}; window={size:?}, viewport={viewport:?}, content={content:?}"
    );
}

pub(super) fn selection(app: &CedarApp) -> SelectionKey {
    let value = egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1_u64)))
        .unwrap()
        .cursor
        .char_range()
        .unwrap();
    (
        value.primary.index,
        value.primary.prefer_next_row,
        value.secondary.index,
        value.secondary.prefer_next_row,
    )
}
pub(super) fn history(app: &CedarApp) -> HistorySnapshot {
    let state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1_u64))).unwrap();
    let current = (
        state.cursor.char_range().unwrap(),
        app.documents[0].text.clone(),
    );
    let mut snapshots: [Vec<_>; 2] = Default::default();
    for (index, redo) in [false, true].into_iter().enumerate() {
        let mut undoer = state.undoer();
        let mut current = current.clone();
        for _ in 0..=editor_state::MAX_UNDO_STATES + 1 {
            let next = if redo {
                undoer.redo(&current)
            } else {
                undoer.undo(&current)
            }
            .cloned();
            let Some(next) = next else {
                break;
            };
            let r = next.0;
            snapshots[index].push((
                (
                    r.primary.index,
                    r.primary.prefer_next_row,
                    r.secondary.index,
                    r.secondary.prefer_next_row,
                ),
                next.1.clone(),
            ));
            current = next;
        }
    }
    snapshots
}

#[test]
fn search_cjk_accepted_path_or_snippet_triggers_existing_font_probe_on_both_surfaces() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for scoped_request in [false, true] {
            // ASCII controls must remain lazy. Each CJK case is search-only:
            // document, query, scope, masks and the other result field are ASCII
            // or non-CJK, and the CJK row is not the first result.
            for field in 0..3 {
                let (mut app, commands) = fixture();
                let departure = selection(&app);
                let undo = history(&app);
                enter(&mut app, surface);
                let pending = if scoped_request {
                    scoped(&mut app, &commands, surface)
                } else {
                    replace(&mut app, surface.query(), "needle");
                    let pending = submit(&mut app, &commands, surface);
                    assert!(
                        matches!(&pending.op, Operation::Search { query, limit: 500 } if query == "needle")
                    );
                    pending
                };
                let epoch = app.navigation_epoch;
                let mut matches = rows();
                if field == 1 {
                    matches.last_mut().unwrap().path = "src/未読.rs".into();
                } else if field == 2 {
                    matches.last_mut().unwrap().text = "needle 中文注释 Ω😀".into();
                }
                let expected: Vec<_> = matches
                    .iter()
                    .map(|row| (row.path.clone(), row.line, row.text.clone()))
                    .collect();
                let result = if scoped_request {
                    payload(request("needle", "src", &["*.rs"]), matches)
                } else {
                    Payload::Matches {
                        matches,
                        truncated: false,
                    }
                };
                assert!(!app.cjk_seen && !app.language.cjk_seen);
                assert!(app.documents.iter().all(|doc| !doc.has_cjk));
                assert!(app.system_fonts.needs_probe());
                app.result_tx
                    .send(WorkerEvent::Response(Event {
                        generation: app.generation,
                        id: pending.id,
                        connected: true,
                        result: Ok(result),
                    }))
                    .unwrap();
                // Stop on the acceptance frame, as in Browse's discovery/font
                // regression. A further frame could start optional OS font I/O.
                frame(&mut app, vec![]);
                assert_eq!(
                    app.cjk_seen,
                    field != 0,
                    "{surface:?}, scoped={scoped_request}, field={field}"
                );
                assert!(app.system_fonts.needs_probe());
                assert!(app.search_rows_active());
                assert_eq!(
                    app.search_state.accepted.as_ref().unwrap().request,
                    pending.id
                );
                assert_eq!(
                    app.search_results
                        .iter()
                        .map(|row| (row.path.clone(), row.line, row.text.clone()))
                        .collect::<Vec<_>>(),
                    expected,
                    "font detection must preserve exact UTF-8 result triples and order"
                );
                assert_eq!(app.documents[0].text, DRAFT);
                assert_eq!(app.documents[0].saved_text, SAVED);
                assert!(app.documents[0].dirty());
                assert_eq!(selection(&app), departure);
                assert_eq!(history(&app), undo);
                assert_eq!(app.navigation_epoch, epoch);
                assert!(app.location_history.back.is_empty());
                assert!(app.location_history.forward.is_empty());
                assert_eq!(app.documents.len(), 1);
                assert!(!app.has_search_preview() && !app.browse.preview_visible());
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn search_cjk_invalid_scoped_batch_or_payload_cannot_probe_or_replace_retained_rows() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for invalid in 0..7 {
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            accepted(&mut app, &commands, surface);
            let source = app.search_state.accepted.clone();
            let expected: Vec<_> = app
                .search_results
                .iter()
                .map(|row| (row.path.clone(), row.line, row.text.clone()))
                .collect();
            let pending = submit(&mut app, &commands, surface);
            let mut matches = rows();
            matches[0].text = "needle 中文 result in otherwise rejected batch".into();
            let mut result = match payload(request("needle", "src", &["*.rs"]), matches) {
                Payload::ScopedMatches { result } => result,
                _ => unreachable!(),
            };
            let response = match invalid {
                0 => {
                    result.matches.last_mut().unwrap().path = "other/outside.rs".into();
                    Ok(Payload::ScopedMatches { result })
                }
                1 => {
                    result.matches.last_mut().unwrap().line = 0;
                    Ok(Payload::ScopedMatches { result })
                }
                2 => {
                    result.matches.last_mut().unwrap().text.push('\n');
                    Ok(Payload::ScopedMatches { result })
                }
                3 => {
                    result.incomplete = true; // missing required reason
                    Ok(Payload::ScopedMatches { result })
                }
                4 => Ok(Payload::Matches {
                    matches: result.matches,
                    truncated: false,
                }),
                5 => Ok(Payload::GitStatus {
                    text: "中文 wrong payload".into(),
                }),
                _ => Err("中文 search failure".into()),
            };
            reply(&mut app, pending.id, response);
            frame(&mut app, vec![]);
            assert!(!app.cjk_seen, "{surface:?}, invalid={invalid}");
            assert!(app.system_fonts.needs_probe());
            assert_eq!(app.search_state.accepted, source);
            assert_eq!(
                app.search_results
                    .iter()
                    .map(|row| (row.path.clone(), row.line, row.text.clone()))
                    .collect::<Vec<_>>(),
                expected
            );
            assert!(!app.search_rows_active());
            assert!(app.search_state.pending.is_none());
            assert!(app.error.is_some());
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn scoped_find_both_surfaces_share_exact_filters_and_require_explicit_submit() {
    let (mut app, commands) = fixture();
    enter(&mut app, Surface::Bottom);
    replace(&mut app, "search_query", "needle");
    let legacy = submit(&mut app, &commands, Surface::Bottom);
    assert!(matches!(&legacy.op, Operation::Search { query, limit: 500 } if query == "needle"));
    reply(
        &mut app,
        legacy.id,
        Ok(Payload::Matches {
            matches: rows(),
            truncated: false,
        }),
    );
    settle(&mut app);
    replace(&mut app, "search_scope", "src/Ω folder");
    replace(&mut app, "search_masks", "*.rs; *.md");
    assert_eq!(
        app.search_specification().unwrap().request,
        request("needle", "src/Ω folder", &["*.rs", "*.md"])
    );
    enter(&mut app, Surface::Browse);
    assert_eq!(app.search_scope, "src/Ω folder");
    assert_eq!(app.search_masks, "*.rs; *.md");
    assert!(commands.try_recv().is_err());
    replace(&mut app, "browse_text_scope", "");
    replace(&mut app, "browse_masks", "*");
    let explicit_star = submit(&mut app, &commands, Surface::Browse);
    assert!(
        matches!(&explicit_star.op, Operation::SearchIn { request: actual } if *actual == request("needle", "", &["*"]))
    );
    reply(
        &mut app,
        explicit_star.id,
        Ok(payload(request("needle", "", &["*"]), rows())),
    );
    settle(&mut app);
    assert_eq!(
        app.search_state
            .accepted
            .as_ref()
            .unwrap()
            .specification
            .request,
        request("needle", "", &["*"])
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn scoped_find_old_and_unknown_peers_never_fall_back_to_broad_search() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for descriptor in [None, Some(ScopedSearchDescriptor { version: 2 })] {
            let (mut app, commands) = fixture();
            app.scoped_search = descriptor;
            enter(&mut app, surface);
            replace(&mut app, surface.query(), "needle");
            replace(&mut app, surface.scope(), "src");
            let output = frame(&mut app, vec![]);
            assert!(!recorded_response(&app, surface.submit()).enabled());
            assert!(has_text(&output, "require scoped search support"));
            app.search();
            assert!(commands.try_recv().is_err());
            replace(&mut app, surface.scope(), "");
            replace(&mut app, surface.masks(), "*");
            app.search();
            assert!(commands.try_recv().is_err());
            replace(&mut app, surface.masks(), "");
            let command = submit(&mut app, &commands, surface);
            assert!(
                matches!(&command.op, Operation::Search { query, limit: 500 } if query == "needle")
            );
        }
    }
}

#[test]
fn scoped_find_invalid_directory_or_masks_block_submission_without_normalization() {
    for (scope, masks) in [
        ("../src", ""),
        ("src/./nested", ""),
        ("src//nested", ""),
        ("/src", ""),
        ("src\\nested", ""),
        ("", "*.rs;"),
        ("", " "),
        ("", "[ab].rs"),
        ("", "a;b;c;d;e;f;g;h;i"),
    ] {
        let (mut app, commands) = fixture();
        app.search_query = "needle".into();
        app.search_scope = scope.into();
        app.search_masks = masks.into();
        assert!(
            app.search_specification().is_err(),
            "scope={scope:?}, masks={masks:?}"
        );
        app.search();
        assert!(commands.try_recv().is_err());
    }
    let (mut app, commands) = fixture();
    app.search_query = "needle".into();
    app.search_masks = "Ω".repeat(129);
    assert!(app.search_specification().is_err());
    app.search();
    assert!(commands.try_recv().is_err());
    app.search_masks = "Ω".repeat(128);
    assert!(app.search_specification().is_ok());
    app.search_scope = "src/Ω folder".into();
    assert_eq!(
        app.search_specification().unwrap().request.scope,
        "src/Ω folder"
    );
}

#[test]
fn scoped_find_edit_and_edit_back_retire_late_success_error_and_preview_authority() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for late_error in [false, true] {
            for field in [0, 1, 2, 3] {
                let (mut app, commands) = fixture();
                enter(&mut app, surface);
                accepted(&mut app, &commands, surface);
                let accepted_source = app.search_state.accepted.clone().unwrap();
                let accepted_request = app.search_request;
                assert_eq!(accepted_source.request, accepted_request);
                let accepted_rows = app
                    .search_results
                    .iter()
                    .map(|row| (row.path.clone(), row.line, row.text.clone()))
                    .collect::<Vec<_>>();
                focus(&mut app, &surface.row(0));
                frame(&mut app, key(egui::Key::Space, egui::Modifiers::NONE));
                settle(&mut app);
                assert!(commands.try_recv().is_err());
                frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
                settle(&mut app);
                let in_flight = submit(&mut app, &commands, surface);
                assert!(in_flight.id > accepted_request);
                let (probe, original, changed) = match field {
                    0 => (surface.query(), "needle", "other"),
                    1 => (surface.scope(), "src", "other"),
                    2 => (surface.masks(), "*.rs", "*.md"),
                    _ => (surface.masks(), "*.rs", "*.rs;"),
                };
                replace(&mut app, probe, changed);
                if field == 3 {
                    assert!(app.search_specification().is_err());
                }
                replace(&mut app, probe, original);
                let error_before_reply = app.error.clone();
                reply(
                    &mut app,
                    in_flight.id,
                    if late_error {
                        Err("中文 obsolete scoped error".into())
                    } else {
                        let obsolete_rows = rows()
                            .into_iter()
                            .map(|mut row| {
                                row.line += 10;
                                row.text = "中文 obsolete needle scoped result".into();
                                row
                            })
                            .collect();
                        Ok(payload(request("needle", "src", &["*.rs"]), obsolete_rows))
                    },
                );
                let output = frame(&mut app, vec![]);
                settle(&mut app);
                assert_eq!(app.error, error_before_reply, "{surface:?}, field={field}, late_error={late_error}: late reply changed the settled error state");
                assert_eq!(app.search_state.accepted.as_ref(), Some(&accepted_source), "{surface:?}, field={field}, late_error={late_error}: late reply replaced source identity");
                assert_eq!(
                    app.search_state.accepted.as_ref().unwrap().request,
                    accepted_request
                );
                assert_eq!(
                    app.search_request, in_flight.id,
                    "retirement must not roll request identity back"
                );
                assert_eq!(app.search_results.iter().map(|row| (row.path.clone(), row.line, row.text.clone())).collect::<Vec<_>>(), accepted_rows, "{surface:?}, field={field}, late_error={late_error}: late reply replaced historical row triples");
                assert!(!app.search_rows_active());
                assert!(!recorded_response(&app, &surface.row(0)).enabled());
                assert!(!app.has_search_preview() && !app.browse.preview_visible());
                assert!(!has_text(&output, "obsolete scoped error"));
                assert!(!app.cjk_seen);
                assert!(app.system_fonts.needs_probe());
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn scoped_find_rejects_every_mismatched_echo_component() {
    for changed in 0..4 {
        let (mut app, commands) = fixture();
        enter(&mut app, Surface::Bottom);
        let pending = scoped(&mut app, &commands, Surface::Bottom);
        let mut echoed = request("needle", "src", &["*.rs"]);
        match changed {
            0 => echoed.query.push('x'),
            1 => echoed.scope = "other".into(),
            2 => echoed.masks = vec!["*".into()],
            _ => echoed.limit = 499,
        }
        let mut rejected = rows();
        rejected[0].text = "needle 中文 rejected echoed request".into();
        reply(&mut app, pending.id, Ok(payload(echoed, rejected)));
        settle(&mut app);
        assert!(!app.search_rows_active());
        assert!(app.search_results.is_empty());
        assert!(!app.cjk_seen);
        assert!(app.system_fonts.needs_probe());
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn scoped_find_mixed_native_batches_and_stale_capture_cannot_preview_or_open() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for activation in [egui::Key::Enter, egui::Key::Space] {
            for extra in [
                egui::Event::Text("x".into()),
                egui::Event::Paste("x".into()),
                egui::Event::Ime(egui::ImeEvent::Commit("x".into())),
            ] {
                let (mut app, commands) = fixture();
                enter(&mut app, surface);
                accepted(&mut app, &commands, surface);
                focus(&mut app, &surface.row(2));
                let before = app.navigation_epoch;
                let mut events = key(activation, egui::Modifiers::NONE);
                events.insert(1, extra);
                frame(&mut app, events);
                settle(&mut app);
                assert_eq!(app.navigation_epoch, before);
                assert!(commands.try_recv().is_err());
            }
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            accepted(&mut app, &commands, surface);
            focus(&mut app, &surface.row(2));
            let before = app.navigation_epoch;
            captured_frame(
                &mut app,
                SIZE,
                key(activation, egui::Modifiers::NONE),
                |app| {
                    app.search_scope = "other".into();
                    app.search_filters_changed();
                },
            );
            settle(&mut app);
            assert_eq!(app.navigation_epoch, before);
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn scoped_find_pointer_press_cannot_activate_after_filters_change() {
    for surface in [Surface::Browse, Surface::Bottom] {
        let (mut app, commands) = fixture();
        enter(&mut app, surface);
        accepted(&mut app, &commands, surface);
        let probe = surface.row(2);
        let pos = recorded_rect(&app, &probe)
            .intersect(recorded_clip(&app, &probe))
            .center();
        frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
        frame(&mut app, pointer(pos, true));
        let before = app.navigation_epoch;
        captured_frame(&mut app, SIZE, pointer(pos, false), |app| {
            app.search_masks = "*.md".into();
            app.search_filters_changed();
        });
        assert_eq!(app.navigation_epoch, before);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn scoped_find_group_preview_open_back_and_native_undo_preserve_dirty_origin() {
    for surface in [Surface::Browse, Surface::Bottom] {
        let (mut app, commands) = fixture();
        let selected = selection(&app);
        let undo = history(&app);
        enter(&mut app, surface);
        accepted(&mut app, &commands, surface);
        click(&mut app, &surface.group(0));
        settle(&mut app);
        click(&mut app, &surface.group(0));
        settle(&mut app);
        focus(&mut app, &surface.row(0));
        frame(&mut app, key(egui::Key::Space, egui::Modifiers::NONE));
        settle(&mut app);
        assert_eq!(
            match surface {
                Surface::Browse => app.browse_preview_text(),
                Surface::Bottom => app.search_preview_text(),
            },
            Some(DRAFT)
        );
        assert!(commands.try_recv().is_err());
        assert_eq!(selection(&app), selected);
        assert_eq!(history(&app), undo);
        if matches!(surface, Surface::Browse) {
            frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
            settle(&mut app);
        }
        // Bottom Find leaves row ownership in place during preview. Escape is
        // a modal Browse return action, not a Bottom Search keyboard action.
        for index in [1, 2] {
            frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
            settle(&mut app);
            assert_eq!(
                app.editor_ctx.memory(|m| m.focused()),
                Some(recorded_response(&app, &surface.row(index)).id),
                "{surface:?}: dirty preview to unopened row {index}"
            );
            assert_eq!(
                match surface {
                    Surface::Browse => app.browse.selected_index(),
                    Surface::Bottom => app.search_state.selected_index(),
                },
                Some(index),
                "{surface:?}: movement selection {index}"
            );
        }
        frame(&mut app, key(egui::Key::Space, egui::Modifiers::NONE));
        let preview = commands
            .try_recv()
            .unwrap_or_else(|error| panic!("{surface:?}: unopened preview Read missing: {error}"));
        assert!(matches!(&preview.op, Operation::Read { path } if path == "src/target.rs"));
        reply(
            &mut app,
            preview.id,
            Ok(Payload::File {
                path: "src/target.rs".into(),
                text: "preview\nneedle\n".into(),
                revision: "b".repeat(64),
            }),
        );
        settle(&mut app);
        assert_eq!(app.documents.len(), 1);
        assert_eq!(history(&app), undo);
        if matches!(surface, Surface::Browse) {
            frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
            settle(&mut app);
        }
        assert_eq!(
            app.editor_ctx.memory(|m| m.focused()),
            Some(recorded_response(&app, &surface.row(2)).id),
            "{surface:?}: unopened preview retained or returned exact row focus"
        );
        frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
        let open = commands
            .try_recv()
            .unwrap_or_else(|error| panic!("{surface:?}: ordinary Open Read missing: {error}"));
        assert!(matches!(&open.op, Operation::Read { path } if path == "src/target.rs"));
        assert_ne!(open.id, preview.id);
        reply(
            &mut app,
            open.id,
            Ok(Payload::File {
                path: "src/target.rs".into(),
                text: "opened\nneedle\n".into(),
                revision: "c".repeat(64),
            }),
        );
        settle(&mut app);
        assert_eq!(app.active().unwrap().path, "src/target.rs");
        frame(&mut app, key(egui::Key::OpenBracket, command_modifiers()));
        settle(&mut app);
        assert_eq!(app.active_document, Some(1));
        assert_eq!(selection(&app), selected);
        assert_eq!(history(&app), undo);
        assert_eq!(app.documents[0].text, DRAFT);
        assert_eq!(app.documents[0].saved_text, SAVED);
        frame(&mut app, key(egui::Key::Z, command_modifiers()));
        assert_eq!(app.documents[0].text, SAVED);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn scoped_find_shared_controls_have_visible_native_geometry_at_both_font_sizes() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for size in [
            egui::vec2(1280.0, 900.0),
            egui::vec2(780.0, 540.0),
            egui::vec2(823.0, 540.0),
            egui::vec2(825.0, 540.0),
        ] {
            for font in [13.0, 18.0] {
                let (mut app, commands) = fixture();
                enter(&mut app, surface);
                accepted(&mut app, &commands, surface);
                app.tools_height = None;
                app.editor_ctx.style_mut(|style| {
                    for kind in [egui::TextStyle::Body, egui::TextStyle::Button] {
                        style.text_styles.insert(kind, FontId::proportional(font));
                    }
                    style
                        .text_styles
                        .insert(egui::TextStyle::Monospace, FontId::monospace(font));
                });
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                let visible = |app: &CedarApp, probe: &str, full: bool, stage: &str| {
                    let rect = recorded_rect(app, probe);
                    let clip = recorded_clip(app, probe);
                    let intersection = rect.intersect(clip).intersect(screen);
                    assert!(rect.is_finite() && rect.is_positive() && intersection.is_positive(), "{stage}: {surface:?}, font={font}, size={size:?}, {probe}: {rect:?} in {clip:?}");
                    if full {
                        assert!(screen.contains_rect(rect) && clip.contains_rect(rect), "{stage}: {surface:?}, font={font}, size={size:?}, {probe} clipped: {rect:?} in {clip:?}");
                    }
                };
                for _ in 0..4 {
                    frame_at(&mut app, size, vec![]);
                }
                for probe in [
                    surface.query(),
                    surface.scope(),
                    surface.masks(),
                    surface.submit(),
                ] {
                    visible(&app, probe, true, "accepted controls");
                }
                let output = match surface {
                    Surface::Browse => browse_status_scroll(&mut app, size, true),
                    Surface::Bottom => frame_at(&mut app, size, vec![]),
                };
                assert!(
                    has_text(&output, "4 entries examined") && has_text(&output, "128 bytes read")
                );
                if matches!(surface, Surface::Browse) {
                    browse_status_scroll(&mut app, size, false);
                }
                visible(&app, &surface.group(0), true, "accepted group");
                visible(&app, &surface.row(0), true, "accepted row");
                recorded_response(&app, &surface.row(0)).request_focus();
                for _ in 0..3 {
                    frame_at(&mut app, size, vec![]);
                }
                frame_at(&mut app, size, key(egui::Key::Space, egui::Modifiers::NONE));
                for _ in 0..4 {
                    frame_at(&mut app, size, vec![]);
                }
                let pane = match surface {
                    Surface::Browse => "browse_preview_content",
                    Surface::Bottom => "search_preview_content",
                };
                assert_eq!(
                    match surface {
                        Surface::Browse => app.browse_preview_text(),
                        Surface::Bottom => app.search_preview_text(),
                    },
                    Some(DRAFT)
                );
                if matches!(surface, Surface::Browse) {
                    let (_, viewport): (egui::Id, egui::Rect) = app
                        .editor_ctx
                        .data(|data| {
                            data.get_temp(egui::Id::new(("project_text_context_scroll", false)))
                        })
                        .expect("production Text preview viewport");
                    assert!(
                        viewport.is_finite()
                            && viewport.is_positive()
                            && screen.contains_rect(viewport)
                            && recorded_clip(&app, pane).contains_rect(viewport)
                    );
                    assert!(recorded_rect(&app, pane).intersects(viewport));
                    let code_font = egui::TextStyle::Monospace.resolve(&app.editor_ctx.style());
                    let (line_height, marker_width) = app.editor_ctx.fonts(|fonts| {
                        let line = fonts.layout_no_wrap(
                            "▶ 2 | needle second".into(),
                            code_font.clone(),
                            MUTED,
                        );
                        let marker = fonts.layout_no_wrap("▶".into(), code_font, MUTED);
                        (line.size().y, marker.size().x)
                    });
                    assert!(viewport.height() >= 3.0 * line_height,
                        "Browse font={font}, size={size:?}: three complete preview lines: {viewport:?}/{line_height}");
                    let target = recorded_rect(&app, "project_text_context_target");
                    assert!(
                        viewport.contains_rect(egui::Rect::from_min_max(
                            target.min,
                            egui::pos2(target.left() + marker_width, target.bottom())
                        )),
                        "Browse font={font}, size={size:?}: complete historical target marker"
                    );
                } else {
                    visible(&app, pane, true, "opened preview");
                }
                if matches!(surface, Surface::Browse) {
                    for probe in ["browse_preview", "browse_open", "browse_close"] {
                        visible(&app, probe, true, "preview action targets");
                    }
                } else {
                    // Bottom Find uses the real selected row for Space/Enter actions.
                    visible(&app, &surface.row(0), true, "preview and open key target");
                }
                for probe in [
                    surface.query(),
                    surface.scope(),
                    surface.masks(),
                    surface.submit(),
                ] {
                    visible(&app, probe, true, "preview controls");
                }
                // Browse Escape disarms and returns to its row. Bottom keeps
                // row ownership and follows the next row after settlement.
                let (return_key, returned_index) = match surface {
                    Surface::Browse => (egui::Key::Escape, 0),
                    Surface::Bottom => (egui::Key::ArrowDown, 1),
                };
                frame_at(&mut app, size, key(return_key, egui::Modifiers::NONE));
                for _ in 0..3 {
                    frame_at(&mut app, size, vec![]);
                }
                assert_eq!(
                    app.editor_ctx.memory(|m| m.focused()),
                    Some(recorded_response(&app, &surface.row(returned_index)).id),
                    "{surface:?}, font={font}, size={size:?}: preview return or movement focus"
                );
                assert_eq!(
                    match surface {
                        Surface::Browse => app.browse.selected_index(),
                        Surface::Bottom => app.search_state.selected_index(),
                    },
                    Some(returned_index),
                    "{surface:?}: preview return or movement selection"
                );
                if matches!(surface, Surface::Bottom) {
                    assert!(app.project_text_review_armed());
                    assert!(!app.project_text_review_pending());
                    assert_eq!(app.search_preview_text(), Some(DRAFT));
                    assert!(app
                        .project_text_preview_context()
                        .unwrap()
                        .text
                        .contains("▶ 3 | last"));
                    visible(&app, pane, true, "settled next-row preview");
                    visible(&app, &surface.row(1), true, "settled next-row key target");
                    assert!(commands.try_recv().is_err(), "draft context must not Read");
                    frame_at(
                        &mut app,
                        size,
                        key(egui::Key::Escape, egui::Modifiers::NONE),
                    );
                    for _ in 0..3 {
                        frame_at(&mut app, size, vec![]);
                    }
                    assert_eq!(
                        app.editor_ctx.memory(|m| m.focused()),
                        Some(recorded_response(&app, &surface.row(1)).id),
                        "{surface:?}, font={font}, size={size:?}: Escape retains exact row focus"
                    );
                    assert_eq!(app.search_state.selected_index(), Some(1));
                }
                assert!(!app.project_text_review_armed());
                assert!(!app.project_text_review_pending());
                assert!(match surface {
                    Surface::Browse => app.browse_preview_text(),
                    Surface::Bottom => app.search_preview_text(),
                }
                .is_none());
                // Historical rows retain their actual display geometry after a filter
                // edit, while every activation target loses its source authority.
                app.search_masks = "*.md".into();
                app.search_filters_changed();
                for _ in 0..4 {
                    frame_at(&mut app, size, vec![]);
                }
                let historical = frame_at(&mut app, size, vec![]);
                assert!(has_text(&historical, "Historical results for"));
                assert!(!recorded_response(&app, &surface.row(0)).enabled());
                visible(&app, &surface.row(0), true, "historical row");
                for probe in [
                    surface.query(),
                    surface.scope(),
                    surface.masks(),
                    surface.submit(),
                ] {
                    visible(&app, probe, true, "historical controls");
                }
                if matches!(surface, Surface::Browse) {
                    for probe in ["browse_preview", "browse_open"] {
                        visible(&app, probe, true, "historical disabled actions");
                        assert!(!recorded_response(&app, probe).enabled());
                    }
                }
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn scoped_find_preview_budget_keeps_controls_and_long_results_reachable() {
    // Keep the former breakpoint neighbors as resize pressure cases.
    // Text now uses the same vertical results/preview order at both widths.
    for width in [823.0, 825.0] {
        for height in [540.0, 900.0] {
            for font in [13.0, 18.0] {
                for empty in [false, true] {
                    let size = egui::vec2(width, height);
                    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                    let (mut app, commands) = fixture();
                    let draft = (1..=120)
                        .map(|line| format!("needle line {line}\n"))
                        .collect::<String>();
                    editor_state::commit(&app.editor_ctx, &mut app.documents[0], draft, 8);
                    enter(&mut app, Surface::Browse);
                    let pending = scoped(&mut app, &commands, Surface::Browse);
                    let matches = if empty {
                        vec![]
                    } else {
                        (2..=41)
                            .map(|line| SearchMatch {
                                path: "src/origin.rs".into(),
                                line,
                                text: "needle historical disk match".into(),
                            })
                            .collect()
                    };
                    reply(
                        &mut app,
                        pending.id,
                        Ok(payload(request("needle", "src", &["*.rs"]), matches)),
                    );
                    settle(&mut app);
                    app.editor_ctx.style_mut(|style| {
                        for kind in [egui::TextStyle::Body, egui::TextStyle::Button] {
                            style.text_styles.insert(kind, FontId::proportional(font));
                        }
                        style
                            .text_styles
                            .insert(egui::TextStyle::Monospace, FontId::monospace(font));
                    });
                    let visible = |app: &CedarApp, probe: &str| {
                        let rect = recorded_rect(app, probe);
                        let clip = recorded_clip(app, probe);
                        assert!(rect.is_positive() && screen.contains_rect(rect) && clip.contains_rect(rect),
                            "size={size:?}, font={font}, empty={empty}, {probe}: {rect:?} in {clip:?}");
                    };
                    let controls = [
                        "browse_query",
                        "browse_text_scope",
                        "browse_masks",
                        "browse_submit",
                        "browse_preview",
                        "browse_open",
                        "browse_close",
                    ];
                    for _ in 0..4 {
                        frame_at(&mut app, size, vec![]);
                    }
                    for probe in controls {
                        visible(&app, probe);
                    }
                    assert!(app.browse_preview_text().is_none());
                    if empty {
                        assert_eq!(app.browse.selected_index(), None);
                        assert!(!recorded_response(&app, "browse_preview").enabled());
                        assert!(!recorded_response(&app, "browse_open").enabled());
                    } else {
                        // Real pointer/key ownership opens and closes preview on the same rows.
                        click_at_size(&mut app, size, "browse_query");
                        frame_at(
                            &mut app,
                            size,
                            key(egui::Key::ArrowDown, egui::Modifiers::NONE),
                        );
                        for _ in 0..3 {
                            frame_at(&mut app, size, vec![]);
                        }
                        for index in [0, 39] {
                            assert_eq!(app.browse.selected_index(), Some(index));
                            visible(&app, &format!("browse_row:{index}"));
                            frame_at(&mut app, size, key(egui::Key::Space, egui::Modifiers::NONE));
                            for _ in 0..4 {
                                frame_at(&mut app, size, vec![]);
                            }
                            for probe in controls {
                                visible(&app, probe);
                            }
                            assert!(app.project_text_review_armed());
                            let (_, viewport) = app
                                .editor_ctx
                                .data(|data| {
                                    data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new((
                                        "project_text_context_scroll",
                                        false,
                                    )))
                                })
                                .expect("production preview viewport");
                            let content = recorded_rect(&app, "browse_preview_content");
                            assert!(viewport.is_positive() && screen.contains_rect(viewport));
                            assert!(
                                content.height() > viewport.height(),
                                "long context must really scroll"
                            );
                            let target = recorded_rect(&app, "project_text_context_target");
                            assert!(viewport
                                .contains(egui::pos2(target.left() + 1.0, target.center().y)));
                            let (_, results, content_size) = app
                                .editor_ctx
                                .data(|data| {
                                    data.get_temp::<(egui::Id, egui::Rect, egui::Vec2)>(
                                        egui::Id::new("browse_results_scroll_probe"),
                                    )
                                })
                                .expect("production results viewport");
                            assert!(screen.contains_rect(results) && results.is_positive());
                            let row_height =
                                recorded_rect(&app, &format!("browse_row:{index}")).height();
                            let group_height = recorded_rect(&app, "browse_group:0").height();
                            let spacing = app.editor_ctx.style().spacing.item_spacing.y;
                            assert!(
                                results.height() >= group_height + 2.0 * row_height + 2.0 * spacing,
                                "results must retain one complete group and two complete hits: {results:?}"
                            );
                            let code_font =
                                egui::TextStyle::Monospace.resolve(&app.editor_ctx.style());
                            let code_height = app.editor_ctx.fonts(|fonts| {
                                fonts
                                    .layout_no_wrap(
                                        "▶ 41 | needle line 41".into(),
                                        code_font,
                                        MUTED,
                                    )
                                    .size()
                                    .y
                            });
                            assert!(viewport.height() >= 3.0 * code_height,
                                "preview must retain three complete measured code lines: {viewport:?}, line={code_height}");
                            assert!(
                                content_size.y > results.height(),
                                "long results must really scroll"
                            );
                            frame_at(
                                &mut app,
                                size,
                                key(egui::Key::Escape, egui::Modifiers::NONE),
                            );
                            for _ in 0..3 {
                                frame_at(&mut app, size, vec![]);
                            }
                            assert!(!app.project_text_review_armed());
                            assert!(app.browse_preview_text().is_none());
                            for probe in controls {
                                visible(&app, probe);
                            }
                            visible(&app, &format!("browse_row:{index}"));
                            if index == 0 {
                                frame_at(
                                    &mut app,
                                    size,
                                    key(egui::Key::End, egui::Modifiers::NONE),
                                );
                                for _ in 0..3 {
                                    frame_at(&mut app, size, vec![]);
                                }
                            }
                        }
                    }
                    assert!(commands.try_recv().is_err());
                    assert_eq!(app.documents.len(), 1);
                    assert!(app.documents[0].dirty());
                }
            }
        }
    }
}

#[test]
fn scoped_find_complete_specification_is_required_for_retained_authority() {
    let (mut app, commands) = fixture();
    enter(&mut app, Surface::Bottom);
    accepted(&mut app, &commands, Surface::Bottom);
    let source = app.search_state.accepted.clone().unwrap();
    for part in 0..7 {
        let mut changed = source.clone();
        match part {
            0 => changed.specification.request.query.push('x'),
            1 => changed.specification.request.scope.push('x'),
            2 => changed.specification.request.masks.push("*.md".into()),
            3 => changed.specification.request.limit -= 1,
            4 => changed.specification.route = search_keyboard::Route::CaseV1,
            5 => changed.specification.route = search_keyboard::Route::Legacy,
            _ => changed.specification.case_sensitive = false,
        }
        assert_ne!(changed, source);
        app.search_state.accepted = Some(changed);
        assert!(
            !app.search_rows_active(),
            "altered specification part {part} retained authority"
        );
    }
    app.search_state.accepted = Some(source);
    assert!(app.search_rows_active());
    app.scoped_search = Some(ScopedSearchDescriptor { version: 2 });
    assert!(!app.search_rows_active());
    assert!(commands.try_recv().is_err());
}

#[test]
fn scoped_find_reconnect_rejects_late_success_error_and_requires_fresh_submit() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for failure in [false, true] {
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            accepted(&mut app, &commands, surface);
            let old = submit(&mut app, &commands, surface);
            let old_generation = app.generation;
            let source = app.search_state.accepted.clone();
            app.disconnected("scoped fixture reconnect".into());
            assert!(app.scoped_search.is_none());
            app.generation += 1;
            let (worker, reconnected) = Worker::recording();
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
                    scoped_search: Some(ScopedSearchDescriptor { version: 1 }),
                    case_search: None,
                }),
            );
            let list = reconnected.try_recv().unwrap();
            assert!(matches!(&list.op, Operation::List { path } if path.is_empty()));
            reply(&mut app, list.id, Ok(Payload::Entries { entries: vec![] }));
            settle(&mut app);
            app.apply_event(Event {
                generation: old_generation,
                id: old.id,
                connected: true,
                result: if failure {
                    Err("obsolete session error".into())
                } else {
                    Ok(payload(request("needle", "src", &["*.rs"]), rows()))
                },
            });
            settle(&mut app);
            assert_eq!(app.search_state.accepted, source);
            assert!(!app.search_rows_active());
            assert!(!app
                .error
                .as_deref()
                .is_some_and(|text| text.contains("obsolete session error")));
            if matches!(surface, Surface::Browse) {
                enter(&mut app, surface);
            }
            let fresh = submit(&mut app, &reconnected, surface);
            assert!(
                matches!(&fresh.op, Operation::SearchIn { request: actual } if *actual == request("needle", "src", &["*.rs"]))
            );
            reply(
                &mut app,
                fresh.id,
                Ok(payload(request("needle", "src", &["*.rs"]), rows())),
            );
            settle(&mut app);
            assert!(app.search_rows_active());
            assert!(recorded_response(&app, &surface.row(0)).enabled());
            assert_ne!(app.search_state.accepted, source);
            assert!(reconnected.try_recv().is_err());
        }
    }
}

#[test]
fn scoped_find_incomplete_empty_is_reported_separately_from_complete_no_match() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for incomplete in [false, true] {
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            let pending = scoped(&mut app, &commands, surface);
            reply(
                &mut app,
                pending.id,
                Ok(Payload::ScopedMatches {
                    result: ScopedSearchResult {
                        request: request("needle", "src", &["*.rs"]),
                        matches: vec![],
                        incomplete,
                        reasons: if incomplete {
                            vec![cedar_protocol::ScopedSearchIncompleteReason::ReadByteLimit]
                        } else {
                            vec![]
                        },
                        examined_entries: 7,
                        read_bytes: 1024,
                    },
                }),
            );
            settle(&mut app);
            match surface {
                Surface::Browse => {
                    let mut fields = vec!["7 entries examined", "1024 bytes read"];
                    if incomplete {
                        fields.extend([
                            "Incomplete: read byte limit",
                            "Search incomplete; more matching lines may exist.",
                        ]);
                    }
                    browse_assert_status_fields_reachable(
                        &mut app,
                        SIZE,
                        &fields,
                        if incomplete {
                            &[]
                        } else {
                            &["read byte limit", "Search incomplete;"]
                        },
                    );
                }
                Surface::Bottom => {
                    let output = frame(&mut app, vec![]);
                    assert!(has_text(&output, "7 entries examined"));
                    assert!(has_text(&output, "1024 bytes read"));
                    assert_eq!(has_text(&output, "read byte limit"), incomplete);
                }
            }
            assert_eq!(app.search_truncated, incomplete);
            assert!(app.search_results.is_empty());
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn scoped_find_long_valid_scope_keeps_historical_summary_and_actions_visible() {
    let scope = format!(
        "{}/{}",
        vec!["a".repeat(124); 31].join("/"),
        "b".repeat(125)
    );
    assert_eq!(scope.len(), 4000);
    let origin_path = format!("{scope}/origin.rs");
    assert!(cedar_protocol::valid_file_discovery_path(&origin_path));
    let specification = request("needle", &scope, &["*.rs"]);
    assert!(specification.validate().is_ok());
    let size = egui::vec2(780.0, 540.0);
    for surface in [Surface::Browse, Surface::Bottom] {
        for font in [13.0, 18.0] {
            let (mut app, commands) = fixture();
            app.documents[0].path = origin_path.clone();
            enter(&mut app, surface);
            replace(&mut app, surface.query(), "needle");
            replace(&mut app, surface.scope(), &scope);
            replace(&mut app, surface.masks(), "*.rs");
            assert!(commands.try_recv().is_err());
            let command = submit(&mut app, &commands, surface);
            assert!(
                matches!(&command.op, Operation::SearchIn { request } if request == &specification)
            );
            reply(
                &mut app,
                command.id,
                Ok(payload(
                    specification.clone(),
                    vec![SearchMatch {
                        path: origin_path.clone(),
                        line: 2,
                        text: "needle historical disk match".into(),
                    }],
                )),
            );
            settle(&mut app);
            app.tools_height = None;
            app.editor_ctx.style_mut(|style| {
                for kind in [egui::TextStyle::Body, egui::TextStyle::Button] {
                    style.text_styles.insert(kind, FontId::proportional(font));
                }
                style
                    .text_styles
                    .insert(egui::TextStyle::Monospace, FontId::monospace(font));
            });
            let history_probe = match surface {
                Surface::Browse => "browse_history",
                Surface::Bottom => "search_history",
            };
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for active in [true, false] {
                if !active {
                    app.search_masks = "*.md".into();
                    app.search_filters_changed();
                }
                for _ in 0..4 {
                    frame_at(&mut app, size, vec![]);
                }
                assert_eq!(
                    &app.search_state
                        .accepted
                        .as_ref()
                        .unwrap()
                        .specification
                        .request,
                    &specification,
                    "{surface:?}, font={font}: rendered summary changed the exact submitted tuple"
                );
                let history_font = match surface {
                    Surface::Browse => egui::TextStyle::Body,
                    Surface::Bottom => egui::TextStyle::Small,
                }
                .resolve(&app.editor_ctx.style());
                let single_line = app.editor_ctx.fonts(|fonts| {
                    fonts
                        .layout_no_wrap("Historical results".into(), history_font, MUTED)
                        .size()
                        .y
                });
                let history = recorded_rect(&app, history_probe);
                assert!(history.height() <= single_line + 1.0, "{surface:?}, font={font}, active={active}: long history wrapped to {} points instead of {single_line}", history.height());
                for probe in [
                    surface.query().to_owned(),
                    surface.scope().to_owned(),
                    surface.masks().to_owned(),
                    surface.submit().to_owned(),
                    history_probe.to_owned(),
                    surface.group(0),
                    surface.row(0),
                ] {
                    let rect = recorded_rect(&app, &probe);
                    let clip = recorded_clip(&app, &probe);
                    assert!(rect.is_finite() && rect.is_positive() && screen.contains_rect(rect) && clip.contains_rect(rect), "{surface:?}, font={font}, active={active}, {probe}: {rect:?} clipped by {clip:?}");
                }
                assert_eq!(recorded_response(&app, &surface.row(0)).enabled(), active);
                if matches!(surface, Surface::Browse) {
                    for probe in ["browse_preview", "browse_open", "browse_close"] {
                        let rect = recorded_rect(&app, probe);
                        let clip = recorded_clip(&app, probe);
                        assert!(rect.is_positive() && screen.contains_rect(rect) && clip.contains_rect(rect), "{surface:?}, font={font}, active={active}: {probe} outside visible bounds");
                    }
                    assert_eq!(recorded_response(&app, "browse_preview").enabled(), active);
                    assert_eq!(recorded_response(&app, "browse_open").enabled(), active);
                }
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn scoped_text_count_uses_accepted_line_indexes_and_exact_path_groups() {
    let (mut app, commands) = fixture();
    enter(&mut app, Surface::Browse);
    let command = scoped(&mut app, &commands, Surface::Browse);
    let specification = request("needle", "src", &["*.rs"]);
    // Two literal occurrences in a snippet are still one accepted matching line.
    // Scoped wire rows must be strictly ordered by path/line. Distinct indexes
    // retain their identities, and same-basename files are distinct path groups.
    // Identical legacy-row indexes remain covered by the retained Browse tests.
    let matches = vec![
        SearchMatch {
            path: "src/one/same.rs".into(),
            line: 2,
            text: "needle needle".into(),
        },
        SearchMatch {
            path: "src/one/same.rs".into(),
            line: 3,
            text: "needle needle".into(),
        },
        SearchMatch {
            path: "src/two/same.rs".into(),
            line: 1000,
            text: "needle".into(),
        },
    ];
    reply(
        &mut app,
        command.id,
        Ok(payload(specification.clone(), matches)),
    );
    frame(&mut app, vec![]);
    assert_eq!(
        app.search_state.accepted.as_ref().unwrap().request,
        command.id
    );
    let first = frame(&mut app, vec![]);
    assert!(has_text(&first, "3 matching lines in 2 files"));
    let column = |index: usize, text: &str| {
        let row = recorded_rect(&app, &format!("browse_row:{index}"));
        first
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(draw)
                    if draw.galley.text() == text
                        && (draw.pos.y + draw.galley.size().y / 2.0 - row.center().y).abs()
                            <= 1.0 =>
                {
                    Some(egui::Rect::from_min_size(draw.pos, draw.galley.size()))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing native painted column {index}/{text}"))
    };
    assert!(
        (column(0, "2").right() - column(2, "1000").right()).abs() <= 1.0,
        "accepted-batch line-number column must remain right-aligned"
    );
    assert!(
        (column(0, "needle needle").left() - column(2, "needle").left()).abs() <= 1.0,
        "snippet start must not shift with each row's number of digits"
    );
    settle(&mut app);
    assert_ne!(
        recorded_response(&app, "browse_row:0").id,
        recorded_response(&app, "browse_row:1").id
    );
    assert_ne!(
        recorded_response(&app, "browse_group:0").id,
        recorded_response(&app, "browse_group:2").id
    );
    assert_eq!(app.search_state.visible_indices(), vec![0, 1, 2]);
    click(&mut app, "browse_group:0");
    let collapsed = frame(&mut app, vec![]);
    assert_eq!(app.search_state.visible_indices(), vec![2]);
    assert!(
        has_text(&collapsed, "3 matching lines in 2 files"),
        "collapse cannot reduce accepted totals"
    );
    assert_eq!(app.search_results.len(), 3);
    assert_eq!(
        app.search_state
            .accepted
            .as_ref()
            .unwrap()
            .specification
            .request,
        specification
    );
    click(&mut app, "browse_group:0");
    settle(&mut app);
    assert_eq!(app.search_state.visible_indices(), vec![0, 1, 2]);
    assert!(has_text(
        &frame(&mut app, vec![]),
        "3 matching lines in 2 files"
    ));
    assert!(
        commands.try_recv().is_err(),
        "group presentation does not Search or Read"
    );
}

#[test]
fn scoped_text_count_distinguishes_unsubmitted_complete_empty_and_incomplete() {
    for empty in [false, true] {
        for incomplete in [false, true] {
            let (mut app, commands) = fixture();
            enter(&mut app, Surface::Browse);
            assert!(!has_text(&frame(&mut app, vec![]), "0 matching lines"));
            let command = scoped(&mut app, &commands, Surface::Browse);
            let pending = frame(&mut app, vec![]);
            assert!(has_text(&pending, "Searching"));
            assert!(!has_text(&pending, "No matching lines"));
            let matches = if empty { vec![] } else { rows() };
            let count = matches.len();
            reply(
                &mut app,
                command.id,
                Ok(Payload::ScopedMatches {
                    result: ScopedSearchResult {
                        request: request("needle", "src", &["*.rs"]),
                        matches,
                        incomplete,
                        reasons: if incomplete {
                            vec![cedar_protocol::ScopedSearchIncompleteReason::ReadByteLimit]
                        } else {
                            vec![]
                        },
                        examined_entries: 37,
                        read_bytes: 8192,
                    },
                }),
            );
            frame(&mut app, vec![]);
            assert_eq!(
                app.search_state.accepted.as_ref().unwrap().request,
                command.id
            );
            let first = frame(&mut app, vec![]);
            let expected = if incomplete {
                format!(
                    "{count} matching lines observed in {} files · incomplete",
                    if empty { 0 } else { 2 }
                )
            } else {
                format!(
                    "{count} matching lines in {} files",
                    if empty { 0 } else { 2 }
                )
            };
            assert!(
                has_text(&first, &expected),
                "first accepted count {expected}"
            );
            settle(&mut app);
            let settled = frame(&mut app, vec![]);
            assert!(has_text(&settled, &expected));
            let mut fields = vec!["37 entries examined", "8192 bytes read"];
            if incomplete {
                fields.extend([
                    "Incomplete: read byte limit",
                    "Search incomplete; more matching lines may exist.",
                ]);
            }
            for (size, font) in [
                (SIZE, 13.0),
                (egui::vec2(780.0, 540.0), 13.0),
                (egui::vec2(780.0, 540.0), 18.0),
            ] {
                app.editor_ctx.style_mut(|style| {
                    for kind in [egui::TextStyle::Body, egui::TextStyle::Button] {
                        style.text_styles.insert(kind, FontId::proportional(font));
                    }
                    style
                        .text_styles
                        .insert(egui::TextStyle::Monospace, FontId::monospace(font + 1.0));
                });
                for _ in 0..4 {
                    frame_at(&mut app, size, vec![]);
                }
                browse_assert_status_fields_reachable(
                    &mut app,
                    size,
                    &fields,
                    if incomplete {
                        &[]
                    } else {
                        &["read byte limit", "Search incomplete;"]
                    },
                );
                assert_eq!(app.search_truncated, incomplete);
            }
            if empty {
                assert_eq!(
                    has_text(&settled, "No matching lines in the searched scope."),
                    !incomplete,
                    "only a completed empty scan may claim no matching lines"
                );
            }
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn scoped_text_retired_flight_keeps_previous_count_without_searching_new_tuple() {
    for failed in [false, true] {
        let (mut app, commands) = fixture();
        enter(&mut app, Surface::Browse);
        accepted(&mut app, &commands, Surface::Browse);
        let accepted = app.search_state.accepted.clone();
        replace(&mut app, "browse_query", "pending tuple");
        let pending = submit(&mut app, &commands, Surface::Browse);
        assert!(
            matches!(&pending.op, Operation::SearchIn { request } if request.query == "pending tuple")
        );
        replace(&mut app, "browse_query", "edited newer tuple");
        assert!(app.search_busy());
        assert!(app.search_state.pending.is_none());
        let waiting = frame(&mut app, vec![]);
        assert!(has_text(&waiting, "Waiting for previous search to finish"));
        assert!(!has_text(&waiting, "Searching…"));
        assert!(has_text(&waiting, "3 matching lines in 2 files"));
        assert!(has_text(&waiting, "Historical results for \"needle\""));
        assert!(!has_text(
            &waiting,
            "Historical results for \"edited newer tuple\""
        ));
        assert!(!recorded_response(&app, "browse_submit").enabled());
        assert!(!recorded_response(&app, "browse_row:0").enabled());
        assert_eq!(app.search_state.accepted, accepted);
        reply(
            &mut app,
            pending.id,
            if failed {
                Err("retired tuple failed".into())
            } else {
                Ok(payload(request("pending tuple", "src", &["*.rs"]), vec![]))
            },
        );
        let retired = frame(&mut app, vec![]);
        assert!(has_text(&retired, "3 matching lines in 2 files"));
        assert!(!has_text(&retired, "0 matching lines"));
        assert!(!has_text(&retired, "retired tuple failed"));
        assert_eq!(app.search_state.accepted, accepted);
        assert!(!app.search_rows_active());
        replace(&mut app, "browse_query", "needle");
        assert!(
            !app.search_rows_active(),
            "editing back cannot revive accepted authority"
        );
        assert!(has_text(
            &frame(&mut app, vec![]),
            "3 matching lines in 2 files"
        ));
        assert!(
            commands.try_recv().is_err(),
            "retired flight does not schedule a replacement"
        );
    }
}
