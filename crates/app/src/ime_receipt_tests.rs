//! Synthetic native egui 0.31.1 receipt boundaries, without private IME state.
//!
//! Each ordinary fixture runs twice in independent Contexts: once with String and
//! once with a mutation-only String-delegating TextBuffer. Receipts are compared
//! with actual native output, never inferred from the adapter's mutation log.
//! This is headless native event evidence, not an OS IME/candidate-window test.
use eframe::egui::{
    self,
    text::CCursor,
    text_edit::{ImeEventOutcome, TextEditImeReceipt},
    Event, ImeEvent, Key, Modifiers, TextBuffer,
};
use serde_json::{json, Value};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Mutation {
    Insert {
        at: usize,
        text: String,
        inserted_scalars: usize,
    },
    Delete {
        start: usize,
        end: usize,
    },
    Replace {
        text: String,
    },
    Clear,
    Take,
}

impl Mutation {
    fn json(&self) -> Value {
        match self {
            Self::Insert {
                at,
                text,
                inserted_scalars,
            } => json!({
                "method": "insert_text", "at": at, "text": text,
                "inserted_scalars": inserted_scalars,
            }),
            Self::Delete { start, end } => {
                json!({"method": "delete_char_range", "start": start, "end": end})
            }
            Self::Replace { text } => json!({"method": "replace_with", "text": text}),
            Self::Clear => json!({"method": "clear"}),
            Self::Take => json!({"method": "take"}),
        }
    }
}

#[derive(Default)]
struct MutationBuffer {
    text: String,
    mutations: Vec<Mutation>,
}

impl MutationBuffer {
    fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            mutations: Vec::new(),
        }
    }
}

impl TextBuffer for MutationBuffer {
    fn is_mutable(&self) -> bool {
        <String as TextBuffer>::is_mutable(&self.text)
    }

    fn as_str(&self) -> &str {
        <String as TextBuffer>::as_str(&self.text)
    }

    fn insert_text(&mut self, text: &str, char_index: usize) -> usize {
        let inserted_scalars =
            <String as TextBuffer>::insert_text(&mut self.text, text, char_index);
        self.mutations.push(Mutation::Insert {
            at: char_index,
            text: text.to_owned(),
            inserted_scalars,
        });
        inserted_scalars
    }

    fn delete_char_range(&mut self, char_range: Range<usize>) {
        <String as TextBuffer>::delete_char_range(&mut self.text, char_range.clone());
        self.mutations.push(Mutation::Delete {
            start: char_range.start,
            end: char_range.end,
        });
    }

    // Delegate String's overrides directly. In particular, an undo replacement
    // must not become a fabricated clear + insert sequence in this observer.
    fn replace_with(&mut self, text: &str) {
        <String as TextBuffer>::replace_with(&mut self.text, text);
        self.mutations.push(Mutation::Replace {
            text: text.to_owned(),
        });
    }

    fn clear(&mut self) {
        <String as TextBuffer>::clear(&mut self.text);
        self.mutations.push(Mutation::Clear);
    }

    fn take(&mut self) -> String {
        let result = <String as TextBuffer>::take(&mut self.text);
        self.mutations.push(Mutation::Take);
        result
    }
}

#[derive(Clone, Debug, PartialEq)]
struct NativePass {
    widget_id: egui::Id,
    text: String,
    // Explicit affinity bits: CCursor's PartialEq ignores prefer_next_row.
    cursor: Option<(usize, bool, usize, bool)>,
    changed: bool,
    focused: bool,
    input_events: Vec<String>,
    receipts: Vec<TextEditImeReceipt>,
    caret_start: egui::Pos2,
    caret_end: egui::Pos2,
}

fn outcome_json(outcome: &ImeEventOutcome) -> Value {
    match outcome {
        ImeEventOutcome::Enabled => json!({"kind": "Enabled"}),
        ImeEventOutcome::PreeditApplied => json!({"kind": "PreeditApplied"}),
        ImeEventOutcome::PreeditIgnoredLineBreak => json!({"kind": "PreeditIgnoredLineBreak"}),
        ImeEventOutcome::CommitAccepted {
            insertion_start,
            insertion_end,
        } => json!({
            "kind": "CommitAccepted", "insertion_start": insertion_start,
            "insertion_end": insertion_end,
        }),
        ImeEventOutcome::CommitEmpty => json!({"kind": "CommitEmpty"}),
        ImeEventOutcome::CommitIgnoredLineBreak => json!({"kind": "CommitIgnoredLineBreak"}),
        ImeEventOutcome::CommitRejectedAnchor => json!({"kind": "CommitRejectedAnchor"}),
        ImeEventOutcome::Disabled => json!({"kind": "Disabled"}),
    }
}

impl NativePass {
    fn json(&self) -> Value {
        json!({
            "widget_id": format!("{:?}", self.widget_id),
            "text": self.text,
            "text_scalars": self.text.chars().count(),
            "text_bytes": self.text.len(),
            "cursor": self.cursor,
            "changed": self.changed,
            "focused": self.focused,
            "input_snapshot": self.input_events,
            "receipts": self.receipts.iter().map(|r| json!({
                "input_event_index": r.input_event_index,
                "processing_order": r.processing_order,
                "outcome": outcome_json(&r.outcome),
            })).collect::<Vec<_>>(),
        })
    }

    fn indices(&self) -> (usize, usize) {
        let (primary, _, secondary, _) = self.cursor.expect("native cursor exists");
        (primary, secondary)
    }
}

#[derive(Clone, Copy)]
struct FrameOptions {
    interactive: bool,
    focus: Option<bool>,
    discard: bool,
    // Deliberately models an earlier consumer of InputState.events. Receipts
    // refer to the snapshot at TextEdit entry, not the original RawInput list.
    remove_prefix: usize,
}

