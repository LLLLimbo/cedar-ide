//! Deterministic native-Close regression tests. No Worker thread, recovery actor,
//! peer process, filesystem recovery store, or host font probe is started here.
//! Mailbox disconnection proves retirement only; locally typed receipts are the
//! sole synthetic ownership evidence. All close assertions use complete frames.
use super::*;
use crate::close_transport::{OwnerOutcome, OwnerReceipt, OwnerTicket};
use crate::recovery::ClosePhase;
use cedar_recovery::{Draft, WorkspaceIdentity};
use std::time::Duration;

const ROOT: &str = "/synthetic-close-barrier";
const BASE: &str = "saved text\n";
const DRAFT: &str = "retained draft 草稿\n";

struct Harness {
    app: CedarApp,
    // Keep the sender alive across every frame, including multipass rendering.
    _hold_font_probe: Box<dyn FnOnce()>,
}

impl Harness {
    fn new() -> Self {
        let mut app = CedarApp::empty();
        let (fonts, hold_font_probe) = system_fonts::SystemFonts::pending_failure_for_test();
        app.system_fonts = fonts;
        app.generation = 7;
        app.form = form();
        app.active_form = Some(form());
        app.workspace_key = Some(form().key());
        app.root = ROOT.into();
        app.open_form = false;
        app.state = ConnectionState::Disconnected;
        app.documents.push(Document::new(
            1,
            "draft.txt".into(),
            BASE.into(),
            "r0".into(),
        ));
        app.active_document = Some(1);
        app.next_document = 2;
        // Recovery is explicitly disabled and never started. Its ordinary
        // no-actor quiescence still creates and confirms the real close ticket.
        assert!(!app.recovery.enabled);
        Self {
            app,
            _hold_font_probe: Box::new(hold_font_probe),
        }
    }

    fn frame(&mut self, close: bool, events: Vec<egui::Event>) -> egui::FullOutput {
        self.frame_inspect(close, events, |_, _| {})
    }

    fn frame_inspect(
        &mut self,
        close: bool,
        events: Vec<egui::Event>,
        mut inspect: impl FnMut(&CedarApp, &egui::Context),
    ) -> egui::FullOutput {
        let ctx = self.app.editor_ctx.clone();
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 960.0),
            )),
            time: Some(ctx.cumulative_pass_nr() as f64 * 0.25),
            events,
            ..Default::default()
        };
        if close {
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .events
                .push(egui::ViewportEvent::Close);
        }
        eframe::App::raw_input_hook(&mut self.app, &ctx, &mut input);
        let mut native = eframe::Frame::_new_kittest();
        ctx.run(input, |ctx| {
            eframe::App::update(&mut self.app, ctx, &mut native);
            inspect(&self.app, ctx);
        })
    }

    fn warm(&mut self) {
        for _ in 0..3 {
            let output = self.frame(false, vec![]);
            assert_no_close(&self.app, &output);
        }
    }

    fn receipt(&self, ticket: OwnerTicket, outcome: OwnerOutcome) {
        self.app
            .result_tx
            .send(WorkerEvent::OwnerSettled(OwnerReceipt { ticket, outcome }))
            .unwrap();
    }
}

fn form() -> ConnectForm {
    ConnectForm {
        local_root: ROOT.into(),
        allow_run: false,
        ..Default::default()
    }
}

fn attach_recording(app: &mut CedarApp) -> (OwnerTicket, Receiver<Command>) {
    assert!(app.worker.is_none() && app.active_owner.is_none());
    let ticket = app.owner_ledger.register(app.generation).unwrap();
    let (worker, commands) = Worker::recording_with_owner(ticket);
    app.worker = Some(worker);
    app.active_owner = Some(ticket);
    app.state = ConnectionState::Ready;
    (ticket, commands)
}

fn connected() -> (Harness, OwnerTicket, Receiver<Command>) {
    let mut harness = Harness::new();
    let (ticket, commands) = attach_recording(&mut harness.app);
    harness.warm();
    (harness, ticket, commands)
}

fn close_count(output: &egui::FullOutput) -> usize {
    output.viewport_output[&egui::ViewportId::ROOT]
        .commands
        .iter()
        .filter(|command| **command == egui::ViewportCommand::Close)
        .count()
}

fn assert_no_close(app: &CedarApp, output: &egui::FullOutput) {
    assert!(!app.allow_close);
    assert_eq!(close_count(output), 0);
}

fn assert_waiting(app: &CedarApp, output: &egui::FullOutput) {
    assert_no_close(app, output);
    assert!(app.state == ConnectionState::ClosingTransport);
    assert!(app.transport_close.is_some());
    assert!(app.recovery.visible);
    assert!(output.viewport_output[&egui::ViewportId::ROOT]
        .commands
        .contains(&egui::ViewportCommand::CancelClose));
}

fn assert_retired(commands: &Receiver<Command>) {
    assert!(
        matches!(commands.try_recv(), Err(mpsc::TryRecvError::Disconnected)),
        "recording mailbox must be retired, without fabricating a receipt"
    );
}

fn assert_live(commands: &Receiver<Command>) {
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}

