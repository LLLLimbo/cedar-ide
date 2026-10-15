//! The read-only Project context runs in the actual raw-input/update frame.
//! The observer copies values only: no Response/Context retention or UI changes.
use super::*;
use explorer_tree::Mode;

const PROBE: &str = "project_context_test_probe";
const NORMAL: [f32; 2] = [1178.0, 814.0];

#[derive(Clone)]
struct Observation {
    text: String,
    id: egui::Id,
    rect: egui::Rect,
    clip: egui::Rect,
    viewport: egui::Rect,
    sense: egui::Sense,
    changed: bool,
    #[cfg(debug_assertions)]
    info: Option<egui::WidgetInfo>,
}

pub(super) fn record(ui: &egui::Ui, response: &egui::Response, viewport: egui::Rect, text: &str) {
    let observation = Observation {
        text: text.into(),
        id: response.id,
        rect: response.rect,
        clip: ui.clip_rect(),
        viewport,
        sense: response.sense,
        changed: response.changed(),
        #[cfg(debug_assertions)]
        info: ui
            .ctx()
            .viewport(|viewport| viewport.this_pass.widgets.info(response.id).cloned()),
    };
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new(PROBE), observation));
}

fn observed(app: &CedarApp) -> Observation {
    app.editor_ctx
        .data(|data| data.get_temp(egui::Id::new(PROBE)).unwrap())
}

pub(super) fn assert_recorded(app: &CedarApp, expected: &str) {
    assert_eq!(observed(app).text, expected);
}

fn fixture(width: f32, font: f32) -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    app.state = ConnectionState::Ready;
    app.open_form = false;
    app.tools_open = false;
    let form = ConnectForm {
        local_root: "/workspace/project".into(),
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/workspace/project".into();
    app.editor_ctx.set_fonts(system_fonts::base_fonts());
    app.editor_ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        for text in [egui::TextStyle::Body, egui::TextStyle::Button] {
            style.text_styles.insert(text, FontId::proportional(font));
        }
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(14.0));
        // Observe native WidgetInfo rather than inventing accessibility events.
        #[cfg(debug_assertions)]
        {
            style.debug.show_interactive_widgets = true;
        }
    });
    set_width(&app, width);
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn set_width(app: &CedarApp, width: f32) {
    app.editor_ctx.data_mut(|data| {
        data.insert_persisted(
            egui::Id::new("explorer"),
            egui::containers::panel::PanelState {
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 540.0)),
            },
        )
    });
}

fn frame(app: &mut CedarApp, size: [f32; 2], events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
        time: Some(ctx.cumulative_pass_nr() as f64 * 0.25),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native))
}

fn settle(app: &mut CedarApp, size: [f32; 2]) -> egui::FullOutput {
    for _ in 0..3 {
        frame(app, size, vec![]);
    }
    frame(app, size, vec![])
}

fn key(app: &mut CedarApp, key: egui::Key, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        frame(
            app,
            NORMAL,
            vec![egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            }],
        );
    }
}