impl Default for FrameOptions {
    fn default() -> Self {
        Self {
            interactive: true,
            focus: None,
            discard: false,
            remove_prefix: 0,
        }
    }
}

fn capture(output: egui::text_edit::TextEditOutput, text: &str, events: Vec<String>) -> NativePass {
    let cursor = output.state.cursor.char_range().map(|range| {
        (
            range.primary.index,
            range.primary.prefer_next_row,
            range.secondary.index,
            range.secondary.prefer_next_row,
        )
    });
    let caret_point = |index| {
        output
            .galley
            .pos_from_cursor(&output.galley.from_ccursor(CCursor::new(index)))
            .center()
            + output.galley_pos.to_vec2()
    };
    NativePass {
        widget_id: output.response.id,
        text: text.to_owned(),
        cursor,
        changed: output.response.changed(),
        focused: output.response.has_focus(),
        input_events: events,
        caret_start: caret_point(0),
        caret_end: caret_point(text.chars().count()),
        receipts: output.ime_receipts,
    }
}

fn raw_input(time: f64, events: Vec<Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1000.0, 700.0),
        )),
        time: Some(time),
        focused: true,
        events,
        ..Default::default()
    }
}

struct NativeHarness<B> {
    ctx: egui::Context,
    id: egui::Id,
    buffer: B,
    time: f64,
    char_limit: usize,
}

impl<B: TextBuffer> NativeHarness<B> {
    fn new(buffer: B, char_limit: usize) -> Self {
        let ctx = egui::Context::default();
        ctx.options_mut(|o| o.max_passes = 1.try_into().unwrap());
        Self {
            ctx,
            id: egui::Id::new("ime_native_receipt_field"),
            buffer,
            time: 0.0,
            char_limit,
        }
    }

    fn frame(&mut self, events: Vec<Event>, options: FrameOptions) -> Vec<NativePass> {
        self.time += 1.0;
        self.ctx.options_mut(|o| {
            o.max_passes = if options.discard { 2 } else { 1 }.try_into().unwrap();
        });
        let id = self.id;
        let char_limit = self.char_limit;
        let buffer = &mut self.buffer;
        let mut passes = Vec::new();
        let result = self.ctx.run(raw_input(self.time, events), |ctx| {
            let pass_index = passes.len();
            if pass_index == 0 {
                match options.focus {
                    Some(true) => ctx.memory_mut(|m| m.request_focus(id)),
                    Some(false) => ctx.memory_mut(|m| m.surrender_focus(id)),
                    None => {}
                }
                if options.remove_prefix != 0 {
                    ctx.input_mut(|i| {
                        assert!(options.remove_prefix <= i.events.len());
                        i.events.drain(..options.remove_prefix);
                    });
                }
            }
            egui::CentralPanel::default().show(ctx, |ui| {
                let snapshot = ui.input(|i| i.events.iter().map(|e| format!("{e:?}")).collect());
                let output = egui::TextEdit::singleline(buffer)
                    .id(id)
                    .cursor_at_end(false)
                    .interactive(options.interactive)
                    .char_limit(char_limit)
                    .desired_width(700.0)
                    .show(ui);
                passes.push(capture(output, buffer.as_str(), snapshot));
            });
            if options.discard && pass_index == 0 {
                ctx.request_discard("native IME receipt extra-pass boundary");
                assert!(ctx.will_discard());
            }
        });
        assert_eq!(result.platform_output.num_completed_passes, passes.len());
        assert_eq!(passes.len(), if options.discard { 2 } else { 1 });
        passes
    }
}

struct PairedHarness {
    plain: NativeHarness<String>,
    observed: NativeHarness<MutationBuffer>,
}

impl PairedHarness {
    fn new(text: &str) -> Self {
        Self::with_limit(text, 4096)
    }

    fn with_limit(text: &str, char_limit: usize) -> Self {
        let mut h = Self {
            plain: NativeHarness::new(text.to_owned(), char_limit),
            observed: NativeHarness::new(MutationBuffer::new(text), char_limit),
        };
        h.frame_with(
            "fixture_focus",
            Vec::new(),
            FrameOptions {
                focus: Some(true),
                ..Default::default()
            },
        );
        h
    }

    fn frame(&mut self, label: &str, events: Vec<Event>) -> NativePass {
        self.frame_with(label, events, FrameOptions::default())
            .remove(0)
    }

    fn frame_with(
        &mut self,
        label: &str,
        events: Vec<Event>,
        options: FrameOptions,
    ) -> Vec<NativePass> {
        self.observed.buffer.mutations.clear();
        let plain = self.plain.frame(events.clone(), options);
        let observed = self.observed.frame(events, options);
        println!(
            "IME_NATIVE_RECEIPT_TRACE {}",
            json!({
                "case": label,
                "plain": plain.iter().map(NativePass::json).collect::<Vec<_>>(),
                "observed": observed.iter().map(NativePass::json).collect::<Vec<_>>(),
                "mutations": self.observed.buffer.mutations.iter()
                    .map(Mutation::json).collect::<Vec<_>>(),
            })
        );
        assert_eq!(plain, observed, "String transparency at {label}");
        observed
    }

    fn preedit(&mut self, value: &str) -> NativePass {
        let p = self.frame(
            "fixture_preedit",
            vec![ime(ImeEvent::Enabled), preedit(value)],
        );
        expect_receipts(
            &p,
            vec![
                (0, 0, ImeEventOutcome::Enabled),
                (1, 1, ImeEventOutcome::PreeditApplied),
            ],
        );
        p
    }

    fn tap(&mut self, label: &str, key: Key) -> NativePass {
        self.frame(label, vec![press(key), release(key)])
    }