fn confirmed(app: &CedarApp) -> (recovery::CloseGuard, recovery_actor::Ticket) {
    let close = app.recovery.closing.as_ref().unwrap();
    let ClosePhase::Confirmed { ticket } = close.phase else {
        panic!("recovery must have confirmed its exact ticket");
    };
    (close.guard.clone(), ticket)
}

#[derive(Debug, PartialEq, Eq)]
struct Buffer {
    id: u64,
    path: String,
    text: String,
    saved_text: String,
    revision: Option<String>,
    edit_version: u64,
    undo_initialized: bool,
}

fn buffer(app: &CedarApp) -> Buffer {
    assert_eq!(app.documents.len(), 1);
    let doc = &app.documents[0];
    Buffer {
        id: doc.id,
        path: doc.path.clone(),
        text: doc.text.clone(),
        saved_text: doc.saved_text.clone(),
        revision: doc.revision.clone(),
        edit_version: doc.edit_version,
        undo_initialized: doc.undo_initialized,
    }
}

fn undo_texts(app: &CedarApp) -> Vec<String> {
    let doc = &app.documents[0];
    let state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", doc.id))).unwrap();
    let mut history = state.undoer();
    let mut current = (
        state
            .cursor
            .char_range()
            .unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(0))),
        doc.text.clone(),
    );
    let mut texts = Vec::new();
    for _ in 0..=editor_state::MAX_UNDO_STATES {
        let Some(previous) = history.undo(&current).cloned() else {
            break;
        };
        if previous.1 != current.1 {
            texts.push(previous.1.clone());
        }
        current = previous;
    }
    texts
}

fn seed_clean_undo(harness: &mut Harness) {
    editor_state::commit(
        &harness.app.editor_ctx,
        &mut harness.app.documents[0],
        DRAFT.into(),
        3,
    );
    editor_state::commit(
        &harness.app.editor_ctx,
        &mut harness.app.documents[0],
        BASE.into(),
        3,
    );
    harness.frame(false, vec![]);
    assert!(!harness.app.dirty());
    assert!(undo_texts(&harness.app).contains(&DRAFT.to_string()));
}

fn text_shapes(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    fn collect(shape: &egui::epaint::Shape, result: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::epaint::Shape::Text(text) => result.push((
                text.galley.job.text.clone(),
                text.galley.rect.translate(text.pos.to_vec2()),
            )),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, result);
                }
            }
            _ => {}
        }
    }
    let mut result = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, &mut result);
    }
    result
}