fn texts(output: &egui::FullOutput) -> Vec<&egui::epaint::TextShape> {
    fn visit<'a>(shape: &'a egui::Shape, out: &mut Vec<&'a egui::epaint::TextShape>) {
        match shape {
            egui::Shape::Text(text) => out.push(text),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for shape in &output.shapes {
        visit(&shape.shape, &mut out);
    }
    out
}

fn assert_context(app: &CedarApp, output: &egui::FullOutput, expected: &str) {
    let o = observed(app);
    assert_eq!(o.text, expected);
    assert!(
        o.clip.contains_rect(o.rect),
        "context clipped: {:?} / {:?}",
        o.rect,
        o.clip
    );
    assert_eq!(o.sense, egui::Sense::hover());
    assert!(!o.changed);
    #[cfg(debug_assertions)]
    {
        let info = o.info.unwrap();
        assert_eq!(info.typ, egui::WidgetType::Label);
        assert_eq!(info.label.as_deref(), Some(expected));
    }
    let text = texts(output)
        .into_iter()
        .find(|text| text.galley.text() == expected)
        .expect("actual visible frame must paint the production context");
    assert_eq!(text.galley.rows.len(), 1);
    assert!(o.viewport.contains_rect(o.rect));
    // Font ink may exceed advance/allocation bounds (root '/' extends 0.2pt).
    // Require exact real-viewport and paint-clip containment, without tolerance,
    // plus separation from the actual toolbar and file viewport.
    let ink = text.visual_bounding_rect();
    assert!(o.viewport.contains_rect(ink));
    assert!(o.clip.contains_rect(ink));
    let heading = workspace_access_tests::recorded_rect(app, "project_heading");
    let explorer = sidebar_layout_tests::explorer_geometry(app).0;
    assert!(heading.bottom() <= ink.top());
    assert!(ink.bottom() <= explorer.top());
    eprintln!("CONTEXT_PAINT text={expected:?} allocation={:?} ink={ink:?} viewport={:?} clip={:?} row={:?} heading={heading:?} explorer={explorer:?}", o.rect, o.viewport, o.clip, text.galley.rows[0].rect);
    let prefix = expected.split('/').next().unwrap();
    let painted: String = text.galley.rows[0].glyphs.iter().map(|g| g.chr).collect();
    assert!(
        painted.starts_with(prefix),
        "prefix lost: {painted:?} / {prefix:?}"
    );
}

fn ledger(app: &CedarApp) -> String {
    format!(
        "{:?}",
        (
            app.generation,
            app.next_request,
            app.directory_request,
            app.pending.len(),
            &app.explorer.outstanding,
            app.explorer.mode_epoch,
            app.explorer.epoch,
            &app.directory,
            &app.entries,
            &app.explorer.selected,
            app.active_document,
            app.navigation_epoch
        )
    )
}

fn entry(path: &str, is_dir: bool) -> Entry {
    Entry {
        name: path.rsplit('/').next().unwrap().into(),
        path: path.into(),
        is_dir,
    }
}

fn listed(app: &mut CedarApp, commands: &Receiver<Command>, path: &str, entries: Vec<Entry>) {
    let command = commands.try_recv().unwrap();
    assert!(matches!(&command.op, Operation::List { path: actual } if actual == path));
    app.apply_worker_event(WorkerEvent::Response(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::Entries { entries }),
    }));
}

#[test]
fn context_first_visible_and_settled_geometry_uses_production_fonts() {
    let scopes = [
        String::new(),
        "other".into(),
        format!("中文/{}同名", "深层/".repeat(20)),
    ];
    for size in [[780.0, 540.0], NORMAL] {
        for width in [180.0, 246.0, 460.0] {
            for font in [13.0, 18.0] {
                for scope in &scopes {
                    for mode in [Mode::Flat, Mode::Tree] {
                        let (mut app, commands) = fixture(width, font);
                        if mode == Mode::Flat {
                            app.list(scope.clone());
                            listed(&mut app, &commands, scope, vec![]);
                        } else {
                            app.explorer_set_mode(mode);
                            let mut parent = String::new();
                            for segment in scope.split('/').filter(|s| !s.is_empty()) {
                                let child = if parent.is_empty() {
                                    segment.into()
                                } else {
                                    format!("{parent}/{segment}")
                                };
                                app.explorer_expand(&parent);
                                listed(&mut app, &commands, &parent, vec![entry(&child, true)]);
                                parent = child;
                            }
                            app.explorer_select(&parent);
                        }
                        let before = ledger(&app);
                        let expected = format!("{mode:?} · Scope: /{scope}");
                        let first = frame(&mut app, size, vec![]);
                        assert_context(&app, &first, &expected);
                        let first_rect = observed(&app).rect;
                        let output = settle(&mut app, size);
                        assert_context(&app, &output, &expected);
                        let settled_rect = observed(&app).rect;
                        // Native scrollbars may release their initially reserved
                        // width. The row's position/height must stay fixed.
                        assert_eq!(first_rect.min, settled_rect.min);
                        assert_eq!(first_rect.height(), settled_rect.height());
                        for _ in 0..3 {
                            let output = frame(&mut app, size, vec![]);
                            assert_context(&app, &output, &expected);
                            assert_eq!(settled_rect, observed(&app).rect);
                        }
                        assert_eq!(before, ledger(&app));
                        assert!(commands.try_recv().is_err());
                    }
                }
            }
        }
    }
}