    fn mutations(&self) -> &[Mutation] {
        &self.observed.buffer.mutations
    }
}

fn ime(event: ImeEvent) -> Event {
    Event::Ime(event)
}

fn commit(text: &str) -> Event {
    ime(ImeEvent::Commit(text.to_owned()))
}

fn preedit(text: &str) -> Event {
    ime(ImeEvent::Preedit(text.to_owned()))
}

fn key_event(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers,
    }
}

fn press(key: Key) -> Event {
    key_event(key, true, Modifiers::NONE)
}

fn release(key: Key) -> Event {
    key_event(key, false, Modifiers::NONE)
}

fn accepted(at: usize, count: usize) -> ImeEventOutcome {
    ImeEventOutcome::CommitAccepted {
        insertion_start: at,
        insertion_end: at + count,
    }
}

fn expect_receipts(pass: &NativePass, expected: Vec<(usize, usize, ImeEventOutcome)>) {
    let expected: Vec<_> = expected
        .into_iter()
        .map(
            |(input_event_index, processing_order, outcome)| TextEditImeReceipt {
                input_event_index,
                processing_order,
                outcome,
            },
        )
        .collect();
    assert_eq!(pass.receipts, expected);
}

fn primitive_insert(at: usize, text: &str) -> Mutation {
    Mutation::Insert {
        at,
        text: text.to_owned(),
        inserted_scalars: text.chars().count(),
    }
}

#[test]
fn ime_receipt_same_value_accept_and_anchor_reject_have_equal_visible_results() {
    let mut good = PairedHarness::new("");
    let mut bad = PairedHarness::new("");
    good.preedit("x");
    bad.preedit("x");
    bad.tap("same_value_move_anchor", Key::End);
    let accepted_pass = good.frame("same_value_accepted", vec![commit("x")]);
    let rejected_pass = bad.frame("same_value_rejected", vec![commit("x")]);
    expect_receipts(&accepted_pass, vec![(0, 0, accepted(0, 1))]);
    expect_receipts(
        &rejected_pass,
        vec![(0, 0, ImeEventOutcome::CommitRejectedAnchor)],
    );
    assert_eq!(accepted_pass.text, "x");
    assert_eq!(accepted_pass.text, rejected_pass.text);
    assert_eq!(accepted_pass.indices(), rejected_pass.indices());
    assert_eq!(accepted_pass.indices(), (1, 1));
    assert!(accepted_pass.changed && rejected_pass.changed);
    assert_eq!(
        good.mutations(),
        &[
            Mutation::Delete { start: 0, end: 1 },
            primitive_insert(0, "x")
        ]
    );
    assert!(bad.mutations().is_empty());
}

#[test]
fn ime_receipt_repeated_identical_commits_keep_distinct_event_identity() {
    let cases = [
        (
            "duplicate_commit",
            vec![commit("x"), commit("x")],
            "x",
            vec![
                (0, 0, accepted(0, 1)),
                (1, 1, ImeEventOutcome::CommitRejectedAnchor),
            ],
        ),
        (
            "duplicate_with_disabled",
            vec![commit("x"), ime(ImeEvent::Disabled), commit("x")],
            "x",
            vec![
                (0, 0, accepted(0, 1)),
                (1, 1, ImeEventOutcome::Disabled),
                (2, 2, ImeEventOutcome::CommitRejectedAnchor),
            ],
        ),
        (
            "duplicate_with_new_enabled",
            vec![commit("x"), ime(ImeEvent::Enabled), commit("x")],
            "xx",
            vec![
                (0, 0, accepted(0, 1)),
                (1, 1, ImeEventOutcome::Enabled),
                (2, 2, accepted(1, 1)),
            ],
        ),
        (
            "duplicate_with_new_preedit",
            vec![
                commit("x"),
                ime(ImeEvent::Enabled),
                preedit("x"),
                commit("x"),
            ],
            "xx",
            vec![
                (0, 0, accepted(0, 1)),
                (1, 1, ImeEventOutcome::Enabled),
                (2, 2, ImeEventOutcome::PreeditApplied),
                (3, 3, accepted(1, 1)),
            ],
        ),
    ];
    for (label, events, final_text, expected) in cases {
        let mut h = PairedHarness::new("");
        h.preedit("x");
        let p = h.frame(label, events);
        expect_receipts(&p, expected);
        assert_eq!(p.text, final_text);
        assert_eq!(p.indices(), (final_text.len(), final_text.len()));
    }
}

#[test]
fn ime_receipt_accepted_then_new_preedit_remains_visibly_unfinished() {
    let mut h = PairedHarness::new("");
    h.preedit("x");
    let p = h.frame(
        "accepted_then_new_lifecycle",
        vec![commit("x"), ime(ImeEvent::Enabled), preedit("x")],
    );
    expect_receipts(
        &p,
        vec![
            (0, 0, accepted(0, 1)),
            (1, 1, ImeEventOutcome::Enabled),
            (2, 2, ImeEventOutcome::PreeditApplied),
        ],
    );
    assert_eq!(p.text, "xx");
    assert_eq!(p.indices(), (2, 1));
    assert!(!matches!(
        p.receipts.last().unwrap().outcome,
        ImeEventOutcome::CommitAccepted { .. }
    ));
}