fn rendered(output: &egui::FullOutput) -> String {
    text_shapes(output)
        .into_iter()
        .map(|(text, _)| text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn text_rect(output: &egui::FullOutput, wanted: &str) -> egui::Rect {
    text_shapes(output)
        .into_iter()
        .find_map(|(text, rect)| (text == wanted).then_some(rect))
        .unwrap_or_else(|| panic!("missing rendered control: {wanted}"))
}

fn click(harness: &mut Harness, at: egui::Pos2) {
    harness.frame(false, vec![egui::Event::PointerMoved(at)]);
    for pressed in [true, false] {
        let output = harness.frame(
            false,
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
        assert_no_close(&harness.app, &output);
    }
}

fn pending_restore() -> Draft {
    Draft {
        workspace: WorkspaceIdentity::Local { root: ROOT.into() },
        path: "recovered.txt".into(),
        text: DRAFT.into(),
        base_text: BASE.into(),
        base_revision: Some("r0".into()),
        modified_ms: 1,
    }
}

#[test]
fn native_close_waits_for_active_owner_and_emits_close_exactly_once() {
    let (mut harness, ticket, commands) = connected();
    seed_clean_undo(&mut harness);
    let before = buffer(&harness.app);
    let history = undo_texts(&harness.app);
    let guard = harness.app.recovery_close_guard();
    let output = harness.frame(true, vec![]);
    assert_waiting(&harness.app, &output);
    assert_retired(&commands);
    assert!(harness.app.worker.is_none() && harness.app.active_owner.is_none());
    assert!(!harness.app.owner_ledger.all_verified());
    assert_eq!(harness.app.generation, ticket.generation);
    assert_eq!(harness.app.recovery_close_guard(), guard);
    assert_eq!(buffer(&harness.app), before);
    assert_eq!(undo_texts(&harness.app), history);
    let empty = harness.frame(false, vec![]);
    assert_waiting(&harness.app, &empty);
    harness.receipt(ticket, OwnerOutcome::Verified);
    let settled = harness.frame(false, vec![]);
    assert!(harness.app.owner_ledger.all_verified());
    assert!(harness.app.allow_close && harness.app.transport_close.is_none());
    assert_eq!(close_count(&settled), 1);
    assert_eq!(buffer(&harness.app), before);
    assert_eq!(undo_texts(&harness.app), history);
    for close_again in [false, true, false] {
        assert_eq!(close_count(&harness.frame(close_again, vec![])), 0);
    }
}

#[test]
fn native_close_waits_for_retired_owner_after_current_no_child_receipt() {
    let (mut harness, old, old_commands) = connected();
    harness.app.state = ConnectionState::Connecting;
    harness.app.connecting_form = Some(form());
    harness.app.cancel_connection();
    assert_retired(&old_commands);
    assert_eq!(harness.app.generation, old.generation + 1);
    let (current, commands) = attach_recording(&mut harness.app);
    assert_ne!(old, current);
    harness.app.state = ConnectionState::Connecting;
    harness.app.connecting_form = Some(form());
    let output = harness.frame(true, vec![]);
    assert_waiting(&harness.app, &output);
    harness.receipt(current, OwnerOutcome::NoChild);
    let output = harness.frame(false, vec![]);
    assert_waiting(&harness.app, &output);
    assert!(!harness.app.owner_ledger.all_verified());
    assert!(harness
        .app
        .transport_close_message()
        .contains("1 local connection cleanup receipt(s)"));
    assert_retired(&commands);
    harness.receipt(old, OwnerOutcome::Verified);
    let output = harness.frame(false, vec![]);
    assert_eq!(close_count(&output), 1);
    assert!(harness.app.allow_close);
}

#[test]
fn ownerless_dirty_close_still_honors_confirmation() {
    let mut harness = Harness::new();
    harness.warm();
    editor_state::commit(
        &harness.app.editor_ctx,
        &mut harness.app.documents[0],
        DRAFT.into(),
        3,
    );
    let output = harness.frame(true, vec![]);
    assert_no_close(&harness.app, &output);
    assert!(matches!(harness.app.confirm, Some(Confirm::CloseWindow)));
    assert!(harness.app.recovery.closing.is_none() && harness.app.transport_close.is_none());
    // The same control handler used by the existing discard-confirmation UI.
    harness.app.confirm = None;
    let ctx = harness.app.editor_ctx.clone();
    harness.app.begin_close(&ctx);
    let output = harness.frame(false, vec![]);
    assert_eq!(close_count(&output), 1);
    assert!(harness.app.allow_close && harness.app.owner_ledger.all_verified());
    assert_eq!(harness.app.documents[0].text, DRAFT);
}

#[test]
fn unregistered_recording_worker_is_fail_closed_even_with_empty_ledger() {
    let mut harness = Harness::new();
    let (worker, commands) = Worker::recording();
    harness.app.worker = Some(worker);
    harness.app.state = ConnectionState::Ready;
    assert!(harness.app.owner_ledger.all_verified());
    harness.warm();
    let output = harness.frame(true, vec![]);
    assert_waiting(&harness.app, &output);
    assert_retired(&commands);
    assert!(!harness.app.owner_ledger.all_verified());
    assert!(harness.app.unverified_local_close);
    assert!(harness
        .app
        .transport_close_message()
        .contains("unregistered owner"));
}

#[test]
fn repeated_native_close_preserves_intent_ticket_and_guard() {
    let (mut harness, _, commands) = connected();
    let output = harness.frame(true, vec![]);
    assert_waiting(&harness.app, &output);
    let serial = harness.app.transport_close.as_ref().unwrap().serial;
    let recovery = confirmed(&harness.app);
    let generation = harness.app.generation;
    for _ in 0..3 {
        let output = harness.frame(true, vec![]);
        assert_waiting(&harness.app, &output);
        assert_eq!(harness.app.transport_close.as_ref().unwrap().serial, serial);
        assert_eq!(harness.app.next_transport_close, serial);
        assert_eq!(confirmed(&harness.app), recovery);
        assert_eq!(harness.app.generation, generation);
        assert_retired(&commands);
    }
}

#[test]
fn keep_editing_button_preserves_undo_and_late_receipt_cannot_close() {
    let (mut harness, ticket, commands) = connected();
    seed_clean_undo(&mut harness);
    let before = buffer(&harness.app);
    let history = undo_texts(&harness.app);
    harness.frame(true, vec![]);
    harness.frame(false, vec![]);
    let painted = harness.frame(false, vec![]);
    let at = text_rect(&painted, "Keep editing").center();
    click(&mut harness, at);
    assert!(harness.app.transport_close.is_none() && harness.app.recovery.closing.is_none());
    assert!(harness.app.state == ConnectionState::Disconnected);
    assert_eq!(harness.app.generation, ticket.generation + 1);
    assert!(harness.app.worker.is_none());
    assert_retired(&commands);
    assert_eq!(buffer(&harness.app), before);
    assert_eq!(undo_texts(&harness.app), history);
    harness.receipt(ticket, OwnerOutcome::Verified);
    let output = harness.frame(false, vec![]);
    assert_no_close(&harness.app, &output);
    assert!(harness.app.owner_ledger.all_verified());
    assert_eq!(harness.app.generation, ticket.generation + 1);
    // Verify the retained native history actually remains usable after release.
    let id = egui::Id::new(("editor", 1u64));
    harness
        .app
        .editor_ctx
        .memory_mut(|memory| memory.request_focus(id));
    let events = [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key: egui::Key::Z,
            physical_key: Some(egui::Key::Z),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        })
        .collect();
    let output = harness.frame(false, events);
    assert_no_close(&harness.app, &output);
    assert_eq!(harness.app.documents[0].text, DRAFT);
}

#[test]
fn owner_before_closed_partial_drain_retains_registration_capability() {
    let (mut harness, ticket, commands) = connected();
    harness.receipt(ticket, OwnerOutcome::Verified);
    // Deliberately stop the channel drain before the separate legacy event exists.
    harness.app.poll();
    assert!(harness.app.owner_ledger.all_verified());
    assert_eq!(harness.app.active_owner, Some(ticket));
    assert!(harness.app.worker.is_some());
    assert_live(&commands);
    let output = harness.frame(true, vec![]);
    assert_eq!(close_count(&output), 1);
    assert!(harness.app.allow_close && !harness.app.unverified_local_close);
    assert_retired(&commands);
    harness
        .app
        .result_tx
        .send(WorkerEvent::Closed {
            generation: ticket.generation,
            result: Ok(()),
        })
        .unwrap();
    assert_eq!(close_count(&harness.frame(false, vec![])), 0);
    assert!(!harness.app.unverified_local_close);
}

#[test]
fn closed_before_owner_retires_handle_but_retains_pending_responsibility() {
    let (mut harness, ticket, commands) = connected();
    harness
        .app
        .result_tx
        .send(WorkerEvent::Closed {
            generation: ticket.generation,
            result: Ok(()),
        })
        .unwrap();
    let output = harness.frame(false, vec![]);
    assert_no_close(&harness.app, &output);
    assert!(harness.app.state == ConnectionState::Disconnected);
    assert!(harness.app.worker.is_none() && harness.app.active_owner.is_none());
    assert_retired(&commands);
    assert!(!harness.app.owner_ledger.all_verified());
    let output = harness.frame(true, vec![]);
    assert_waiting(&harness.app, &output);
    assert!(!harness.app.unverified_local_close);
    harness.receipt(ticket, OwnerOutcome::Verified);
    let output = harness.frame(false, vec![]);
    assert_eq!(close_count(&output), 1);
    assert!(!harness.app.unverified_local_close);
}

#[test]
fn closing_blocks_ordinary_events_and_navigation_predispatch_but_accepts_owner() {
    let (mut harness, ticket, commands) = connected();
    harness.app.open("late.txt".into(), None);
    let command = commands.try_recv().unwrap();
    assert!(matches!(command.op, Operation::Read { .. }));
    let job = harness.app.pending.remove(&command.id).unwrap();
    let output = harness.frame(true, vec![]);
    assert_waiting(&harness.app, &output);
    let navigation = harness.app.navigation_epoch;
    let before = buffer(&harness.app);
    let error = harness.app.error.clone();
    // Deliberate stale-intent injection: production retirement normally clears
    // this map. Restore the real issued job with a current navigation stamp to
    // independently test the pre-dispatch gate, rather than relying on clearing.
    let Job::Open { path, line, .. } = job else {
        panic!("expected a real open job");
    };
    harness.app.pending.insert(
        command.id,
        Job::Open {
            path,
            line,
            navigation,
        },
    );
    harness
        .app
        .result_tx
        .send(WorkerEvent::Response(Event {
            generation: ticket.generation,
            id: command.id,
            connected: true,
            result: Ok(Payload::File {
                path: "late.txt".into(),
                text: "must not open".into(),
                revision: "late".into(),
            }),
        }))
        .unwrap();
    harness
        .app
        .result_tx
        .send(WorkerEvent::Closed {
            generation: ticket.generation,
            result: Err("ordinary event must be ignored".into()),
        })
        .unwrap();
    harness
        .app
        .result_tx
        .send(WorkerEvent::TransportLost {
            generation: ticket.generation,
            message: "must not replace the close UI".into(),
        })
        .unwrap();
    harness.receipt(ticket, OwnerOutcome::Verified);
    let events = vec![egui::Event::Key {
        key: egui::Key::F6,
        physical_key: Some(egui::Key::F6),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }];
    let output = harness.frame(false, events);
    assert_eq!(close_count(&output), 1);
    assert_eq!(
        harness.app.navigation_epoch, navigation,
        "new input must not supersede navigation before ordinary-event filtering"
    );
    assert_eq!(buffer(&harness.app), before);
    assert_eq!(harness.app.error, error);
    assert!(harness.app.state == ConnectionState::ClosingTransport);
    assert!(harness.app.pending.contains_key(&command.id));
    assert!(harness.app.owner_ledger.all_verified() && !harness.app.unverified_local_close);
}

#[test]
fn central_connect_and_restore_admission_are_blocked_through_close_pipeline() {
    for language_stage in [true, false] {
        let (mut harness, ticket, commands) = connected();
        if language_stage {
            harness.app.close_after_language_stop = true;
        } else {
            harness
                .app
                .recovery
                .begin_close(harness.app.recovery_close_guard());
        }
        harness.app.recovery.pending_restore = Some(pending_restore());
        let ctx = harness.app.editor_ctx.clone();
        harness.app.connect(&ctx, form());
        harness.app.begin_restore(&ctx);
        assert_eq!(harness.app.generation, ticket.generation);
        assert_eq!(harness.app.active_owner, Some(ticket));
        assert!(harness.app.worker.is_some());
        assert!(harness.app.recovery.restoring_generation.is_none());
        assert!(harness.app.recovery.pending_restore.is_some());
        assert!(harness
            .app
            .error
            .as_deref()
            .unwrap()
            .contains("Keep editing"));
        assert!(harness
            .app
            .recovery
            .error
            .as_deref()
            .unwrap()
            .contains("Keep editing"));
        assert_live(&commands);
    }
    let (mut harness, ticket, commands) = connected();
    harness.frame(true, vec![]);
    let serial = harness.app.next_transport_close;
    let guard = confirmed(&harness.app);
    harness.app.recovery.pending_restore = Some(pending_restore());
    let ctx = harness.app.editor_ctx.clone();
    harness.app.connect(&ctx, form());
    harness.app.begin_restore(&ctx);
    let next_request = harness.app.next_request;
    assert_eq!(
        harness.app.request(
            Operation::Read {
                path: "forbidden.txt".into()
            },
            Job::Open {
                path: "forbidden.txt".into(),
                line: None,
                navigation: harness.app.navigation_epoch
            }
        ),
        0
    );
    harness.app.begin_copy_draft();
    assert!(
        !harness.app.copy_draft_open(),
        "production Copy Draft remains rejected; it is not late-mutation coverage"
    );
    assert_eq!(harness.app.next_request, next_request);
    assert_eq!(harness.app.generation, ticket.generation);
    assert_eq!(harness.app.next_transport_close, serial);
    assert_eq!(confirmed(&harness.app), guard);
    assert!(harness.app.worker.is_none() && harness.app.active_owner.is_none());
    assert!(harness.app.recovery.restoring_generation.is_none());
    assert!(!harness.app.ready() && harness.app.pending.is_empty());
    assert_retired(&commands);
}

#[test]
fn cancel_restore_button_uses_owner_retirement_and_preserves_late_receipt() {
    let (mut harness, ticket, commands) = connected();
    let before = buffer(&harness.app);
    harness.app.state = ConnectionState::Connecting;
    harness.app.connecting_form = Some(form());
    harness.app.recovery.visible = true;
    harness.app.recovery.pending_restore = Some(pending_restore());
    harness.app.recovery.restoring_generation = Some(ticket.generation);
    harness.frame(false, vec![]);
    let output = harness.frame(false, vec![]);
    click(&mut harness, text_rect(&output, "Cancel restore").center());
    assert!(harness.app.state == ConnectionState::Disconnected);
    assert_eq!(harness.app.generation, ticket.generation + 1);
    assert!(harness.app.worker.is_none() && harness.app.active_owner.is_none());
    assert!(harness.app.recovery.pending_restore.is_none());
    assert!(harness.app.recovery.restoring_generation.is_none());
    assert!(harness.app.connecting_form.is_none());
    assert!(!harness.app.owner_ledger.all_verified());
    assert_eq!(buffer(&harness.app), before);
    assert_retired(&commands);
    harness.receipt(ticket, OwnerOutcome::NoChild);
    let output = harness.frame(false, vec![]);
    assert_no_close(&harness.app, &output);
    assert!(harness.app.owner_ledger.all_verified());
}

#[test]
fn generation_exhaustion_never_wraps_or_admits_stale_ui_but_allows_guarded_close() {
    let mut harness = Harness::new();
    harness.app.generation = u64::MAX;
    let (ticket, commands) = attach_recording(&mut harness.app);
    harness.warm();
    let ctx = harness.app.editor_ctx.clone();
    harness.app.connect(&ctx, form());
    assert_eq!(harness.app.generation, u64::MAX);
    assert_eq!(harness.app.active_owner, Some(ticket));
    assert!(harness.app.worker.is_some());
    assert_live(&commands);
    assert!(harness.app.error.as_deref().unwrap().contains("exhausted"));
    harness.app.state = ConnectionState::Connecting;
    harness.app.connecting_form = Some(form());
    harness.app.cancel_connection();
    assert_eq!(harness.app.generation, u64::MAX);
    assert!(harness.app.connection_generation_exhausted);
    assert_retired(&commands);
    let before = buffer(&harness.app);
    // Inject a matching old-generation read intent as well as a Hello: the
    // Hello alone would already be rejected by Disconnected state and would
    // not prove the terminal generation-exhausted ordinary-response gate.
    harness.app.pending.insert(
        91,
        Job::Open {
            path: "stale.txt".into(),
            line: None,
            navigation: harness.app.navigation_epoch,
        },
    );
    harness
        .app
        .result_tx
        .send(WorkerEvent::Response(Event {
            generation: u64::MAX,
            id: 91,
            connected: true,
            result: Ok(Payload::File {
                path: "stale.txt".into(),
                text: "stale response must not open a tab".into(),
                revision: "stale".into(),
            }),
        }))
        .unwrap();
    harness
        .app
        .result_tx
        .send(WorkerEvent::Response(Event {
            generation: u64::MAX,
            id: 0,
            connected: true,
            result: Ok(Payload::Hello {
                protocol: cedar_protocol::PROTOCOL_VERSION,
                root: "/wrong-stale-root".into(),
                agent: None,
                file_discovery: None,
                scoped_search: None,
                case_search: None,
            }),
        }))
        .unwrap();
    harness.app.connect(&ctx, form());
    let output = harness.frame(false, vec![]);
    assert_no_close(&harness.app, &output);
    assert_eq!(buffer(&harness.app), before);
    assert!(harness.app.pending.contains_key(&91));
    let output = harness.frame(true, vec![]);
    assert_waiting(&harness.app, &output);
    assert_eq!(harness.app.root, ROOT);
    assert_eq!(buffer(&harness.app), before);
    harness.receipt(ticket, OwnerOutcome::Verified);
    let output = harness.frame(false, vec![]);
    assert_eq!(close_count(&output), 1);
    assert_eq!(harness.app.generation, u64::MAX);
    assert!(harness.app.connection_generation_exhausted && harness.app.owner_ledger.all_verified());
}

#[test]
fn disconnected_result_channel_is_sticky_unverified_not_empty_success() {
    let (mut harness, ticket, commands) = connected();
    // The application's ordinary sender stays alive. Replace only the observed
    // receiver with a genuinely disconnected channel, not an Empty simulation.
    let (lost_tx, lost_rx) = mpsc::channel();
    drop(lost_tx);
    harness.app.result_rx = lost_rx;
    let output = harness.frame(true, vec![]);
    assert_waiting(&harness.app, &output);
    assert_retired(&commands);
    assert!(harness.app.unverified_local_close);
    assert!(harness
        .app
        .transport_close_message()
        .contains("channel disconnected"));
    let (tx, rx) = mpsc::channel();
    harness.app.result_tx = tx;
    harness.app.result_rx = rx;
    harness.receipt(ticket, OwnerOutcome::Verified);
    let output = harness.frame(false, vec![]);
    assert_waiting(&harness.app, &output);
    assert!(!harness.app.owner_ledger.all_verified());
    let painted = harness.frame(false, vec![]);
    assert!(rendered(&painted).contains("Quitting is blocked"));
    assert!(rendered(&painted).contains("Keep editing"));
}

#[test]
fn unverified_owner_stays_blocked_after_keep_editing_and_fresh_success() {
    let (mut harness, old, _) = connected();
    harness.frame(true, vec![]);
    harness.receipt(
        old,
        OwnerOutcome::Unverified("synthetic consuming wait failure".into()),
    );
    let output = harness.frame(false, vec![]);
    assert_waiting(&harness.app, &output);
    let ctx = harness.app.editor_ctx.clone();
    harness.app.keep_editing_recovery(&ctx);
    assert!(!harness.app.owner_ledger.all_verified());
    // A new recording registration models an explicit later connection without
    // invoking production Worker::spawn or pretending reconnect verifies old work.
    let (current, commands) = attach_recording(&mut harness.app);
    assert_ne!(old, current);
    harness.receipt(old, OwnerOutcome::Verified);
    harness.receipt(current, OwnerOutcome::Verified);
    let output = harness.frame(false, vec![]);
    assert_no_close(&harness.app, &output);
    assert_eq!(harness.app.active_owner, Some(current));
    assert!(harness.app.worker.is_some());
    assert_live(&commands);
    let output = harness.frame(true, vec![]);
    assert_waiting(&harness.app, &output);
    assert_retired(&commands);
    assert!(harness.app.unverified_local_close);
    assert!(harness
        .app
        .transport_close_message()
        .contains("synthetic consuming wait failure"));
}

#[test]
fn missing_receipt_requests_repaint_and_reveals_usable_wait_without_worker_event() {
    let (mut harness, _, commands) = connected();
    let output = harness.frame(true, vec![]);
    assert_waiting(&harness.app, &output);
    assert_eq!(
        output.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
        Duration::ZERO
    );
    assert_retired(&commands);
    // No event is injected between entry and visibility on the next full frame.
    // Check egui's tracked caller evidence too, so an unrelated editor animation
    // alone cannot satisfy the first-transition immediate-repaint assertion.
    let mut barrier_repaint_causes = 0;
    harness.frame_inspect(false, vec![], |_, ctx| {
        barrier_repaint_causes = barrier_repaint_causes.max(
            ctx.repaint_causes()
                .iter()
                .filter(|cause| cause.file.ends_with("close_transport.rs"))
                .count(),
        );
    });
    assert!(
        barrier_repaint_causes >= 2,
        "entry must request immediate repaint and schedule its pending-observation repaint"
    );
    let painted = harness.frame(false, vec![]);
    assert_waiting(&harness.app, &painted);
    let text = rendered(&painted);
    assert!(text.contains("Waiting for 1 local connection cleanup receipt(s)"));
    assert!(text.contains("1 retired owner(s)"));
    assert!(text.contains("Keep editing"));
    assert!(
        painted.viewport_output[&egui::ViewportId::ROOT].repaint_delay <= Duration::from_secs(5)
    );
    assert!(!harness.app.owner_ledger.all_verified() && !harness.app.unverified_local_close);
}

#[test]
fn closing_waiting_window_takes_keep_editing_path() {
    let (mut harness, ticket, _) = connected();
    harness.frame(true, vec![]);
    harness.frame(false, vec![]);
    let output = harness.frame(false, vec![]);
    let title = text_rect(&output, "Private draft recovery");
    let ctx = harness.app.editor_ctx.clone();
    let area = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("Private draft recovery")))
        .unwrap();
    // egui's titlebar close button is centered in the rightmost title-height
    // square. Derive its center from the actual painted title and window rect.
    let stroke = egui::Frame::window(&ctx.style()).stroke.width;
    let half_title_height = title.center().y - area.top() - stroke;
    assert!(half_title_height > 0.0);
    click(
        &mut harness,
        egui::pos2(area.right() - stroke - half_title_height, title.center().y),
    );
    assert!(harness.app.transport_close.is_none() && harness.app.recovery.closing.is_none());
    assert!(!harness.app.recovery.visible);
    assert!(harness.app.state == ConnectionState::Disconnected);
    assert_eq!(harness.app.generation, ticket.generation + 1);
    assert!(!harness.app.owner_ledger.all_verified());
    harness.receipt(ticket, OwnerOutcome::Verified);
    let output = harness.frame(false, vec![]);
    assert_no_close(&harness.app, &output);
}