#[test]
fn context_root_empty_unloaded_and_connection_states_remain_distinct() {
    let (mut app, commands) = fixture(246.0, 13.0);
    assert_eq!(app.explorer.mode, Mode::Flat);
    let output = settle(&mut app, NORMAL);
    assert_context(&app, &output, "Flat · Scope: /");
    assert!(!app.explorer_scope_loaded());
    assert!(texts(&output)
        .iter()
        .any(|t| t.galley.text().contains("Directory not loaded")));
    app.list(String::new());
    listed(&mut app, &commands, "", vec![]);
    let output = settle(&mut app, NORMAL);
    assert_context(&app, &output, "Flat · Scope: /");
    assert!(texts(&output)
        .iter()
        .any(|t| t.galley.text() == "This directory is empty"));
    for (state, text) in [
        (ConnectionState::Connecting, "Connecting…"),
        (ConnectionState::Disconnecting, "Disconnecting…"),
        (ConnectionState::Idle, "Not connected"),
        (ConnectionState::Disconnected, "Not connected"),
        (
            ConnectionState::CleanupUnverified,
            "Connection cleanup unverified",
        ),
    ] {
        app.state = state;
        let output = settle(&mut app, NORMAL);
        let o = observed(&app);
        assert_eq!(
            Some(o.text.as_str()),
            Some(format!("Flat · {text}").as_str())
        );
        assert!(texts(&output)
            .iter()
            .any(|t| t.galley.text() == format!("Flat · {text}")));
    }
    assert!(commands.try_recv().is_err());
}

#[test]
fn context_flat_loading_and_failed_navigation_keep_accepted_scope() {
    let (mut app, commands) = fixture(246.0, 13.0);
    app.list("other".into());
    listed(
        &mut app,
        &commands,
        "other",
        vec![entry("other/same.txt", false)],
    );
    app.list("origin".into());
    let command = commands.try_recv().unwrap();
    let before = ledger(&app);
    let output = settle(&mut app, NORMAL);
    assert_context(&app, &output, "Flat · Scope: /other");
    assert!(texts(&output)
        .iter()
        .any(|t| t.galley.text() == "Loading one directory…"));
    assert_eq!(before, ledger(&app));
    app.apply_worker_event(WorkerEvent::Response(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Err("denied fixture".into()),
    }));
    let output = settle(&mut app, NORMAL);
    assert_context(&app, &output, "Flat · Scope: /other");
    assert!(texts(&output)
        .iter()
        .any(|t| t.galley.text() == "Retry directory: /origin"));
    let retry = workspace_access_tests::recorded_rect(&app, "explorer_status_retry");
    assert!(
        workspace_access_tests::recorded_clip(&app, "explorer_status_retry").contains_rect(retry)
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn context_tree_loading_and_stale_error_keep_selected_scope_and_retry_visible() {
    let (mut app, commands) = fixture(180.0, 18.0);
    app.explorer_set_mode(Mode::Tree);
    app.explorer_expand("");
    listed(&mut app, &commands, "", vec![entry("other", true)]);
    app.explorer_expand("other");
    listed(
        &mut app,
        &commands,
        "other",
        vec![entry("other/same.txt", false)],
    );
    app.explorer_select("other/same.txt");
    app.explorer_refresh("other");
    let command = commands.try_recv().unwrap();
    assert!(matches!(&command.op, Operation::List { path } if path == "other"));
    let before = ledger(&app);
    let output = settle(&mut app, [780.0, 540.0]);
    assert_context(&app, &output, "Tree · Scope: /other");
    assert!(texts(&output)
        .iter()
        .any(|t| t.galley.text() == "Loading one directory…"));
    assert_eq!(before, ledger(&app));
    app.apply_worker_event(WorkerEvent::Response(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Err("denied fixture".into()),
    }));
    let output = settle(&mut app, [780.0, 540.0]);
    assert_context(&app, &output, "Tree · Scope: /other");
    let row = app
        .explorer_visible_rows()
        .into_iter()
        .find(|row| row.entry.path == "other")
        .unwrap();
    assert!(row.stale && row.error.is_some());
    let retry = workspace_access_tests::recorded_rect(&app, "tree_retry:other");
    assert!(workspace_access_tests::recorded_clip(&app, "tree_retry:other").contains_rect(retry));
    assert!(observed(&app).viewport.contains_rect(retry));
    assert!(commands.try_recv().is_err());
}