#[test]
fn ime_receipt_empty_and_exact_line_breaks_are_explicit_non_acceptance() {
    for line_break in ["\n", "\r"] {
        let mut h = PairedHarness::new("");
        h.preedit("x");
        let p = h.frame(
            "ignored_line_breaks",
            vec![preedit(line_break), commit(line_break)],
        );
        expect_receipts(
            &p,
            vec![
                (0, 0, ImeEventOutcome::PreeditIgnoredLineBreak),
                (1, 1, ImeEventOutcome::CommitIgnoredLineBreak),
            ],
        );
        assert_eq!(p.text, "x");
        assert_eq!(p.indices(), (1, 0));
        assert!(!p.changed);
        assert!(h.mutations().is_empty());
        let later = h.frame("valid_after_ignored_line_break", vec![commit("x")]);
        expect_receipts(&later, vec![(0, 0, accepted(0, 1))]);
    }
    let mut empty_commit = PairedHarness::new("");
    empty_commit.preedit("x");
    let p = empty_commit.frame("empty_commit", vec![commit("")]);
    expect_receipts(&p, vec![(0, 0, ImeEventOutcome::CommitEmpty)]);
    assert_eq!(p.text, "x");
    assert_eq!(p.indices(), (1, 1));
    assert!(p.changed);
    assert!(empty_commit.mutations().is_empty());

    let mut empty_preedit = PairedHarness::new("");
    empty_preedit.preedit("x");
    let p = empty_preedit.frame("empty_preedit_deletes_selection", vec![preedit("")]);
    expect_receipts(&p, vec![(0, 0, ImeEventOutcome::PreeditApplied)]);
    assert!(p.text.is_empty());
    assert_eq!(p.indices(), (0, 0));
    assert!(p.changed);
    assert_eq!(
        empty_preedit.mutations(),
        &[Mutation::Delete { start: 0, end: 1 }]
    );

    // Pinned egui ignores exact CR or LF, not every payload containing a break.
    let mut embedded = PairedHarness::new("");
    let p = embedded.frame("crlf_payload_is_not_exact_line_break", vec![commit("\r\n")]);
    expect_receipts(&p, vec![(0, 0, accepted(0, 2))]);
    assert_eq!(p.text, "\r\n");
}

fn prepare_saved_anchor(h: &mut PairedHarness, anchor_at_end: bool) {
    if anchor_at_end {
        h.tap("fixture_move_to_end", Key::End);
    }
    let enabled = h.frame("fixture_save_anchor", vec![ime(ImeEvent::Enabled)]);
    expect_receipts(&enabled, vec![(0, 0, ImeEventOutcome::Enabled)]);
    h.tap("fixture_move_home_while_ime_enabled", Key::Home);
    let disabled = h.frame(
        "fixture_disable_without_changing_anchor",
        vec![ime(ImeEvent::Disabled)],
    );
    expect_receipts(&disabled, vec![(0, 0, ImeEventOutcome::Disabled)]);
    let home = h.tap("fixture_common_home", Key::Home);
    assert_eq!(home.indices(), (0, 0));
    assert!(home.receipts.is_empty());
    assert_eq!(home.text, "z");
}

#[test]
fn ime_receipt_inactive_direct_commit_can_accept_or_reject() {
    for (at_end, expected, final_text) in [
        (false, accepted(0, 1), "xz"),
        (true, ImeEventOutcome::CommitRejectedAnchor, "z"),
    ] {
        let mut h = PairedHarness::new("z");
        prepare_saved_anchor(&mut h, at_end);
        let p = h.frame("inactive_direct_commit", vec![commit("x")]);
        expect_receipts(&p, vec![(0, 0, expected)]);
        assert_eq!(p.text, final_text);
        assert!(p.changed);
    }
}

#[test]
fn ime_receipt_mutation_only_twin_has_opposite_latest_lifecycle_outcomes() {
    let mut a = PairedHarness::new("z");
    let mut b = PairedHarness::new("z");
    prepare_saved_anchor(&mut a, false);
    prepare_saved_anchor(&mut b, true);
    let events = vec![
        commit("x"),
        ime(ImeEvent::Enabled),
        press(Key::Home),
        commit("x"),
        press(Key::End),
    ];
    let pa = a.frame("mutation_twin_A", events.clone());
    let pb = b.frame("mutation_twin_B", events);
    expect_receipts(
        &pa,
        vec![
            (0, 0, accepted(0, 1)),
            (1, 1, ImeEventOutcome::Enabled),
            (3, 3, ImeEventOutcome::CommitRejectedAnchor),
        ],
    );
    expect_receipts(
        &pb,
        vec![
            (0, 0, ImeEventOutcome::CommitRejectedAnchor),
            (1, 1, ImeEventOutcome::Enabled),
            (3, 3, accepted(0, 1)),
        ],
    );
    let identical_log = [
        Mutation::Delete { start: 0, end: 0 },
        primitive_insert(0, "x"),
    ];
    assert_eq!(a.mutations(), &identical_log);
    assert_eq!(b.mutations(), &identical_log);
    assert_eq!(pa.input_events, pb.input_events);
    assert_eq!(pa.text, "xz");
    assert_eq!(pa.text, pb.text);
    assert_eq!(pa.cursor, pb.cursor);
    assert_eq!(pa.indices(), (2, 2));
    assert!(pa.changed && pb.changed);
    assert_eq!(pa.focused, pb.focused);
    println!(
        "IME_NATIVE_RECEIPT_TWIN {}",
        json!({
            "mutation_logs_equal": a.mutations() == b.mutations(),
            "final_bytes_equal": pa.text == pb.text,
            "cursor_and_affinity_equal": pa.cursor == pb.cursor,
            "changed_equal": pa.changed == pb.changed,
            "A": pa.json(), "B": pb.json(),
            "scope": "mutation-only observations; not a claim about every public observer",
        })
    );
}