#[test]
fn discarded_pass_cannot_retire_or_close_before_stable_final_pass() {
    #[derive(Debug)]
    struct Pass {
        discarded: bool,
        worker: bool,
        owner: Option<OwnerTicket>,
        intent: bool,
        allow_close: bool,
        closes: usize,
    }
    fn discard_injection(_app: &mut CedarApp, ctx: &egui::Context) {
        // Test-only pass injection after every production finisher, before the
        // final barrier. This is a real egui discarded pass, not an early return.
        ctx.request_discard("native-close stable-pass regression injection");
    }
    let (mut harness, ticket, commands) = connected();
    harness.receipt(ticket, OwnerOutcome::Verified);
    harness.app.poll();
    assert!(harness.app.owner_ledger.all_verified());
    harness
        .app
        .editor_ctx
        .options_mut(|options| options.max_passes = 2.try_into().unwrap());
    harness.app.close_frame_mutation = Some(discard_injection);
    let mut passes = Vec::new();
    let output = harness.frame_inspect(true, vec![], |app, ctx| {
        passes.push(Pass {
            discarded: ctx.will_discard() || ctx.output(|output| output.requested_discard()),
            worker: app.worker.is_some(),
            owner: app.active_owner,
            intent: app.transport_close.is_some(),
            allow_close: app.allow_close,
            closes: ctx.viewport(|viewport| {
                viewport
                    .commands
                    .iter()
                    .filter(|command| **command == egui::ViewportCommand::Close)
                    .count()
            }),
        })
    });
    assert_eq!(
        passes.len(),
        2,
        "expected one discarded pass and its stable replacement: {passes:?}"
    );
    assert!(passes[0].discarded && passes[0].worker && !passes[0].intent && !passes[0].allow_close);
    assert_eq!(passes[0].owner, Some(ticket));
    assert_eq!(passes[0].closes, 0);
    assert!(!passes[1].discarded && !passes[1].worker && passes[1].allow_close);
    assert!(passes[1].owner.is_none());
    assert_eq!(passes[1].closes, 1);
    assert_eq!(close_count(&output), 1);
    assert_retired(&commands);
    assert!(!harness.app.unverified_local_close);
}