#[test]
fn context_tree_follows_selected_directory_or_file_parent_and_collapse() {
    let (mut app, commands) = fixture(246.0, 13.0);
    app.explorer_set_mode(Mode::Tree);
    assert!(commands.try_recv().is_err(), "switch itself must not load");
    let output = settle(&mut app, NORMAL);
    assert_context(&app, &output, "Tree · Scope: /");
    app.explorer_expand("");
    listed(
        &mut app,
        &commands,
        "",
        vec![entry("origin", true), entry("other", true)],
    );
    app.explorer_expand("origin");
    listed(
        &mut app,
        &commands,
        "origin",
        vec![entry("origin/层", true), entry("origin/same.txt", false)],
    );
    app.explorer_expand("other");
    listed(
        &mut app,
        &commands,
        "other",
        vec![entry("other/same.txt", false)],
    );
    app.explorer_expand("origin/层");
    listed(
        &mut app,
        &commands,
        "origin/层",
        vec![entry("origin/层/child.txt", false)],
    );
    for (selected, expected) in [
        ("origin", "origin"),
        ("origin/same.txt", "origin"),
        ("other/same.txt", "other"),
        ("origin/层/child.txt", "origin/层"),
    ] {
        app.explorer_select(selected);
        let before = ledger(&app);
        let output = settle(&mut app, NORMAL);
        assert_context(&app, &output, &format!("Tree · Scope: /{expected}"));
        assert_eq!(before, ledger(&app));
        assert!(commands.try_recv().is_err());
    }
    app.explorer_collapse("origin");
    let output = settle(&mut app, NORMAL);
    assert_context(&app, &output, "Tree · Scope: /origin");
    assert!(app
        .explorer_visible_rows()
        .iter()
        .any(|row| row.entry.path == "other/same.txt"));
    assert!(commands.try_recv().is_err());
}

#[test]
fn context_full_hover_and_native_label_preserve_original_scope_and_workspace() {
    let (mut app, commands) = fixture(180.0, 18.0);
    app.directory = format!("同名/{}/终点", "完整目录/".repeat(12));
    let expected = format!("Flat · Scope: /{}", app.directory);
    let output = settle(&mut app, NORMAL);
    assert_context(&app, &output, &expected);
    assert!(texts(&output)
        .iter()
        .any(|t| t.galley.text() == expected && t.galley.elided));
    app.editor_ctx
        .memory_mut(|memory| memory.options.screen_reader = true);
    let before = ledger(&app);
    let position = observed(&app).rect.center();
    frame(&mut app, NORMAL, vec![egui::Event::PointerMoved(position)]);
    let output = settle(&mut app, NORMAL);
    assert_context(&app, &output, &expected);
    let hover = format!(
        "Workspace: {}\nProject directory scope: /{}",
        app.root, app.directory
    );
    assert!(
        texts(&output)
            .iter()
            .any(|text| text.galley.text() == hover),
        "real tooltip missing"
    );
    assert_eq!(before, ledger(&app));
    assert!(commands.try_recv().is_err());
}