#[test]
fn ime_receipt_absence_means_unprocessed_without_focus_or_interactivity() {
    for (label, options) in [
        (
            "unfocused",
            FrameOptions {
                focus: Some(false),
                ..Default::default()
            },
        ),
        (
            "noninteractive",
            FrameOptions {
                interactive: false,
                ..Default::default()
            },
        ),
    ] {
        let mut h = PairedHarness::new("z");
        let p = h
            .frame_with(
                label,
                vec![
                    ime(ImeEvent::Enabled),
                    preedit("x"),
                    commit("x"),
                    ime(ImeEvent::Disabled),
                ],
                options,
            )
            .remove(0);
        assert_eq!(p.input_events.len(), 4);
        assert!(p.receipts.is_empty());
        assert!(!p.changed);
        assert_eq!(p.text, "z");
        assert!(h.mutations().is_empty());
    }
}

#[test]
fn ime_receipt_enter_break_does_not_fabricate_later_receipts() {
    let mut h = PairedHarness::new("");
    let p = h.frame(
        "enter_break_inactive_entry",
        vec![
            ime(ImeEvent::Enabled),
            press(Key::Enter),
            commit("x"),
            preedit("y"),
            ime(ImeEvent::Disabled),
        ],
    );
    expect_receipts(&p, vec![(0, 0, ImeEventOutcome::Enabled)]);
    assert_eq!(p.input_events.len(), 5);
    assert!(p.text.is_empty());
    assert!(!p.changed);
    assert!(!p.focused);
    assert!(h.mutations().is_empty());

    let mut active = PairedHarness::new("");
    active.preedit("x");
    let p = active.frame(
        "active_entry_sorts_commit_before_enter",
        vec![press(Key::Enter), commit("x")],
    );
    expect_receipts(&p, vec![(1, 0, accepted(0, 1))]);
    assert_eq!(p.text, "x");
    assert!(p.changed);
    assert!(!p.focused);
}

#[test]
fn ime_receipt_mixed_text_and_paste_equal_payload_do_not_credit_rejected_commit() {
    let mut h = PairedHarness::new("");
    let p = h.frame(
        "mixed_equal_payload_inactive",
        vec![
            Event::Copy,
            Event::Text("x".to_owned()),
            commit("x"),
            Event::Paste("x".to_owned()),
        ],
    );
    expect_receipts(&p, vec![(2, 2, ImeEventOutcome::CommitRejectedAnchor)]);
    assert_eq!(p.text, "xx");
    assert_eq!(
        h.mutations(),
        &[
            Mutation::Delete { start: 0, end: 0 },
            primitive_insert(0, "x"),
            Mutation::Delete { start: 1, end: 1 },
            primitive_insert(1, "x"),
        ]
    );
    let mut accepted_first = PairedHarness::new("");
    let p = accepted_first.frame(
        "mixed_equal_payload_accepted_first",
        vec![
            commit("x"),
            Event::Text("x".to_owned()),
            Event::Paste("x".to_owned()),
        ],
    );
    expect_receipts(&p, vec![(0, 0, accepted(0, 1))]);
    assert_eq!(p.text, "xxx");
}

#[test]
fn ime_receipt_enabled_mid_batch_does_not_retroactively_sort() {
    let mut h = PairedHarness::new("z");
    let p = h.frame(
        "enabled_mid_batch_keeps_native_order",
        vec![
            Event::Text("x".to_owned()),
            ime(ImeEvent::Enabled),
            press(Key::Home),
            commit("x"),
            press(Key::End),
        ],
    );
    expect_receipts(
        &p,
        vec![
            (1, 1, ImeEventOutcome::Enabled),
            (3, 3, ImeEventOutcome::CommitRejectedAnchor),
        ],
    );
    assert_eq!(p.text, "xz");
    assert_eq!(p.indices(), (2, 2));
}

#[test]
fn ime_receipt_active_entry_stable_sort_retains_snapshot_indices_after_filtering() {
    let mut h = PairedHarness::new("");
    h.preedit("x");
    let p = h.frame(
        "active_stable_sort_and_filter",
        vec![
            press(Key::ArrowLeft), // removed by IME compatibility filter
            Event::Text("x".to_owned()),
            commit("x"),
            Event::Paste("x".to_owned()),
            ime(ImeEvent::Enabled),
            preedit("y"),
            press(Key::End),
            press(Key::End),      // InputState marks this press as repeat
            release(Key::Escape), // removed by TextEdit EventFilter
        ],
    );
    expect_receipts(
        &p,
        vec![
            (2, 0, accepted(0, 1)),
            (4, 1, ImeEventOutcome::Enabled),
            (5, 2, ImeEventOutcome::PreeditApplied),
        ],
    );
    assert_eq!(p.input_events.len(), 9);
    assert!(p.input_events[7].contains("repeat: true"));
    assert_eq!(p.text, "xxx");
    assert_eq!(p.indices(), (3, 3));
}

#[test]
fn ime_receipt_processing_order_counts_non_ime_events_but_not_filtered_events() {
    let mut h = PairedHarness::new("");
    let p = h.frame(
        "filtered_index_and_processing_gap",
        vec![
            release(Key::Escape),
            Event::Copy,
            ime(ImeEvent::Enabled),
            Event::Copy,
            commit("x"),
        ],
    );
    expect_receipts(
        &p,
        vec![(2, 1, ImeEventOutcome::Enabled), (4, 3, accepted(0, 1))],
    );
    assert_eq!(p.input_events.len(), 5);
    assert_eq!(p.text, "x");
}

#[test]
fn ime_receipt_index_is_textedit_input_snapshot_not_original_raw_input() {
    let mut h = PairedHarness::new("");
    let p = h
        .frame_with(
            "earlier_consumer_changes_input_snapshot",
            vec![Event::Text("discarded".to_owned()), commit("x")],
            FrameOptions {
                remove_prefix: 1,
                ..Default::default()
            },
        )
        .remove(0);
    expect_receipts(&p, vec![(0, 0, accepted(0, 1))]);
    assert_eq!(p.input_events.len(), 1);
    assert_eq!(p.text, "x");
}