#[test]
fn deterministic_late_document_edit_injection_invalidates_confirmed_exit() {
    fn edit_injection(app: &mut CedarApp, ctx: &egui::Context) {
        let before = app.recovery_close_guard();
        assert!(matches!(
            app.recovery.closing.as_ref().unwrap().phase,
            ClosePhase::Confirmed { .. }
        ));
        // Test-only injection invokes the actual native-history document edit
        // helper after all finishers. No claim is made that Copy Draft or another
        // otherwise-blocked production finisher was allowed through its gate.
        editor_state::commit(ctx, &mut app.documents[0], DRAFT.into(), 4);
        assert_ne!(
            app.recovery_close_guard(),
            before,
            "the injection must actually change the approved guard"
        );
    }
    let (mut harness, ticket, commands) = connected();
    harness.receipt(ticket, OwnerOutcome::Verified);
    harness.app.poll();
    harness.app.close_frame_mutation = Some(edit_injection);
    let output = harness.frame(true, vec![]);
    assert_no_close(&harness.app, &output);
    assert_eq!(harness.app.documents[0].text, DRAFT);
    assert!(harness.app.documents[0].dirty());
    assert!(harness.app.transport_close.is_none() && harness.app.recovery.closing.is_none());
    assert!(harness.app.worker.is_some());
    assert_eq!(harness.app.active_owner, Some(ticket));
    assert_eq!(harness.app.generation, ticket.generation);
    assert_live(&commands);
    assert!(harness.app.owner_ledger.all_verified());
    let output = harness.frame(false, vec![]);
    assert_no_close(&harness.app, &output);
    let output = harness.frame(true, vec![]);
    assert_no_close(&harness.app, &output);
    assert!(matches!(harness.app.confirm, Some(Confirm::CloseWindow)));
}