fn draft_witness(app: &CedarApp) -> (u64, String) {
    let doc = &app.documents[0];
    let state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", doc.id))).unwrap();
    let selection = state.cursor.char_range().unwrap();
    let mut branches: [Vec<_>; 2] = Default::default();
    for (index, redo) in [false, true].into_iter().enumerate() {
        let mut undoer = state.undoer();
        let mut current = (selection, doc.text.clone());
        for _ in 0..=editor_state::MAX_UNDO_STATES + 1 {
            let next = if redo {
                undoer.redo(&current)
            } else {
                undoer.undo(&current)
            }
            .cloned();
            let Some(next) = next else { break };
            branches[index].push(next.clone());
            current = next;
        }
        assert!(!branches[index].is_empty());
    }
    (
        doc.edit_version,
        format!(
            "{:?}",
            (
                &doc.text,
                &doc.saved_text,
                &doc.revision,
                doc.cursor,
                selection,
                branches
            )
        ),
    )
}

#[test]
fn context_hover_repaint_resize_preserve_editor_focus_dirty_selection_and_undo() {
    let (mut app, commands) = fixture(246.0, 13.0);
    app.directory = "other".into();
    app.documents.push(Document::new(
        1,
        "origin/same.txt".into(),
        "saved draft".into(),
        "r1".into(),
    ));
    app.active_document = Some(1);
    settle(&mut app, NORMAL);
    let editor = egui::Id::new(("editor", 1_u64));
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(editor));
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        "dirty draft".into(),
        3,
    );
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        "later draft".into(),
        4,
    );
    key(&mut app, egui::Key::Z, egui::Modifiers::COMMAND);
    settle(&mut app, NORMAL);
    let before = ledger(&app);
    let dirty = draft_witness(&app);
    let position = observed(&app).rect.center();
    frame(&mut app, NORMAL, vec![egui::Event::PointerMoved(position)]);
    for (width, size) in [(180.0, [780.0, 540.0]), (460.0, NORMAL), (246.0, NORMAL)] {
        set_width(&app, width);
        let output = settle(&mut app, size);
        assert_context(&app, &output, "Flat · Scope: /other");
        assert_eq!(app.active().unwrap().path, "origin/same.txt");
        assert_eq!(
            app.editor_ctx.memory(|memory| memory.focused()),
            Some(editor)
        );
        assert_eq!(dirty, draft_witness(&app));
        assert_eq!(before, ledger(&app));
        assert!(commands.try_recv().is_err());
    }
    key(
        &mut app,
        egui::Key::Z,
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
    );
    assert_eq!(app.active().unwrap().text, "later draft");
    key(&mut app, egui::Key::Z, egui::Modifiers::COMMAND);
    assert_eq!((dirty.0 + 2, dirty.1), draft_witness(&app));
}

fn assert_single_full_scope_tooltip(mode: Mode) {
    for size in [[780.0, 540.0], NORMAL] {
        for width in [180.0, 460.0] {
            for font in [13.0, 18.0] {
                for scope in [
                    String::new(),
                    format!("同名/{}终点", "完整目录/".repeat(12)),
                ] {
                    let (mut app, commands) = fixture(width, font);
                    if mode == Mode::Flat {
                        app.list(scope.clone());
                        listed(&mut app, &commands, &scope, vec![]);
                    } else {
                        app.explorer_set_mode(mode);
                        let mut parent = String::new();
                        for segment in scope.split('/').filter(|s| !s.is_empty()) {
                            let child = if parent.is_empty() {
                                segment.into()
                            } else {
                                format!("{parent}/{segment}")
                            };
                            app.explorer_expand(&parent);
                            listed(&mut app, &commands, &parent, vec![entry(&child, true)]);
                            parent = child;
                        }
                        app.explorer_select(&parent);
                    }
                    let expected = format!("{mode:?} · Scope: /{scope}");
                    let output = settle(&mut app, size);
                    assert_context(&app, &output, &expected);
                    if !scope.is_empty() {
                        assert!(texts(&output)
                            .iter()
                            .any(|text| text.galley.text() == expected && text.galley.elided));
                    }
                    let before = ledger(&app);
                    let position = observed(&app).rect.center();
                    frame(&mut app, size, vec![egui::Event::PointerMoved(position)]);
                    let output = settle(&mut app, size);
                    assert_context(&app, &output, &expected);
                    let hover =
                        format!("Workspace: {}\nProject directory scope: /{scope}", app.root);
                    assert_eq!(
                        texts(&output)
                            .iter()
                            .filter(|text| text.galley.text() == hover)
                            .count(),
                        1,
                        "exactly one full workspace/scope tooltip is required"
                    );
                    assert_eq!(
                        texts(&output).iter().filter(|text| text.galley.text() == expected).count(),
                        1,
                        "only the sidebar body may paint the context text; no duplicate automatic tooltip"
                    );
                    assert_eq!(before, ledger(&app));
                    assert!(commands.try_recv().is_err());
                }
            }
        }
    }
}