#[test]
fn ime_receipt_pointer_selection_precedes_raw_commit_order() {
    for (at_end, expected, final_text) in [
        (false, accepted(0, 1), "xx"),
        (true, ImeEventOutcome::CommitRejectedAnchor, "x"),
    ] {
        let mut h = PairedHarness::new("");
        let selected = h.preedit("x");
        let pos = if at_end {
            selected.caret_end
        } else {
            selected.caret_start
        };
        let p = h.frame(
            "commit_before_pointer_in_raw_batch",
            vec![
                commit("x"),
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        expect_receipts(&p, vec![(0, 0, expected)]);
        assert_eq!(p.text, final_text);
    }
}

#[test]
fn ime_receipt_unicode_offsets_and_truncation_are_scalar_counts() {
    let mut h = PairedHarness::new("é🙂");
    h.tap("unicode_end", Key::End);
    h.frame(
        "unicode_save_anchor",
        vec![ime(ImeEvent::Enabled), ime(ImeEvent::Disabled)],
    );
    let p = h.frame("unicode_scalar_insertion_start", vec![commit("雪\u{301}")]);
    expect_receipts(&p, vec![(0, 0, accepted(2, 2))]);
    assert_eq!(p.text, "é🙂雪\u{301}");
    assert_eq!(p.indices(), (4, 4));
    assert_ne!(p.text.len(), p.text.chars().count());

    let initial = "é".repeat(4095);
    let mut truncated = PairedHarness::with_limit(&initial, 4096);
    let p = truncated.frame(
        "unicode_4096_limit_truncates_payload",
        vec![commit("雪🙂e\u{301}")],
    );
    expect_receipts(&p, vec![(0, 0, accepted(0, 1))]);
    assert_eq!(p.text, format!("雪{initial}"));
    assert_eq!(p.text.chars().count(), 4096);
    assert_eq!(
        truncated.mutations(),
        &[
            Mutation::Delete { start: 0, end: 0 },
            primitive_insert(0, "雪")
        ]
    );
}

#[test]
fn ime_receipt_zero_inserted_scalars_is_still_native_acceptance() {
    for limit in [0, 4096] {
        let initial = "🙂".repeat(limit);
        let mut h = PairedHarness::with_limit(&initial, limit);
        let p = h.frame("accepted_with_zero_available_scalars", vec![commit("雪🙂")]);
        expect_receipts(&p, vec![(0, 0, accepted(0, 0))]);
        assert_eq!(p.text, initial);
        assert_eq!(p.indices(), (0, 0));
        assert!(p.changed);
        assert_eq!(
            h.mutations(),
            &[
                Mutation::Delete { start: 0, end: 0 },
                primitive_insert(0, "")
            ]
        );
    }
}

#[test]
fn ime_receipt_undo_redo_replace_with_is_transparent_and_not_ime_acceptance() {
    let mut h = PairedHarness::new("z");
    h.frame("undo_fixture_type", vec![Event::Text("x".to_owned())]);
    h.frame("undo_fixture_stable", Vec::new());
    let undo = h.frame(
        "undo_uses_string_replace_with",
        vec![
            key_event(Key::Z, true, Modifiers::COMMAND),
            key_event(Key::Z, false, Modifiers::COMMAND),
        ],
    );
    assert_eq!(undo.text, "z");
    assert!(undo.receipts.is_empty());
    assert!(undo.changed);
    assert_eq!(
        h.mutations(),
        &[Mutation::Replace {
            text: "z".to_owned()
        }]
    );
    let redo = h.frame(
        "redo_uses_string_replace_with",
        vec![
            key_event(Key::Y, true, Modifiers::COMMAND),
            key_event(Key::Y, false, Modifiers::COMMAND),
        ],
    );
    assert_eq!(redo.text, "xz");
    assert!(redo.receipts.is_empty());
    assert!(redo.changed);
    assert_eq!(
        h.mutations(),
        &[Mutation::Replace {
            text: "xz".to_owned()
        }]
    );
    let mixed = h.frame(
        "undo_then_equal_commit_has_own_receipt",
        vec![
            key_event(Key::Z, true, Modifiers::COMMAND),
            commit("x"),
            key_event(Key::Z, false, Modifiers::COMMAND),
        ],
    );
    expect_receipts(&mixed, vec![(1, 1, accepted(0, 1))]);
    assert_eq!(mixed.text, "xz");
    assert_eq!(
        h.mutations(),
        &[
            Mutation::Replace {
                text: "z".to_owned()
            },
            Mutation::Delete { start: 0, end: 0 },
            primitive_insert(0, "x"),
        ]
    );
}

#[test]
fn ime_receipt_adapter_clear_take_and_replace_delegate_without_fake_insertions() {
    let mut plain = "é🙂".to_owned();
    let mut logged = MutationBuffer::new("é🙂");
    <String as TextBuffer>::replace_with(&mut plain, "雪");
    logged.replace_with("雪");
    assert_eq!(plain, logged.text);
    assert_eq!(
        logged.mutations,
        vec![Mutation::Replace {
            text: "雪".to_owned()
        }]
    );
    assert_eq!(<String as TextBuffer>::take(&mut plain), logged.take());
    assert_eq!(plain, logged.text);
    <String as TextBuffer>::replace_with(&mut plain, "x");
    logged.replace_with("x");
    <String as TextBuffer>::clear(&mut plain);
    logged.clear();
    assert_eq!(plain, logged.text);
    assert_eq!(
        logged.mutations,
        vec![
            Mutation::Replace {
                text: "雪".to_owned()
            },
            Mutation::Take,
            Mutation::Replace {
                text: "x".to_owned()
            },
            Mutation::Clear,
        ]
    );
    println!(
        "IME_NATIVE_RECEIPT_TRACE {}",
        json!({
            "case": "adapter_string_overrides", "final_text": logged.text,
            "mutations": logged.mutations.iter().map(Mutation::json).collect::<Vec<_>>(),
        })
    );
}

fn two_widget_sequence<B: TextBuffer>(first: &mut B, second: &mut B) -> Vec<Vec<NativePass>> {
    let ctx = egui::Context::default();
    ctx.options_mut(|o| o.max_passes = 1.try_into().unwrap());
    let a = egui::Id::new("receipt_widget_A");
    let b = egui::Id::new("receipt_widget_B");
    let mut result = Vec::new();
    for (index, (focus, events)) in [
        (a, Vec::new()),
        (a, vec![ime(ImeEvent::Enabled), preedit("x")]),
        // A loses focus while composing. Its native cleanup consumes all IME
        // events before B is shown, including this same-batch Commit.
        (b, vec![commit("y")]),
        // A fresh event on B's stable-focus frame is independently processed.
        (b, vec![commit("y")]),
        // A's former selection was collapsed on focus loss, so its saved
        // anchor no longer matches. No prior receipt is replayed here.
        (a, vec![commit("x")]),
    ]
    .into_iter()
    .enumerate()
    {
        let mut pair = Vec::new();
        let _ = ctx.run(raw_input(index as f64 + 1.0, events), |ctx| {
            ctx.memory_mut(|m| m.request_focus(focus));
            egui::CentralPanel::default().show(ctx, |ui| {
                let snapshot =
                    || ctx.input(|i| i.events.iter().map(|e| format!("{e:?}")).collect());
                let before_a = snapshot();
                let oa = egui::TextEdit::singleline(first)
                    .id(a)
                    .cursor_at_end(false)
                    .show(ui);
                pair.push(capture(oa, first.as_str(), before_a));
                let before_b = snapshot();
                let ob = egui::TextEdit::singleline(second)
                    .id(b)
                    .cursor_at_end(false)
                    .show(ui);
                pair.push(capture(ob, second.as_str(), before_b));
            });
        });
        assert_eq!(pair.len(), 2);
        assert_eq!(pair[0].widget_id, a);
        assert_eq!(pair[1].widget_id, b);
        result.push(pair);
    }
    result
}

#[test]
fn ime_receipt_two_widgets_do_not_share_or_replay_receipts() {
    let plain = two_widget_sequence(&mut String::new(), &mut String::new());
    let observed = two_widget_sequence(
        &mut MutationBuffer::default(),
        &mut MutationBuffer::default(),
    );
    assert_eq!(plain, observed);
    expect_receipts(
        &plain[1][0],
        vec![
            (0, 0, ImeEventOutcome::Enabled),
            (1, 1, ImeEventOutcome::PreeditApplied),
        ],
    );
    assert!(plain[1][1].receipts.is_empty());
    assert!(plain[2][0].receipts.is_empty());
    assert_eq!(plain[2][0].input_events.len(), 1);
    assert!(plain[2][1].receipts.is_empty());
    assert!(plain[2][1].input_events.is_empty());
    assert_eq!(plain[2][0].text, "x");
    assert!(plain[2][1].text.is_empty());
    assert_eq!(plain[2][0].indices(), (1, 1));
    assert!(!plain[2][0].changed && !plain[2][1].changed);
    assert!(plain[3][0].receipts.is_empty());
    expect_receipts(&plain[3][1], vec![(0, 0, accepted(0, 1))]);
    assert_eq!(plain[3][1].input_events.len(), 1);
    expect_receipts(
        &plain[4][0],
        vec![(0, 0, ImeEventOutcome::CommitRejectedAnchor)],
    );
    assert!(plain[4][1].receipts.is_empty());
    assert_eq!(plain[4][0].text, "x");
    assert_eq!(plain[4][1].text, "y");
    println!(
        "IME_NATIVE_RECEIPT_TRACE {}",
        json!({
            "case": "two_widgets_focus_transfer",
            "frames": plain.iter().map(|pair| pair.iter().map(NativePass::json).collect::<Vec<_>>()).collect::<Vec<_>>(),
        })
    );
}

#[test]
fn ime_receipt_real_discard_extra_pass_has_no_events_or_stale_receipts() {
    for reject in [false, true] {
        let mut h = PairedHarness::new("");
        h.preedit("x");
        if reject {
            h.tap("discard_fixture_reject_anchor", Key::End);
        }
        let passes = h.frame_with(
            "real_request_discard",
            vec![commit("x")],
            FrameOptions {
                discard: true,
                ..Default::default()
            },
        );
        expect_receipts(
            &passes[0],
            vec![(
                0,
                0,
                if reject {
                    ImeEventOutcome::CommitRejectedAnchor
                } else {
                    accepted(0, 1)
                },
            )],
        );
        assert_eq!(passes[0].input_events.len(), 1);
        assert!(passes[0].changed);
        assert!(passes[1].input_events.is_empty());
        assert!(passes[1].receipts.is_empty());
        assert!(!passes[1].changed);
        assert_eq!(passes[0].text, passes[1].text);
        assert_eq!(passes[0].cursor, passes[1].cursor);
        let later = h.frame("frame_after_discard_no_stale_receipt", Vec::new());
        assert!(later.receipts.is_empty());
        assert!(!later.changed);
        assert_eq!(later.text, "x");
        // The first-pass value remains available; an empty later pass does not
        // retroactively change the outcome of the actual processing invocation.
        assert_eq!(passes[0].receipts.len(), 1);
    }
}

#[test]
fn ime_receipt_new_preedit_then_disabled_has_no_latest_completion() {
    let mut h = PairedHarness::new("");
    h.preedit("x");
    let p = h.frame(
        "commit_then_new_preedit_then_disabled",
        vec![commit("x"), preedit("y"), ime(ImeEvent::Disabled)],
    );
    expect_receipts(
        &p,
        vec![
            (0, 0, accepted(0, 1)),
            (1, 1, ImeEventOutcome::PreeditApplied),
            (2, 2, ImeEventOutcome::Disabled),
        ],
    );
    assert_eq!(p.text, "xy");
    assert_eq!(p.indices(), (2, 1));
    assert!(p.changed);
    // The earlier successful Commit cannot stand in for a successful terminal
    // after the later applied Preedit. Disabled itself is not such a terminal.
    assert!(!p.receipts[2..]
        .iter()
        .any(|r| matches!(r.outcome, ImeEventOutcome::CommitAccepted { .. })));
    assert_eq!(
        h.mutations(),
        &[
            Mutation::Delete { start: 0, end: 1 },
            primitive_insert(0, "x"),
            Mutation::Delete { start: 1, end: 1 },
            primitive_insert(1, "y"),
        ]
    );
}

#[test]
fn ime_receipt_different_payload_text_and_paste_preserve_native_ordering() {
    for active in [false, true] {
        let mut h = PairedHarness::new("");
        if active {
            h.preedit("x");
        }
        let p = h.frame(
            "mixed_different_payloads",
            vec![
                Event::Text("t".to_owned()),
                commit("c"),
                Event::Paste("p".to_owned()),
            ],
        );
        if active {
            expect_receipts(&p, vec![(1, 0, accepted(0, 1))]);
            assert_eq!(p.text, "ctp");
            assert_eq!(
                h.mutations(),
                &[
                    Mutation::Delete { start: 0, end: 1 },
                    primitive_insert(0, "c"),
                    Mutation::Delete { start: 1, end: 1 },
                    primitive_insert(1, "t"),
                    Mutation::Delete { start: 2, end: 2 },
                    primitive_insert(2, "p"),
                ]
            );
        } else {
            expect_receipts(&p, vec![(1, 1, ImeEventOutcome::CommitRejectedAnchor)]);
            assert_eq!(p.text, "tp");
            assert_eq!(
                h.mutations(),
                &[
                    Mutation::Delete { start: 0, end: 0 },
                    primitive_insert(0, "t"),
                    Mutation::Delete { start: 1, end: 1 },
                    primitive_insert(1, "p"),
                ]
            );
        }
    }
}

#[test]
fn ime_receipt_custom_insertion_can_return_backwards_cursor_without_observer_panic() {
    // This deliberately non-String cursor policy is an API-boundary stress
    // case, separate from the transparent String adapter used above. The
    // receipt must report actual endpoints without subtracting or asserting
    // that every implementor's cursor advance equals an inserted scalar count.
    struct BackwardCursorBuffer(MutationBuffer);
    impl TextBuffer for BackwardCursorBuffer {
        fn is_mutable(&self) -> bool {
            self.0.is_mutable()
        }
        fn as_str(&self) -> &str {
            self.0.as_str()
        }
        fn insert_text(&mut self, text: &str, char_index: usize) -> usize {
            self.0.insert_text(text, char_index)
        }
        fn delete_char_range(&mut self, char_range: Range<usize>) {
            self.0.delete_char_range(char_range);
        }
        fn replace_with(&mut self, text: &str) {
            self.0.replace_with(text);
        }
        fn clear(&mut self) {
            self.0.clear();
        }
        fn take(&mut self) -> String {
            self.0.take()
        }
        fn insert_text_at(&mut self, cursor: &mut CCursor, text: &str, limit: usize) {
            <MutationBuffer as TextBuffer>::insert_text_at(&mut self.0, cursor, text, limit);
            cursor.index = 0;
        }
    }
    let mut h = NativeHarness::new(BackwardCursorBuffer(MutationBuffer::new("z")), 4096);
    h.frame(
        Vec::new(),
        FrameOptions {
            focus: Some(true),
            ..Default::default()
        },
    );
    let at_end = h.frame(
        vec![press(Key::End), release(Key::End)],
        FrameOptions::default(),
    );
    assert_eq!(at_end[0].indices(), (1, 1));
    let saved = h.frame(
        vec![ime(ImeEvent::Enabled), ime(ImeEvent::Disabled)],
        FrameOptions::default(),
    );
    expect_receipts(
        &saved[0],
        vec![
            (0, 0, ImeEventOutcome::Enabled),
            (1, 1, ImeEventOutcome::Disabled),
        ],
    );
    h.buffer.0.mutations.clear();
    let p = h
        .frame(vec![commit("x")], FrameOptions::default())
        .remove(0);
    println!(
        "IME_NATIVE_RECEIPT_TRACE {}",
        json!({
            "case": "custom_backwards_cursor", "output": p.json(),
            "mutations": h.buffer.0.mutations.iter().map(Mutation::json).collect::<Vec<_>>(),
        })
    );
    expect_receipts(
        &p,
        vec![(
            0,
            0,
            ImeEventOutcome::CommitAccepted {
                insertion_start: 1,
                insertion_end: 0,
            },
        )],
    );
    assert_eq!(p.text, "zx");
    assert_eq!(p.indices(), (0, 0));
    assert!(p.changed);
    assert_eq!(
        h.buffer.0.mutations,
        vec![
            Mutation::Delete { start: 1, end: 1 },
            primitive_insert(1, "x"),
        ]
    );
}