#[test]
fn disconnect_then_close_does_not_invent_exit_approval_from_receipts() {
    let (mut harness, ticket, commands) = connected();
    harness.app.disconnect_idle();
    assert!(harness.app.state == ConnectionState::Disconnecting);
    assert_retired(&commands);
    let output = harness.frame(true, vec![]);
    assert_no_close(&harness.app, &output);
    assert!(harness.app.recovery.closing.is_none() && harness.app.transport_close.is_none());
    harness.receipt(ticket, OwnerOutcome::Verified);
    harness
        .app
        .result_tx
        .send(WorkerEvent::Closed {
            generation: ticket.generation,
            result: Ok(()),
        })
        .unwrap();
    let output = harness.frame(false, vec![]);
    assert_no_close(&harness.app, &output);
    assert!(harness.app.state == ConnectionState::Disconnected);
    assert!(harness.app.recovery.closing.is_none());
    assert_eq!(close_count(&harness.frame(true, vec![])), 1);
}

#[test]
fn close_then_disconnect_preserves_waiting_transaction() {
    let (mut harness, ticket, commands) = connected();
    harness.frame(true, vec![]);
    let serial = harness.app.next_transport_close;
    let recovery = confirmed(&harness.app);
    harness.app.disconnect_idle();
    assert!(harness.app.state == ConnectionState::ClosingTransport);
    assert_eq!(harness.app.transport_close.as_ref().unwrap().serial, serial);
    assert_eq!(confirmed(&harness.app), recovery);
    assert_eq!(harness.app.generation, ticket.generation);
    assert!(!harness.app.owner_ledger.all_verified());
    assert_retired(&commands);
    let output = harness.frame(false, vec![]);
    assert_waiting(&harness.app, &output);
}