#[test]
fn context_flat_full_scope_hover_has_one_tooltip() {
    assert_single_full_scope_tooltip(Mode::Flat);
}

#[test]
fn context_tree_full_scope_hover_has_one_tooltip() {
    assert_single_full_scope_tooltip(Mode::Tree);
}

#[derive(Clone, Copy, Debug)]
struct LabelParityCase {
    width: f32,
    font: f32,
    enabled: bool,
    screen_reader: bool,
    selectable_labels: bool,
    emphasis: u8,
    clipped: bool,
}

#[derive(Debug, PartialEq)]
struct LabelParityFrame {
    id: egui::Id,
    rect: egui::Rect,
    interact_rect: egui::Rect,
    sense: egui::Sense,
    enabled: bool,
    changed: bool,
    has_focus: bool,
    gained_focus: bool,
    highlighted: bool,
    following_rect: egui::Rect,
    #[cfg(debug_assertions)]
    info: Option<egui::WidgetInfo>,
    paint: Vec<(egui::Rect, egui::epaint::TextShape)>,
    events: Vec<egui::output::OutputEvent>,
}

fn label_parity_frames(case: LabelParityCase, native: bool) -> Vec<LabelParityFrame> {
    let (app, _) = fixture(case.width, case.font);
    let ctx = &app.editor_ctx;
    ctx.style_mut(|style| {
        style.interaction.selectable_labels = case.selectable_labels;
        // The same explicit foreground override used by CedarApp::new.
        style.visuals.override_text_color = Some(TEXT);
    });
    ctx.memory_mut(|memory| memory.options.screen_reader = case.screen_reader);
    let text = format!("Tree · Scope: /同名/{}终点", "完整目录/".repeat(12));
    let mut id = None;
    let mut frames = vec![];
    for index in 0..3 {
        let mut observed = None;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, NORMAL.into())),
                time: Some(index as f64 * 0.25),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_max_width(case.width);
                    if !case.enabled {
                        ui.disable();
                    }
                    if case.clipped {
                        ui.set_clip_rect(egui::Rect::NOTHING);
                    }
                    if case.emphasis == 2 {
                        if let Some(id) = id {
                            ui.memory_mut(|memory| memory.request_focus(id));
                        }
                    }
                    let response = if native {
                        ui.add(
                            egui::Label::new(RichText::new(&text).color(MUTED))
                                .truncate()
                                .selectable(false)
                                .sense(egui::Sense::hover()),
                        )
                    } else {
                        CedarApp::project_context_label(ui, &text, true)
                    };
                    id = Some(response.id);
                    if case.emphasis == 1 {
                        // Queue inside the current pass for the next real frame.
                        ui.ctx().highlight_widget(response.id);
                    }
                    #[cfg(debug_assertions)]
                    let info = ui
                        .ctx()
                        .viewport(|viewport| viewport.this_pass.widgets.info(response.id).cloned());
                    let following_rect = ui
                        .allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover())
                        .0;
                    observed = Some(LabelParityFrame {
                        id: response.id,
                        rect: response.rect,
                        interact_rect: response.interact_rect,
                        sense: response.sense,
                        enabled: response.enabled(),
                        changed: response.changed(),
                        has_focus: response.has_focus(),
                        gained_focus: response.gained_focus(),
                        highlighted: response.highlighted(),
                        following_rect,
                        #[cfg(debug_assertions)]
                        info,
                        paint: vec![],
                        events: vec![],
                    });
                });
            },
        );
        let mut observed = observed.unwrap();
        observed.paint = output
            .shapes
            .into_iter()
            .filter_map(|shape| match shape.shape {
                egui::Shape::Text(paint) if paint.galley.text() == text => {
                    Some((shape.clip_rect, paint))
                }
                _ => None,
            })
            .collect();
        observed.events = output.platform_output.events;
        frames.push(observed);
    }
    frames
}