#[test]
fn retired_owner_receipt_settles_globally_without_mutating_new_current_session() {
    let (mut harness, old, old_commands) = connected();
    harness.app.state = ConnectionState::Connecting;
    harness.app.connecting_form = Some(form());
    harness.app.cancel_connection();
    assert_retired(&old_commands);
    let (current, commands) = attach_recording(&mut harness.app);
    let before = buffer(&harness.app);
    let notice = harness.app.notice.clone();
    let error = harness.app.error.clone();
    let navigation = harness.app.navigation_epoch;
    harness
        .app
        .result_tx
        .send(WorkerEvent::TransportLost {
            generation: old.generation,
            message: "retired owner loss must not replace current UI".into(),
        })
        .unwrap();
    harness
        .app
        .result_tx
        .send(WorkerEvent::Closed {
            generation: old.generation,
            result: Ok(()),
        })
        .unwrap();
    harness.receipt(old, OwnerOutcome::Verified);
    let output = harness.frame(false, vec![]);
    assert_no_close(&harness.app, &output);
    assert!(harness.app.state == ConnectionState::Ready);
    assert_eq!(harness.app.generation, current.generation);
    assert_eq!(harness.app.active_owner, Some(current));
    assert!(harness.app.worker.is_some());
    assert_eq!(buffer(&harness.app), before);
    assert_eq!(harness.app.notice, notice);
    assert_eq!(harness.app.error, error);
    assert_eq!(harness.app.navigation_epoch, navigation);
    assert!(
        !harness.app.owner_ledger.all_verified(),
        "the current pending owner is independent of the settled retired owner"
    );
    assert_live(&commands);
    harness.receipt(current, OwnerOutcome::Verified);
    let output = harness.frame(true, vec![]);
    assert_eq!(close_count(&output), 1);
    assert!(harness.app.owner_ledger.all_verified() && !harness.app.unverified_local_close);
}

#[test]
fn mismatched_active_registration_cannot_be_repaired_by_success_receipts() {
    let mut harness = Harness::new();
    let registered = harness
        .app
        .owner_ledger
        .register(harness.app.generation)
        .unwrap();
    let other = harness
        .app
        .owner_ledger
        .register(harness.app.generation)
        .unwrap();
    let (worker, commands) = Worker::recording_with_owner(other);
    harness.app.worker = Some(worker);
    harness.app.active_owner = Some(registered);
    harness.app.state = ConnectionState::Ready;
    harness.warm();
    let output = harness.frame(true, vec![]);
    assert_waiting(&harness.app, &output);
    assert_retired(&commands);
    harness.receipt(registered, OwnerOutcome::Verified);
    harness.receipt(other, OwnerOutcome::Verified);
    let output = harness.frame(false, vec![]);
    assert_waiting(&harness.app, &output);
    assert!(!harness.app.owner_ledger.all_verified());
    assert!(harness.app.unverified_local_close);
    assert!(harness
        .app
        .transport_close_message()
        .contains("did not match its registered local owner"));
}

#[test]
fn native_ownerless_clean_close_confirms_recovery_and_finishes_once() {
    let mut harness = Harness::new();
    harness.warm();
    let guard = harness.app.recovery_close_guard();
    let output = harness.frame(true, vec![]);
    assert!(harness.app.owner_ledger.all_verified());
    assert!(harness.app.worker.is_none() && harness.app.active_owner.is_none());
    assert_eq!(confirmed(&harness.app).0, guard);
    assert!(harness.app.allow_close && harness.app.transport_close.is_none());
    assert_eq!(close_count(&output), 1);
    assert_eq!(close_count(&harness.frame(false, vec![])), 0);
    assert_eq!(close_count(&harness.frame(true, vec![])), 0);
}