#[test]
fn context_label_matches_native_layout_paint_focus_and_widget_events() {
    let mut cases = 0;
    let mut event_count = 0;
    let mut highlighted_frames = 0;
    for width in [180.0, 460.0] {
        for font in [13.0, 18.0] {
            for enabled in [false, true] {
                for screen_reader in [false, true] {
                    for selectable_labels in [false, true] {
                        for emphasis in 0..3 {
                            for clipped in [false, true] {
                                let case = LabelParityCase {
                                    width,
                                    font,
                                    enabled,
                                    screen_reader,
                                    selectable_labels,
                                    emphasis,
                                    clipped,
                                };
                                let native = label_parity_frames(case, true);
                                let context = label_parity_frames(case, false);
                                assert_eq!(context, native, "native label parity: {case:?}");
                                for frame in &context {
                                    assert_eq!(frame.sense, egui::Sense::hover());
                                    assert!(!frame.changed);
                                    // Native allocation surrenders forced focus for a
                                    // hover-only label; it must not emit focus events.
                                    assert!(!frame.has_focus);
                                    assert!(!frame.gained_focus);
                                    assert!(frame.events.is_empty());
                                    #[cfg(debug_assertions)]
                                    {
                                        let info = frame.info.as_ref().unwrap();
                                        assert_eq!(info.typ, egui::WidgetType::Label);
                                        assert_eq!(info.enabled, enabled);
                                    }
                                    assert_eq!(frame.paint.len(), usize::from(!clipped));
                                    event_count += frame.events.len();
                                    if frame.highlighted && !frame.paint.is_empty() {
                                        assert!(frame.paint.iter().all(|(_, paint)| {
                                            paint.underline != egui::Stroke::NONE
                                        }));
                                        highlighted_frames += 1;
                                    }
                                }
                                cases += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 192);
    assert_eq!(event_count, 0);
    assert!(
        highlighted_frames > 0,
        "the native underline branch must be exercised"
    );
    eprintln!(
        "CONTEXT_NATIVE_PARITY cases={cases} frames={} events={event_count} highlighted_frames={highlighted_frames}",
        cases * 3
    );
}

#[test]
fn context_is_inert_and_stable_across_scope_mode_and_native_tabs() {
    let (mut app, commands) = fixture(246.0, 13.0);
    settle(&mut app, NORMAL);
    let context_id = observed(&app).id;
    let heading = workspace_access_tests::recorded_response(&app, "project_heading").id;
    let refresh = workspace_access_tests::recorded_response(&app, "explorer_refresh").id;
    for (mode, scope) in [
        (Mode::Flat, "other"),
        (Mode::Tree, ""),
        (Mode::Flat, "origin"),
    ] {
        app.explorer_set_mode(mode);
        app.directory = scope.into();
        settle(&mut app, NORMAL);
        assert_eq!(observed(&app).id, context_id);
        assert_eq!(
            workspace_access_tests::recorded_response(&app, "project_heading").id,
            heading
        );
        assert_eq!(
            workspace_access_tests::recorded_response(&app, "explorer_refresh").id,
            refresh
        );
        assert!(!observed(&app).sense.is_focusable());
    }
    assert!(
        commands.try_recv().is_err(),
        "mode changes and frames must not load"
    );
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(heading));
    let before = ledger(&app);
    for _ in 0..16 {
        key(&mut app, egui::Key::Tab, egui::Modifiers::NONE);
        assert_ne!(
            app.editor_ctx.memory(|memory| memory.focused()),
            Some(context_id)
        );
    }
    assert_eq!(before, ledger(&app));
    assert!(commands.try_recv().is_err());
}
